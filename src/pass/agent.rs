//! Supervision des agents SSH Proton Pass.
//!
//! Trois modes coexistent (cf. `AgentMode`):
//!
//! * `OwnAgent` — sshpass-gui demarre un `pass-cli ssh-agent start` **par coffre**,
//!   sur une socket qui lui est propre. Chaque onglet terminal recoit dans son
//!   environnement le `SSH_AUTH_SOCK` de l'agent du coffre associe a sa
//!   connexion. Un coffre = un agent = une socket, partage par tous les
//!   onglets qui l'utilisent: demarrer un agent par onglet multiplierait les
//!   deverrouillages de session pour rien.
//! * `LoadIntoExisting` — `pass-cli ssh-agent load` pousse les cles dans
//!   l'agent deja reference par `SSH_AUTH_SOCK`; sshpass-gui ne surcharge alors
//!   aucune variable.
//! * `Disabled` — les onglets heritent simplement de l'environnement.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::config::{runtime_dir, secure_runtime_dir, AgentMode};
use crate::pass::cli::{PassCli, PassError};

/// Nombre de lignes de sortie conservees par agent pour le diagnostic.
const LOG_LINES: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    Stopped,
    /// Processus lance, socket pas encore apparue.
    Starting,
    Running,
    Failed(String),
}

impl AgentState {
    pub fn label(&self) -> &str {
        match self {
            AgentState::Stopped => "arrete",
            AgentState::Starting => "demarrage",
            AgentState::Running => "actif",
            AgentState::Failed(_) => "erreur",
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, AgentState::Running)
    }
}

struct Agent {
    child: Option<Child>,
    socket: PathBuf,
    state: AgentState,
    log: Arc<Mutex<Vec<String>>>,
}

impl Agent {
    fn last_log(&self) -> Option<String> {
        self.log.lock().ok().and_then(|lines| lines.last().cloned())
    }
}

pub struct AgentManager {
    cli: PassCli,
    mode: AgentMode,
    refresh_interval: u64,
    /// Accepter de poser les sockets dans `/tmp` faute de `XDG_RUNTIME_DIR`.
    ///
    /// Une socket d'agent SSH est une porte ouverte sur toutes les cles du
    /// coffre: elle n'a rien a faire dans un repertoire que la machine entiere
    /// peut ecrire. Cf. `config::secure_runtime_dir`.
    allow_temp_runtime_dir: bool,
    agents: HashMap<String, Agent>,
}

impl AgentManager {
    pub fn new(
        binary: &str,
        mode: AgentMode,
        refresh_interval: u64,
        allow_temp_runtime_dir: bool,
    ) -> Self {
        Self {
            cli: PassCli::new(binary),
            mode,
            refresh_interval,
            allow_temp_runtime_dir,
            agents: HashMap::new(),
        }
    }

    /// Applique une nouvelle configuration. Changer de binaire ou quitter le
    /// mode `OwnAgent` arrete les agents en cours.
    pub fn reconfigure(
        &mut self,
        binary: &str,
        mode: AgentMode,
        refresh_interval: u64,
        allow_temp_runtime_dir: bool,
    ) {
        let binary_changed = binary != self.cli.binary();
        self.cli = PassCli::new(binary);
        self.refresh_interval = refresh_interval;
        self.allow_temp_runtime_dir = allow_temp_runtime_dir;
        if self.mode != mode || binary_changed {
            self.mode = mode;
            self.stop_all();
        }
    }

    /// Chemin de la socket dediee a un coffre.
    ///
    /// Le nom est tronque et suffixe par un hachage: les sockets Unix sont
    /// limitees a ~108 octets, un nom de coffre long ferait echouer le bind.
    pub fn socket_path(vault: &str) -> PathBuf {
        let slug: String = vault
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(24)
            .collect();
        let mut hasher = DefaultHasher::new();
        vault.hash(&mut hasher);
        runtime_dir().join(format!(
            "agent-{}-{:x}.sock",
            slug.trim_matches('-'),
            hasher.finish()
        ))
    }

    /// Demarre si besoin l'agent du coffre et renvoie son etat courant.
    pub fn ensure(&mut self, vault: &str) -> AgentState {
        if self.mode != AgentMode::OwnAgent || vault.is_empty() {
            return AgentState::Stopped;
        }
        if let Some(agent) = self.agents.get(vault) {
            if !matches!(agent.state, AgentState::Failed(_) | AgentState::Stopped) {
                return agent.state.clone();
            }
        }
        match self.spawn(vault) {
            Ok(()) => AgentState::Starting,
            Err(err) => {
                let message = err.to_string();
                log::error!("agent Proton Pass pour {vault}: {message}");
                self.agents.insert(
                    vault.to_string(),
                    Agent {
                        child: None,
                        socket: Self::socket_path(vault),
                        state: AgentState::Failed(message.clone()),
                        log: Arc::new(Mutex::new(vec![message])),
                    },
                );
                AgentState::Failed(err.to_string())
            }
        }
    }

    fn spawn(&mut self, vault: &str) -> Result<(), PassError> {
        let socket = Self::socket_path(vault);
        // Cree le repertoire **et** verifie qu'il n'est qu'a nous: la socket
        // qui va y naitre donne acces aux cles du coffre.
        secure_runtime_dir(self.allow_temp_runtime_dir)?;
        // Une socket orpheline (agent tue sans nettoyage) empecherait le bind.
        if socket.exists() {
            let _ = std::fs::remove_file(&socket);
        }

        let mut child = Command::new(self.cli.binary())
            .args([
                "ssh-agent",
                "start",
                "--vault-name",
                vault,
                "--socket-path",
                &socket.to_string_lossy(),
                "--refresh-interval",
                &self.refresh_interval.to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    PassError::NotFound(self.cli.binary().to_string())
                } else {
                    PassError::Io(err)
                }
            })?;

