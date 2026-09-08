//! Pilotage de `pass-cli` (Proton Pass) en sous-processus.
//!
//! `pass-cli` expose `--output json` sur `vault list`, `item list` et
//! `item view`; c'est ce mode qui est utilise ici. Le schema exact des objets
//! JSON n'etant pas fige dans la documentation publique, les parseurs sont
//! volontairement tolerants: les noms de champs sont normalises (minuscules,
//! sans `_` ni `-`) et plusieurs alias sont acceptes, et un tableau peut etre
//! renvoye directement ou enveloppe dans un objet (`{"vaults": [...]}`).

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

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
    #[error("erreur d'entree/sortie: {0}")]
    Io(#[from] std::io::Error),
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

    // Note: aucune methode ne lit un secret. Les mots de passe sont fournis a
    // `ssh` par le script SSH_ASKPASS (cf. `pass::write_askpass_script`), qui
    // execute `pass-cli` lui-meme: la valeur ne transite jamais par sshpass.

    /// Execute `pass-cli` et renvoie sa sortie standard.
    fn run(&self, args: &[String]) -> Result<String> {
        let display = format!("{} {}", self.binary, args.join(" "));
        let mut child = match Command::new(&self.binary)
            .args(args)
            .stdin(Stdio::null())
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
                    return Err(PassError::Timeout(display));
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };

        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();

        if !status.success() {
            return Err(PassError::Command {
                command: display,
                code: status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into()),
                stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }
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
