//! Reconnexion a Proton Pass: `pass-cli login` et son lien web.
//!
//! Une session Proton Pass ne dure pas: elle expire, et l'arret de la machine
//! y met fin. sshpass-gui devenait alors inutilisable jusqu'a ce que
//! l'utilisateur pense a taper `pass-cli login` dans un terminal.
//!
//! Ce module relance le flux tout seul. Il ne peut pas passer par `PassCli::run`:
//! `pass-cli login` attend que l'authentification web aboutisse, ce qui prend
//! le temps qu'il faut a un humain — bien au-dela du delai des autres appels.
//! Le processus est donc supervise comme un agent: sa sortie est lue ligne a
//! ligne pendant qu'il tourne, et la premiere URL qu'il imprime est ouverte
//! dans le navigateur.
//!
//! Aucun identifiant ne transite ici: l'authentification se fait entierement
//! entre le navigateur et Proton. sshpass-gui ne voit qu'une URL publique.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::pass::cli::{PassCli, PassError};

/// Au-dela, on considere le flux abandonne et on arrete le processus.
///
/// Genereux: il faut le temps d'ouvrir un navigateur, de s'identifier et de
/// valider un second facteur. Mais borne, sinon un `pass-cli login` oublie
/// resterait a tourner pour la duree de la session.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// Nombre de lignes de sortie conservees pour le diagnostic.
const LOG_LINES: usize = 40;

/// Sursis accorde aux lecteurs de sortie apres la mort du processus.
///
/// Un processus peut mourir avant que ses tubes aient ete lus: la derniere
/// ligne — celle qui dit *pourquoi* la connexion a echoue — est encore en
/// transit. Sans ce sursis, l'utilisateur lit « s'est arrete sans se
/// connecter » au lieu de « network unreachable », une fois sur dix.
const DRAIN_GRACE: Duration = Duration::from_millis(200);

/// Ou en est la reconnexion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginState {
    /// Aucune reconnexion en cours.
    Idle,
    /// Processus lance, l'URL n'est pas encore imprimee.
    Starting,
    /// URL connue: le navigateur a ete ouvert, on attend la fin du flux.
    Waiting(String),
}

impl LoginState {
    pub fn is_running(&self) -> bool {
        !matches!(self, LoginState::Idle)
    }

    /// URL a proposer si l'ouverture automatique n'a rien donne.
    pub fn url(&self) -> Option<&str> {
        match self {
            LoginState::Waiting(url) => Some(url),
            _ => None,
        }
    }
}

/// Issue d'un flux termine, remontee une seule fois par `poll`.
#[derive(Debug, Clone)]
pub enum LoginOutcome {
    /// `pass-cli login` s'est termine correctement: la session doit etre
    /// resondee pour confirmer.
    Succeeded,
    Failed(String),
}

/// Supervise au plus un `pass-cli login` a la fois.
pub struct LoginManager {
    child: Option<Child>,
    /// Sortie constatee, pas encore remontee: on attend que les lecteurs
    /// aient fini de vider les tubes (cf. `DRAIN_GRACE`).
    exited: Option<(ExitStatus, Instant)>,
    /// Threads de lecture des tubes. Leur fin signale un tube vide.
    readers: Vec<JoinHandle<()>>,
    state: LoginState,
    log: Arc<Mutex<Vec<String>>>,
    /// Premiere URL vue dans la sortie, remplie par les threads de lecture.
    url: Arc<Mutex<Option<String>>>,
    started: Option<Instant>,
}

impl Default for LoginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginManager {
    pub fn new() -> Self {
        Self {
            child: None,
            exited: None,
            readers: Vec::new(),
            state: LoginState::Idle,
            log: Arc::new(Mutex::new(Vec::new())),
            url: Arc::new(Mutex::new(None)),
            started: None,
        }
    }

    pub fn state(&self) -> &LoginState {
        &self.state
    }

    pub fn is_running(&self) -> bool {
        self.state.is_running()
    }

