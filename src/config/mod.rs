//! Modele de donnees et persistance de la configuration.
//!
//! Le fichier de configuration est un TOML stocke dans
//! `$XDG_CONFIG_HOME/sshpass-gui/config.toml`, surchargeable via la variable
//! d'environnement `SSHPASS_GUI_CONFIG`.
//!
//! Regle absolue: **aucun secret n'est ecrit dans ce fichier**. Les mots de
//! passe et les cles SSH restent dans Proton Pass; on ne stocke que des
//! references (`ProtonRef`) vers les items du coffre.
//!
//! ## Le fichier de configuration est une politique d'execution
//!
//! Une connexion porte des options `ssh -o` libres (`ssh_options`) et une
//! commande distante. Or `ssh` sait executer des programmes **locaux** pour
//! le compte de sa configuration: `ProxyCommand`, `LocalCommand` couple a
//! `PermitLocalCommand`, `KnownHostsCommand`, `Match exec`... Un TOML pose la
//! par un tiers n'est donc pas « juste de la configuration »: c'est du code
//! qui s'executera sous l'identite de l'utilisateur. Le fichier doit etre
//! traite comme tel — cf. `SECURITY.md` et `command::dangerous_options`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;

/// Racine du fichier de configuration.
///
/// L'ordre des champs est significatif: le serialiseur TOML exige que les
/// valeurs scalaires precedent les tables, et les tables les tableaux de
/// tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub ui: UiConfig,
    pub proton_pass: ProtonPassConfig,
    pub security: SecurityConfig,
    #[serde(rename = "folders")]
    pub folders: Vec<Folder>,
    #[serde(rename = "connections")]
    pub connections: Vec<Connection>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            ui: UiConfig::default(),
            proton_pass: ProtonPassConfig::default(),
            security: SecurityConfig::default(),
            folders: Vec::new(),
            connections: Vec::new(),
        }
    }
}

/// Reglages de securite. Tous ont une valeur par defaut **fermee**: ce qui
/// s'ouvre ici s'ouvre a la machine distante ou aux autres comptes locaux, et
/// doit donc etre un choix, jamais un heritage.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    /// Repondre aux requetes OSC 52 « lecture du presse-papiers ».
    ///
    /// La sequence `OSC 52 ; c ; ? ST` demande au terminal de **renvoyer** le
    /// presse-papiers dans le flux, donc a l'application distante. Pour un
    /// gestionnaire SSH, ce presse-papiers contient regulierement un mot de
    /// passe ou un jeton: un serveur compromis n'aurait qu'a emettre la
    /// sequence pour le recuperer, sans que personne ne colle quoi que ce
    /// soit. C'est refuse par defaut.
    pub remote_clipboard_read: bool,
    /// Laisser une application distante **ecrire** dans le presse-papiers
    /// local (OSC 52 avec une charge utile).
    ///
    /// Bien plus benin que la lecture, et reellement utile (`tmux`, `vim`,
    /// `yank` a distance): autorise par defaut, mais borne par
    /// `clipboard_write_limit` et desactivable.
    pub remote_clipboard_write: bool,
    /// Taille maximale d'une ecriture OSC 52, en octets.
    ///
    /// Sans borne, un distant hostile peut pousser plusieurs mega-octets dans
    /// le presse-papiers du poste a chaque frappe.
    pub clipboard_write_limit: usize,
    /// Accepter de se replier sur `TMPDIR`/`/tmp` quand `XDG_RUNTIME_DIR` est
    /// absent.
    ///
    /// `/tmp` est partage par tous les comptes de la machine: un voisin peut y
    /// deposer un lien symbolique a l'emplacement que l'on s'apprete a creer.
    /// Le repli reste possible, mais il se demande.
    pub allow_temp_runtime_dir: bool,
    /// Autoriser `pass-cli item update --field password=...`, ou le secret
    /// figure dans `/proc/<pid>/cmdline`.
    ///
    /// `/proc/<pid>/cmdline` est lisible par **tous** les comptes de la
    /// machine. Le repli est donc refuse par defaut: si la version de
    /// `pass-cli` installee ne sait pas lire un gabarit sur son entree
    /// standard, la mise a jour echoue avec un message qui l'explique plutot
    /// que d'exposer le mot de passe en silence.
    pub allow_argv_fallback: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            remote_clipboard_read: false,
            remote_clipboard_write: true,
            clipboard_write_limit: 64 * 1024,
            allow_temp_runtime_dir: false,
            allow_argv_fallback: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Taille de police de l'interface, en points.
    pub font_size: f32,
    /// Taille de police du terminal, en points.
    pub terminal_font_size: f32,
    /// Largeur de la barre laterale, en points.
    pub sidebar_width: f32,
    /// Nombre de lignes conservees dans l'historique de chaque terminal.
    pub scrollback_lines: usize,
    /// Facteur d'echelle des sprites pixel art (entier, pour rester net).
    pub pixel_scale: u32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            terminal_font_size: 14.0,
            sidebar_width: 260.0,
            scrollback_lines: 10_000,
            pixel_scale: 2,
        }
    }
}

