//! Barre d'outils superieure.

use egui::{Align, Frame, Layout, Margin, RichText};

use crate::app::{Action, PassStatus, SshpassApp, ToastKind};
use crate::ui::{anim, pixel};

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
                ui.label(RichText::new("sshpass-gui").strong().size(15.0));
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

/// Pastille d'etat de Proton Pass, cliquable pour relancer la detection.
///
/// La couleur distingue le binaire de la session: une session fermee n'est pas
/// une panne, seulement une reconnexion a faire — d'ou l'orange plutot que le
/// rouge, reserve a ce qui demande une intervention.
fn status_chip(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let reconnecting = app.login.is_running();
    let (color, label) = match &app.pass_status {
        PassStatus::Probing => (palette.warning, "Proton Pass"),
        PassStatus::Ready { .. } => (palette.success, "Proton Pass"),
        PassStatus::LoggedOut { .. } => (palette.warning, "Session fermee"),
        PassStatus::Locked { .. } => (palette.danger, "Session verrouillee"),
        PassStatus::Missing(_) => (palette.danger, "Proton Pass"),
    };
    let (color, label) = if reconnecting {
        (palette.warning, "Reconnexion...")
    } else {
        (color, label)
    };
    let tooltip = if reconnecting {
        match app.login.state().url() {
            Some(url) => format!("Terminez la connexion dans le navigateur:\n{url}"),
            None => "Ouverture du lien de connexion Proton Pass...".to_string(),
        }
    } else {
        app.pass_status.summary()
    };

    // Une detection ou une reconnexion en cours se voit: chenillard pendant
    // que `pass-cli login` tourne, pastille qui bat pendant la detection.
    let probing = matches!(app.pass_status, PassStatus::Probing);
    let response = ui
        .horizontal(|ui| {
            if reconnecting {
                pixel::loader(ui, scale, color);
            } else if probing {
                let beat = anim::lerp(0.3, 1.0, anim::breathe(ui.ctx(), anim::PULSE));
                pixel::status_dot(ui, anim::fade(color, beat), scale, "");
            } else {
                pixel::status_dot(ui, color, scale, "");
            }
            ui.label(RichText::new(label).color(palette.text_dim).size(12.0));
        })
        .response;

    let response = response.on_hover_text(&tooltip);
    if response.interact(egui::Sense::click()).clicked() && !reconnecting {
        app.actions.push(Action::RefreshVaults);
        app.actions.push(Action::Toast(
            "Verification de la session Proton Pass...".into(),
            ToastKind::Info,
        ));
    }
}
