//! Pilotage de `pass-cli` (Proton Pass) en sous-processus.
//!
//! `pass-cli` expose `--output json` sur `vault list`, `item list` et
//! `item view`; c'est ce mode qui est utilise ici. Le schema exact des objets
//! JSON n'etant pas fige dans la documentation publique, les parseurs sont
//! volontairement tolerants: les noms de champs sont normalises (minuscules,
//! sans `_` ni `-`) et plusieurs alias sont acceptes, et un tableau peut etre
//! renvoye directement ou enveloppe dans un objet (`{"vaults": [...]}`).
//!
//! ## Ecriture
//!
//! En plus de la lecture, ce module **cree** des items: un identifiant pour
//! une connexion en mot de passe, une cle SSH importee ou generee. Regle de
//! transmission des secrets:
//!
//! * creation d'un identifiant — le mot de passe part par **stdin**
//!   (`item create login --from-template -`), jamais par la ligne de commande,
//!   donc jamais visible dans `/proc/<pid>/cmdline`;
//! * cle SSH — sshpass-gui ne voit jamais la cle: `import` recoit un
//!   **chemin**, `generate` fait tout le travail cote `pass-cli`;
//! * mise a jour d'un mot de passe existant — le gabarit part lui aussi par
//!   **stdin** (`item update --from-template -`). Si la version installee ne
//!   connait pas cette forme, le seul autre chemin documente est
//!   `--field cle=valeur`, donc la ligne de commande, donc `/proc/<pid>/cmdline`
//!   — lisible par **tous** les comptes de la machine. Ce repli est refuse
//!   sauf autorisation explicite (`security.allow_argv_fallback`).
//!
//! Tout ce qui ressort de `pass-cli` — sortie standard resumee, sortie
//! d'erreur — traverse `secret::sanitize_external_output` avant d'atteindre
//! l'interface ou le journal.
//!
//! ## Session
//!
//! Toutes les commandes ci-dessus supposent une session Proton Pass ouverte.
//! Elle ne l'est pas eternellement: elle expire, et l'arret de la machine y met
//! fin. `session()` la sonde avec `pass-cli info`, et distingue trois cas —
//! ouverte, fermee (une reconnexion suffit, cf. `pass::login`), ou verrouillee
//! par un code (`pass-cli session unlock`, qui exige une saisie humaine).

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::pass::secret::{redact, sanitize_external_output, Secret};

/// Delai au-dela duquel un appel `pass-cli` est considere comme bloque
/// (session verrouillee attendant une saisie, reseau coupe...).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, thiserror::Error)]
pub enum PassError {
    #[error("`{0}` est introuvable: installez Proton Pass CLI ou corrigez le chemin du binaire")]
    NotFound(String),
    #[error("`{command}` a echoue (code {code}): {stderr}")]
    Command {
        command: String,
        code: String,
        stderr: String,
    },
    #[error("`{0}` ne repond pas (delai depasse)")]
    Timeout(String),
    #[error("sortie JSON illisible: {0}")]
    Json(String),
    #[error("{0}")]
    Invalid(String),
    #[error("erreur d'entree/sortie: {0}")]
    Io(#[from] std::io::Error),
}

impl PassError {
    /// Texte le plus parlant de l'erreur: la sortie d'erreur de `pass-cli`
    /// quand il en a produit une, sinon le message complet.
    pub fn detail(&self) -> String {
        match self {
            PassError::Command { stderr, .. } if !stderr.is_empty() => stderr.clone(),
            other => other.to_string(),
        }
    }

    /// Vrai si l'echec vient d'une session fermee ou expiree.
    ///
    /// Seule la sortie d'erreur est examinee, jamais la commande: `item create
    /// login` contient « login » sans rien dire de la session.
    pub fn is_session_closed(&self) -> bool {
        match self {
            PassError::Command { stderr, .. } => {
                !mentions(stderr, &LOCKED_SESSION_PHRASES)
                    && mentions(stderr, &CLOSED_SESSION_PHRASES)
            }
            _ => false,
        }
    }

    /// Vrai si l'echec vient d'une session verrouillee par un code.
    pub fn is_session_locked(&self) -> bool {
        match self {
            PassError::Command { stderr, .. } => mentions(stderr, &LOCKED_SESSION_PHRASES),
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, PassError>;

/// Un coffre Proton Pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    /// Share id du coffre, utilisable a la place du nom.
    pub id: String,
    pub name: String,
    pub item_count: Option<u64>,
}

/// Type d'item, seul `SshKey` et `Login` nous interessent vraiment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    SshKey,
    Login,
    Note,
    Alias,
    CreditCard,
    Other(String),
}

impl ItemKind {
    fn parse(raw: &str) -> Self {
        match normalize_key(raw).as_str() {
            "sshkey" | "ssh" => ItemKind::SshKey,
            "login" | "credentials" => ItemKind::Login,
            "note" | "securenote" => ItemKind::Note,
            "alias" => ItemKind::Alias,
            "creditcard" | "card" => ItemKind::CreditCard,
            _ => ItemKind::Other(raw.to_string()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ItemKind::SshKey => "Cle SSH",
            ItemKind::Login => "Identifiant",
            ItemKind::Note => "Note",
            ItemKind::Alias => "Alias",
            ItemKind::CreditCard => "Carte",
            ItemKind::Other(raw) => raw,
        }
    }

    pub fn is_ssh_key(&self) -> bool {
        matches!(self, ItemKind::SshKey)
    }
}

/// Un item d'un coffre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub kind: ItemKind,
    pub username: Option<String>,
}

impl Item {
    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        self.title.to_lowercase().contains(&needle)
            || self
                .username
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&needle)
    }
}

/// Ce qu'il faut ecrire dans un coffre pour qu'un identifiant y soit
/// exploitable: un titre, de quoi reconnaitre le compte, et le secret.
///
/// Le champ `url` sert a ce que l'item soit **rempli comme il faut** cote
/// Proton Pass — l'application y affiche le service concerne au lieu d'un
/// titre nu.
#[derive(Debug, Default)]
pub struct LoginDraft {
    pub title: String,
    pub username: String,
    pub url: String,
    pub password: Secret,
}

