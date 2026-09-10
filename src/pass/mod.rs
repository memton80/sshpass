//! Integration Proton Pass: client `pass-cli`, agents SSH et pont askpass.

pub mod agent;
pub mod cli;
pub mod login;
pub mod secret;

/// Aides partagees par les tests des sous-modules.
///
/// Les faux `pass-cli` sont des scripts que les tests ecrivent puis executent.
/// Or un binaire de test est multi-thread: si un autre test forke pendant
/// qu'on ecrit le script, son fils herite du descripteur d'ecriture encore
/// ouvert, et le noyau refuse d'executer un fichier ouvert en ecriture —
/// `ETXTBSY`, « Text file busy » — tant que ce fils n'a pas exec ou quitte.
///
/// La course est inherente a l'ecriture d'un executable depuis un processus
/// multi-thread: le descripteur est deja duplique quand `write` rend la main.
/// On ne peut donc pas l'eviter, seulement laisser passer l'orage.
#[cfg(all(test, unix))]
pub(crate) mod testing {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::cli::PassError;

    /// Delai au-dela duquel un « Text file busy » n'est plus une course mais
    /// un vrai probleme.
    const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

    /// Ecrit un faux `pass-cli` executable par son seul proprietaire.
    pub fn write_stub(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("ecriture");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    }

    /// Rejoue l'appel tant que le script n'est pas encore executable.
    pub fn unhurried<T>(mut attempt: impl FnMut() -> Result<T, PassError>) -> Result<T, PassError> {
        let deadline = Instant::now() + BUSY_TIMEOUT;
        loop {
            match attempt() {
                Err(PassError::Io(err))
                    if err.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                outcome => return outcome,
            }
        }
    }
}

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

pub use agent::{AgentManager, AgentState};
pub use cli::{
    Item, LoginDraft, PassCli, PassFailure, Probe, Session, SshKeySource, SshKeyType, Vault,
};
pub use login::{LoginManager, LoginOutcome};
pub use secret::Secret;

