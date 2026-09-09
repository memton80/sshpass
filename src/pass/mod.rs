//! Integration Proton Pass: client `pass-cli`, agents SSH et pont askpass.

pub mod agent;
pub mod cli;
pub mod login;
pub mod secret;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

pub use agent::{AgentManager, AgentState};
pub use cli::{
    Item, LoginDraft, PassCli, PassFailure, Probe, Session, SshKeySource, SshKeyType, Vault,
};
pub use login::{LoginManager, LoginOutcome};
pub use secret::Secret;

use crate::config::runtime_dir;

/// Requete adressee au thread Proton Pass. Le client est transporte avec la
/// requete: le thread reste sans etat et suit donc immediatement un changement
/// de binaire dans la configuration.
pub enum PassRequest {
    Probe(PassCli),
    Vaults(PassCli),
    Items(PassCli, String),
    LoadAgent(PassCli, String),
    /// Ecrit le mot de passe d'une connexion dans un coffre.
    ///
    /// Le secret voyage dans le `LoginDraft`, qui n'est ni clonable ni
    /// affichable: il n'existe qu'ici, et il est detruit avec la requete.
    SaveLogin {
        cli: PassCli,
        vault: String,
        draft: LoginDraft,
        /// Connexion a rattacher a l'item une fois celui-ci ecrit.
        connection: String,
        /// Un item de ce titre existe deja: mettre a jour plutot que creer.
        /// Sans cela Proton Pass accepterait un doublon, et l'URI
        /// `pass://coffre/titre` deviendrait ambigue.
        replace: bool,
    },
    /// Range une cle SSH dans un coffre, importee ou generee.
    SaveSshKey {
        cli: PassCli,
        vault: String,
        title: String,
        source: SshKeySource,
        connection: String,
    },
}

/// Reponse du thread Proton Pass. Les erreurs sont deja mises en forme: le
/// thread d'interface n'a plus qu'a les afficher.
///
/// Aucune variante ne rapporte de secret: seulement de quoi rattacher la
/// connexion a l'item et de quoi afficher un message.
pub enum PassResponse {
    Probe(Result<Probe, PassFailure>),
    Vaults(Result<Vec<Vault>, PassFailure>),
    Items(String, Result<Vec<Item>, PassFailure>),
    LoadAgent(String, Result<String, PassFailure>),
    SavedLogin {
        connection: String,
        vault: String,
        item: String,
        result: Result<String, PassFailure>,
    },
    SavedSshKey {
        connection: String,
        vault: String,
        item: String,
        result: Result<String, PassFailure>,
    },
}

/// Executeur des appels `pass-cli`, qui sont bloquants (deverrouillage de
/// session, reseau) et ne doivent jamais s'executer sur le thread de rendu.
pub struct PassWorker {
    tx: Sender<PassRequest>,
    rx: Receiver<PassResponse>,
    pending: usize,
}

impl PassWorker {
    pub fn spawn() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<PassRequest>();
        let (res_tx, res_rx) = mpsc::channel::<PassResponse>();

