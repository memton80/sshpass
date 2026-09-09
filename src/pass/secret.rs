//! Valeur sensible detenue le temps d'un appel `pass-cli`.
//!
//! sshpass-gui ne **lit** jamais un secret: les mots de passe sont servis a
//! `ssh` par le script `SSH_ASKPASS` (cf. `pass::write_askpass_script`).
//! L'**ecriture**, elle, oblige a tenir la valeur en memoire le temps de la
//! transmettre a `pass-cli`. `Secret` reduit cette fenetre au minimum:
//!
//! * la valeur n'est jamais affichee (`Debug` masque),
//! * elle n'est pas clonable, donc il n'en existe qu'un exemplaire,
//! * son tampon est ecrase a zero a la destruction.
//!
//! Ce n'est pas une garantie absolue — l'allocateur, la pagination ou une
//! copie faite par `serde_json` echappent a ce controle — mais c'est ce qui
//! est atteignable sans dependance supplementaire.

use std::fmt;

/// Noms de champs dont la valeur ne doit jamais apparaitre dans un journal ou
/// un message d'erreur.
pub const SENSITIVE_FIELDS: [&str; 5] = ["password", "passphrase", "secret", "totp", "privatekey"];

/// Une valeur sensible. Stockee en octets: `String` ne permet pas d'ecraser
/// son tampon sans `unsafe`, `Vec<u8>` si.
#[derive(Default)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }

    /// Reprend le contenu d'une chaine **et l'efface a la source**: le champ
    /// de saisie qui l'a produite ne garde rien.
    pub fn take(source: &mut String) -> Self {
        let secret = Self::new(source);
        scrub(source);
        secret
    }

    /// Expose la valeur. Le nom est volontairement penible a ecrire: chaque
    /// appel est un endroit ou le secret quitte son enveloppe.
    pub fn expose(&self) -> &str {
        // Le tampon vient toujours d'un `&str`: la conversion ne peut echouer.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// `Debug` masque la valeur: une trace ou un `dbg!` ne doit rien reveler.
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({} octets masques)", self.0.len())
    }
}

/// Ecrase le contenu d'une chaine avant de la vider.
///
/// `String::clear` ne fait que remettre la longueur a zero: les octets
/// restent lisibles dans le tas. `into_bytes` reprend la **meme** allocation,
/// que l'on remplit de zeros avant de la liberer.
pub fn scrub(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
}

/// Masque la valeur d'un argument `cle=valeur` dont la cle est sensible.
///
/// Sert aux messages d'erreur et aux journaux. Les formes qualifiees de
/// `pass-cli` (`Section.password=...`) sont reconnues elles aussi.
pub fn redact(arg: &str) -> String {
    let Some((key, _)) = arg.split_once('=') else {
        return arg.to_string();
    };
    let name: String = key
        .rsplit('.')
        .next()
        .unwrap_or(key)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if SENSITIVE_FIELDS.contains(&name.as_str()) {
        format!("{key}=***")
    } else {
        arg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_prints_its_value() {
        let secret = Secret::new("hunter2");
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains("hunter2"), "fuite: {rendered}");
        assert!(rendered.contains("masques"));
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn taking_a_secret_empties_the_source() {
        let mut field = String::from("mot de passe");
        let secret = Secret::take(&mut field);
        assert_eq!(secret.expose(), "mot de passe");
        assert!(field.is_empty(), "le champ source doit etre vide");
    }

    #[test]
    fn empty_secret_is_detected() {
        assert!(Secret::default().is_empty());
        assert!(Secret::new("").is_empty());
        assert!(!Secret::new("x").is_empty());
    }

    #[test]
    fn scrub_empties_the_string() {
        let mut text = String::from("secret");
        scrub(&mut text);
        assert!(text.is_empty());
    }

    #[test]
    fn redaction_masks_sensitive_fields_only() {
        assert_eq!(redact("password=hunter2"), "password=***");
        assert_eq!(redact("Production.password=x"), "Production.password=***");
        assert_eq!(redact("PASSWORD=x"), "PASSWORD=***");
        assert_eq!(redact("username=alex"), "username=alex");
        assert_eq!(redact("--vault-name"), "--vault-name");
        assert_eq!(redact("url=https://x/?a=b"), "url=https://x/?a=b");
    }
}
