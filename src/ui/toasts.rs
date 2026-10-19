//! Notifications ephemeres, en bas a droite.

use egui::{Align2, Frame, Margin, Rect, RichText, Vec2};

use crate::app::{SshpassApp, ToastKind, TOAST_DURATION};
use crate::theme;
use crate::ui::{anim, widgets};

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
    let look = theme::look(ctx);
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

                    // Entree par la droite — avec un rebond dans l'allure
                    // « Vivant » —, puis sortie par le meme chemin juste avant
                    // l'expiration: une notification qui s'evapore d'un coup
                    // laisse croire a un bug d'affichage.
                    let id = egui::Id::new(("toast", toast.id));
                    let entry = anim::appear(ctx, id, look.duration(anim::VIEW));
                    let leaving = (((toast.expires_at - now) / FADE_OUT) as f32).clamp(0.0, 1.0);
                    let shift = anim::lerp_unclamped(SLIDE, 0.0, look.arrive(entry))
                        + anim::lerp(SLIDE, 0.0, anim::ease_out(leaving));
                    // Temps qui reste, pour la jauge du bas. Une notification
                    // sans echeance (erreur de sauvegarde) n'en a pas.
                    let life = ((toast.expires_at - now) / TOAST_DURATION) as f32;

                    ui.horizontal(|ui| {
                        ui.multiply_opacity(entry.min(leaving).clamp(0.0, 1.0));
                        ui.add_space(shift.max(0.0));
                        // Fond et halo passent sous le contenu: on reserve leur
                        // place avant de le dessiner.
                        let glow = ui.painter().add(egui::Shape::Noop);
                        let background = ui.painter().add(egui::Shape::Noop);
                        let response = Frame::new()
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_max_width(COLUMN - 24.0);
                                ui.horizontal(|ui| {
                                    widgets::status_dot(ui, color, scale, "");
                                    ui.label(RichText::new(&toast.message).size(12.0));
                                });
                            })
                            .response;
                        let rect = response.rect;
                        let radius = look.medium;
                        let hover = anim::hover(ui, id.with("survol"), response.hovered());
                        if look.glow {
                            ui.painter()
                                .set(glow, widgets::glow(rect, radius, color, 0.35 + 0.3 * hover));
                        } else if !look.pixel() {
                            ui.painter()
                                .set(glow, widgets::shadow(rect, radius, 2.0, 1.0));
                        }
                        ui.painter().set(
                            background,
                            egui::Shape::rect_filled(
                                rect,
                                radius,
                                anim::lerp_color(palette.surface, palette.surface_high, hover),
                            ),
                        );
                        widgets::outline(
                            ui.painter(),
                            rect,
                            radius,
                            if look.pixel() {
                                color
                            } else {
                                anim::fade(color, anim::lerp(0.5, 1.0, hover))
                            },
                            &look,
                        );
                        if (0.0..=1.0).contains(&life) && !look.pixel() {
                            // Jauge de vie: elle se vide de droite a gauche.
                            let inset = radius as f32;
                            let bar = Rect::from_min_size(
                                egui::pos2(rect.left() + inset, rect.bottom() - 3.0),
                                Vec2::new((rect.width() - 2.0 * inset) * life, 2.0),
                            );
                            ui.painter()
                                .rect_filled(bar, look.pill(2.0), anim::fade(color, 0.7));
                            ctx.request_repaint();
                        }
                        if response.interact(egui::Sense::click()).clicked() {
                            dismissed = Some(index);
                        }
                    });
                    if leaving < 1.0 {
                        // La sortie suit l'horloge, pas les animateurs d'egui:
                        // c'est a nous de reclamer les frames intermediaires.
                        ctx.request_repaint();
                    }
                    ui.add_space(8.0);
                }
            });
        });

    if let Some(index) = dismissed {
        app.toasts.remove(index);
    }
}