impl LoginDraft {
    /// Brouillon pour une connexion SSH: l'URL `ssh://user@hote:port` fait le
    /// lien entre l'item du coffre et la machine.
    pub fn for_ssh(title: &str, user: &str, host: &str, port: u16, password: Secret) -> Self {
        let host = host.trim();
        let user = user.trim();
        let url = if host.is_empty() {
            String::new()
        } else if user.is_empty() {
            format!("ssh://{host}:{port}")
        } else {
            format!("ssh://{user}@{host}:{port}")
        };
        Self {
            title: title.trim().to_string(),
            username: user.to_string(),
            url,
            password,
        }
    }

    /// Gabarit JSON attendu par `item create login --from-template`.
    ///
    /// Les champs vides sont omis plutot qu'envoyes vides: un `username` vide
    /// afficherait une ligne inutile dans Proton Pass.
    pub fn template(&self) -> Vec<u8> {
        let mut object = serde_json::Map::new();
        object.insert("title".into(), Value::String(self.title.trim().to_string()));
        if !self.username.trim().is_empty() {
            object.insert(
                "username".into(),
                Value::String(self.username.trim().to_string()),
            );
        }
        object.insert(
            "password".into(),
            Value::String(self.password.expose().to_string()),
        );
        if !self.url.trim().is_empty() {
            object.insert(
                "urls".into(),
                Value::Array(vec![Value::String(self.url.trim().to_string())]),
            );
        }
        serde_json::to_vec(&Value::Object(object)).unwrap_or_default()
    }
}

/// Algorithmes acceptes par `item create ssh-key generate`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SshKeyType {
    #[default]
    Ed25519,
    Rsa2048,
    Rsa4096,
}

impl SshKeyType {
    pub const ALL: [SshKeyType; 3] = [
        SshKeyType::Ed25519,
        SshKeyType::Rsa2048,
        SshKeyType::Rsa4096,
    ];

    /// Valeur passee a `--key-type`.
    pub fn flag(self) -> &'static str {
        match self {
            SshKeyType::Ed25519 => "ed25519",
            SshKeyType::Rsa2048 => "rsa2048",
            SshKeyType::Rsa4096 => "rsa4096",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SshKeyType::Ed25519 => "Ed25519",
            SshKeyType::Rsa2048 => "RSA 2048",
            SshKeyType::Rsa4096 => "RSA 4096",
        }
    }
}

/// D'ou vient la cle SSH qu'on range dans le coffre.
#[derive(Debug, Clone)]
pub enum SshKeySource {
    /// Un fichier de cle privee deja sur le disque.
    Import(std::path::PathBuf),
    /// Une paire generee par Proton Pass; le commentaire aide a la reconnaitre.
    Generate {
        key_type: SshKeyType,
        comment: String,
    },
}

/// Verifie qu'une valeur obligatoire est renseignee.
fn require(value: &str, message: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(PassError::Invalid(message.to_string()))
    } else {
        Ok(value.to_string())
    }
}

/// Premiere ligne utile de la sortie de `pass-cli`, ou un repli.
///
/// `item create` affiche un recapitulatif dont le format n'est pas fige; on
/// n'en montre que la premiere ligne, et on retombe sur notre propre phrase
/// si la commande est restee muette.
fn summarize(output: &str, fallback: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        // Cette ligne vient d'un programme externe et part droit dans une
        // notification: elle passe par le filtre commun.
        .map(sanitize_external_output)
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// Etat de la session Proton Pass, tel que `pass-cli info` le laisse voir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    /// Session utilisable. `account` est l'adresse ou le nom rapporte, s'il y
    /// en a un — la sortie de `info` n'est pas un format fige.
    Open { account: String },
    /// Session absente ou expiree: `pass-cli login` la retablit.
    Closed(String),
    /// Session authentifiee mais verrouillee par un code. Rien d'automatique
    /// n'est possible: `session unlock` reclame une saisie.
    Locked(String),
}

/// Resultat d'une detection complete: le binaire et l'etat de sa session.
#[derive(Debug, Clone)]
pub struct Probe {
    pub version: String,
    pub session: Session,
}

/// Echec deja mis en forme pour l'interface, avec ce qu'elle doit en deduire.
///
/// Le booleen evite que chaque appelant ait a refaire l'analyse du texte
/// d'erreur pour savoir si la session est en cause.
#[derive(Debug, Clone)]
pub struct PassFailure {
    pub message: String,
    /// La session est fermee: une reconnexion reglerait le probleme.
    pub session_closed: bool,
}

impl From<PassError> for PassFailure {
    fn from(err: PassError) -> Self {
        Self {
            session_closed: err.is_session_closed(),
            message: err.to_string(),
        }
    }
}

impl std::fmt::Display for PassFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Tournures par lesquelles `pass-cli` signale une session absente ou expiree.
///
/// Aucun code de sortie ne distingue les motifs d'echec: seul le texte le
/// fait. La liste est volontairement large, dans le meme esprit que les
/// analyseurs JSON — mieux vaut proposer une reconnexion de trop qu'aucune.
const CLOSED_SESSION_PHRASES: [&str; 10] = [
    "not logged in",
    "no session",
    "no active session",
    "session expired",
    "session not found",
    "invalid session",
    "not authenticated",
    "authentication required",
    "please log in",
    "unauthorized",
];

/// Tournures d'une session verrouillee. Testees en premier: un message de
/// verrouillage peut lui aussi parler d'autorisation.
const LOCKED_SESSION_PHRASES: [&str; 3] = ["session is locked", "session locked", "unlock"];

/// Tournures par lesquelles un analyseur d'arguments dit qu'il ne connait pas
/// une option. Elles couvrent `clap`, `cobra`, `getopt` et les messages faits
/// main: `pass-cli` peut changer de bibliotheque sans nous prevenir.
const UNKNOWN_FLAG_PHRASES: [&str; 8] = [
    "unknown flag",
    "unknown option",
    "unrecognized",
    "unrecognised",
    "unexpected argument",
    "invalid option",
    "no such option",
    "illegal option",
];

fn mentions(haystack: &str, phrases: &[&str]) -> bool {
    let haystack = haystack.to_lowercase();
    phrases.iter().any(|phrase| haystack.contains(phrase))
}

