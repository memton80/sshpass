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

/// Combinaisons `Ctrl+Maj+…` que l'interface se reserve.
///
/// Le reste des `Ctrl+Maj` descend dans le PTY: `Ctrl+Maj+-` est le `^_` que
/// `readline` attend pour annuler, `Ctrl+Maj+&` le `^^` de `vim`. Les bloquer
/// tous, comme on le faisait, privait le terminal de touches qui n'ont aucun
/// equivalent sans `Maj` sur un clavier francais.
fn is_ui_shortcut(key: Key) -> bool {
    matches!(
        key,
        Key::C | Key::V | Key::X | Key::A | Key::W | Key::T | Key::P | Key::F | Key::Tab
    )
}

/// Encode une touche. `None` signifie "a laisser au flux de texte".
pub fn encode(key: Key, mods: &Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    if mods.ctrl && mods.shift && is_ui_shortcut(key) {
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
            // `Alt+Maj+B` vaut `ESC B` et non `ESC b`: c'est ainsi que
            // `readline` distingue « mot precedent » de « majuscule au mot ».
            let c = if mods.shift {
                c.to_ascii_uppercase()
            } else {
                c
            };
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

/// Bouton de souris, numerote comme le fait xterm.
///
/// La molette n'est pas un bouton pour le systeme, mais elle en est un pour un
/// terminal: c'est ainsi que `less`, `htop` ou `tmux` la recoivent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    /// Aucun bouton: le code que xterm reserve au simple survol.
    None = 3,
    WheelUp = 64,
    WheelDown = 65,
}

/// Ce qui arrive au bouton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// Deplacement du pointeur, bouton tenu ou non.
    Motion,
}

/// Vrai si le programme distant a demande a recevoir la souris.
pub fn wants_mouse(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_MODE)
}

/// Encode un evenement souris pour le programme distant.
///
/// `column` et `line` sont des coordonnees de grille comptees a partir de
/// zero. `None` quand le distant ne demande pas la souris, ou quand la
/// position sort de ce que l'encodage historique sait dire.
pub fn mouse_report(
    button: MouseButton,
    action: MouseAction,
    column: usize,
    line: usize,
    mods: &Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !wants_mouse(mode) {
        return None;
    }

    let modifiers = 4 * u8::from(mods.shift) + 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let base = match action {
        MouseAction::Press | MouseAction::Motion => button as u8,
        // Seul le mode SGR sait dire quel bouton a ete relache; l'encodage
        // historique n'a qu'un code unique pour « un bouton s'est leve ».
        MouseAction::Release if sgr => button as u8,
        MouseAction::Release => 3,
    };
    let motion = if matches!(action, MouseAction::Motion) {
        32
    } else {
        0
    };
    let code = base + modifiers + motion;

    if sgr {
        let final_byte = match action {
            MouseAction::Release => 'm',
            _ => 'M',
        };
        return Some(
            format!("\x1b[<{};{};{}{}", code, column + 1, line + 1, final_byte).into_bytes(),
        );
    }

    // Encodage historique: chaque coordonnee tient dans un octet, decale de
    // 33. Au-dela, il n'y a rien a envoyer qui ne soit pas faux.
    let utf8 = mode.contains(TermMode::UTF8_MOUSE);
    let limit = if utf8 { 2015 } else { 223 };
    if column >= limit || line >= limit {
        return None;
    }
    let mut bytes = vec![0x1b, b'[', b'M', 32 + code];
    push_coordinate(column, utf8, &mut bytes);
    push_coordinate(line, utf8, &mut bytes);
    Some(bytes)
}

/// Une coordonnee de l'encodage historique, eventuellement en UTF-8.
fn push_coordinate(value: usize, utf8: bool, out: &mut Vec<u8>) {
    let value = value + 33;
    if utf8 && value >= 128 {
        out.push((0xC0 + value / 64) as u8);
        out.push((0x80 + (value & 63)) as u8);
    } else {
        out.push(value as u8);
    }
}

