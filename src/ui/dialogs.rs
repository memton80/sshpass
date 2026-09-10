//! Boites de dialogue: confirmation, creation de dossier, reglages.

use egui::RichText;

use crate::app::{Action, SshpassApp};
use crate::config::AgentMode;
use crate::ui::{self, modal::Modal};

pub fn show(app: &mut SshpassApp, ctx: &egui::Context, _now: f64) {
    delete_confirmation(app, ctx);
    new_folder(app, ctx);
    settings(app, ctx);
    // Apres toutes les modales: c'est l'absence d'une fenetre a cette passe
    // qui declenche son animation de fermeture.
    ui::modal::fade_out_closed(ctx, &app.palette);
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

    Modal::new("Supprimer la connexion").show(ctx, &palette, |ui| {
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

    Modal::new("Nouveau dossier").show(ctx, &palette, |ui| {
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

    Modal::new("Reglages")
        .closable(&mut open)
        .width(460.0)
        .show(ctx, &palette, |ui| {
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
            ui::section_title(ui, "Securite", &palette);
            ui::hint(
                ui,
                "Chaque case decochee est fermee volontairement: la cocher ouvre \
                 quelque chose a la machine distante ou aux autres comptes du poste.",
                &palette,
            );
            ui.add_space(4.0);
            egui::Grid::new("settings_security")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Presse-papiers");
                    changed |= ui
                        .checkbox(
                            &mut app.config.security.remote_clipboard_read,
                            "Le serveur peut LIRE le presse-papiers",
                        )
                        .on_hover_text(
                            "Deconseille. La sequence OSC 52 permet a une application \
                             distante de demander le contenu du presse-papiers local, et \
                             sshpass-gui le lui renverrait — sans que vous ayez colle quoi \
                             que ce soit. Un presse-papiers d'administrateur contient \
                             souvent un mot de passe ou un jeton.",
                        )
                        .changed();
                    ui.end_row();

                    ui.label("");
                    changed |= ui
                        .checkbox(
                            &mut app.config.security.remote_clipboard_write,
                            "Le serveur peut ECRIRE dans le presse-papiers",
                        )
                        .on_hover_text(
                            "Bien plus benin que la lecture, et utile: c'est ce qui fait \
                             marcher la copie depuis tmux ou vim a distance.",
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Ecriture max");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.config.security.clipboard_write_limit)
                                .range(1024..=1_048_576)
                                .suffix(" o"),
                        )
                        .on_hover_text(
                            "Au-dela, l'ecriture est ignoree: un distant hostile ne \
                             remplit pas le presse-papiers du poste a chaque frappe.",
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Repli /tmp");
                    changed |= ui
                        .checkbox(
                            &mut app.config.security.allow_temp_runtime_dir,
                            "Utiliser /tmp faute de XDG_RUNTIME_DIR",
                        )
                        .on_hover_text(
                            "Les scripts askpass et les sockets d'agent SSH y seraient \
                             poses. /tmp est partage par tous les comptes de la machine.",
                        )
                        .changed();
                    ui.end_row();

                    ui.label("Mise a jour");
                    changed |= ui
                        .checkbox(
                            &mut app.config.security.allow_argv_fallback,
                            "Mot de passe en ligne de commande si besoin",
                        )
                        .on_hover_text(
                            "Le gabarit part normalement par l'entree standard. Si la \
                             version de `pass-cli` installee ne le sait pas, le seul autre \
                             chemin place le mot de passe dans /proc/<pid>/cmdline, que \
                             tous les comptes de la machine peuvent lire.",
                        )
                        .changed();
                    ui.end_row();
                });

            if !crate::config::has_private_runtime_dir() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "XDG_RUNTIME_DIR est absent sur ce poste: sans le repli, \
                         l'authentification par mot de passe et l'agent dedie ne \
                         demarreront pas.",
                    )
                    .size(11.0)
                    .color(palette.danger),
                );
            }

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
            app.config.security.allow_temp_runtime_dir,
        );
        app.save_config();
    }
    if !open {
        app.settings_open = false;
    }
}
