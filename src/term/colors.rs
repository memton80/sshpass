//! Conversion des couleurs ANSI vers les couleurs egui.

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use egui::Color32;

/// Palette 16 couleurs + couleurs speciales du terminal.
#[derive(Debug, Clone)]
pub struct TerminalPalette {
    pub ansi: [Color32; 16],
    pub foreground: Color32,
    pub background: Color32,
    pub cursor: Color32,
    pub cursor_text: Color32,
    pub selection: Color32,
}

impl Default for TerminalPalette {
    fn default() -> Self {
        let palette = crate::theme::DARK;
        Self {
            ansi: [
                Color32::from_rgb(0x1C, 0x1C, 0x24), // noir
                Color32::from_rgb(0xF8, 0x71, 0x71), // rouge
                Color32::from_rgb(0x4A, 0xDE, 0x80), // vert
                Color32::from_rgb(0xFB, 0xBF, 0x24), // jaune
                Color32::from_rgb(0x60, 0xA5, 0xFA), // bleu
                Color32::from_rgb(0xA7, 0x8B, 0xFA), // magenta
                Color32::from_rgb(0x22, 0xD3, 0xEE), // cyan
                Color32::from_rgb(0xC9, 0xC9, 0xD4), // blanc
                Color32::from_rgb(0x4A, 0x4A, 0x5A), // noir clair
                Color32::from_rgb(0xFC, 0xA5, 0xA5),
                Color32::from_rgb(0x86, 0xEF, 0xAC),
                Color32::from_rgb(0xFD, 0xE6, 0x8A),
                Color32::from_rgb(0x93, 0xC5, 0xFD),
                Color32::from_rgb(0xC4, 0xB5, 0xFD),
                Color32::from_rgb(0x67, 0xE8, 0xF9),
                Color32::from_rgb(0xF4, 0xF4, 0xF8), // blanc clair
            ],
            foreground: palette.text,
            background: palette.bg_deep,
            cursor: palette.accent_soft,
            cursor_text: palette.bg_deep,
            selection: palette.accent_dim,
        }
    }
}

impl TerminalPalette {
    /// Resout une couleur de cellule.
    ///
    /// `overrides` contient les couleurs redefinies par le programme distant
    /// via OSC 4/10/11; elles ont la priorite sur la palette du theme.
    pub fn resolve(&self, color: Color, overrides: &Colors, flags: Flags) -> Color32 {
        match color {
            Color::Spec(rgb) => rgb_to_color32(rgb),
            Color::Indexed(index) => self.indexed_color(index, overrides),
            Color::Named(named) => self.named(named, overrides, flags),
        }
    }

    fn named(&self, named: NamedColor, overrides: &Colors, flags: Flags) -> Color32 {
        if let Some(rgb) = overrides[named] {
            return rgb_to_color32(rgb);
        }
        let index = match named {
            NamedColor::Foreground => {
                // Le gras eclaircit le texte par defaut, comme le fait xterm.
                return if flags.contains(Flags::BOLD) {
                    lighten(self.foreground, 1.15)
                } else if flags.contains(Flags::DIM) {
                    dim(self.foreground)
                } else {
                    self.foreground
                };
            }
            NamedColor::Background => return self.background,
            NamedColor::Cursor => return self.cursor,
            other => other as usize,
        };
        match index {
            0..=15 => self.ansi[index],
            // Couleurs "dim" (256..) : on assombrit la couleur de base.
            i if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
                dim(self.ansi[i - NamedColor::DimBlack as usize])
            }
            _ => self.foreground,
        }
    }

    pub fn indexed_color(&self, index: u8, overrides: &Colors) -> Color32 {
        if let Some(rgb) = overrides[index as usize] {
            return rgb_to_color32(rgb);
        }
        match index {
            0..=15 => self.ansi[index as usize],
            // Cube 6x6x6 des couleurs 16..231.
            16..=231 => {
                let index = index as u32 - 16;
                let level = |v: u32| if v == 0 { 0u8 } else { (v * 40 + 55) as u8 };
                Color32::from_rgb(
                    level((index / 36) % 6),
                    level((index / 6) % 6),
                    level(index % 6),
                )
            }
            // Degrade de gris 232..255.
            _ => {
                let value = 8 + (index as u32 - 232) * 10;
                let value = value.min(255) as u8;
                Color32::from_rgb(value, value, value)
            }
        }
    }
}