use crate::config::secure_runtime_dir;

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
        /// Autorise le repli `--field password=…` si `pass-cli` ne sait pas
        /// lire un gabarit sur son entree standard (cf. `set_login_password`).
        allow_argv_fallback: bool,
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
                            allow_argv_fallback,
                        } => {
                            let item = draft.title.clone();
                            let result = if replace {
                                cli.set_login_password(
                                    &vault,
                                    &item,
                                    &draft.password,
                                    allow_argv_fallback,
                                )
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
///
/// `id` identifie **une session terminal**, pas une connexion: deux onglets
/// ouverts sur la meme connexion doivent avoir chacun leur script. Sinon le
/// second reecrit le fichier du premier, et une demande de mot de passe
/// tardive du premier onglet servirait la reference du second — c'est-a-dire,
/// selon les cas, le secret d'une autre machine.
///
/// `allow_temp_fallback` est relaye a `secure_runtime_dir`: sans
/// `XDG_RUNTIME_DIR`, ecrire un askpass dans `/tmp` expose le script — et donc
/// la reference — a tous les comptes de la machine.
pub fn write_askpass_script(
    binary: &str,
    uri: &str,
    id: &str,
    allow_temp_fallback: bool,
) -> std::io::Result<PathBuf> {
    let dir = secure_runtime_dir(allow_temp_fallback)?;
    let path = dir.join(format!("askpass-{id}.sh"));
    let script = format!(
        "{}exec {} item view {}\n",
        ASKPASS_PREAMBLE,
        shell_quote(binary),
        shell_quote(uri)
    );
    write_private_executable(&path, script.as_bytes())?;
    Ok(path)
}

/// En-tete du script askpass: le garde-fou qui decide si l'on repond.
///
/// `ssh` passe le texte de l'invite en premier argument. Deux invites peuvent
/// arriver ici, et une seule merite le secret du coffre:
///
/// * `alex@hote's password:` — l'invite du mot de passe, redigee par `ssh`
///   lui-meme. C'est celle qu'on sert.
/// * tout le reste — la confirmation d'empreinte d'hote
///   (`Are you sure you want to continue connecting`), et surtout les
///   questions de `keyboard-interactive`, **redigees par le serveur**. Un
///   serveur hostile n'aurait qu'a demander `Password:` a sa facon, ou
///   `Entrez le mot de passe de la base:`, pour que le pont lui serve le
///   secret. On refuse, et on le dit sur la sortie d'erreur, que `ssh` recopie
///   dans le terminal.
///
/// Le motif colle donc a la forme exacte que `ssh` fabrique — `%s@%s's
/// password: ` — et non a un vague « contient le mot password ». C'est ce qui
/// separe `alex@hote's password: ` de `Enter your database password:`.
/// `ssh` ne traduit pas ses invites: le motif n'a pas a suivre la locale.
///
/// Ce garde-fou est une **seconde ligne**: la premiere est
/// `PreferredAuthentications=password`, qui empeche `keyboard-interactive`
/// d'etre negocie (cf. `term::command::build_ssh`). Un serveur ne peut donc
/// pas, en pratique, choisir le texte qui arrive ici.
const ASKPASS_PREAMBLE: &str = "\
#!/bin/sh
# Genere par sshpass-gui. Ne contient aucun secret, seulement une reference.
# Le secret n'est servi qu'a l'invite de mot de passe redigee par ssh lui-meme
# (`utilisateur@hote's password: `). Toute autre question — confirmation
# d'empreinte, challenge keyboard-interactive ecrit par le serveur — est
# refusee sans reponse.
case \"$1\" in
  *\"'s password: \"|*\"'s password:\") ;;
  *)
    printf '%s\\n' \"sshpass-gui: invite inattendue, aucun secret fourni: $1\" >&2
    exit 1
    ;;
esac
";

/// Cree un fichier prive et executable, sans jamais suivre ce qui existe deja.
///
/// `create_new` demande `O_CREAT | O_EXCL`: si le chemin existe — fichier
/// ordinaire ou lien symbolique pose par un voisin — l'ouverture echoue au
/// lieu d'ecrire a travers. `mode(0o700)` fixe les droits **a la creation**:
/// un `set_permissions` apres coup laisserait une fenetre ou le fichier est
/// lisible par tout le monde.
fn write_private_executable(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(path)?;
    file.write_all(contents)?;
    file.sync_all()
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

    /// Isole `XDG_RUNTIME_DIR` le temps d'un test et rend le repertoire.
    fn with_runtime_dir<R>(name: &str, body: impl FnOnce() -> R) -> R {
        let _guard = crate::config::env_guard();
        let dir = std::env::temp_dir().join(format!(
            "sshpass-gui-test-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        let outcome = body();
        match previous {
            Some(previous) => std::env::set_var("XDG_RUNTIME_DIR", previous),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        outcome
    }

    #[test]
    fn askpass_script_contains_uri_not_secret() {
        with_runtime_dir("askpass", || {
            let path =
                write_askpass_script("pass-cli", "pass://Vault/Item/password", "test", false)
                    .expect("ecriture");
            let content = std::fs::read_to_string(&path).expect("lecture");
            assert!(content.starts_with("#!/bin/sh"));
            assert!(content.contains("'pass://Vault/Item/password'"));
            assert!(content.contains("item view"));
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700, "le script doit rester prive");
        });
    }

    #[test]
    fn each_session_gets_its_own_askpass_script() {
        with_runtime_dir("sessions", || {
            let first = write_askpass_script("pass-cli", "pass://V/prod-1/password", "s1", false)
                .expect("ecriture");
            let second = write_askpass_script("pass-cli", "pass://V/prod-2/password", "s2", false)
                .expect("ecriture");
            assert_ne!(first, second, "deux sessions partagent le meme fichier");
            // Le premier script n'a pas ete reecrit par le second.
            assert!(std::fs::read_to_string(&first)
                .expect("lecture")
                .contains("prod-1"));
            assert!(std::fs::read_to_string(&second)
                .expect("lecture")
                .contains("prod-2"));
        });
    }

    #[test]
    fn askpass_refuses_to_overwrite_an_existing_path() {
        with_runtime_dir("exclusif", || {
            write_askpass_script("pass-cli", "pass://V/I/password", "meme-id", false)
                .expect("ecriture");
            // Un fichier — ou un lien symbolique — deja en place n'est jamais
            // traverse: l'ouverture echoue au lieu d'ecrire a travers.
            let err = write_askpass_script("pass-cli", "pass://V/I/password", "meme-id", false)
                .expect_err("l'ecriture doit echouer");
            assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        });
    }

    /// Le garde-fou du script est du shell: on l'execute pour de vrai.
    #[cfg(unix)]
    #[test]
    fn askpass_answers_the_password_prompt_only() {
        with_runtime_dir("invites", || {
            // `echo` remplace `pass-cli`: le script doit l'atteindre — ou non.
            let path = write_askpass_script("echo", "pass://V/I/password", "garde", false)
                .expect("ecriture");
            let ask = |prompt: &str| {
                testing::unhurried(|| {
                    std::process::Command::new(&path)
                        .arg(prompt)
                        .output()
                        .map_err(crate::pass::cli::PassError::Io)
                })
                .expect("execution")
            };

            // L'invite emise par ssh lui-meme: on repond.
            let served = ask("alex@example.com's password: ");
            assert!(served.status.success());
            assert!(String::from_utf8_lossy(&served.stdout).contains("item view"));

            // Une question de keyboard-interactive, redigee par le serveur.
            for hostile in [
                "Enter your database password:",
                "OTP:",
                "Verification code:",
                "Are you sure you want to continue connecting (yes/no)?",
            ] {
                let refused = ask(hostile);
                assert!(
                    !refused.status.success(),
                    "invite servie alors qu'elle vient du serveur: {hostile}"
                );
                assert!(String::from_utf8_lossy(&refused.stdout).trim().is_empty());
                assert!(String::from_utf8_lossy(&refused.stderr).contains("invite inattendue"));
            }
        });
    }
}
