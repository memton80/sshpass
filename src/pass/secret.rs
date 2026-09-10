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
pub const SENSITIVE_FIELDS: &[&str] = &[
    "password",
    "passphrase",
    "secret",
    "totp",
    "privatekey",
    // Variantes rencontrees dans les sorties JSON et les messages d'erreur,
    // ou le separateur survit a la normalisation.
    "private_key",
    "private-key",
    "apikey",
    "api_key",
    "token",
];

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

/// Longueur au-dela de laquelle la sortie d'un programme externe est coupee.
///
/// Ce qui vient de `pass-cli` finit dans une notification et dans le journal.
/// Un binaire bavard — ou remplace — pourrait y deverser des megaoctets.
pub const MAX_EXTERNAL_OUTPUT: usize = 2000;

/// Passage oblige de tout texte produit par un programme externe avant qu'il
/// n'atteigne l'interface, une notification ou le journal.
///
/// `redact` ne protege que ce que **nous** ecrivons: les arguments que nous
/// avons construits. Or la sortie d'erreur de `pass-cli`, elle, est reprise
/// telle quelle — et rien ne garantit qu'une version future ne recopiera pas
/// l'argument fautif (`failed to update password=hunter2`) dans son message.
/// Trois traitements, dans cet ordre:
///
/// 1. masquage des `cle=valeur` et des `"cle": "valeur"` sensibles, **ou
///    qu'ils soient** dans le texte et pas seulement en tete d'argument;
/// 2. neutralisation des caracteres de controle. Un `\x1b]0;...\x07` glisse
///    dans un message d'erreur repeint le terminal ou le titre de la fenetre:
///    une sortie externe ne doit jamais pouvoir piloter l'affichage;
/// 3. troncature a `MAX_EXTERNAL_OUTPUT`.
pub fn sanitize_external_output(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len().min(MAX_EXTERNAL_OUTPUT + 16));
    for ch in redact_inline(text).chars() {
        match ch {
            '\n' | '\t' => cleaned.push(ch),
            c if c.is_control() => cleaned.push('\u{fffd}'),
            c => cleaned.push(c),
        }
    }
    truncate(&cleaned)
}

/// Coupe proprement sur une frontiere de caractere, et le dit.
fn truncate(text: &str) -> String {
    if text.len() <= MAX_EXTERNAL_OUTPUT {
        return text.to_string();
    }
    let mut end = MAX_EXTERNAL_OUTPUT;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (sortie tronquee)", &text[..end])
}