        let log = Arc::new(Mutex::new(Vec::new()));
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let log = Arc::clone(&log);
            std::thread::spawn(move || {
                for line in BufReader::new(pipe)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    if let Ok(mut lines) = log.lock() {
                        lines.push(line);
                        let overflow = lines.len().saturating_sub(LOG_LINES);
                        lines.drain(..overflow);
                    }
                }
            });
        }

        self.agents.insert(
            vault.to_string(),
            Agent {
                child: Some(child),
                socket,
                state: AgentState::Starting,
                log,
            },
        );
        Ok(())
    }

    /// Met a jour les etats. A appeler a chaque frame: les operations sont un
    /// `try_wait` et un `metadata` par agent.
    pub fn poll(&mut self) {
        for (vault, agent) in &mut self.agents {
            let exited = match agent.child.as_mut() {
                Some(child) => child.try_wait().ok().flatten(),
                None => None,
            };
            if let Some(status) = exited {
                agent.child = None;
                let detail = agent
                    .last_log()
                    .unwrap_or_else(|| format!("processus termine ({status})"));
                agent.state = AgentState::Failed(detail);
                let _ = std::fs::remove_file(&agent.socket);
                continue;
            }
            if agent.child.is_some() {
                // La socket apparait quelques dizaines de millisecondes apres
                // le lancement; c'est elle, et non le texte affiche, qui fait
                // foi pour declarer l'agent utilisable.
                agent.state = if agent.socket.exists() {
                    AgentState::Running
                } else {
                    AgentState::Starting
                };
                if agent.state.is_running() {
                    log::debug!("agent {vault} pret sur {}", agent.socket.display());
                }
            }
        }
    }

    /// Socket a exporter dans `SSH_AUTH_SOCK` pour ce coffre, si elle existe.
    pub fn socket_for(&self, vault: &str) -> Option<PathBuf> {
        let agent = self.agents.get(vault)?;
        (agent.state.is_running() && agent.socket.exists()).then(|| agent.socket.clone())
    }

    pub fn state_of(&self, vault: &str) -> AgentState {
        self.agents
            .get(vault)
            .map(|a| a.state.clone())
            .unwrap_or(AgentState::Stopped)
    }

    pub fn stop(&mut self, vault: &str) {
        if let Some(mut agent) = self.agents.remove(vault) {
            if let Some(child) = agent.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = std::fs::remove_file(&agent.socket);
        }
    }

    pub fn stop_all(&mut self) {
        for vault in self.agents.keys().cloned().collect::<Vec<_>>() {
            self.stop(&vault);
        }
    }

    /// Mode `LoadIntoExisting`: pousse les cles du coffre dans l'agent courant.
    /// Appel bloquant, a executer hors du thread d'interface.
    pub fn load_into_existing(cli: &PassCli, vault: &str) -> Result<String, PassError> {
        let binary = cli.binary().to_string();
        let mut args = vec!["ssh-agent".to_string(), "load".to_string()];
        if !vault.is_empty() {
            args.push("--vault-name".to_string());
            args.push(vault.to_string());
        }
        let output = Command::new(&binary)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    PassError::NotFound(binary.clone())
                } else {
                    PassError::Io(err)
                }
            })?;
        if !output.status.success() {
            return Err(PassError::Command {
                command: format!("{binary} {}", args.join(" ")),
                code: output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into()),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Drop for AgentManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_paths_are_short_and_unique() {
        let long_vault = "Un nom de coffre vraiment tres long avec des accents et des espaces";
        let path = AgentManager::socket_path(long_vault);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.len() < 64, "nom trop long: {name}");
        assert!(name.ends_with(".sock"));
        assert_ne!(
            AgentManager::socket_path("A"),
            AgentManager::socket_path("B")
        );
        assert_eq!(
            AgentManager::socket_path("A"),
            AgentManager::socket_path("A")
        );
    }

    #[test]
    fn socket_name_has_no_separator() {
        let path = AgentManager::socket_path("SSH/Keys");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(!name.contains('/'));
    }

    #[test]
    fn disabled_mode_never_spawns() {
        let mut manager = AgentManager::new("pass-cli", AgentMode::Disabled, 3600, false);
        assert_eq!(manager.ensure("Coffre"), AgentState::Stopped);
        assert_eq!(manager.state_of("Coffre"), AgentState::Stopped);
    }

    #[test]
    fn missing_binary_marks_agent_failed() {
        let mut manager =
            AgentManager::new("pass-cli-qui-n-existe-pas", AgentMode::OwnAgent, 3600, true);
        assert!(matches!(manager.ensure("Coffre"), AgentState::Failed(_)));
        assert!(manager.socket_for("Coffre").is_none());
        assert!(matches!(manager.state_of("Coffre"), AgentState::Failed(_)));
    }

    #[test]
    fn empty_vault_is_ignored() {
        let mut manager = AgentManager::new("pass-cli", AgentMode::OwnAgent, 3600, false);
        assert_eq!(manager.ensure(""), AgentState::Stopped);
    }
}