/// Vrai si l'echec vient de ce que `pass-cli` ne connait pas l'option, et non
/// de ce qu'il a essaye et rate.
///
/// La distinction decide si l'on a le droit de se replier sur la ligne de
/// commande (cf. `set_login_password`): un coffre introuvable ne justifie pas
/// d'exposer le mot de passe une seconde fois.
fn mentions_unknown_flag(err: &PassError) -> bool {
    match err {
        PassError::Command { stderr, .. } => mentions(stderr, &UNKNOWN_FLAG_PHRASES),
        _ => false,
    }
}

/// Client `pass-cli`.
#[derive(Debug, Clone)]
pub struct PassCli {
    binary: String,
    timeout: Duration,
}

impl PassCli {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }

    /// Version rapportee par `pass-cli --version`; sert de test de presence.
    pub fn version(&self) -> Result<String> {
        let out = self.run(&["--version".into()])?;
        Ok(out.trim().to_string())
    }

    /// Detection complete: le binaire repond, et sa session est-elle ouverte.
    pub fn probe(&self) -> Result<Probe> {
        Ok(Probe {
            version: self.version()?,
            session: self.session()?,
        })
    }

    /// Etat de la session, lu avec `pass-cli info`.
    ///
    /// Un `info` qui echoue est la situation **normale** quand la session est
    /// fermee: ce n'est donc pas une erreur, mais un `Session::Closed` portant
    /// les mots de `pass-cli`. Seuls les echecs qui ne disent rien de la
    /// session — binaire introuvable, delai depasse, entree/sortie —
    /// remontent en `Err`, car une reconnexion n'y changerait rien.
    pub fn session(&self) -> Result<Session> {
        match self.run(&["info".into()]) {
            Ok(out) => Ok(Session::Open {
                account: parse_account(&out),
            }),
            Err(err @ PassError::Command { .. }) => {
                let detail = err.detail();
                if err.is_session_locked() {
                    Ok(Session::Locked(detail))
                } else {
                    Ok(Session::Closed(detail))
                }
            }
            Err(other) => Err(other),
        }
    }

    /// `pass-cli vault list --output json`
    pub fn vaults(&self) -> Result<Vec<Vault>> {
        let out = self.run(&[
            "vault".into(),
            "list".into(),
            "--output".into(),
            "json".into(),
        ])?;
        parse_vaults(&out)
    }

    /// `pass-cli item list --vault-name <vault> --output json`
    pub fn items(&self, vault: &str) -> Result<Vec<Item>> {
        let mut args = vec!["item".to_string(), "list".to_string()];
        if !vault.is_empty() {
            args.push("--vault-name".to_string());
            args.push(vault.to_string());
        }
        args.push("--output".to_string());
        args.push("json".to_string());
        let out = self.run(&args)?;
        parse_items(&out)
    }

    // Note: aucune methode ne **lit** un secret. Les mots de passe sont fournis
    // a `ssh` par le script SSH_ASKPASS (cf. `pass::write_askpass_script`), qui
    // execute `pass-cli` lui-meme: la valeur ne remonte jamais dans sshpass-gui.
    // Les methodes d'ecriture ci-dessous font le chemin inverse, une seule fois
    // et sans retour.

    /// Cree un identifiant dans un coffre.
    ///
    /// `pass-cli item create login --vault-name <coffre> --from-template -`
    ///
    /// Le brouillon est serialise en JSON et pousse dans **stdin**: le mot de
    /// passe n'apparait donc pas dans la ligne de commande, la ou n'importe
    /// quel processus de la machine pourrait le lire.
    pub fn create_login(&self, vault: &str, draft: &LoginDraft) -> Result<String> {
        let title = require(&draft.title, "Le titre de l'item est obligatoire.")?;
        let vault = require(vault, "Choisissez un coffre Proton Pass.")?;
        if draft.password.is_empty() {
            return Err(PassError::Invalid("Le mot de passe est vide.".into()));
        }

        let args = vec![
            "item".to_string(),
            "create".to_string(),
            "login".to_string(),
            "--vault-name".to_string(),
            vault,
            "--from-template".to_string(),
            "-".to_string(),
        ];
        let out = self.run_with_input(&args, Some(draft.template()))?;
        Ok(summarize(&out, &format!("« {title} » cree dans le coffre")))
    }

    /// Remplace le mot de passe d'un item existant.
    ///
    /// Deux chemins, essayes dans cet ordre:
    ///
    /// 1. `item update --from-template -`, ou le gabarit JSON part par
    ///    **stdin**, comme a la creation. Rien du secret n'apparait alors dans
    ///    `/proc/<pid>/cmdline`.
    /// 2. `item update --field password=…`, ou le secret est un argument.
    ///
    /// Le second est un **repli**, et il n'est pris que si la version de
    /// `pass-cli` installee ne connait pas `--from-template` sur `update`, et
    /// seulement si `allow_argv_fallback` l'autorise. `/proc/<pid>/cmdline`
    /// est lisible par tous les comptes de la machine, pas seulement par le
    /// notre: exposer le mot de passe la doit rester un choix explicite. Sans
    /// cette permission, l'echec est franc et son message dit quoi faire.
    pub fn set_login_password(
        &self,
        vault: &str,
        item: &str,
        password: &Secret,
        allow_argv_fallback: bool,
    ) -> Result<String> {
        let item = require(item, "Le titre de l'item est obligatoire.")?;
        let vault = require(vault, "Choisissez un coffre Proton Pass.")?;
        if password.is_empty() {
            return Err(PassError::Invalid("Le mot de passe est vide.".into()));
        }
        let fallback = format!("mot de passe de « {item} » mis a jour");

        let template = vec![
            "item".to_string(),
            "update".to_string(),
            "--vault-name".to_string(),
            vault.clone(),
            "--item-title".to_string(),
            item.clone(),
            "--from-template".to_string(),
            "-".to_string(),
        ];
        let payload = serde_json::to_vec(&serde_json::json!({
            "password": password.expose(),
        }))
        .unwrap_or_default();

        match self.run_with_input(&template, Some(payload)) {
            Ok(out) => return Ok(summarize(&out, &fallback)),
            // La commande existe et a echoue pour une vraie raison (coffre
            // inconnu, session fermee): le repli n'y changerait rien, et il
            // exposerait le secret pour le meme echec.
            Err(err) if !mentions_unknown_flag(&err) => return Err(err),
            Err(err) => {
                if !allow_argv_fallback {
                    return Err(PassError::Invalid(format!(
                        "Cette version de `pass-cli` ne sait pas mettre un item a jour \
                         depuis son entree standard ({}). Le seul autre chemin place le \
                         mot de passe dans la ligne de commande, lisible par tous les \
                         comptes de la machine via /proc. Modifiez l'item depuis Proton \
                         Pass, ou activez « Mot de passe en ligne de commande » dans les \
                         reglages.",
                        err.detail()
                    )));
                }
                log::warn!(
                    "`item update --from-template` indisponible, repli sur --field: {}",
                    err.detail()
                );
            }
        }

        let args = vec![
            "item".to_string(),
            "update".to_string(),
            "--vault-name".to_string(),
            vault,
            "--item-title".to_string(),
            item,
            "--field".to_string(),
            format!("password={}", password.expose()),
        ];
        let out = self.run(&args)?;
        Ok(summarize(&out, &fallback))
    }

    /// Importe une cle privee existante dans un coffre.
    ///
    /// `pass-cli item create ssh-key import --from-private-key <chemin> …`
    ///
    /// sshpass-gui ne lit pas le fichier: il ne transmet qu'un chemin, et
    /// `pass-cli` s'occupe du reste.
    pub fn import_ssh_key(&self, vault: &str, title: &str, key_file: &Path) -> Result<String> {
        let title = require(title, "Le titre de l'item est obligatoire.")?;
        let vault = require(vault, "Choisissez un coffre Proton Pass.")?;
        if !key_file.exists() {
            return Err(PassError::Invalid(format!(
                "Fichier de cle introuvable: {}",
                key_file.display()
            )));
        }

        let args = vec![
            "item".to_string(),
            "create".to_string(),
            "ssh-key".to_string(),
            "import".to_string(),
            "--vault-name".to_string(),
            vault,
            "--title".to_string(),
            title.clone(),
            "--from-private-key".to_string(),
            key_file.to_string_lossy().into_owned(),
        ];
        let out = self.run(&args)?;
        Ok(summarize(
            &out,
            &format!("« {title} » importee dans le coffre"),
        ))
    }

    /// Fait generer une paire de cles par Proton Pass.
    ///
    /// `pass-cli item create ssh-key generate --key-type <type> …`
    ///
    /// La cle privee nait dans le coffre et n'en sort pas: elle ne touche ni
    /// le disque local ni la memoire de sshpass-gui.
    pub fn generate_ssh_key(
        &self,
        vault: &str,
        title: &str,
        key_type: SshKeyType,
        comment: &str,
    ) -> Result<String> {
        let title = require(title, "Le titre de l'item est obligatoire.")?;
        let vault = require(vault, "Choisissez un coffre Proton Pass.")?;

        let mut args = vec![
            "item".to_string(),
            "create".to_string(),
            "ssh-key".to_string(),
            "generate".to_string(),
            "--vault-name".to_string(),
            vault,
            "--title".to_string(),
            title.clone(),
            "--key-type".to_string(),
            key_type.flag().to_string(),
        ];
        let comment = comment.trim();
        if !comment.is_empty() {
            args.push("--comment".to_string());
            args.push(comment.to_string());
        }
        let out = self.run(&args)?;
        Ok(summarize(
            &out,
            &format!("cle {} « {title} » generee", key_type.label()),
        ))
    }

    /// Execute `pass-cli` et renvoie sa sortie standard.
    fn run(&self, args: &[String]) -> Result<String> {
        self.run_with_input(args, None)
    }

    /// Variante avec une entree standard: `input` est ecrit dans le tube puis
    /// efface, et le tube ferme pour signaler la fin des donnees.
    fn run_with_input(&self, args: &[String], input: Option<Vec<u8>>) -> Result<String> {
        // Les arguments sont expurges: `item update` en porte un qui contient
        // un mot de passe, et cette chaine finit dans les messages d'erreur.
        let display = format!(
            "{} {}",
            self.binary,
            args.iter().map(|a| redact(a)).collect::<Vec<_>>().join(" ")
        );
        let mut child = match Command::new(&self.binary)
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(PassError::NotFound(self.binary.clone()))
            }
            Err(err) => return Err(PassError::Io(err)),
        };

        // L'ecriture part dans un thread: `pass-cli` peut ne lire son entree
        // qu'apres avoir deverrouille la session, et un `write_all` direct
        // bloquerait le compte a rebours du delai.
        let mut stdin_thread = match (input, child.stdin.take()) {
            (Some(mut bytes), Some(mut pipe)) => Some(std::thread::spawn(move || {
                let _ = pipe.write_all(&bytes);
                let _ = pipe.flush();
                // Fermeture explicite: sans EOF, `pass-cli` attendrait la suite.
                drop(pipe);
                bytes.fill(0);
            })),
            // Pas de tube alors qu'on avait des donnees: on les efface quand meme.
            (Some(mut bytes), None) => {
                bytes.fill(0);
                None
            }
            (None, _) => None,
        };

        // La sortie est drainee dans des threads dedies: sans cela un binaire
        // bavard remplirait le tube et se bloquerait avant d'avoir fini.
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let out_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stdout.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        });
        let err_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(pipe) = stderr.as_mut() {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        });

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait()? {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Le tube est ferme par la mort du processus: l'ecriture
                    // rend la main sur EPIPE, la jointure ne peut pas coincer.
                    if let Some(handle) = stdin_thread.take() {
                        let _ = handle.join();
                    }
                    return Err(PassError::Timeout(display));
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };

        if let Some(handle) = stdin_thread {
            let _ = handle.join();
        }
        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();

        if !status.success() {
            return Err(PassError::Command {
                command: display,
                code: status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into()),
                // La sortie d'erreur d'un programme externe finit dans une
                // notification et dans le journal: elle est expurgee et bornee
                // ici, une bonne fois, plutot qu'a chaque point d'affichage.
                stderr: sanitize_external_output(String::from_utf8_lossy(&stderr).trim()),
            });
        }
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }
}

