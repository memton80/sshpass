//! Panneau Proton Pass: coffres, items et etat des agents SSH.

use egui::{ComboBox, Frame, Margin, RichText};

use crate::app::{Action, PassStatus, SshpassApp};
use crate::config::AgentMode;
use crate::pass::AgentState;
use crate::ui::{self, pixel};

/// Affiche le panneau, en le faisant glisser a l'ouverture et a la fermeture.
///
/// `show_collapsible` est l'animation native d'egui: le panneau sort et rentre
/// par son bord, et la zone centrale suit le mouvement. Il replie aussi le
/// panneau quand on tire la poignee de redimensionnement en deca de sa largeur
/// minimale, ce qui evite d'avoir a viser le bouton de la barre d'outils.
pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    // `show_collapsible` emprunte le booleen: on travaille sur une copie, que
    // l'on recopie ensuite dans l'application.
    let mut expanded = app.show_vault_panel;

    egui::Panel::right("vault_panel")
        .resizable(true)
        .default_size(300.0)
        .size_range(240.0..=460.0)
        .frame(
            Frame::new()
                .fill(palette.surface)
                .inner_margin(Margin::same(10)),
        )
        .show_collapsible(ui, &mut expanded, |ui| {
            ui.horizontal(|ui| {
                pixel::icon_two_tone(ui, &pixel::KEY, scale, palette.accent_soft, palette.success);
                ui.label(RichText::new("Proton Pass").strong().size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Actualiser").clicked() {
                        app.actions.push(Action::RefreshVaults);
                    }
                });
            });

            match &app.pass_status {
                PassStatus::Available(version) => {
                    ui::hint(ui, version, &palette);
                }
                PassStatus::Probing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui::hint(ui, "Detection de pass-cli...", &palette);
                    });
                }
                PassStatus::Missing(err) => {
                    ui.label(RichText::new(err).color(palette.danger).size(11.0));
                    ui::hint(
                        ui,
                        "Installez Proton Pass CLI, puis « Actualiser ».",
                        &palette,
                    );
                }
            }

            ui::separator(ui, &palette);
            agents_section(app, ui);
            ui::separator(ui, &palette);
            items_section(app, ui);
        });

    app.show_vault_panel = expanded;
}

fn agents_section(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    ui::section_title(ui, "Agent SSH", &palette);

    let mode = app.config.proton_pass.agent_mode;
    ui.horizontal(|ui| {
        pixel::icon(ui, &pixel::PLUG, scale, palette.text_dim);
        ui.label(RichText::new(mode.label()).size(12.0));
    });

    if mode == AgentMode::Disabled {
        ui::hint(ui, "Les onglets heritent de SSH_AUTH_SOCK.", &palette);
        return;
    }

    // Coffres a surveiller: ceux references par une connexion, plus celui
    // selectionne dans le panneau.
    let mut vaults = app.config.referenced_vaults();
    if let Some(selected) = app.selected_vault.clone() {
        if !selected.is_empty() && !vaults.contains(&selected) {
            vaults.push(selected);
        }
    }
    if vaults.is_empty() {
        ui::hint(ui, "Aucun coffre associe a une connexion.", &palette);
        return;
    }

    for vault in vaults {
        let state = app.agents.state_of(&vault);
        let color = match &state {
            AgentState::Running => palette.success,
            AgentState::Starting => palette.warning,
            AgentState::Stopped => palette.text_dim,
            AgentState::Failed(_) => palette.danger,
        };
        let tooltip = match &state {
            AgentState::Failed(err) => err.clone(),
            other => format!("Agent {}", other.label()),
        };
        ui.horizontal(|ui| {
            pixel::status_dot(ui, color, scale, &tooltip);
            ui.label(RichText::new(&vault).size(12.0));
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match mode {
                    AgentMode::OwnAgent => {
                        if state.is_running() || matches!(state, AgentState::Starting) {
                            if ui.small_button("Arreter").clicked() {
                                app.actions.push(Action::StopAgent(vault.clone()));
                            }
                        } else if ui.small_button("Demarrer").clicked() {
                            app.actions.push(Action::StartAgent(vault.clone()));
                        }
                    }
                    AgentMode::LoadIntoExisting => {
                        if ui.small_button("Charger les cles").clicked() {
                            app.actions
                                .push(Action::LoadIntoExistingAgent(vault.clone()));
                        }
                    }
                    AgentMode::Disabled => {}
                },
            );
        });
        if let AgentState::Failed(err) = &state {
            ui.label(RichText::new(err).color(palette.danger).size(10.0));
        }
    }
}