pub fn rgb_to_color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

pub fn color32_to_rgb(color: Color32) -> Rgb {
    Rgb {
        r: color.r(),
        g: color.g(),
        b: color.b(),
    }
}

fn dim(color: Color32) -> Color32 {
    Color32::from_rgb(
        (color.r() as f32 * 0.6) as u8,
        (color.g() as f32 * 0.6) as u8,
        (color.b() as f32 * 0.6) as u8,
    )
}

fn lighten(color: Color32, factor: f32) -> Color32 {
    let scale = |v: u8| ((v as f32 * factor).min(255.0)) as u8;
    Color32::from_rgb(scale(color.r()), scale(color.g()), scale(color.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_cube_matches_xterm() {
        let palette = TerminalPalette::default();
        let overrides = Colors::default();
        // 16 = premier de la grille (0,0,0), 231 = dernier (255,255,255).
        assert_eq!(
            palette.indexed_color(16, &overrides),
            Color32::from_rgb(0, 0, 0)
        );
        assert_eq!(
            palette.indexed_color(231, &overrides),
            Color32::from_rgb(255, 255, 255)
        );
        // 196 = rouge vif (5,0,0).
        assert_eq!(
            palette.indexed_color(196, &overrides),
            Color32::from_rgb(255, 0, 0)
        );
    }

    #[test]
    fn grayscale_ramp_is_monotonic() {
        let palette = TerminalPalette::default();
        let overrides = Colors::default();
        let mut previous = 0;
        for index in 232..=255u8 {
            let value = palette.indexed_color(index, &overrides).r();
            assert!(value >= previous, "index {index} casse la progression");
            previous = value;
        }
        assert_eq!(palette.indexed_color(232, &overrides).r(), 8);
    }

    #[test]
    fn basic_indexes_use_theme_palette() {
        let palette = TerminalPalette::default();
        let overrides = Colors::default();
        assert_eq!(palette.indexed_color(1, &overrides), palette.ansi[1]);
        assert_eq!(palette.indexed_color(15, &overrides), palette.ansi[15]);
    }

    #[test]
    fn spec_colors_pass_through() {
        let palette = TerminalPalette::default();
        let overrides = Colors::default();
        let color = palette.resolve(
            Color::Spec(Rgb { r: 1, g: 2, b: 3 }),
            &overrides,
            Flags::empty(),
        );
        assert_eq!(color, Color32::from_rgb(1, 2, 3));
    }

    #[test]
    fn named_background_and_foreground() {
        let palette = TerminalPalette::default();
        let overrides = Colors::default();
        assert_eq!(
            palette.resolve(
                Color::Named(NamedColor::Background),
                &overrides,
                Flags::empty()
            ),
            palette.background
        );
        assert_eq!(
            palette.resolve(
                Color::Named(NamedColor::Foreground),
                &overrides,
                Flags::empty()
            ),
            palette.foreground
        );
    }

    #[test]
    fn bold_brightens_default_foreground() {
        let palette = TerminalPalette::default();
        let overrides = Colors::default();
        let normal = palette.resolve(
            Color::Named(NamedColor::Foreground),
            &overrides,
            Flags::empty(),
        );
        let bold = palette.resolve(
            Color::Named(NamedColor::Foreground),
            &overrides,
            Flags::BOLD,
        );
        assert!(bold.r() >= normal.r() && bold.g() >= normal.g());
    }

    #[test]
    fn osc_overrides_win_over_theme() {
        let palette = TerminalPalette::default();
        let mut overrides = Colors::default();
        overrides[1usize] = Some(Rgb { r: 9, g: 9, b: 9 });
        assert_eq!(
            palette.indexed_color(1, &overrides),
            Color32::from_rgb(9, 9, 9)
        );
    }

    #[test]
    fn rgb_conversions_roundtrip() {
        let rgb = Rgb {
            r: 10,
            g: 20,
            b: 30,
        };
        assert_eq!(color32_to_rgb(rgb_to_color32(rgb)), rgb);
    }
}
