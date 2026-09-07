//! Barre d'outils superieure.

use egui::{Align, Frame, Layout, Margin, RichText};

use crate::app::{Action, PassStatus, SshpassApp, ToastKind};
use crate::ui::pixel;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;

    egui::Panel::top("toolbar")
        .exact_size(44.0)
        .frame(
            Frame::new()
                .fill(palette.bg_deep)
                .inner_margin(Margin::symmetric(10, 6)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                pixel::icon_two_tone(
                    ui,
                    &pixel::TERMINAL,
                    scale,
                    palette.accent,
                    palette.accent_soft,
                );
                ui.add_space(6.0);
                ui.label(RichText::new("sshpass").strong().size(15.0));
                ui.add_space(12.0);

                if ui
                    .button("+ Connexion")
                    .on_hover_text("Nouvelle connexion SSH")
                    .clicked()
                {
                    app.actions.push(Action::NewConnection(None));
                }
                if ui
                    .button("Terminal local")
                    .on_hover_text("Ouvrir un shell local (Ctrl+Maj+T)")
                    .clicked()
                {
                    app.actions.push(Action::OpenLocalShell);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if app.show_vault_panel {
                        "Masquer le coffre"
                    } else {
                        "Coffre"
                    };
                    if ui
                        .button(label)
                        .on_hover_text("Panneau Proton Pass (Ctrl+Maj+P)")
                        .clicked()
                    {
                        app.show_vault_panel = !app.show_vault_panel;
                    }
                    ui.add_space(6.0);
                    if ui.button("Reglages").clicked() {
                        app.settings_open = true;
                    }
                    ui.add_space(6.0);
                    status_chip(app, ui);
                });
            });
        });
}

/// Pastille d'etat de `pass-cli`, cliquable pour relancer la detection.
fn status_chip(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let (color, label, tooltip) = match &app.pass_status {
        PassStatus::Probing => (
            palette.warning,
            "Proton Pass",
            "Detection en cours".to_string(),
        ),
        PassStatus::Available(version) => (
            palette.success,
            "Proton Pass",
            format!("Detecte: {version}"),
        ),
        PassStatus::Missing(err) => (palette.danger, "Proton Pass", err.clone()),
    };

    let response = ui
        .horizontal(|ui| {
            pixel::status_dot(ui, color, scale, "");
            ui.label(RichText::new(label).color(palette.text_dim).size(12.0));
        })
        .response;

    let response = response.on_hover_text(&tooltip);
    if response.interact(egui::Sense::click()).clicked() {
        app.actions.push(Action::RefreshVaults);
        app.actions.push(Action::Toast(
            "Detection de pass-cli...".into(),
            ToastKind::Info,
        ));
    }
}
