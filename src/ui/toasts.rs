//! Notifications ephemeres, en bas a droite.

use egui::{Align2, Frame, Margin, RichText};

use crate::app::{SshpassApp, ToastKind};
use crate::ui::{anim, pixel};

/// Largeur de la colonne des notifications.
const COLUMN: f32 = 380.0;

/// Distance parcourue a l'entree et a la sortie.
const SLIDE: f32 = 24.0;

/// Duree du fondu de sortie, prelevee sur la fin de vie de la notification.
const FADE_OUT: f64 = 0.4;

pub fn show(app: &mut SshpassApp, ctx: &egui::Context) {
    if app.toasts.is_empty() {
        return;
    }
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let now = ctx.input(|i| i.time);
    let mut dismissed: Option<usize> = None;

    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
        .interactable(true)
        .show(ctx, |ui| {
            // Largeur imposee: le glissement se fait a l'interieur de la
            // colonne. Sans elle, la zone etant ancree a droite, decaler une
            // notification l'elargirait vers la gauche au lieu de la faire
            // entrer par le bord.
            ui.set_width(COLUMN + SLIDE);
            ui.vertical(|ui| {
                for (index, toast) in app.toasts.iter().enumerate().take(4) {
                    let color = match toast.kind {
                        ToastKind::Info => palette.accent_soft,
                        ToastKind::Success => palette.success,
                        ToastKind::Error => palette.danger,
                    };

                    // Entree par la droite, puis sortie par le meme chemin
                    // juste avant l'expiration: une notification qui
                    // s'evapore d'un coup laisse croire a un bug d'affichage.
                    let id = egui::Id::new(("toast", toast.id));
                    let entry = anim::ease_out(anim::appear(ctx, id, anim::VIEW));
                    let leaving = (((toast.expires_at - now) / FADE_OUT) as f32).clamp(0.0, 1.0);
                    let shift = anim::lerp(SLIDE, 0.0, entry)
                        + anim::lerp(SLIDE, 0.0, anim::ease_out(leaving));

                    ui.horizontal(|ui| {
                        ui.multiply_opacity(entry.min(leaving));
                        ui.add_space(shift);
                        let response = Frame::new()
                            .fill(palette.surface)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_max_width(COLUMN - 20.0);
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
                    });
                    if leaving < 1.0 {
                        // La sortie suit l'horloge, pas les animateurs d'egui:
                        // c'est a nous de reclamer les frames intermediaires.
                        ctx.request_repaint();
                    }
                    ui.add_space(6.0);
                }
            });
        });

    if let Some(index) = dismissed {
        app.toasts.remove(index);
    }
}