/// Prise et perte du focus, pour les programmes qui la demandent (`vim`
/// recharge un fichier modifie, `tmux` change la teinte du volet actif).
pub fn focus_report(focused: bool, mode: TermMode) -> Option<Vec<u8>> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return None;
    }
    Some(match focused {
        true => b"\x1b[I".to_vec(),
        false => b"\x1b[O".to_vec(),
    })
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
    fn reserved_ui_shortcuts_are_not_forwarded_but_the_others_are() {
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(encode(Key::C, &ctrl_shift, TermMode::NONE), None);
        assert_eq!(encode(Key::V, &ctrl_shift, TermMode::NONE), None);
        assert_eq!(encode(Key::Tab, &ctrl_shift, TermMode::NONE), None);
        // `Ctrl+Maj+-` est le `^_` de readline: il doit descendre.
        assert_eq!(
            encode(Key::Minus, &ctrl_shift, TermMode::NONE),
            Some(vec![31])
        );
        assert_eq!(encode(Key::Z, &ctrl_shift, TermMode::NONE), Some(vec![26]));
    }

    #[test]
    fn alt_shift_sends_an_uppercase_letter() {
        let alt_shift = Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(encoded(Key::B, &alt_shift, TermMode::NONE), "\x1bB");
    }

    #[test]
    fn the_mouse_is_silent_unless_the_program_asks_for_it() {
        assert_eq!(
            mouse_report(
                MouseButton::Left,
                MouseAction::Press,
                0,
                0,
                &NONE,
                TermMode::NONE
            ),
            None
        );
    }

    #[test]
    fn sgr_reports_name_the_released_button() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let press = mouse_report(MouseButton::Right, MouseAction::Press, 10, 4, &NONE, mode)
            .expect("rapport attendu");
        assert_eq!(String::from_utf8(press).expect("utf8"), "\x1b[<2;11;5M");

        let release = mouse_report(MouseButton::Right, MouseAction::Release, 10, 4, &NONE, mode)
            .expect("rapport attendu");
        assert_eq!(String::from_utf8(release).expect("utf8"), "\x1b[<2;11;5m");
    }

    #[test]
    fn legacy_reports_lose_the_button_on_release() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        let press = mouse_report(MouseButton::Middle, MouseAction::Press, 0, 0, &NONE, mode)
            .expect("rapport attendu");
        assert_eq!(press, vec![0x1b, b'[', b'M', 32 + 1, 33, 33]);
        let release = mouse_report(MouseButton::Middle, MouseAction::Release, 0, 0, &NONE, mode)
            .expect("rapport attendu");
        assert_eq!(release, vec![0x1b, b'[', b'M', 32 + 3, 33, 33]);
    }

    #[test]
    fn motion_and_modifiers_shift_the_button_code() {
        let mode = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        let report = mouse_report(MouseButton::Left, MouseAction::Motion, 0, 0, &ctrl(), mode)
            .expect("rapport attendu");
        // 0 (bouton gauche) + 16 (Ctrl) + 32 (deplacement).
        assert_eq!(String::from_utf8(report).expect("utf8"), "\x1b[<48;1;1M");
    }

    #[test]
    fn the_wheel_is_a_button_for_the_remote_program() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let report = mouse_report(MouseButton::WheelUp, MouseAction::Press, 2, 3, &NONE, mode)
            .expect("rapport attendu");
        assert_eq!(String::from_utf8(report).expect("utf8"), "\x1b[<64;3;4M");
    }

    #[test]
    fn legacy_reports_stop_where_the_encoding_does() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert!(mouse_report(MouseButton::Left, MouseAction::Press, 222, 0, &NONE, mode).is_some());
        assert_eq!(
            mouse_report(MouseButton::Left, MouseAction::Press, 223, 0, &NONE, mode),
            None,
            "au-dela de 223 colonnes l'encodage historique ment"
        );
        // En UTF-8 la coordonnee passe sur deux octets au lieu de deborder.
        let utf8 = mode | TermMode::UTF8_MOUSE;
        let report = mouse_report(MouseButton::Left, MouseAction::Press, 200, 0, &NONE, utf8)
            .expect("rapport attendu");
        assert_eq!(report.len(), 7, "colonne sur deux octets attendue");
    }

    #[test]
    fn focus_is_only_reported_when_requested() {
        assert_eq!(focus_report(true, TermMode::NONE), None);
        assert_eq!(
            focus_report(true, TermMode::FOCUS_IN_OUT),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            focus_report(false, TermMode::FOCUS_IN_OUT),
            Some(b"\x1b[O".to_vec())
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