/// Strategie d'integration avec l'agent SSH de Proton Pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentMode {
    /// Aucune integration: on herite du `SSH_AUTH_SOCK` de l'environnement.
    Disabled,
    /// `pass-cli ssh-agent start` est pilote par sshpass-gui, un agent par coffre.
    #[default]
    OwnAgent,
    /// `pass-cli ssh-agent load` injecte les cles dans l'agent deja en place.
    LoadIntoExisting,
}

impl AgentMode {
    pub fn label(self) -> &'static str {
        match self {
            AgentMode::Disabled => "Desactive",
            AgentMode::OwnAgent => "Agent dedie",
            AgentMode::LoadIntoExisting => "Agent existant",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtonPassConfig {
    /// Binaire `pass-cli` (cherche dans le `PATH` si ce n'est pas un chemin).
    pub binary: String,
    pub agent_mode: AgentMode,
    /// Intervalle de rafraichissement des cles de l'agent, en secondes.
    pub refresh_interval: u64,
    /// Relance `pass-cli login` toute seule quand la session est fermee, et
    /// ouvre le lien d'authentification dans le navigateur.
    ///
    /// Active par defaut: une session Proton Pass ne survit pas a l'arret de
    /// la machine, et sans reconnexion l'application n'a plus acces a rien.
    /// Se desactive pour les postes ou l'ouverture d'un navigateur n'est pas
    /// souhaitable.
    pub auto_login: bool,
    /// Coffre propose par defaut lors de la creation d'une connexion.
    pub default_vault: Option<String>,
}

impl Default for ProtonPassConfig {
    fn default() -> Self {
        Self {
            binary: "pass-cli".to_string(),
            agent_mode: AgentMode::default(),
            refresh_interval: 3600,
            auto_login: true,
            default_vault: None,
        }
    }
}

/// Dossier de rangement des connexions. Les dossiers sont imbricables via
/// `parent`, ce qui evite d'encoder une hierarchie dans les noms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            parent: None,
        }
    }
}

/// Methode d'authentification d'une connexion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    /// Cle servie par un agent SSH (Proton Pass ou agent du systeme).
    #[default]
    Agent,
    /// Mot de passe recupere dans Proton Pass et fourni a `ssh` via SSH_ASKPASS.
    ///
    /// **Uniquement** la methode `password` du protocole. `keyboard-interactive`
    /// en est exclu a dessein: c'est le serveur qui y redige les questions, et
    /// un serveur hostile n'aurait qu'a en poser une pour que le pont askpass
    /// lui serve le secret du coffre. Cf. `AuthMethod::KeyboardInteractive`.
    Password,
    /// Echange `keyboard-interactive` (PAM, code a usage unique, second
    /// facteur), **saisi a la main**.
    ///
    /// Aucun secret automatique n'est branche sur ce mode: les questions
    /// viennent du serveur, donc seul un humain peut decider quoi y repondre.
    KeyboardInteractive,
    /// Fichier de cle privee sur disque (`ssh -i`).
    KeyFile,
}

impl AuthMethod {
    pub const ALL: [AuthMethod; 4] = [
        AuthMethod::Agent,
        AuthMethod::Password,
        AuthMethod::KeyboardInteractive,
        AuthMethod::KeyFile,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::Agent => "Agent SSH",
            AuthMethod::Password => "Mot de passe (Proton Pass)",
            AuthMethod::KeyboardInteractive => "Interactif (saisie manuelle)",
            AuthMethod::KeyFile => "Fichier de cle",
        }
    }

    /// Vrai si la methode va chercher un secret dans Proton Pass toute seule.
    pub fn uses_stored_password(self) -> bool {
        matches!(self, AuthMethod::Password)
    }
}