/// Compte rapporte par `pass-cli info`.
///
/// La sortie est faite pour etre lue par un humain (`- Email: x@proton.me`) et
/// n'a pas de variante JSON documentee. L'analyse suit donc le meme principe
/// que celle des items: on cherche des cles connues, normalisees, et l'absence
/// de reponse n'est pas une erreur — c'est juste une pastille sans nom.
fn parse_account(output: &str) -> String {
    let mut fallback = String::new();
    for line in output.lines() {
        let line = line.trim().trim_start_matches(['-', '*']).trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match normalize_key(key).as_str() {
            "email" => return value.to_string(),
            // Une session par jeton n'a pas d'adresse: son nom fait l'affaire.
            "username" | "personalaccesstoken" if fallback.is_empty() => {
                fallback = value.to_string()
            }
            _ => {}
        }
    }
    fallback
}

/// Normalise un nom de champ: minuscules, sans separateurs.
/// `share_id`, `shareId` et `Share-ID` donnent tous `shareid`.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Cherche une valeur parmi plusieurs alias de cles, en ignorant la casse et
/// les separateurs.
fn pick<'a>(value: &'a Value, aliases: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    let wanted: Vec<String> = aliases.iter().map(|a| normalize_key(a)).collect();
    object
        .iter()
        .find(|(key, _)| wanted.contains(&normalize_key(key)))
        .map(|(_, value)| value)
}

