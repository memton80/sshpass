//! Palette sombre et chargement des polices systeme.
//!
//! Le texte (libelles, champs, terminal) utilise la police par defaut du
//! systeme, chargee via `fontdb` (100% Rust, pas de freetype/font-kit). Le
//! pixel art est dessine, pas ecrit: aucune police bitmap n'est necessaire.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};

/// Famille egui pour le texte du terminal en gras.
pub const MONO_BOLD: &str = "sshpass-mono-bold";

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

/// Applique la palette au style egui.
///
/// Tous les arrondis sont a zero: des angles droits sont la condition pour que
/// les bordures pixel art restent nettes.
pub fn apply(ctx: &egui::Context, palette: &Palette, font_size: f32) {
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
    visuals.selection.stroke = Stroke::new(1.0, palette.text);
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.popup_shadow.color = Color32::from_black_alpha(120);
    visuals.window_shadow.color = Color32::from_black_alpha(140);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::ZERO;
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
            "sshpass-sans",
            fontdb::Family::SansSerif,
            fontdb::Weight::NORMAL,
            FontFamily::Proportional,
        ),
        (
            "sshpass-mono",
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
            .unwrap_or_else(|| "sshpass-mono".to_string())
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
    fn text_contrasts_with_background() {
        // Luminance approximative, suffisante pour verifier qu'on n'a pas
        // interverti texte et fond.
        let luma = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
        assert!(luma(DARK.text) - luma(DARK.bg) > 120.0);
        assert!(luma(DARK.text_dim) > luma(DARK.surface));
    }
}
