//! Encodage des evenements clavier egui en sequences d'echappement xterm.
//!
//! Les caracteres imprimables ne passent pas par ici: egui les livre via
//! `Event::Text`, deja composes (accents, IME). Ce module ne traite que les
//! touches de fonction, les touches de navigation et les combinaisons Ctrl/Alt.

use alacritty_terminal::term::TermMode;
use egui::{Key, Modifiers};

/// Code de modificateur xterm: `1 + Shift + 2*Alt + 4*Ctrl`.
fn modifier_code(mods: &Modifiers) -> u8 {
    1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl)
}

fn has_modifier(mods: &Modifiers) -> bool {
    mods.shift || mods.alt || mods.ctrl
}

/// `ESC [ <lettre>` ou, en mode curseur applicatif, `ESC O <lettre>`.
fn cursor_key(letter: char, mods: &Modifiers, mode: TermMode) -> Vec<u8> {
    if has_modifier(mods) {
        format!("\x1b[1;{}{}", modifier_code(mods), letter).into_bytes()
    } else if mode.contains(TermMode::APP_CURSOR) {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{letter}").into_bytes()
    }
}

/// `ESC [ <numero> ~`, avec modificateurs optionnels.
fn tilde_key(number: u8, mods: &Modifiers) -> Vec<u8> {
    if has_modifier(mods) {
        format!("\x1b[{};{}~", number, modifier_code(mods)).into_bytes()
    } else {
        format!("\x1b[{number}~").into_bytes()
    }
}

/// Encode une touche. `None` signifie "a laisser au flux de texte".
pub fn encode(key: Key, mods: &Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    // Ctrl+Shift+X est reserve a l'interface (copier/coller, onglets).
    if mods.ctrl && mods.shift {
        return None;
    }

    let bytes = match key {
        Key::ArrowUp => cursor_key('A', mods, mode),
        Key::ArrowDown => cursor_key('B', mods, mode),
        Key::ArrowRight => cursor_key('C', mods, mode),
        Key::ArrowLeft => cursor_key('D', mods, mode),
        Key::Home => cursor_key('H', mods, mode),
        Key::End => cursor_key('F', mods, mode),

        Key::Insert => tilde_key(2, mods),
        Key::Delete => tilde_key(3, mods),
        Key::PageUp => tilde_key(5, mods),
        Key::PageDown => tilde_key(6, mods),

        Key::F1 => function_key('P', 11, mods),
        Key::F2 => function_key('Q', 12, mods),
        Key::F3 => function_key('R', 13, mods),
        Key::F4 => function_key('S', 14, mods),
        Key::F5 => tilde_key(15, mods),
        Key::F6 => tilde_key(17, mods),
        Key::F7 => tilde_key(18, mods),
        Key::F8 => tilde_key(19, mods),
        Key::F9 => tilde_key(20, mods),
        Key::F10 => tilde_key(21, mods),
        Key::F11 => tilde_key(23, mods),
        Key::F12 => tilde_key(24, mods),

        Key::Enter => prefix_alt(b"\r".to_vec(), mods),
        Key::Escape => prefix_alt(b"\x1b".to_vec(), mods),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => prefix_alt(b"\t".to_vec(), mods),
        // Le terminal attend DEL (0x7f) et non BS (0x08) pour effacer a gauche.
        Key::Backspace if mods.ctrl => b"\x08".to_vec(),
        Key::Backspace => prefix_alt(b"\x7f".to_vec(), mods),

        Key::Space if mods.ctrl => b"\0".to_vec(),
        Key::Space if mods.alt => b"\x1b ".to_vec(),

        // Ctrl + lettre produit le code de controle correspondant.
        key if mods.ctrl => control_code(key)?,
        // Alt + caractere produit ESC suivi du caractere.
        key if mods.alt => {
            let c = printable(key)?;
            let mut bytes = vec![0x1b];
            bytes.extend_from_slice(c.to_string().as_bytes());
            bytes
        }

        _ => return None,
    };
    Some(bytes)
}

fn function_key(ss3: char, tilde: u8, mods: &Modifiers) -> Vec<u8> {
    if has_modifier(mods) {
        format!("\x1b[1;{}{}", modifier_code(mods), ss3).into_bytes()
    } else {
        let _ = tilde;
        format!("\x1bO{ss3}").into_bytes()
    }
}