fn pick_string(value: &Value, aliases: &[&str]) -> Option<String> {
    match pick(value, aliases)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn pick_u64(value: &Value, aliases: &[&str]) -> Option<u64> {
    pick(value, aliases)?.as_u64()
}

/// Extrait le tableau utile d'une sortie JSON, qu'elle soit nue ou enveloppee.
fn extract_array<'a>(root: &'a Value, wrappers: &[&str]) -> Option<&'a Vec<Value>> {
    if let Some(array) = root.as_array() {
        return Some(array);
    }
    let object = root.as_object()?;
    let wanted: Vec<String> = wrappers.iter().map(|w| normalize_key(w)).collect();
    if let Some(array) = object
        .iter()
        .find(|(k, v)| wanted.contains(&normalize_key(k)) && v.is_array())
    {
        return array.1.as_array();
    }
    // Dernier recours: le premier tableau rencontre dans l'objet.
    object.values().find_map(|v| v.as_array())
}

pub fn parse_vaults(json: &str) -> Result<Vec<Vault>> {
    let root: Value = serde_json::from_str(json).map_err(|e| PassError::Json(e.to_string()))?;
    let array = extract_array(&root, &["vaults", "data", "results", "items"])
        .ok_or_else(|| PassError::Json("aucun tableau de coffres dans la sortie".into()))?;

    Ok(array
        .iter()
        .filter_map(|entry| {
            let name = pick_string(entry, &["name", "title", "vaultName", "vault"])?;
            let id = pick_string(entry, &["shareId", "id", "vaultId"]).unwrap_or_default();
            let item_count = pick_u64(entry, &["itemCount", "count", "numItems", "items"]);
            Some(Vault {
                id,
                name,
                item_count,
            })
        })
        .collect())
}