    /// Lance `pass-cli login`. Sans effet si un flux est deja en cours: deux
    /// processus concurrents ouvriraient deux onglets de navigateur pour la
    /// meme session.
    pub fn start(&mut self, cli: &PassCli) -> Result<(), PassError> {
        if self.is_running() {
            return Ok(());
        }
        // Repartir d'une ardoise propre: le log et l'URL du flux precedent
        // n'ont plus de sens.
        self.log = Arc::new(Mutex::new(Vec::new()));
        self.url = Arc::new(Mutex::new(None));

        let mut child = Command::new(cli.binary())
            // Pas de `stdin` heritee: sans terminal, `pass-cli` doit choisir le
            // flux web — le seul qui n'attende aucune saisie de notre part.
            .arg("login")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    PassError::NotFound(cli.binary().to_string())
                } else {
                    PassError::Io(err)
                }
            })?;

        self.readers.clear();
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
            let log = Arc::clone(&self.log);
            let url = Arc::clone(&self.url);
            self.readers.push(std::thread::spawn(move || {
                for line in BufReader::new(pipe)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    if let Some(found) = extract_url(&line) {
                        if let Ok(mut slot) = url.lock() {
                            slot.get_or_insert(found);
                        }
                    }
                    if let Ok(mut lines) = log.lock() {
                        lines.push(line);
                        let overflow = lines.len().saturating_sub(LOG_LINES);
                        lines.drain(..overflow);
                    }
                }
            }));
        }

        self.child = Some(child);
        self.exited = None;
        self.state = LoginState::Starting;
        self.started = Some(Instant::now());
        Ok(())
    }

    /// Fait avancer le flux. A appeler a chaque frame.
    ///
    /// Renvoie une issue une seule fois, quand le processus se termine ou que
    /// le delai est depasse.
    pub fn poll(&mut self) -> Option<LoginOutcome> {
        if self.exited.is_some() {
            return self.deliver();
        }
        self.child.as_ref()?;

        // L'URL peut apparaitre a tout moment: des qu'elle est la, on la donne
        // au navigateur, une seule fois.
        if matches!(self.state, LoginState::Starting) {
            if let Some(url) = self.url.lock().ok().and_then(|slot| slot.clone()) {
                open_in_browser(&url);
                self.state = LoginState::Waiting(url);
            }
        }

        let exited = self
            .child
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten());
        if let Some(status) = exited {
            self.child = None;
            self.exited = Some((status, Instant::now()));
            return self.deliver();
        }

        if self
            .started
            .is_some_and(|since| since.elapsed() > LOGIN_TIMEOUT)
        {
            self.cancel();
            return Some(LoginOutcome::Failed(format!(
                "Aucune confirmation du navigateur en {} s: reconnexion abandonnee.",
                LOGIN_TIMEOUT.as_secs()
            )));
        }
        None
    }

    /// Remonte l'issue d'un processus termine, une fois ses tubes vides.
    ///
    /// Tant qu'un lecteur tourne encore, la derniere ligne de sortie peut etre
    /// en route: on attend une frame de plus plutot que de perdre la raison de
    /// l'echec. Passe `DRAIN_GRACE`, on remonte ce qu'on a — un petit-fils qui
    /// aurait herite du tube le tiendrait ouvert indefiniment, et l'interface
    /// n'a pas a l'attendre.
    fn deliver(&mut self) -> Option<LoginOutcome> {
        let (status, since) = self.exited?;
        let drained = self.readers.iter().all(JoinHandle::is_finished);
        if !drained && since.elapsed() < DRAIN_GRACE {
            return None;
        }

        self.exited = None;
        self.readers.clear();
        self.state = LoginState::Idle;
        self.started = None;
        Some(if status.success() {
            LoginOutcome::Succeeded
        } else {
            LoginOutcome::Failed(self.last_log().unwrap_or_else(|| {
                format!("`pass-cli login` s'est arrete sans se connecter ({status})")
            }))
        })
    }

    /// Arrete le flux en cours.
    pub fn cancel(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.exited = None;
        self.readers.clear();
        self.state = LoginState::Idle;
        self.started = None;
    }

    fn last_log(&self) -> Option<String> {
        self.log
            .lock()
            .ok()
            .and_then(|lines| lines.iter().rev().find(|l| !l.trim().is_empty()).cloned())
    }
}

impl Drop for LoginManager {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Extrait une URL `https://` d'une ligne de sortie.
///
/// `pass-cli login` imprime l'adresse au milieu d'une phrase; on ne retient
/// que le jeton qui commence par `https://`. **Seul** ce schema est accepte:
/// la chaine part vers un ouvreur d'URL, et il n'est pas question de lui
/// confier un `file://` ou un `javascript:` venu d'une sortie mal analysee.
pub fn extract_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        // Les terminaux entourent parfois l'adresse de ponctuation ou de
        // sequences d'echappement: on s'arrete au premier caractere qui ne
        // peut pas faire partie d'une URL.
        .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | '`' | '\u{1b}'))
        .collect();
    let url = url.trim_end_matches(['.', ',', ')', ']', ';']).to_string();
    // `https://` seul ne mene nulle part.
    (url.len() > "https://".len()).then_some(url)
}