fn prefix_alt(bytes: Vec<u8>, mods: &Modifiers) -> Vec<u8> {
    if mods.alt {
        let mut prefixed = vec![0x1b];
        prefixed.extend(bytes);
        prefixed
    } else {
        bytes
    }
}

/// Code de controle ASCII pour Ctrl + touche.
fn control_code(key: Key) -> Option<Vec<u8>> {
    let byte = match key {
        Key::A => 1,
        Key::B => 2,
        Key::C => 3,
        Key::D => 4,
        Key::E => 5,
        Key::F => 6,
        Key::G => 7,
        Key::H => 8,
        Key::I => 9,
        Key::J => 10,
        Key::K => 11,
        Key::L => 12,
        Key::M => 13,
        Key::N => 14,
        Key::O => 15,
        Key::P => 16,
        Key::Q => 17,
        Key::R => 18,
        Key::S => 19,
        Key::T => 20,
        Key::U => 21,
        Key::V => 22,
        Key::W => 23,
        Key::X => 24,
        Key::Y => 25,
        Key::Z => 26,
        Key::OpenBracket => 27,
        Key::Backslash => 28,
        Key::CloseBracket => 29,
        Key::Num6 => 30,  // Ctrl+^
        Key::Minus => 31, // Ctrl+_
        Key::Slash => 31,
        _ => return None,
    };
    Some(vec![byte])
}

/// Caractere de base d'une touche, pour les combinaisons Alt.
fn printable(key: Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        // Les variantes a un seul caractere (`A`, `,`) sont directes.
        (Some(c), None) => Some(c.to_ascii_lowercase()),
        _ => match key {
            Key::Num0 => Some('0'),
            Key::Num1 => Some('1'),
            Key::Num2 => Some('2'),
            Key::Num3 => Some('3'),
            Key::Num4 => Some('4'),
            Key::Num5 => Some('5'),
            Key::Num6 => Some('6'),
            Key::Num7 => Some('7'),
            Key::Num8 => Some('8'),
            Key::Num9 => Some('9'),
            Key::Minus => Some('-'),
            Key::Plus => Some('+'),
            Key::Equals => Some('='),
            Key::Comma => Some(','),
            Key::Period => Some('.'),
            Key::Slash => Some('/'),
            Key::Backslash => Some('\\'),
            Key::Semicolon => Some(';'),
            Key::Quote => Some('\''),
            Key::Backtick => Some('`'),
            Key::OpenBracket => Some('['),
            Key::CloseBracket => Some(']'),
            _ => None,
        },
    }
}

/// Encode un collage, en respectant le mode "bracketed paste".
pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    // Un CR isole serait interprete comme une validation par le shell; les
    // fins de ligne sont normalisees comme le fait alacritty.
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        text.into_bytes()
    }
}

