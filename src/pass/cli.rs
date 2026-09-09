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
//! * mise a jour d'un mot de passe existant — `pass-cli item update` n'accepte
//!   les valeurs que par `--field cle=valeur`, donc sur la ligne de commande.
//!   C'est la seule exception, elle est signalee dans l'interface et les
//!   messages d'erreur sont expurges (`secret::redact`).

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::pass::secret::{redact, Secret};

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
        .map(|line| line.to_string())
        .unwrap_or_else(|| fallback.to_string())
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
    /// `pass-cli item update --vault-name V --item-title T --field password=…`
    ///
    /// **Seul appel ou un secret passe par la ligne de commande**: `item
    /// update` n'offre aucune forme `--from-template`. La valeur est donc
    /// visible dans `/proc/<pid>/cmdline` pendant l'appel, pour les processus
    /// du meme utilisateur. Les messages d'erreur, eux, sont expurges.
    pub fn set_login_password(&self, vault: &str, item: &str, password: &Secret) -> Result<String> {
        let item = require(item, "Le titre de l'item est obligatoire.")?;
        let vault = require(vault, "Choisissez un coffre Proton Pass.")?;
        if password.is_empty() {
            return Err(PassError::Invalid("Le mot de passe est vide.".into()));
        }

        let args = vec![
            "item".to_string(),
            "update".to_string(),
            "--vault-name".to_string(),
            vault,
            "--item-title".to_string(),
            item.clone(),
            "--field".to_string(),
            format!("password={}", password.expose()),
        ];
        let out = self.run(&args)?;
        Ok(summarize(
            &out,
            &format!("mot de passe de « {item} » mis a jour"),
        ))
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
            cli.set_login_password("Coffre", "", &Secret::new("s")),
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
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "sshpass-gui-stub-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("pass-cli");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{dir}/args'\ncat > '{dir}/stdin'\n\
                 echo 'Item created'\n",
                dir = dir.display()
            ),
        )
        .expect("ecriture");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        (PassCli::new(script.to_string_lossy().into_owned()), dir)
    }

    #[cfg(unix)]
    #[test]
    fn creating_a_login_sends_the_password_on_stdin_only() {
        let (cli, dir) = stub_cli("create");
        let draft = LoginDraft::for_ssh("web-01", "root", "10.0.0.4", 2222, Secret::new("hunter2"));
        let summary = cli.create_login("SSH Keys", &draft).expect("creation");
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

        cli.import_ssh_key("SSH Keys", "web-01", &key)
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
        cli.generate_ssh_key("SSH Keys", "web-01", SshKeyType::Rsa4096, "root@10.0.0.4")
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
    fn updating_a_password_uses_the_documented_field_syntax() {
        let (cli, dir) = stub_cli("update");
        cli.set_login_password("SSH Keys", "web-01", &Secret::new("hunter2"))
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
