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
pub mod widgets;

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
