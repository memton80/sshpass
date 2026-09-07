//! Notifications ephemeres, en bas a droite.

use egui::{Align2, Frame, Margin, RichText};

use crate::app::{SshpassApp, ToastKind};
use crate::ui::pixel;

pub fn show(app: &mut SshpassApp, ctx: &egui::Context) {
    if app.toasts.is_empty() {
        return;
    }
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let mut dismissed: Option<usize> = None;

    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
        .interactable(true)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                for (index, toast) in app.toasts.iter().enumerate().take(4) {
                    let color = match toast.kind {
                        ToastKind::Info => palette.accent_soft,
                        ToastKind::Success => palette.success,
                        ToastKind::Error => palette.danger,
                    };
                    let response = Frame::new()
                        .fill(palette.surface)
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.horizontal(|ui| {
                                pixel::status_dot(ui, color, scale, "");
                                ui.label(RichText::new(&toast.message).size(12.0));
                            });
                        })
                        .response;
                    pixel::frame(ui.painter(), response.rect, color, scale);
                    if response.interact(egui::Sense::click()).clicked() {
                        dismissed = Some(index);
                    }
                    ui.add_space(6.0);
                }
            });
        });

    if let Some(index) = dismissed {
        app.toasts.remove(index);
    }
}
