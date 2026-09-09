//! Boites de dialogue: confirmation, creation de dossier, reglages.

use egui::{Margin, RichText};

use crate::app::{Action, SshpassApp};
use crate::config::AgentMode;
use crate::ui;

pub fn show(app: &mut SshpassApp, ctx: &egui::Context, _now: f64) {
    delete_confirmation(app, ctx);
    new_folder(app, ctx);
    settings(app, ctx);
}

fn delete_confirmation(app: &mut SshpassApp, ctx: &egui::Context) {
    let Some(id) = app.pending_delete.clone() else {
        return;
    };
    let palette = app.palette;
    let Some(name) = app.config.connection(&id).map(|c| c.display_name()) else {
        app.pending_delete = None;
        return;
    };

    egui::Window::new("Supprimer la connexion")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(palette.surface)
                .inner_margin(Margin::same(14)),
        )
        .show(ctx, |ui| {
            ui.label(format!("Supprimer « {name} » ?"));
            ui::hint(
                ui,
                "Les secrets restent dans Proton Pass; seule la reference est retiree.",
                &palette,
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Supprimer").color(palette.danger))
                    .clicked()
                {
                    app.actions.push(Action::DeleteConnection(id.clone()));
                }
                if ui.button("Annuler").clicked() {
                    app.pending_delete = None;
                }
            });
        });
}

fn new_folder(app: &mut SshpassApp, ctx: &egui::Context) {
    let Some(mut name) = app.new_folder_name.clone() else {
        return;
    };
    let palette = app.palette;
    let mut close = false;
    let mut create = false;

    egui::Window::new("Nouveau dossier")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(palette.surface)
                .inner_margin(Margin::same(14)),
        )
        .show(ctx, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text("Production")
                    .desired_width(240.0),
            );
            // Focus a l'ouverture seulement: le redemander a chaque frame
            // empecherait la validation par Entree d'etre detectee.
            if ui.memory(|m| m.focused()).is_none() {
                response.request_focus();
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Creer").clicked() {
                    create = true;
                }
                if ui.button("Annuler").clicked() {
                    close = true;
                }
            });
        });

    app.new_folder_name = Some(name.clone());
    if create && !name.trim().is_empty() {
        app.actions.push(Action::NewFolder(name));
        close = true;
    }
    if close {
        app.new_folder_name = None;
    }
}

fn settings(app: &mut SshpassApp, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let palette = app.palette;
    let mut open = true;
    let mut changed = false;

    egui::Window::new("Reglages")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(palette.surface)
                .inner_margin(Margin::same(14)),
        )
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui::section_title(ui, "Proton Pass", &palette);
            egui::Grid::new("settings_pass")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Binaire");
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.proton_pass.binary)
                                .hint_text("pass-cli")
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Mode agent");
                    let mode = app.config.proton_pass.agent_mode;
                    egui::ComboBox::from_id_salt("agent_mode")
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            for candidate in [
                                AgentMode::OwnAgent,
                                AgentMode::LoadIntoExisting,
                                AgentMode::Disabled,
                            ] {
                                changed |= ui
                                    .selectable_value(
                                        &mut app.config.proton_pass.agent_mode,
                                        candidate,
                                        candidate.label(),
                                    )
                                    .changed();
                            }
                        });
                    ui.end_row();

                    ui.label("Rafraichissement");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.config.proton_pass.refresh_interval)
                                .range(60..=86_400)
                                .suffix(" s"),
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Reconnexion");
                    changed |= ui
                        .checkbox(
                            &mut app.config.proton_pass.auto_login,
                            "Rouvrir la session automatiquement",
                        )
                        .on_hover_text(
                            "Une session Proton Pass ne survit pas a l'arret de la machine. \
                             Quand elle est fermee, sshpass-gui lance `pass-cli login` et \
                             ouvre le lien d'authentification dans le navigateur.",
                        )
                        .changed();
                    ui.end_row();
                });

            ui::separator(ui, &palette);
            ui::section_title(ui, "Affichage", &palette);
            egui::Grid::new("settings_ui")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Police interface");
                    changed |= ui
                        .add(egui::DragValue::new(&mut app.config.ui.font_size).range(10.0..=22.0))
                        .changed();
                    ui.end_row();

                    ui.label("Police terminal");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.config.ui.terminal_font_size)
                                .range(8.0..=28.0),
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Echelle pixel art");
                    changed |= ui
                        .add(egui::DragValue::new(&mut app.config.ui.pixel_scale).range(1..=4))
                        .changed();
                    ui.end_row();

                    ui.label("Historique");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.config.ui.scrollback_lines)
                                .range(1000..=200_000)
                                .suffix(" lignes"),
                        )
                        .changed();
                    ui.end_row();
                });

            ui::separator(ui, &palette);
            ui::hint(
                ui,
                &format!("Configuration: {}", crate::config::config_path().display()),
                &palette,
            );
        });

    if changed {
        crate::theme::apply(ctx, &palette, app.config.ui.font_size);
        app.agents.reconfigure(
            &app.config.proton_pass.binary.clone(),
            app.config.proton_pass.agent_mode,
            app.config.proton_pass.refresh_interval,
        );
        app.save_config();
    }
    if !open {
        app.settings_open = false;
    }
}
