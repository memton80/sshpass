//! Interface: panneaux, boites de dialogue et primitives pixel art.

pub mod anim;
pub mod autocomplete;
pub mod dialogs;
pub mod editor;
pub mod home;
pub mod modal;
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
    let t = anim::hover(ui, response.id.with("hover"), response.hovered());
    // Enfoncement: le contenu descend d'un pixel tant que le bouton est tenu.
    // C'est bref (0,06 s) parce qu'un relachement mou donnerait l'impression
    // que le clic n'a pas ete pris.
    let pressed = anim::toggle(
        ui,
        response.id.with("appui"),
        response.is_pointer_button_down_on(),
        0.06,
    );
    let sink = anim::lerp(0.0, 1.0, pressed);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let idle = anim::lerp_color(palette.surface, palette.surface_high, t);
        painter.rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            anim::lerp_color(idle, palette.accent_dim, pressed),
        );
        let color = anim::lerp_color(palette.text_dim, palette.accent_soft, t.max(pressed));
        pixel::frame(
            painter,
            rect,
            anim::lerp_color(palette.border, palette.accent, t.max(pressed)),
            1.0,
        );
        pixel::draw(
            painter,
            egui::pos2(
                rect.left() + 6.0,
                rect.center().y - sprite.size(scale).y / 2.0 + sink,
            ),
            sprite,
            scale,
            color,
            color,
        );
        painter.text(
            egui::pos2(
                rect.left() + 10.0 + sprite.size(scale).x,
                rect.center().y + sink,
            ),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            palette.text,
        );
    }
    response
}
