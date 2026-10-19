//! Palette sombre, allure de l'interface et chargement des polices systeme.
//!
//! Le texte (libelles, champs, terminal) utilise la police par defaut du
//! systeme, chargee via `fontdb` (100% Rust, pas de freetype/font-kit). Le
//! pixel art est dessine, pas ecrit: aucune police bitmap n'est necessaire.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Id, Shadow, Stroke};

use crate::config::UiStyle;

/// Famille egui pour le texte du terminal en gras.
pub const MONO_BOLD: &str = "sshpass-gui-mono-bold";

/// Palette de l'interface. Violet/gris sombre, dans l'esprit de SSH Pilot.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg_deep: Color32,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_high: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_dim: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

pub const DARK: Palette = Palette {
    bg_deep: Color32::from_rgb(0x16, 0x16, 0x1C),
    bg: Color32::from_rgb(0x1C, 0x1C, 0x24),
    surface: Color32::from_rgb(0x23, 0x23, 0x2D),
    surface_high: Color32::from_rgb(0x2C, 0x2C, 0x38),
    border: Color32::from_rgb(0x35, 0x35, 0x43),
    text: Color32::from_rgb(0xE6, 0xE6, 0xEC),
    text_dim: Color32::from_rgb(0x9A, 0x9A, 0xAB),
    accent: Color32::from_rgb(0x8B, 0x5C, 0xF6),
    accent_soft: Color32::from_rgb(0xA7, 0x8B, 0xFA),
    accent_dim: Color32::from_rgb(0x51, 0x35, 0x94),
    success: Color32::from_rgb(0x4A, 0xDE, 0x80),
    warning: Color32::from_rgb(0xFB, 0xBF, 0x24),
    danger: Color32::from_rgb(0xF8, 0x71, 0x71),
};

/// Formes et mouvements d'une allure.
///
/// La palette dit de quelle couleur sont les choses; l'allure dit comment
/// elles se posent et bougent. Elle est rangee dans la memoire d'egui par
/// `apply`, et relue par `look` la ou l'on dessine: c'est ce qui evite de la
/// faire descendre, parametre par parametre, dans chaque fonction de dessin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub style: UiStyle,
    /// Rayon des petits elements: boutons, champs, lignes de liste, onglets.
    pub small: u8,
    /// Rayon des cartes, des notifications et des menus.
    pub medium: u8,
    /// Rayon des fenetres et des panneaux.
    pub large: u8,
    /// Marge autour des panneaux, qui deviennent des cartes posees sur le
    /// fond. Nulle, ils restent colles aux bords de la fenetre.
    pub gap: i8,
    /// Depassement des apparitions: 0 pour une arrivee sans rebond.
    pub overshoot: f32,
    /// Facteur applique aux durees des transitions, pas au survol: un rebond
    /// trop bref ne se voit pas, un survol trop lent parait mou.
    pub pace: f32,
    /// Onde qui part du point d'appui d'un bouton ou d'une carte.
    pub ripple: bool,
    /// Halo autour de ce qui est actif, focalise ou survole.
    pub glow: bool,
    /// Elevation d'une carte au survol, en points.
    pub lift: f32,
}

impl Look {
    pub const fn of(style: UiStyle) -> Self {
        match style {
            UiStyle::Lively => Self {
                style,
                small: 8,
                medium: 12,
                large: 16,
                gap: 8,
                overshoot: 1.4,
                pace: 1.6,
                ripple: true,
                glow: true,
                lift: 3.0,
            },
            UiStyle::Soft => Self {
                style,
                small: 5,
                medium: 8,
                large: 10,
                gap: 6,
                overshoot: 0.0,
                pace: 1.2,
                ripple: false,
                glow: false,
                lift: 1.0,
            },
            UiStyle::Retro => Self {
                style,
                small: 0,
                medium: 0,
                large: 0,
                gap: 0,
                overshoot: 0.0,
                pace: 1.0,
                ripple: false,
                glow: false,
                lift: 0.0,
            },
        }
    }

    /// Liseres pixel art aux coins evides plutot que traits arrondis.
    pub fn pixel(&self) -> bool {
        self.style == UiStyle::Retro
    }

    /// Courbe d'arrivee: freinage simple, ou depassement puis retour.
    pub fn arrive(&self, t: f32) -> f32 {
        if self.overshoot > 0.0 {
            crate::ui::anim::ease_out_back(t, self.overshoot)
        } else {
            crate::ui::anim::ease_out(t)
        }
    }

    /// Duree d'une transition, au rythme de l'allure.
    pub fn duration(&self, base: f32) -> f32 {
        base * self.pace
    }

