//! Interface: panneaux, boites de dialogue et primitives pixel art.

pub mod dialogs;
pub mod editor;
pub mod home;
pub mod pixel;
pub mod sidebar;
pub mod tabs;
pub mod toasts;
pub mod toolbar;
pub mod vault;

use crate::theme::Palette;

/// Intitule de section: petites capitales grises, comme dans SSH Pilot.
pub fn section_title(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .color(palette.text_dim)
            .size(11.0)
            .strong(),
    );
    ui.add_space(2.0);
}

/// Texte secondaire.
pub fn hint(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.label(egui::RichText::new(text).color(palette.text_dim).size(12.0));
}

/// Separateur pixel: une ligne d'un pixel, sans degrade.
pub fn separator(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(4.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, palette.border);
    ui.add_space(4.0);
}

/// Bouton compose d'un sprite pixel art et d'un libelle.
pub fn pixel_button(
    ui: &mut egui::Ui,
    sprite: &pixel::Sprite,
    label: &str,
    palette: &Palette,
    scale: f32,
) -> egui::Response {
    let text_width = label.chars().count() as f32 * 7.0;
    let size = egui::vec2(text_width + 16.0 + sprite.size(scale).x, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if response.hovered() {
            palette.surface_high
        } else {
            palette.surface
        };
        painter.rect_filled(rect, egui::CornerRadius::ZERO, fill);
        let color = if response.hovered() {
            palette.accent_soft
        } else {
            palette.text_dim
        };
        pixel::frame(
            painter,
            rect,
            if response.hovered() {
                palette.accent
            } else {
                palette.border
            },
            1.0,
        );
        pixel::draw(
            painter,
            egui::pos2(
                rect.left() + 6.0,
                rect.center().y - sprite.size(scale).y / 2.0,
            ),
            sprite,
            scale,
            color,
            color,
        );
        painter.text(
            egui::pos2(rect.left() + 10.0 + sprite.size(scale).x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            palette.text,
        );
    }
    response
}