/// Masque `cle=valeur` et `"cle": "valeur"` partout dans un texte libre.
///
/// La recherche se fait sur une copie en minuscules **ASCII**: `to_lowercase`
/// peut changer la longueur de certains caracteres (« İ »), et les decalages
/// ne correspondraient plus a la chaine d'origine.
fn redact_inline(text: &str) -> String {
    let haystack = text.to_ascii_lowercase();
    let bytes = haystack.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;

    while cursor < bytes.len() {
        let Some((start, field)) = next_sensitive_field(&haystack, cursor) else {
            break;
        };
        let after_name = start + field.len();
        let Some(value) = value_span(bytes, after_name) else {
            // Le nom apparait sans valeur derriere (« password requis »):
            // rien a masquer, on avance d'un cran et on continue.
            out.push_str(&text[cursor..after_name]);
            cursor = after_name;
            continue;
        };
        out.push_str(&text[cursor..value.0]);
        out.push_str("***");
        cursor = value.1;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Prochaine occurrence d'un nom de champ sensible, isolee par ses frontieres.
///
/// `password` doit etre reconnu dans `password=x` et `"password":"x"`, mais
/// pas au milieu de `oldpasswordhash`.
fn next_sensitive_field(haystack: &str, from: usize) -> Option<(usize, &'static str)> {
    let bytes = haystack.as_bytes();
    let mut best: Option<(usize, &'static str)> = None;
    for &field in SENSITIVE_FIELDS {
        let mut at = from;
        while let Some(offset) = haystack[at..].find(field) {
            let start = at + offset;
            let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
            let after = start + field.len();
            let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                if best.is_none_or(|(seen, _)| start < seen) {
                    best = Some((start, field));
                }
                break;
            }
            at = start + field.len();
        }
    }
    best
}

/// Etendue de la valeur qui suit un nom de champ, si valeur il y a.
///
/// Accepte `=`, `:` et les guillemets qui les entourent, pour couvrir aussi
/// bien la ligne de commande que le JSON.
fn value_span(bytes: &[u8], after_name: usize) -> Option<(usize, usize)> {
    let mut i = after_name;
    // Guillemet fermant du nom, puis espaces, puis le separateur.
    if bytes.get(i) == Some(&b'"') {
        i += 1;
    }
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    if !matches!(bytes.get(i), Some(b'=' | b':')) {
        return None;
    }
    i += 1;
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    let quoted = bytes.get(i) == Some(&b'"');
    let start = i;
    if quoted {
        i += 1;
    }
    while i < bytes.len() {
        let byte = bytes[i];
        if quoted {
            // Un guillemet echappe fait partie de la valeur.
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == b'"' {
                i += 1;
                break;
            }
        } else if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b',' | b'}' | b')') {
            break;
        }
        i += 1;
    }
    (i > start).then_some((start, i.min(bytes.len())))
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

    #[test]
    fn external_output_is_stripped_of_secrets() {
        // Le cas qui motive la fonction: `pass-cli` recopie l'argument fautif.
        assert_eq!(
            sanitize_external_output("failed to update password=hunter2"),
            "failed to update password=***"
        );
        // Forme JSON, avec ou sans espace apres le deux-points.
        assert_eq!(
            sanitize_external_output(r#"{"title":"web","password": "hunter2"}"#),
            r#"{"title":"web","password": ***}"#
        );
        // Un nom sensible sans valeur derriere n'est pas mutile.
        assert_eq!(
            sanitize_external_output("password requis"),
            "password requis"
        );
        // Un mot qui contient seulement le nom n'est pas une fuite.
        assert_eq!(
            sanitize_external_output("oldpasswordhash mismatch"),
            "oldpasswordhash mismatch"
        );
        // Plusieurs occurrences dans le meme message.
        assert_eq!(
            sanitize_external_output("password=a et token=b"),
            "password=*** et token=***"
        );
    }

    #[test]
    fn external_output_cannot_drive_the_terminal() {
        let hostile = "erreur \x1b]0;pirate\x07 \x1b[2J fin";
        let cleaned = sanitize_external_output(hostile);
        assert!(
            !cleaned.contains('\x1b'),
            "sequence ANSI conservee: {cleaned}"
        );
        assert!(!cleaned.contains('\x07'));
        // Le texte lisible, lui, survit.
        assert!(cleaned.contains("erreur"));
        assert!(cleaned.contains("fin"));
        // Les sauts de ligne et tabulations restent: ils mettent en forme.
        assert_eq!(sanitize_external_output("a\nb\tc"), "a\nb\tc");
    }

    #[test]
    fn external_output_survives_anything_it_is_handed() {
        // La sortie d'un programme externe n'est pas du texte bien eleve: elle
        // peut couper un mot sensible, coller un caractere multi-octets contre
        // un guillemet echappe, ou s'arreter au milieu. Rien de tout cela ne
        // doit paniquer sur une frontiere de caractere.
        for hostile in [
            "password=é",
            "password=\"a\\é\"",
            "password=",
            "password",
            "\"password\":",
            "\"password\":\"\\\"é\"",
            "é=password",
            "passwordé=x",
            "\u{0}password\u{0}=x",
            "password=é\u{1b}[2Jsuite",
            "「password」=秘密",
        ] {
            let cleaned = sanitize_external_output(hostile);
            assert!(!cleaned.contains('\u{1b}'), "sequence ANSI: {cleaned}");
        }
    }

    #[test]
    fn external_output_is_bounded() {
        let flood = "x".repeat(MAX_EXTERNAL_OUTPUT * 3);
        let cleaned = sanitize_external_output(&flood);
        assert!(cleaned.len() < flood.len());
        assert!(cleaned.ends_with("(sortie tronquee)"));

        // La coupe tombe sur une frontiere de caractere, jamais au milieu.
        let accents = "é".repeat(MAX_EXTERNAL_OUTPUT);
        assert!(sanitize_external_output(&accents).ends_with("(sortie tronquee)"));
    }
}