/// Reference vers un item Proton Pass. Ne contient jamais de secret, seulement
/// de quoi reconstruire une URI `pass://coffre/item[/champ]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtonRef {
    /// Nom du coffre (ou share id).
    pub vault: String,
    /// Titre de l'item (ou item id).
    pub item: String,
    /// Champ a lire; `password` par defaut cote `pass-cli`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ProtonRef {
    /// URI comprise par `pass-cli item view`.
    ///
    /// Chaque composant est encode: un coffre nomme `Prod/Backup` produisait
    /// jusqu'ici `pass://Prod/Backup/item`, ou l'item et le coffre ne se
    /// distinguent plus. Cf. `encode_uri_component` pour le detail de ce qui
    /// est encode — et de ce qui ne l'est volontairement pas.
    pub fn uri(&self) -> String {
        let vault = encode_uri_component(&self.vault);
        let item = encode_uri_component(&self.item);
        match &self.field {
            Some(field) if !field.is_empty() => {
                format!("pass://{vault}/{item}/{}", encode_uri_component(field))
            }
            _ => format!("pass://{vault}/{item}"),
        }
    }

    pub fn is_complete(&self) -> bool {
        !self.vault.trim().is_empty() && !self.item.trim().is_empty()
    }
}

/// Une connexion SSH enregistree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    /// Identifiant du dossier parent, `None` pour la racine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    pub favorite: bool,
    pub auth: AuthMethod,
    /// Chemin de cle privee, seulement pour `AuthMethod::KeyFile`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
    /// Commande a executer a la connexion; shell interactif si absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Options `-o` supplementaires passees a `ssh`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ssh_options: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Horodatage unix (secondes) de la derniere ouverture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<u64>,
    /// Reference Proton Pass (cle SSH ou mot de passe). Table TOML: en dernier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton: Option<ProtonRef>,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            host: String::new(),
            user: String::new(),
            port: 22,
            folder: None,
            favorite: false,
            auth: AuthMethod::default(),
            key_file: None,
            command: None,
            ssh_options: Vec::new(),
            tags: Vec::new(),
            last_used: None,
            proton: None,
        }
    }
}

impl Connection {
    /// `user@host`, ou juste `host` si aucun utilisateur n'est defini.
    pub fn target(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }

    /// Libelle affiche dans les listes.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            self.target()
        } else {
            self.name.clone()
        }
    }

    /// Teste si la connexion correspond a une recherche (nom, hote, user, tag).
    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        let haystack = [self.name.as_str(), self.host.as_str(), self.user.as_str()];
        haystack.iter().any(|f| f.to_lowercase().contains(&needle))
            || self.tags.iter().any(|t| t.to_lowercase().contains(&needle))
    }

    pub fn touch(&mut self) {
        self.last_used = Some(now_secs());
    }
}

impl Config {
    pub fn connection(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    pub fn connection_mut(&mut self, id: &str) -> Option<&mut Connection> {
        self.connections.iter_mut().find(|c| c.id == id)
    }

    pub fn upsert_connection(&mut self, conn: Connection) {
        match self.connections.iter_mut().find(|c| c.id == conn.id) {
            Some(slot) => *slot = conn,
            None => self.connections.push(conn),
        }
    }

    pub fn remove_connection(&mut self, id: &str) {
        self.connections.retain(|c| c.id != id);
    }

    /// Supprime un dossier et remonte ses connexions et sous-dossiers a la racine.
    pub fn remove_folder(&mut self, id: &str) {
        self.folders.retain(|f| f.id != id);
        for folder in &mut self.folders {
            if folder.parent.as_deref() == Some(id) {
                folder.parent = None;
            }
        }
        for conn in &mut self.connections {
            if conn.folder.as_deref() == Some(id) {
                conn.folder = None;
            }
        }
    }

    /// Connexions les plus recemment ouvertes, les plus recentes d'abord.
    pub fn recents(&self, limit: usize) -> Vec<&Connection> {
        let mut used: Vec<&Connection> = self
            .connections
            .iter()
            .filter(|c| c.last_used.is_some())
            .collect();
        used.sort_by_key(|c| std::cmp::Reverse(c.last_used));
        used.truncate(limit);
        used
    }

    /// Coffres Proton Pass references par au moins une connexion.
    pub fn referenced_vaults(&self) -> Vec<String> {
        let mut vaults: Vec<String> = self
            .connections
            .iter()
            .filter_map(|c| c.proton.as_ref())
            .filter(|p| !p.vault.is_empty())
            .map(|p| p.vault.clone())
            .collect();
        vaults.sort();
        vaults.dedup();
        vaults
    }
}

/// Chemin du fichier de configuration.
pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SSHPASS_GUI_CONFIG") {
        return PathBuf::from(path);
    }
    config_dir().join("config.toml")
}

