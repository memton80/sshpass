//! Modele de donnees et persistance de la configuration.
//!
//! Le fichier de configuration est un TOML stocke dans
//! `$XDG_CONFIG_HOME/sshpass-gui/config.toml`, surchargeable via la variable
//! d'environnement `SSHPASS_GUI_CONFIG`.
//!
//! Regle absolue: **aucun secret n'est ecrit dans ce fichier**. Les mots de
//! passe et les cles SSH restent dans Proton Pass; on ne stocke que des
//! references (`ProtonRef`) vers les items du coffre.

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
            folders: Vec::new(),
            connections: Vec::new(),
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
    /// Coffre propose par defaut lors de la creation d'une connexion.
    pub default_vault: Option<String>,
}

impl Default for ProtonPassConfig {
    fn default() -> Self {
        Self {
            binary: "pass-cli".to_string(),
            agent_mode: AgentMode::default(),
            refresh_interval: 3600,
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
    Password,
    /// Fichier de cle privee sur disque (`ssh -i`).
    KeyFile,
}

impl AuthMethod {
    pub const ALL: [AuthMethod; 3] = [AuthMethod::Agent, AuthMethod::Password, AuthMethod::KeyFile];

    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::Agent => "Agent SSH",
            AuthMethod::Password => "Mot de passe (Proton Pass)",
            AuthMethod::KeyFile => "Fichier de cle",
        }
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
    pub fn uri(&self) -> String {
        match &self.field {
            Some(field) if !field.is_empty() => {
                format!("pass://{}/{}/{}", self.vault, self.item, field)
            }
            _ => format!("pass://{}/{}", self.vault, self.item),
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

/// Repertoire volatil pour les sockets d'agent et les scripts askpass.
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("sshpass-gui")
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