pub fn parse_items(json: &str) -> Result<Vec<Item>> {
    let root: Value = serde_json::from_str(json).map_err(|e| PassError::Json(e.to_string()))?;
    let array = extract_array(&root, &["items", "data", "results"])
        .ok_or_else(|| PassError::Json("aucun tableau d'items dans la sortie".into()))?;

    Ok(array
        .iter()
        .filter_map(|entry| {
            let title = pick_string(entry, &["title", "name", "itemTitle"])?;
            let id = pick_string(entry, &["itemId", "id", "item"]).unwrap_or_default();
            let kind = pick_string(entry, &["type", "itemType", "category", "kind"])
                .map(|raw| ItemKind::parse(&raw))
                .unwrap_or(ItemKind::Other(String::new()));
            let username = pick_string(entry, &["username", "user", "email", "login"]);
            Some(Item {
                id,
                title,
                kind,
                username,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaults_from_bare_array() {
        let json = r#"[
            {"shareId": "abc123", "name": "Personal", "itemCount": 12},
            {"shareId": "def456", "name": "SSH Keys"}
        ]"#;
        let vaults = parse_vaults(json).expect("parse");
        assert_eq!(vaults.len(), 2);
        assert_eq!(vaults[0].name, "Personal");
        assert_eq!(vaults[0].id, "abc123");
        assert_eq!(vaults[0].item_count, Some(12));
        assert_eq!(vaults[1].item_count, None);
    }

    #[test]
    fn vaults_from_wrapped_object_and_snake_case() {
        let json = r#"{"vaults": [{"share_id": "s1", "vault_name": "Prod", "num_items": 3}]}"#;
        let vaults = parse_vaults(json).expect("parse");
        assert_eq!(vaults[0].id, "s1");
        assert_eq!(vaults[0].name, "Prod");
    }

    #[test]
    fn vaults_tolerate_pascal_case() {
        let json = r#"{"Data": [{"ShareID": "s2", "Name": "Shared"}]}"#;
        let vaults = parse_vaults(json).expect("parse");
        assert_eq!(vaults[0].id, "s2");
        assert_eq!(vaults[0].name, "Shared");
    }

    #[test]
    fn items_detect_ssh_keys() {
        let json = r#"[
            {"itemId": "i1", "title": "prod-key", "type": "SshKey"},
            {"itemId": "i2", "title": "GitHub", "type": "login", "username": "alex"},
            {"item_id": "i3", "title": "backup", "item_type": "ssh-key"}
        ]"#;
        let items = parse_items(json).expect("parse");
        assert_eq!(items.len(), 3);
        assert!(items[0].kind.is_ssh_key());
        assert_eq!(items[1].kind, ItemKind::Login);
        assert_eq!(items[1].username.as_deref(), Some("alex"));
        assert!(items[2].kind.is_ssh_key(), "ssh-key doit etre reconnu");
    }

    #[test]
    fn items_without_type_are_kept() {
        let json = r#"{"items": [{"id": "x", "name": "Sans type"}]}"#;
        let items = parse_items(json).expect("parse");
        assert_eq!(items[0].title, "Sans type");
        assert_eq!(items[0].id, "x");
    }

    #[test]
    fn entries_without_title_are_skipped() {
        let json = r#"[{"itemId": "i1"}, {"itemId": "i2", "title": "ok"}]"#;
        let items = parse_items(json).expect("parse");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "ok");
    }

    #[test]
    fn invalid_json_is_reported() {
        assert!(matches!(
            parse_items("pas du json"),
            Err(PassError::Json(_))
        ));
        assert!(matches!(
            parse_vaults(r#"{"nope": 3}"#),
            Err(PassError::Json(_))
        ));
    }

    #[test]
    fn item_search_covers_title_and_username() {
        let item = Item {
            id: "i".into(),
            title: "Serveur Web".into(),
            kind: ItemKind::Login,
            username: Some("root".into()),
        };
        assert!(item.matches("web"));
        assert!(item.matches("ROOT"));
        assert!(!item.matches("mysql"));
    }

    #[test]
    fn missing_binary_is_a_clear_error() {
        let cli = PassCli::new("pass-cli-qui-n-existe-pas");
        match cli.version() {
            Err(PassError::NotFound(bin)) => assert_eq!(bin, "pass-cli-qui-n-existe-pas"),
            other => panic!("attendu NotFound, obtenu {other:?}"),
        }
    }

    // Ces deux tests pilotent un vrai sous-processus via `sh`: ils n'ont de
    // sens que la ou un shell POSIX existe.
    #[cfg(unix)]
    #[test]
    fn command_failure_carries_stderr() {
        let cli = PassCli::new("sh");
        let err = cli
            .run(&["-c".into(), "echo boum >&2; exit 3".into()])
            .expect_err("doit echouer");
        match err {
            PassError::Command { code, stderr, .. } => {
                assert_eq!(code, "3");
                assert_eq!(stderr, "boum");
            }
            other => panic!("attendu Command, obtenu {other:?}"),
        }
    }

    #[test]
    fn account_is_read_from_the_human_output() {
        let output = "- Release track: stable\n- ID: abc\n- Username: alex\n\
                      - Email: alex@proton.me\n- Session has lock: no\n";
        assert_eq!(parse_account(output), "alex@proton.me");
    }

    #[test]
    fn account_falls_back_to_the_username() {
        assert_eq!(parse_account("- Username: alex\n"), "alex");
        // Une session par jeton n'a pas d'adresse.
        assert_eq!(
            parse_account("- Personal Access Token: ci-runner\n"),
            "ci-runner"
        );
        // Rien d'exploitable n'est pas une erreur: la pastille reste muette.
        assert_eq!(parse_account("bonjour\n- Session has lock: yes\n"), "");
    }

    #[test]
    fn closed_and_locked_sessions_are_told_apart() {
        let closed = |stderr: &str| PassError::Command {
            command: "pass-cli info".into(),
            code: "1".into(),
            stderr: stderr.into(),
        };

        assert!(closed("Error: not logged in").is_session_closed());
        assert!(closed("session expired, please log in again").is_session_closed());
        assert!(closed("UNAUTHORIZED").is_session_closed());

        // Une session verrouillee n'est pas une session fermee: relancer
        // `login` ne servirait a rien, il faut le code de deverrouillage.
        let locked = closed("Session is locked. Run `pass-cli session unlock`");
        assert!(locked.is_session_locked());
        assert!(!locked.is_session_closed());

        // Une panne ordinaire ne doit pas declencher de reconnexion.
        assert!(!closed("vault not found").is_session_closed());
        assert!(!PassError::NotFound("pass-cli".into()).is_session_closed());
        assert!(!PassError::Timeout("pass-cli info".into()).is_session_closed());
    }

    #[test]
    fn the_command_line_never_decides_the_session_verdict() {
        // « login » figure dans la commande, pas dans la sortie d'erreur:
        // creer un identifiant qui echoue n'est pas une session fermee.
        let err = PassError::Command {
            command: "pass-cli item create login --vault-name V".into(),
            code: "1".into(),
            stderr: "vault not found".into(),
        };
        assert!(!err.is_session_closed());
        assert!(!err.is_session_locked());
    }

    #[test]
    fn failures_carry_the_session_verdict_to_the_interface() {
        let failure = PassFailure::from(PassError::Command {
            command: "pass-cli vault list".into(),
            code: "1".into(),
            stderr: "not logged in".into(),
        });
        assert!(failure.session_closed);
        assert!(failure.to_string().contains("not logged in"));

        let other = PassFailure::from(PassError::NotFound("pass-cli".into()));
        assert!(!other.session_closed);
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_info_reports_a_closed_session_not_an_error() {
        // `sh -c` sans `info` valide: le faux binaire echoue en disant qu'il
        // n'y a pas de session, exactement comme `pass-cli` deconnecte.
        let cli = PassCli::new("sh");
        let out = cli
            .run(&[
                "-c".into(),
                "echo 'Error: not logged in' >&2; exit 1".into(),
            ])
            .expect_err("doit echouer");
        assert!(out.is_session_closed());
    }

    #[cfg(unix)]
    #[test]
    fn session_probe_maps_the_three_outcomes() {
        let dir = std::env::temp_dir().join(format!("sshpass-gui-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");

        let make = |name: &str, body: &str| {
            let path = dir.join(name);
            crate::pass::testing::write_stub(&path, body);
            PassCli::new(path.to_string_lossy().into_owned())
        };
        // `unhurried` laisse passer un « Text file busy » (cf. `pass::testing`).
        let probe = |cli: PassCli| crate::pass::testing::unhurried(|| cli.session());

        let open = make("open", "echo '- Email: alex@proton.me'");
        assert_eq!(
            probe(open).expect("sonde"),
            Session::Open {
                account: "alex@proton.me".into()
            }
        );

        let closed = make("closed", "echo 'Error: not logged in' >&2; exit 1");
        assert!(matches!(probe(closed), Ok(Session::Closed(_))));

        let locked = make("locked", "echo 'Session is locked' >&2; exit 1");
        assert!(matches!(probe(locked), Ok(Session::Locked(_))));

        // Un binaire absent reste une erreur: aucune reconnexion n'y changerait
        // quoi que ce soit.
        assert!(matches!(
            PassCli::new("pass-cli-qui-n-existe-pas").session(),
            Err(PassError::NotFound(_))
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ssh_login_draft_builds_a_usable_url() {
        let draft = LoginDraft::for_ssh("web-01", "root", "10.0.0.4", 2222, Secret::new("s"));
        assert_eq!(draft.url, "ssh://root@10.0.0.4:2222");
        assert_eq!(draft.username, "root");
        assert_eq!(draft.title, "web-01");

        // Sans utilisateur, l'URL reste valide et pointe la machine.
        let anonymous = LoginDraft::for_ssh("box", "  ", "example.com", 22, Secret::new("s"));
        assert_eq!(anonymous.url, "ssh://example.com:22");

        // Sans hote, pas d'URL inventee.
        let hostless = LoginDraft::for_ssh("box", "root", "", 22, Secret::new("s"));
        assert!(hostless.url.is_empty());
    }

    #[test]
    fn login_template_carries_every_field() {
        let draft = LoginDraft::for_ssh("web-01", "root", "10.0.0.4", 22, Secret::new("hunter2"));
        let json: Value = serde_json::from_slice(&draft.template()).expect("json");
        assert_eq!(json["title"], "web-01");
        assert_eq!(json["username"], "root");
        assert_eq!(json["password"], "hunter2");
        assert_eq!(json["urls"][0], "ssh://root@10.0.0.4:22");
    }

    #[test]
    fn login_template_omits_empty_fields() {
        let draft = LoginDraft {
            title: "Sans rien".into(),
            username: "  ".into(),
            url: String::new(),
            password: Secret::new("x"),
        };
        let json: Value = serde_json::from_slice(&draft.template()).expect("json");
        let object = json.as_object().expect("objet");
        assert!(!object.contains_key("username"), "username vide envoye");
        assert!(!object.contains_key("urls"), "urls vide envoye");
        assert_eq!(object["title"], "Sans rien");
    }

    #[test]
    fn writes_refuse_incomplete_input_without_spawning() {
        // Le binaire n'existe pas: si la validation laissait passer, l'erreur
        // serait `NotFound` et non `Invalid`.
        let cli = PassCli::new("pass-cli-qui-n-existe-pas");
        let draft = LoginDraft::for_ssh("titre", "root", "h", 22, Secret::new("s"));

        assert!(matches!(
            cli.create_login("  ", &draft),
            Err(PassError::Invalid(_))
        ));
        let untitled = LoginDraft::for_ssh("", "root", "h", 22, Secret::new("s"));
        assert!(matches!(
            cli.create_login("Coffre", &untitled),
            Err(PassError::Invalid(_))
        ));
        assert!(matches!(
            cli.create_login(
                "Coffre",
                &LoginDraft::for_ssh("t", "", "h", 22, Secret::default())
            ),
            Err(PassError::Invalid(_))
        ));
        assert!(matches!(
            cli.set_login_password("Coffre", "", &Secret::new("s"), true),
            Err(PassError::Invalid(_))
        ));
        assert!(matches!(
            cli.generate_ssh_key("", "t", SshKeyType::Ed25519, ""),
            Err(PassError::Invalid(_))
        ));
        assert!(matches!(
            cli.import_ssh_key("Coffre", "t", Path::new("/inexistant/id_ed25519")),
            Err(PassError::Invalid(_))
        ));
    }

    #[test]
    fn key_types_map_to_documented_flags() {
        assert_eq!(SshKeyType::default(), SshKeyType::Ed25519);
        assert_eq!(SshKeyType::Ed25519.flag(), "ed25519");
        assert_eq!(SshKeyType::Rsa2048.flag(), "rsa2048");
        assert_eq!(SshKeyType::Rsa4096.flag(), "rsa4096");
        assert_eq!(SshKeyType::ALL.len(), 3);
    }

    #[test]
    fn summary_falls_back_when_the_command_says_nothing() {
        assert_eq!(summarize("  \n Item cree \n autre", "repli"), "Item cree");
        assert_eq!(summarize("   \n\n", "repli"), "repli");
    }

    // Ces tests pilotent un vrai sous-processus via `sh`: ils n'ont de sens
    // que la ou un shell POSIX existe.
    #[cfg(unix)]
    #[test]
    fn stdin_reaches_the_process_and_is_closed() {
        let cli = PassCli::new("sh");
        // `cat` ne rend la main que sur EOF: ce test verifie a la fois que
        // l'entree est transmise et que le tube est bien ferme derriere.
        let out = cli
            .run_with_input(
                &["-c".into(), "cat".into()],
                Some(b"{\"title\":\"x\"}".to_vec()),
            )
            .expect("doit reussir");
        assert_eq!(out, "{\"title\":\"x\"}");
    }

    #[cfg(unix)]
    #[test]
    fn command_errors_never_echo_a_password() {
        let cli = PassCli::new("sh");
        let err = cli
            .run(&[
                "-c".into(),
                "exit 1".into(),
                "--field".into(),
                "password=hunter2".into(),
            ])
            .expect_err("doit echouer");
        let rendered = err.to_string();
        assert!(!rendered.contains("hunter2"), "fuite: {rendered}");
        assert!(rendered.contains("password=***"), "obtenu: {rendered}");
    }

    /// Faux `pass-cli`: journalise ses arguments et son entree standard, puis
    /// repond comme le vrai. Permet de verifier ce qui est reellement envoye.
    #[cfg(unix)]
    fn stub_cli(name: &str) -> (PassCli, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sshpass-gui-stub-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("pass-cli");
        crate::pass::testing::write_stub(
            &script,
            &format!(
                "printf '%s\\n' \"$@\" > '{dir}/args'\ncat > '{dir}/stdin'\n\
                 echo 'Item created'",
                dir = dir.display()
            ),
        );
        (PassCli::new(script.to_string_lossy().into_owned()), dir)
    }

    #[cfg(unix)]
    #[test]
    fn creating_a_login_sends_the_password_on_stdin_only() {
        let (cli, dir) = stub_cli("create");
        let draft = LoginDraft::for_ssh("web-01", "root", "10.0.0.4", 2222, Secret::new("hunter2"));
        let summary = crate::pass::testing::unhurried(|| cli.create_login("SSH Keys", &draft))
            .expect("creation");
        assert_eq!(summary, "Item created");

        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        let stdin = std::fs::read_to_string(dir.join("stdin")).expect("stdin");

        // La commande documentee, avec le coffre vise.
        let expected = [
            "item",
            "create",
            "login",
            "--vault-name",
            "SSH Keys",
            "--from-template",
            "-",
        ];
        assert_eq!(args.lines().collect::<Vec<_>>(), expected);

        // Le coeur du contrat: le secret est passe par l'entree standard, donc
        // il n'a jamais figure dans `/proc/<pid>/cmdline`.
        assert!(
            !args.contains("hunter2"),
            "mot de passe sur la ligne de commande: {args}"
        );
        let json: Value = serde_json::from_str(&stdin).expect("json");
        assert_eq!(json["password"], "hunter2");
        assert_eq!(json["title"], "web-01");
        assert_eq!(json["username"], "root");
        assert_eq!(json["urls"][0], "ssh://root@10.0.0.4:2222");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn importing_a_key_passes_a_path_and_never_its_content() {
        let (cli, dir) = stub_cli("import");
        let key = dir.join("id_ed25519");
        std::fs::write(&key, "PRIVATE-KEY-CONTENT").expect("ecriture");

        crate::pass::testing::unhurried(|| cli.import_ssh_key("SSH Keys", "web-01", &key))
            .expect("import");
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        let stdin = std::fs::read_to_string(dir.join("stdin")).expect("stdin");

        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "item",
                "create",
                "ssh-key",
                "import",
                "--vault-name",
                "SSH Keys",
                "--title",
                "web-01",
                "--from-private-key",
                key.to_string_lossy().as_ref(),
            ]
        );
        // La cle elle-meme n'a jamais ete lue par sshpass-gui.
        assert!(!args.contains("PRIVATE-KEY-CONTENT"));
        assert!(stdin.is_empty(), "entree standard non vide: {stdin}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn generating_a_key_carries_type_and_comment() {
        let (cli, dir) = stub_cli("generate");
        crate::pass::testing::unhurried(|| {
            cli.generate_ssh_key("SSH Keys", "web-01", SshKeyType::Rsa4096, "root@10.0.0.4")
        })
        .expect("generation");
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "item",
                "create",
                "ssh-key",
                "generate",
                "--vault-name",
                "SSH Keys",
                "--title",
                "web-01",
                "--key-type",
                "rsa4096",
                "--comment",
                "root@10.0.0.4",
            ]
        );

        // Sans commentaire, l'option est simplement absente.
        cli.generate_ssh_key("SSH Keys", "web-01", SshKeyType::Ed25519, "  ")
            .expect("generation");
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert!(!args.contains("--comment"), "option vide envoyee: {args}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn updating_a_password_prefers_standard_input() {
        let (cli, dir) = stub_cli("update");
        crate::pass::testing::unhurried(|| {
            cli.set_login_password("SSH Keys", "web-01", &Secret::new("hunter2"), true)
        })
        .expect("mise a jour");

        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "item",
                "update",
                "--vault-name",
                "SSH Keys",
                "--item-title",
                "web-01",
                "--from-template",
                "-",
            ]
        );
        // Le coeur du contrat: rien du secret dans `/proc/<pid>/cmdline`.
        assert!(
            !args.contains("hunter2"),
            "mot de passe sur la ligne de commande: {args}"
        );
        let stdin = std::fs::read_to_string(dir.join("stdin")).expect("stdin");
        let json: Value = serde_json::from_str(&stdin).expect("json");
        assert_eq!(json["password"], "hunter2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Faux `pass-cli` d'une version qui ignore `--from-template` sur `update`:
    /// il refuse l'option comme le ferait un analyseur d'arguments, et
    /// journalise ce qu'on lui a passe.
    #[cfg(unix)]
    fn stub_cli_without_template(name: &str) -> (PassCli, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sshpass-gui-legacy-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("pass-cli");
        crate::pass::testing::write_stub(
            &script,
            &format!(
                "printf '%s\\n' \"$@\" > '{dir}/args'\n\
                 for a in \"$@\"; do\n\
                 \x20 if [ \"$a\" = '--from-template' ]; then\n\
                 \x20   echo 'unknown flag: --from-template' >&2\n\
                 \x20   exit 2\n\
                 \x20 fi\n\
                 done\n\
                 echo 'Item updated'",
                dir = dir.display()
            ),
        );
        (PassCli::new(script.to_string_lossy().into_owned()), dir)
    }

    #[cfg(unix)]
    #[test]
    fn an_old_cli_falls_back_only_when_it_is_allowed() {
        let (cli, dir) = stub_cli_without_template("refus");

        // Sans permission, l'echec est franc: le mot de passe ne part pas dans
        // `/proc/<pid>/cmdline` a l'insu de l'utilisateur.
        let refused = crate::pass::testing::unhurried(|| {
            cli.set_login_password("SSH Keys", "web-01", &Secret::new("hunter2"), false)
        })
        .expect_err("le repli doit etre refuse");
        assert!(matches!(refused, PassError::Invalid(_)));
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert!(
            !args.contains("hunter2"),
            "mot de passe passe malgre le refus: {args}"
        );

        // Avec permission, le repli documente reprend la main.
        crate::pass::testing::unhurried(|| {
            cli.set_login_password("SSH Keys", "web-01", &Secret::new("hunter2"), true)
        })
        .expect("mise a jour");
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert_eq!(
            args.lines().collect::<Vec<_>>(),
            [
                "item",
                "update",
                "--vault-name",
                "SSH Keys",
                "--item-title",
                "web-01",
                "--field",
                "password=hunter2",
            ]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_real_failure_is_never_retried_on_the_command_line() {
        // Le coffre n'existe pas: le repli n'y changerait rien, et il
        // exposerait le secret pour obtenir le meme echec.
        let dir = std::env::temp_dir().join(format!(
            "sshpass-gui-vaultless-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("pass-cli");
        crate::pass::testing::write_stub(
            &script,
            &format!(
                "printf '%s\\n' \"$@\" > '{dir}/args'\n\
                 echo 'vault not found' >&2\nexit 1",
                dir = dir.display()
            ),
        );
        let cli = PassCli::new(script.to_string_lossy().into_owned());

        let err = crate::pass::testing::unhurried(|| {
            cli.set_login_password("Absent", "web-01", &Secret::new("hunter2"), true)
        })
        .expect_err("doit echouer");
        assert!(err.detail().contains("vault not found"));
        let args = std::fs::read_to_string(dir.join("args")).expect("args");
        assert!(
            !args.contains("hunter2"),
            "secret rejoue sur la ligne de commande: {args}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn hung_command_is_killed() {
        let cli = PassCli {
            binary: "sh".into(),
            timeout: Duration::from_millis(150),
        };
        let err = cli
            .run(&["-c".into(), "sleep 30".into()])
            .expect_err("doit expirer");
        assert!(matches!(err, PassError::Timeout(_)), "obtenu {err:?}");
    }
}