/// Repertoire de configuration (`~/.config/sshpass-gui` sous Linux).
pub fn config_dir() -> PathBuf {
    project_config_dir("sshpass-gui")
}

/// Repertoire utilise avant le renommage en `sshpass-gui`.
fn legacy_config_dir() -> PathBuf {
    project_config_dir("sshpass")
}

fn project_config_dir(name: &str) -> PathBuf {
    directories::ProjectDirs::from("", "", name)
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".config").join(name))
}

/// Reprend une configuration ecrite avant le renommage en `sshpass-gui`.
///
/// Copie plutot que deplacement: en cas de retour en arriere, l'ancien
/// fichier est toujours la. Renvoie le chemin repris, s'il y en a eu un.
pub fn adopt_legacy_config() -> anyhow::Result<Option<PathBuf>> {
    // Un chemin impose explicitement n'a pas a etre ecrase par une reprise.
    if std::env::var_os("SSHPASS_GUI_CONFIG").is_some() {
        return Ok(None);
    }
    adopt_config_from(&legacy_config_dir().join("config.toml"), &config_path())
}

/// Coeur testable de la reprise: ne fait rien si la cible existe deja ou si
/// la source est absente.
fn adopt_config_from(legacy: &Path, target: &Path) -> anyhow::Result<Option<PathBuf>> {
    if target.exists() || !legacy.exists() {
        return Ok(None);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(legacy, target)?;
    Ok(Some(legacy.to_path_buf()))
}

/// Encode un composant d'URI `pass://`.
///
/// Encodage **minimal et deliberement conservateur**: seuls les caracteres qui
/// rendent l'URI ambigue sont echappes.
///
/// * `%` d'abord, sans quoi l'encodage ne serait pas reversible;
/// * `/`, `?` et `#`, les delimiteurs qui decoupent l'URI;
/// * les caracteres de controle, qui n'ont rien a faire dans un nom.
///
/// Les espaces et les lettres accentuees passent **tels quels**: un coffre
/// s'appelle couramment « SSH Keys », et transformer cela en `SSH%20Keys`
/// casserait toutes les configurations existantes si `pass-cli` ne decode pas.
/// L'objectif est de lever l'ambiguite, pas de produire une URI RFC 3986.
pub fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' | '/' | '?' | '#' => {
                let mut buf = [0u8; 4];
                for byte in ch.encode_utf8(&mut buf).as_bytes() {
                    encoded.push_str(&format!("%{byte:02X}"));
                }
            }
            c if c.is_control() => {
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).as_bytes() {
                    encoded.push_str(&format!("%{byte:02X}"));
                }
            }
            c => encoded.push(c),
        }
    }
    encoded
}

/// Repertoire volatil pour les sockets d'agent et les scripts askpass.
///
/// N'ecrit rien et ne verifie rien: c'est `secure_runtime_dir` qui cree le
/// repertoire et refuse de travailler dans un repertoire douteux. Cette
/// fonction ne sert qu'a **nommer** un chemin (socket d'agent, nettoyage).
pub fn runtime_dir() -> PathBuf {
    runtime_base().join("sshpass-gui")
}

/// Racine du repertoire volatil: `XDG_RUNTIME_DIR` s'il existe, sinon le
/// repertoire temporaire du systeme.
fn runtime_base() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::temp_dir(),
    }
}

/// Vrai si `XDG_RUNTIME_DIR` designe une racine propre a l'utilisateur.
pub fn has_private_runtime_dir() -> bool {
    matches!(std::env::var_os("XDG_RUNTIME_DIR"), Some(dir) if !dir.is_empty())
}