fn items_section(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    ui::section_title(ui, "Items", &palette);

    if !app.pass_status.is_available() {
        ui::hint(
            ui,
            "Coffres indisponibles tant que pass-cli n'est pas detecte.",
            &palette,
        );
        return;
    }
    let vaults: Vec<String> = app.vaults.iter().map(|v| v.name.clone()).collect();
    if vaults.is_empty() {
        ui::hint(ui, "Aucun coffre charge.", &palette);
        return;
    }

    let selected = app.selected_vault.clone().unwrap_or_default();
    let mut chosen = selected.clone();
    ComboBox::from_id_salt("vault_select")
        .selected_text(if selected.is_empty() {
            "Choisir un coffre"
        } else {
            &selected
        })
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for vault in &vaults {
                ui.selectable_value(&mut chosen, vault.clone(), vault);
            }
        });
    if chosen != selected {
        app.selected_vault = Some(chosen.clone());
        app.actions.push(Action::LoadItems(chosen.clone()));
    }
    if chosen.is_empty() {
        return;
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        pixel::icon(ui, &pixel::SEARCH, scale, palette.text_dim);
        ui.add(
            egui::TextEdit::singleline(&mut app.vault_search)
                .hint_text("Filtrer les items")
                .desired_width(f32::INFINITY),
        );
    });

    if app.loading_items.contains(&chosen) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui::hint(ui, "Lecture du coffre...", &palette);
        });
        return;
    }

    let Some(items) = app.items.get(&chosen).cloned() else {
        ui.add_space(6.0);
        if ui.button("Charger les items").clicked() {
            app.actions.push(Action::LoadItems(chosen.clone()));
        }
        return;
    };

    // Connexion cible d'une association: celle de l'onglet courant.
    let target = app
        .active_tab
        .and_then(|index| app.tabs.get(index))
        .and_then(|tab| tab.connection.clone())
        .and_then(|id| app.config.connection(&id).map(|c| (id, c.display_name())));

    let filter = app.vault_search.clone();
    let matching: Vec<crate::pass::Item> = items
        .into_iter()
        .filter(|item| item.matches(&filter))
        .collect();

    if matching.is_empty() {
        ui::hint(ui, "Aucun item ne correspond.", &palette);
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for item in matching {
                Frame::new()
                    .fill(palette.bg)
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let sprite = if item.kind.is_ssh_key() {
                                pixel::KEY
                            } else {
                                pixel::DOT
                            };
                            let color = if item.kind.is_ssh_key() {
                                palette.accent_soft
                            } else {
                                palette.text_dim
                            };
                            pixel::icon_two_tone(ui, &sprite, scale, color, palette.success);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&item.title).size(12.0));
                                let detail = match &item.username {
                                    Some(user) => format!("{} · {}", item.kind.label(), user),
                                    None => item.kind.label().to_string(),
                                };
                                ui.label(RichText::new(detail).size(10.0).color(palette.text_dim));
                            });
                            if let Some((connection_id, name)) = &target {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .small_button("Associer")
                                            .on_hover_text(format!("Associer a « {name} »"))
                                            .clicked()
                                        {
                                            app.actions.push(Action::AssignItem {
                                                connection: connection_id.clone(),
                                                vault: chosen.clone(),
                                                item: item.title.clone(),
                                            });
                                        }
                                    },
                                );
                            }
                        });
                    });
                ui.add_space(4.0);
            }
        });

    if target.is_none() {
        ui.add_space(6.0);
        ui::hint(ui, "Ouvrez un onglet pour associer un item.", &palette);
    }
}