/// Ouvre une URL dans le navigateur de l'utilisateur.
///
/// L'adresse est passee comme argument unique, sans shell: rien de ce qu'elle
/// contient ne peut etre interprete comme une commande.
fn open_in_browser(url: &str) {
    if !url.starts_with("https://") {
        log::error!("URL de connexion refusee (schema inattendu)");
        return;
    }
    // `xdg-open` sous Linux, `open` sous macOS: le premier qui repond gagne.
    for opener in ["xdg-open", "open"] {
        match Command::new(opener)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                log::info!("lien de connexion Proton Pass ouvert avec {opener}");
                return;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                log::warn!("{opener} a echoue: {err}");
                return;
            }
        }
    }
    // Pas d'ouvreur: l'interface affiche l'URL, l'utilisateur la copiera.
    log::warn!("aucun ouvreur d'URL (xdg-open, open) trouve");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_extracted_from_a_sentence() {
        assert_eq!(
            extract_url("Open this URL: https://account.proton.me/login/abc123 to continue"),
            Some("https://account.proton.me/login/abc123".to_string())
        );
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_url() {
        assert_eq!(
            extract_url("Visit https://account.proton.me/a/b."),
            Some("https://account.proton.me/a/b".to_string())
        );
        assert_eq!(
            extract_url("(https://proton.me/x)"),
            Some("https://proton.me/x".to_string())
        );
    }

    #[test]
    fn only_https_is_accepted() {
        assert_eq!(extract_url("go to http://proton.me/x"), None);
        assert_eq!(extract_url("file:///etc/passwd"), None);
        assert_eq!(extract_url("javascript:alert(1)"), None);
        assert_eq!(extract_url("rien a signaler"), None);
    }

    #[test]
    fn a_bare_scheme_is_not_a_url() {
        assert_eq!(extract_url("https://"), None);
        assert_eq!(extract_url("https:// "), None);
    }

    #[test]
    fn escape_sequences_do_not_leak_into_the_url() {
        let colored = "\u{1b}[36mhttps://proton.me/login/x\u{1b}[0m";
        assert_eq!(
            extract_url(colored),
            Some("https://proton.me/login/x".to_string())
        );
    }

    #[test]
    fn a_fresh_manager_is_idle() {
        let manager = LoginManager::new();
        assert_eq!(manager.state(), &LoginState::Idle);
        assert!(!manager.is_running());
        assert!(manager.state().url().is_none());
    }

    #[test]
    fn polling_without_a_process_yields_nothing() {
        let mut manager = LoginManager::new();
        assert!(manager.poll().is_none());
    }

    /// Faux `pass-cli` qui n'imprime **aucune** URL: les tests ne doivent
    /// jamais atteindre l'ouvreur de navigateur de la machine qui les lance.
    #[cfg(unix)]
    fn stub(name: &str, body: &str) -> (PassCli, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("sshpass-gui-login-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("pass-cli");
        crate::pass::testing::write_stub(&path, body);
        (PassCli::new(path.to_string_lossy().into_owned()), dir)
    }

    /// Demarre le flux, en laissant passer un « Text file busy » (cf.
    /// `pass::testing`).
    #[cfg(unix)]
    fn start(manager: &mut LoginManager, cli: &PassCli) {
        crate::pass::testing::unhurried(|| manager.start(cli)).expect("lancement");
    }

    #[cfg(unix)]
    fn drain(manager: &mut LoginManager) -> LoginOutcome {
        for _ in 0..500 {
            if let Some(outcome) = manager.poll() {
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("le flux de connexion ne s'est jamais termine");
    }

    #[cfg(unix)]
    #[test]
    fn a_completed_flow_reports_success() {
        let (cli, dir) = stub("ok", "exit 0");
        let mut manager = LoginManager::new();
        start(&mut manager, &cli);
        assert!(manager.is_running());
        assert!(matches!(drain(&mut manager), LoginOutcome::Succeeded));
        // Le flux termine libere la place pour le suivant.
        assert!(!manager.is_running());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Verifie aussi que la sortie d'erreur est bien lue avant que l'echec ne
    /// soit remonte: le processus meurt souvent avant que son tube soit vide
    /// (cf. `DRAIN_GRACE`).
    #[cfg(unix)]
    #[test]
    fn a_failed_flow_carries_the_reason() {
        let (cli, dir) = stub("ko", "echo 'network unreachable' >&2; exit 1");
        let mut manager = LoginManager::new();
        start(&mut manager, &cli);
        match drain(&mut manager) {
            LoginOutcome::Failed(detail) => {
                assert!(detail.contains("network unreachable"), "obtenu: {detail}")
            }
            other => panic!("attendu Failed, obtenu {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_second_start_does_not_open_a_second_browser() {
        // `read` bloque tant que l'entree n'est pas fermee: le processus reste
        // en vie le temps du test.
        let (cli, dir) = stub("busy", "sleep 30");
        let mut manager = LoginManager::new();
        start(&mut manager, &cli);
        // Le second appel est un non-evenement, pas une erreur.
        manager.start(&cli).expect("second lancement");
        assert!(manager.is_running());
        manager.cancel();
        assert!(!manager.is_running());
        assert!(manager.poll().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_binary_is_reported() {
        let mut manager = LoginManager::new();
        let cli = PassCli::new("pass-cli-qui-n-existe-pas");
        assert!(matches!(manager.start(&cli), Err(PassError::NotFound(_))));
        assert!(!manager.is_running());
    }
}