/// Cree — ou revalide — le repertoire volatil, et garantit qu'il est prive.
///
/// Deux dangers, tous deux locaux:
///
/// * `XDG_RUNTIME_DIR` absent, on retombe sur `/tmp`, que **tout le monde**
///   peut ecrire. Un voisin y depose `sshpass-gui` avant nous — un lien
///   symbolique, un repertoire a lui — et lit ou remplace nos scripts askpass
///   et nos sockets d'agent. Ce repli n'a donc lieu que s'il a ete demande
///   (`security.allow_temp_runtime_dir`).
/// * le repertoire existe deja mais n'est pas a nous, ou laisse un bit au
///   groupe ou aux autres. On refuse plutot que de corriger en aveugle: si
///   quelqu'un d'autre le possede, un `chmod` ne nous rendrait pas maitres des
///   fichiers qui s'y trouvent deja.
#[cfg(unix)]
pub fn secure_runtime_dir(allow_temp_fallback: bool) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    if !has_private_runtime_dir() && !allow_temp_fallback {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "XDG_RUNTIME_DIR est absent. Le repli sur /tmp est partage par tous \
             les comptes de la machine: activez « Repli /tmp » dans les \
             reglages si vous l'acceptez malgre tout.",
        ));
    }

    let dir = runtime_dir();
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => return Ok(dir),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }

    // `symlink_metadata` et non `metadata`: c'est le lien qu'on veut voir, pas
    // sa cible. Un lien vers /home/victime/.ssh passerait sinon le controle.
    let meta = std::fs::symlink_metadata(&dir)?;
    if !meta.is_dir() {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("{} n'est pas un repertoire", dir.display()),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!(
                "{} est accessible au groupe ou aux autres (mode {:o})",
                dir.display(),
                meta.mode() & 0o777
            ),
        ));
    }
    if let Some(uid) = current_uid() {
        if meta.uid() != uid {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                format!("{} appartient a un autre compte", dir.display()),
            ));
        }
    }
    Ok(dir)
}

/// Identifiant du compte qui execute ce processus.
///
/// Lu sur `/proc/self` plutot que par `getuid(2)`: la caisse n'a pas de
/// dependance a `libc`, et ce projet ne vise que Linux. La ou `/proc` n'est
/// pas monte, on renvoie `None` et l'appelant se contente du controle des
/// droits — qui suffit deja a rendre le repertoire inutilisable par un tiers.
#[cfg(unix)]
fn current_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// Verrou des tests qui touchent aux variables d'environnement.
///
/// `set_var` agit sur le processus entier et le binaire de test est
/// multi-thread: sans ce verrou, un test qui retire `XDG_RUNTIME_DIR` le
/// retire aussi sous les pieds de celui d'a cote, et l'echec se promene.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Prend le verrou d'environnement, meme s'il a ete empoisonne par un test qui
/// a panique: c'est l'ordre qui nous interesse, pas l'etat protege.
#[cfg(test)]
pub(crate) fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Charge la configuration. Un fichier absent donne une configuration vide,
/// ce qui n'est pas une erreur (premier lancement).
pub fn load() -> anyhow::Result<Config> {
    load_from(&config_path())
}

pub fn load_from(path: &Path) -> anyhow::Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&text)?;
    Ok(config)
}

/// Ecrit la configuration de maniere atomique (fichier temporaire + rename).
pub fn save(config: &Config) -> anyhow::Result<()> {
    save_to(config, &config_path())
}