    /// Rayon d'une pastille: la moitie de sa hauteur, sauf en retro.
    pub fn pill(&self, height: f32) -> CornerRadius {
        if self.pixel() {
            CornerRadius::ZERO
        } else {
            CornerRadius::same((height / 2.0).round().clamp(0.0, 255.0) as u8)
        }
    }
}

impl Default for Look {
    fn default() -> Self {
        Self::of(UiStyle::default())
    }
}

fn look_id() -> Id {
    Id::new("sshpass-gui-allure")
}

fn palette_id() -> Id {
    Id::new("sshpass-gui-palette")
}

/// Allure courante, telle que `apply` l'a posee.
pub fn look(ctx: &egui::Context) -> Look {
    ctx.data(|data| data.get_temp(look_id()))
        .unwrap_or_default()
}

/// Palette courante, pour les widgets qui se dessinent seuls (`ui::widgets`).
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|data| data.get_temp(palette_id())).unwrap_or(DARK)
}

/// Applique la palette et l'allure au style egui.
///
/// En retro, tous les arrondis sont a zero: des angles droits sont la
/// condition pour que les bordures pixel art restent nettes.
pub fn apply(ctx: &egui::Context, palette: &Palette, font_size: f32, style: UiStyle) {
    let look = Look::of(style);
    ctx.data_mut(|data| {
        data.insert_temp(look_id(), look);
        data.insert_temp(palette_id(), *palette);
    });

    // L'application impose son theme sombre: on ecrit le meme style dans les
    // deux variantes pour que l'interface ne change pas si le systeme bascule
    // en clair.
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let visuals = &mut style.visuals;

    visuals.dark_mode = true;
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.surface;
    visuals.extreme_bg_color = palette.bg_deep;
    visuals.faint_bg_color = palette.surface;
    visuals.code_bg_color = palette.bg_deep;
    // Surtout pas de `override_text_color`: il uniformise tous les textes et
    // rend les textes d'aide (hint) aussi vifs que les valeurs saisies. La
    // couleur de base vient de `widgets.noninteractive.fg_stroke`, dont egui
    // derive le gris des textes secondaires.
    visuals.hyperlink_color = palette.accent_soft;
    visuals.selection.bg_fill = palette.accent_dim;
    // Le lisere d'un champ qui a le focus: blanc sur des angles droits, violet
    // la ou il s'accompagne d'un halo de la meme couleur.
    visuals.selection.stroke = Stroke::new(
        1.0,
        if look.pixel() {
            palette.text
        } else {
            palette.accent
        },
    );
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CornerRadius::same(look.large);
    visuals.menu_corner_radius = CornerRadius::same(look.medium);
    if look.pixel() {
        visuals.popup_shadow.color = Color32::from_black_alpha(120);
        visuals.window_shadow.color = Color32::from_black_alpha(140);
    } else {
        // Ombres larges et basses: avec des angles arrondis, c'est le flou qui
        // decolle une fenetre du fond, pas son lisere.
        visuals.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(130),
        };
        visuals.window_shadow = Shadow {
            offset: [0, 12],
            blur: 36,
            spread: 2,
            color: Color32::from_black_alpha(160),
        };
    }

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(look.small);
        widget.bg_stroke = Stroke::new(1.0, palette.border);
        widget.fg_stroke = Stroke::new(1.0, palette.text);
    }
    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.weak_bg_fill = palette.surface;
    visuals.widgets.inactive.bg_fill = palette.surface_high;
    visuals.widgets.inactive.weak_bg_fill = palette.surface;
    visuals.widgets.hovered.bg_fill = palette.surface_high;
    visuals.widgets.hovered.weak_bg_fill = palette.surface_high;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.bg_fill = palette.accent_dim;
    visuals.widgets.active.weak_bg_fill = palette.accent_dim;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);

    // Rythme des animations natives d'egui — fondu d'ouverture d'une fenetre,
    // depliage d'un dossier de la barre laterale. Elles n'ont pas de reglage
    // individuel: sans cette ligne, elles tourneraient a la valeur par defaut
    // (~0,08 s) pendant que les animations maison suivent `ui::anim`, et les
    // deux familles se decaleraient a l'oeil.
    style.animation_time = look.duration(crate::ui::anim::VIEW);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.interact_size.y = 24.0;

    use egui::{FontId, TextStyle};
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(font_size - 3.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(font_size, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(font_size, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(font_size + 5.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(font_size, FontFamily::Monospace),
        ),
    ]
    .into();

    let style: std::sync::Arc<egui::Style> = std::sync::Arc::new(style);
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}

/// Installe les polices systeme dans egui.
///
/// Les polices integrees a egui restent en repli: si le systeme n'expose ni
/// sans-serif ni monospace, l'application demarre quand meme.
pub fn install_system_fonts(ctx: &egui::Context) {
    let mut definitions = FontDefinitions::default();
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    // Repli si fontconfig n'a pas defini les familles generiques.
    db.set_sans_serif_family("DejaVu Sans");
    db.set_monospace_family("DejaVu Sans Mono");

    let faces = [
        (
            "sshpass-gui-sans",
            fontdb::Family::SansSerif,
            fontdb::Weight::NORMAL,
            FontFamily::Proportional,
        ),
        (
            "sshpass-gui-mono",
            fontdb::Family::Monospace,
            fontdb::Weight::NORMAL,
            FontFamily::Monospace,
        ),
    ];

    for (name, family, weight, target) in faces {
        if let Some(data) = load_face(&db, family, weight) {
            definitions
                .font_data
                .insert(name.to_string(), std::sync::Arc::new(data));
            definitions
                .families
                .entry(target)
                .or_default()
                .insert(0, name.to_string());
            log::debug!("police systeme chargee pour {name}");
        } else {
            log::warn!("aucune police systeme trouvee pour {name}, repli sur la police integree");
        }
    }

    // Le gras du terminal a sa propre famille: alacritty signale le gras par un
    // attribut de cellule, pas par un changement de police.
    let bold = load_face(&db, fontdb::Family::Monospace, fontdb::Weight::BOLD);
    let bold_key = if let Some(data) = bold {
        definitions
            .font_data
            .insert(MONO_BOLD.to_string(), std::sync::Arc::new(data));
        MONO_BOLD.to_string()
    } else {
        definitions
            .families
            .get(&FontFamily::Monospace)
            .and_then(|f| f.first().cloned())
            .unwrap_or_else(|| "sshpass-gui-mono".to_string())
    };
    let mut bold_stack = vec![bold_key];
    if let Some(mono) = definitions.families.get(&FontFamily::Monospace) {
        bold_stack.extend(mono.iter().cloned());
    }
    bold_stack.dedup();
    definitions
        .families
        .insert(FontFamily::Name(MONO_BOLD.into()), bold_stack);

    ctx.set_fonts(definitions);
}

fn load_face(
    db: &fontdb::Database,
    family: fontdb::Family<'_>,
    weight: fontdb::Weight,
) -> Option<FontData> {
    let query = fontdb::Query {
        families: &[family],
        weight,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    };
    let id = db.query(&query)?;
    let index = db.face(id).map(|face| face.index).unwrap_or(0);
    let bytes = db.with_face_data(id, |data, _| data.to_vec())?;
    let mut font = FontData::from_owned(bytes);
    font.index = index;
    Some(font)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_colors_are_opaque() {
        for color in [DARK.bg, DARK.surface, DARK.text, DARK.accent, DARK.danger] {
            assert_eq!(color.a(), 255);
        }
    }

    #[test]
    fn only_retro_keeps_square_corners() {
        for style in UiStyle::ALL {
            let look = Look::of(style);
            assert_eq!(look.pixel(), style == UiStyle::Retro, "{style:?}");
            if look.pixel() {
                assert_eq!(
                    (look.small, look.medium, look.large, look.gap),
                    (0, 0, 0, 0)
                );
            } else {
                // Les rayons croissent avec la taille de ce qu'ils arrondissent.
                assert!(0 < look.small && look.small < look.medium && look.medium < look.large);
            }
            // Une arrivee finit toujours a sa place, rebond ou non.
            assert!((look.arrive(1.0) - 1.0).abs() < 1e-6, "{style:?}");
            assert!(look.arrive(0.0).abs() < 1e-6, "{style:?}");
        }
    }

    #[test]
    fn apply_stores_the_look_for_drawing_code() {
        let ctx = egui::Context::default();
        // Sans `apply`, l'allure par defaut.
        assert_eq!(look(&ctx), Look::default());
        apply(&ctx, &DARK, 14.0, UiStyle::Retro);
        assert_eq!(look(&ctx).style, UiStyle::Retro);
        assert_eq!(
            ctx.global_style().visuals.window_corner_radius,
            CornerRadius::ZERO
        );
        apply(&ctx, &DARK, 14.0, UiStyle::Lively);
        assert_eq!(look(&ctx).style, UiStyle::Lively);
        assert_ne!(
            ctx.global_style().visuals.window_corner_radius,
            CornerRadius::ZERO
        );
    }

    #[test]
    fn text_contrasts_with_background() {
        // Luminance approximative, suffisante pour verifier qu'on n'a pas
        // interverti texte et fond.
        let luma = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
        assert!(luma(DARK.text) - luma(DARK.bg) > 120.0);
        assert!(luma(DARK.text_dim) > luma(DARK.surface));
    }
}