/// Sequences envoyees a la molette quand l'ecran alternatif est actif
/// (`less`, `vim`): la plupart des programmes plein ecran attendent des
/// fleches plutot qu'un defilement d'historique.
pub fn alternate_scroll(lines: i32, mode: TermMode) -> Option<Vec<u8>> {
    if !mode.contains(TermMode::ALT_SCREEN) || !mode.contains(TermMode::ALTERNATE_SCROLL) {
        return None;
    }
    let (letter, count) = if lines > 0 {
        ('A', lines as usize)
    } else {
        ('B', lines.unsigned_abs() as usize)
    };
    if count == 0 {
        return None;
    }
    let sequence = if mode.contains(TermMode::APP_CURSOR) {
        format!("\x1bO{letter}")
    } else {
        format!("\x1b[{letter}")
    };
    Some(sequence.repeat(count).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Default::default()
        }
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    fn encoded(key: Key, mods: &Modifiers, mode: TermMode) -> String {
        String::from_utf8(encode(key, mods, mode).expect("sequence attendue")).expect("utf8")
    }

    #[test]
    fn arrows_follow_application_cursor_mode() {
        assert_eq!(encoded(Key::ArrowUp, &NONE, TermMode::NONE), "\x1b[A");
        assert_eq!(encoded(Key::ArrowUp, &NONE, TermMode::APP_CURSOR), "\x1bOA");
        assert_eq!(encoded(Key::ArrowLeft, &NONE, TermMode::NONE), "\x1b[D");
    }

    #[test]
    fn arrows_with_modifiers_use_csi_form() {
        assert_eq!(
            encoded(Key::ArrowUp, &ctrl(), TermMode::APP_CURSOR),
            "\x1b[1;5A"
        );
        assert_eq!(
            encoded(Key::ArrowRight, &shift(), TermMode::NONE),
            "\x1b[1;2C"
        );
        let both = Modifiers {
            ctrl: true,
            shift: false,
            alt: true,
            ..Default::default()
        };
        assert_eq!(encoded(Key::ArrowDown, &both, TermMode::NONE), "\x1b[1;7B");
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(encoded(Key::Delete, &NONE, TermMode::NONE), "\x1b[3~");
        assert_eq!(encoded(Key::PageUp, &NONE, TermMode::NONE), "\x1b[5~");
        assert_eq!(encoded(Key::Home, &NONE, TermMode::NONE), "\x1b[H");
        assert_eq!(encoded(Key::End, &NONE, TermMode::NONE), "\x1b[F");
        assert_eq!(encoded(Key::Delete, &ctrl(), TermMode::NONE), "\x1b[3;5~");
    }

    #[test]
    fn function_keys() {
        assert_eq!(encoded(Key::F1, &NONE, TermMode::NONE), "\x1bOP");
        assert_eq!(encoded(Key::F5, &NONE, TermMode::NONE), "\x1b[15~");
        assert_eq!(encoded(Key::F12, &NONE, TermMode::NONE), "\x1b[24~");
    }

    #[test]
    fn control_letters() {
        assert_eq!(encode(Key::C, &ctrl(), TermMode::NONE), Some(vec![3]));
        assert_eq!(encode(Key::D, &ctrl(), TermMode::NONE), Some(vec![4]));
        assert_eq!(encode(Key::Z, &ctrl(), TermMode::NONE), Some(vec![26]));
        assert_eq!(encode(Key::Space, &ctrl(), TermMode::NONE), Some(vec![0]));
        assert_eq!(
            encode(Key::OpenBracket, &ctrl(), TermMode::NONE),
            Some(vec![27])
        );
    }

    #[test]
    fn plain_letters_are_left_to_text_events() {
        assert_eq!(encode(Key::A, &NONE, TermMode::NONE), None);
        assert_eq!(encode(Key::Num1, &NONE, TermMode::NONE), None);
        assert_eq!(encode(Key::A, &shift(), TermMode::NONE), None);
    }

    #[test]
    fn ui_shortcuts_are_not_forwarded() {
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(encode(Key::C, &ctrl_shift, TermMode::NONE), None);
        assert_eq!(encode(Key::V, &ctrl_shift, TermMode::NONE), None);
        assert_eq!(encode(Key::T, &ctrl_shift, TermMode::NONE), None);
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(encoded(Key::B, &alt(), TermMode::NONE), "\x1bb");
        assert_eq!(encoded(Key::Period, &alt(), TermMode::NONE), "\x1b.");
        assert_eq!(encoded(Key::Enter, &alt(), TermMode::NONE), "\x1b\r");
    }

    #[test]
    fn editing_keys() {
        assert_eq!(
            encode(Key::Enter, &NONE, TermMode::NONE),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode(Key::Tab, &NONE, TermMode::NONE),
            Some(b"\t".to_vec())
        );
        assert_eq!(encoded(Key::Tab, &shift(), TermMode::NONE), "\x1b[Z");
        assert_eq!(
            encode(Key::Backspace, &NONE, TermMode::NONE),
            Some(vec![0x7f])
        );
        assert_eq!(
            encode(Key::Backspace, &ctrl(), TermMode::NONE),
            Some(vec![0x08])
        );
        assert_eq!(encode(Key::Escape, &NONE, TermMode::NONE), Some(vec![0x1b]));
    }

    #[test]
    fn paste_is_bracketed_when_requested() {
        assert_eq!(encode_paste("ls", TermMode::NONE), b"ls".to_vec());
        assert_eq!(
            encode_paste("ls", TermMode::BRACKETED_PASTE),
            b"\x1b[200~ls\x1b[201~".to_vec()
        );
    }

    #[test]
    fn paste_normalizes_newlines() {
        assert_eq!(
            encode_paste("a\r\nb\nc", TermMode::NONE),
            b"a\rb\rc".to_vec()
        );
    }

    #[test]
    fn alternate_scroll_only_in_alt_screen() {
        assert_eq!(alternate_scroll(3, TermMode::NONE), None);
        let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(alternate_scroll(2, mode), Some(b"\x1b[A\x1b[A".to_vec()));
        assert_eq!(alternate_scroll(-1, mode), Some(b"\x1b[B".to_vec()));
        assert_eq!(alternate_scroll(0, mode), None);
    }
}