pub fn save_to(config: &Config, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Formate un horodatage unix en duree relative ("il y a 5 min").
pub fn relative_time(ts: u64) -> String {
    let now = now_secs();
    let delta = now.saturating_sub(ts);
    match delta {
        0..=59 => "a l'instant".to_string(),
        60..=3599 => format!("il y a {} min", delta / 60),
        3600..=86_399 => format!("il y a {} h", delta / 3600),
        86_400..=2_591_999 => format!("il y a {} j", delta / 86_400),
        _ => format!("il y a {} mois", delta / 2_592_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        let mut config = Config::default();
        let folder = Folder::new("Production");
        let mut conn = Connection {
            name: "web-01".into(),
            host: "10.0.0.4".into(),
            user: "root".into(),
            port: 2222,
            folder: Some(folder.id.clone()),
            favorite: true,
            tags: vec!["prod".into(), "web".into()],
            proton: Some(ProtonRef {
                vault: "SSH Keys".into(),
                item: "web-01".into(),
                field: None,
            }),
            ..Default::default()
        };
        conn.touch();
        config.folders.push(folder);
        config.connections.push(conn);
        config
    }

    #[test]
    fn legacy_config_is_adopted_once() {
        let root = std::env::temp_dir().join(format!("sshpass-gui-adopt-{}", std::process::id()));
        let legacy = root.join("ancien/config.toml");
        let target = root.join("nouveau/config.toml");
        std::fs::create_dir_all(legacy.parent().expect("parent")).expect("mkdir");
        std::fs::write(&legacy, "version = 1\n").expect("ecriture");

        let adopted = adopt_config_from(&legacy, &target).expect("reprise");
        assert_eq!(adopted.as_deref(), Some(legacy.as_path()));
        assert!(target.exists(), "la configuration doit avoir ete copiee");
        assert!(legacy.exists(), "l'ancienne doit rester en place");

        // Deuxieme passage: la cible existe, on n'ecrase rien.
        std::fs::write(&target, "version = 1\n# modifiee\n").expect("ecriture");
        assert_eq!(adopt_config_from(&legacy, &target).expect("reprise"), None);
        assert!(std::fs::read_to_string(&target)
            .expect("lecture")
            .contains("modifiee"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn adoption_without_legacy_is_a_no_op() {
        let root = std::env::temp_dir().join(format!("sshpass-gui-noadopt-{}", std::process::id()));
        let result = adopt_config_from(&root.join("absent.toml"), &root.join("cible.toml"))
            .expect("reprise");
        assert_eq!(result, None);
        assert!(!root.join("cible.toml").exists());
    }

    #[test]
    fn config_directories_are_distinct() {
        assert_ne!(config_dir(), legacy_config_dir());
        assert!(config_dir().ends_with("sshpass-gui"));
    }

    #[test]
    fn roundtrip_toml() {
        let config = sample();
        let text = toml::to_string_pretty(&config).expect("serialisation");
        let parsed: Config = toml::from_str(&text).expect("deserialisation");
        assert_eq!(parsed.connections.len(), 1);
        assert_eq!(parsed.folders.len(), 1);
        assert_eq!(parsed.connections[0].port, 2222);
        assert!(parsed.connections[0].favorite);
        assert_eq!(parsed.connections[0].tags, vec!["prod", "web"]);
        assert_eq!(
            parsed.connections[0].proton.as_ref().unwrap().vault,
            "SSH Keys"
        );
    }

    #[test]
    fn empty_config_is_valid_toml() {
        let text = toml::to_string_pretty(&Config::default()).expect("serialisation");
        let parsed: Config = toml::from_str(&text).expect("deserialisation");
        assert!(parsed.connections.is_empty());
        assert_eq!(parsed.version, CONFIG_VERSION);
    }

    #[test]
    fn partial_config_uses_defaults() {
        let text = r#"
            version = 1
            [[connections]]
            id = "abc"
            name = "box"
            host = "example.com"
            user = "alex"
        "#;
        let parsed: Config = toml::from_str(text).expect("deserialisation");
        assert_eq!(parsed.connections[0].port, 22);
        assert_eq!(parsed.connections[0].auth, AuthMethod::Agent);
        assert_eq!(parsed.ui.font_size, 14.0);
        assert_eq!(parsed.proton_pass.binary, "pass-cli");
    }

    #[test]
    fn no_secret_is_ever_serialized() {
        let text = toml::to_string_pretty(&sample()).expect("serialisation");
        for forbidden in ["password", "passphrase", "private_key", "secret"] {
            assert!(
                !text.to_lowercase().contains(forbidden),
                "{forbidden} present dans le TOML"
            );
        }
    }

    #[test]
    fn proton_uri_formats() {
        let mut reference = ProtonRef {
            vault: "Vault".into(),
            item: "Item".into(),
            field: None,
        };
        assert_eq!(reference.uri(), "pass://Vault/Item");
        reference.field = Some("password".into());
        assert_eq!(reference.uri(), "pass://Vault/Item/password");
    }

    #[test]
    fn proton_uri_keeps_components_apart() {
        // Sans encodage, ces trois noms donnaient tous la meme URI.
        let ambiguous = ProtonRef {
            vault: "Prod/Backup".into(),
            item: "web-01".into(),
            field: None,
        };
        assert_eq!(ambiguous.uri(), "pass://Prod%2FBackup/web-01");

        let with_query = ProtonRef {
            vault: "V".into(),
            item: "web?prod#1".into(),
            field: Some("password".into()),
        };
        assert_eq!(with_query.uri(), "pass://V/web%3Fprod%231/password");
    }

    #[test]
    fn uri_encoding_stays_minimal() {
        // Ce qui marche aujourd'hui doit continuer a marcher a l'identique.
        assert_eq!(encode_uri_component("SSH Keys"), "SSH Keys");
        assert_eq!(encode_uri_component("cle-prod_01.v2"), "cle-prod_01.v2");
        assert_eq!(encode_uri_component("Coffre prive"), "Coffre prive");
        // Ce qui rend l'URI ambigue, en revanche, est echappe.
        assert_eq!(encode_uri_component("a/b"), "a%2Fb");
        assert_eq!(encode_uri_component("100%"), "100%25");
        assert_eq!(encode_uri_component("a\nb"), "a%0Ab");
        // L'encodage de `%` passe en premier: il reste reversible.
        assert_eq!(encode_uri_component("%2F"), "%252F");
    }

    #[test]
    fn auth_methods_declare_who_reads_the_vault() {
        assert!(AuthMethod::Password.uses_stored_password());
        // Les questions de `keyboard-interactive` viennent du serveur: aucun
        // secret automatique ne doit y repondre.
        assert!(!AuthMethod::KeyboardInteractive.uses_stored_password());
        assert!(!AuthMethod::Agent.uses_stored_password());
        assert!(!AuthMethod::KeyFile.uses_stored_password());
        assert_eq!(AuthMethod::ALL.len(), 4);
    }

    #[test]
    fn security_defaults_are_closed() {
        let security = SecurityConfig::default();
        assert!(
            !security.remote_clipboard_read,
            "un serveur distant ne doit pas pouvoir lire le presse-papiers"
        );
        assert!(!security.allow_temp_runtime_dir);
        assert!(!security.allow_argv_fallback);
        // L'ecriture reste utile et donc permise, mais bornee.
        assert!(security.remote_clipboard_write);
        assert!(security.clipboard_write_limit > 0);
    }

    #[test]
    fn security_section_survives_a_roundtrip() {
        let mut config = Config::default();
        config.security.remote_clipboard_read = true;
        config.security.clipboard_write_limit = 4096;
        let text = toml::to_string_pretty(&config).expect("serialisation");
        let parsed: Config = toml::from_str(&text).expect("deserialisation");
        assert!(parsed.security.remote_clipboard_read);
        assert_eq!(parsed.security.clipboard_write_limit, 4096);

        // Une configuration ecrite par une version anterieure n'a pas la
        // section: elle doit retomber sur les valeurs fermees.
        let old: Config = toml::from_str("version = 1\n").expect("deserialisation");
        assert!(!old.security.remote_clipboard_read);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_dir_refuses_the_shared_fallback() {
        let _guard = env_guard();
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::remove_var("XDG_RUNTIME_DIR");
        let refused = secure_runtime_dir(false);
        if let Some(previous) = previous {
            std::env::set_var("XDG_RUNTIME_DIR", previous);
        }
        let err = refused.expect_err("le repli /tmp doit etre refuse");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_dir_is_private_and_reusable() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = env_guard();
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        let root = std::env::temp_dir().join(format!(
            "sshpass-gui-runtime-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).expect("mkdir");
        std::env::set_var("XDG_RUNTIME_DIR", &root);

        let dir = secure_runtime_dir(false).expect("creation");
        let mode = std::fs::metadata(&dir)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "le repertoire doit rester prive");
        // Deuxieme appel: le repertoire existe deja et reste accepte.
        assert_eq!(secure_runtime_dir(false).expect("revalidation"), dir);

        // Ouvert au monde, il est refuse plutot que corrige en aveugle.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(secure_runtime_dir(false).is_err(), "mode 0755 accepte");

        if let Some(previous) = previous {
            std::env::set_var("XDG_RUNTIME_DIR", previous);
        } else {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_folder_reparents_children() {
        let mut config = sample();
        let folder_id = config.folders[0].id.clone();
        config.remove_folder(&folder_id);
        assert!(config.folders.is_empty());
        assert_eq!(config.connections[0].folder, None);
    }

    #[test]
    fn search_matches_name_host_and_tags() {
        let config = sample();
        let conn = &config.connections[0];
        assert!(conn.matches("web"));
        assert!(conn.matches("10.0.0"));
        assert!(conn.matches("PROD"));
        assert!(!conn.matches("mysql"));
        assert!(conn.matches(""));
    }
}