        std::thread::Builder::new()
            .name("proton-pass".into())
            .spawn(move || {
                for request in req_rx {
                    let response = match request {
                        PassRequest::Probe(cli) => {
                            PassResponse::Probe(cli.probe().map_err(PassFailure::from))
                        }
                        PassRequest::Vaults(cli) => {
                            PassResponse::Vaults(cli.vaults().map_err(PassFailure::from))
                        }
                        PassRequest::Items(cli, vault) => {
                            let items = cli.items(&vault).map_err(PassFailure::from);
                            PassResponse::Items(vault, items)
                        }
                        PassRequest::LoadAgent(cli, vault) => {
                            let result = AgentManager::load_into_existing(&cli, &vault)
                                .map_err(PassFailure::from);
                            PassResponse::LoadAgent(vault, result)
                        }
                        PassRequest::SaveLogin {
                            cli,
                            vault,
                            draft,
                            connection,
                            replace,
                        } => {
                            let item = draft.title.clone();
                            let result = if replace {
                                cli.set_login_password(&vault, &item, &draft.password)
                            } else {
                                cli.create_login(&vault, &draft)
                            }
                            .map_err(PassFailure::from);
                            // `draft` meurt ici: le mot de passe est efface
                            // avant meme que la reponse ne parte.
                            drop(draft);
                            PassResponse::SavedLogin {
                                connection,
                                vault,
                                item,
                                result,
                            }
                        }
                        PassRequest::SaveSshKey {
                            cli,
                            vault,
                            title,
                            source,
                            connection,
                        } => {
                            let result = match &source {
                                SshKeySource::Import(path) => {
                                    cli.import_ssh_key(&vault, &title, path)
                                }
                                SshKeySource::Generate { key_type, comment } => {
                                    cli.generate_ssh_key(&vault, &title, *key_type, comment)
                                }
                            }
                            .map_err(PassFailure::from);
                            PassResponse::SavedSshKey {
                                connection,
                                vault,
                                item: title,
                                result,
                            }
                        }
                    };
                    if res_tx.send(response).is_err() {
                        break; // l'interface a ferme, plus personne n'ecoute
                    }
                }
            })
            .expect("le thread Proton Pass doit demarrer");

        Self {
            tx: req_tx,
            rx: res_rx,
            pending: 0,
        }
    }

    pub fn send(&mut self, request: PassRequest) {
        if self.tx.send(request).is_ok() {
            self.pending += 1;
        }
    }

    /// Recupere une reponse sans bloquer.
    pub fn try_recv(&mut self) -> Option<PassResponse> {
        match self.rx.try_recv() {
            Ok(response) => {
                self.pending = self.pending.saturating_sub(1);
                Some(response)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = 0;
                None
            }
        }
    }

    /// Vrai tant qu'un appel est en cours (sert a afficher un indicateur et a
    /// programmer un rafraichissement de l'interface).
    pub fn is_busy(&self) -> bool {
        self.pending > 0
    }
}

/// Ecrit un script `SSH_ASKPASS` qui delegue la lecture du mot de passe a
/// `pass-cli`.
///
/// Le secret ne transite ainsi ni par la memoire de sshpass-gui ni par le PTY:
/// `ssh` execute lui-meme le script et lit sa sortie. Le fichier ne contient
/// que l'URI `pass://`, jamais la valeur.
pub fn write_askpass_script(binary: &str, uri: &str, id: &str) -> std::io::Result<PathBuf> {
    let dir = runtime_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("askpass-{id}.sh"));
    let script = format!(
        "#!/bin/sh\n# Genere par sshpass-gui. Ne contient aucun secret, seulement une reference.\nexec {} item view {}\n",
        shell_quote(binary),
        shell_quote(uri),
    );
    std::fs::write(&path, script)?;
    set_executable(&path)?;
    Ok(path)
}

fn set_executable(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // 0o700: le script est lisible et executable par le seul proprietaire.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Entoure une valeur de guillemets simples pour un shell POSIX.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_neutralizes_injection() {
        assert_eq!(shell_quote("simple"), "'simple'");
        assert_eq!(shell_quote("a'; rm -rf /"), r#"'a'\''; rm -rf /'"#);
        assert_eq!(shell_quote("$(whoami)"), "'$(whoami)'");
    }

    #[test]
    fn askpass_script_contains_uri_not_secret() {
        let dir = std::env::temp_dir().join(format!("sshpass-gui-test-{}", std::process::id()));
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        let path = write_askpass_script("pass-cli", "pass://Vault/Item/password", "test")
            .expect("ecriture");
        let content = std::fs::read_to_string(&path).expect("lecture");
        assert!(content.starts_with("#!/bin/sh"));
        assert!(content.contains("'pass://Vault/Item/password'"));
        assert!(content.contains("item view"));
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700, "le script doit rester prive");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
