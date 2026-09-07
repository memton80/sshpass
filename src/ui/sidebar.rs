//! Barre laterale: recherche, favoris, dossiers et connexions.

use std::collections::HashSet;

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Sense, Vec2};

use crate::app::{Action, SshpassApp};
use crate::config::{Config, Connection};
use crate::theme::Palette;
use crate::ui::{self, pixel};

/// Hauteur d'une ligne de la liste.
const ROW_HEIGHT: f32 = 26.0;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let default_width = app.config.ui.sidebar_width;
    // Connexions deja ouvertes: elles portent une pastille dans la liste.
    let opened: HashSet<String> = app
        .tabs
        .iter()
        .filter_map(|t| t.connection.clone())
        .collect();

    let SshpassApp {
        config,
        actions,
        expanded_folders,
        search,
        focus_search,
        ..
    } = app;

    egui::Panel::left("sidebar")
        .resizable(true)
        .default_size(default_width)
        .size_range(210.0..=460.0)
        .frame(
            Frame::new()
                .fill(palette.surface)
                .inner_margin(Margin::same(8)),
        )
        .show(ui, |ui| {
            search_row(ui, search, focus_search, &palette, scale);
            ui::separator(ui, &palette);

            egui::Panel::bottom("sidebar_footer")
                .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui::pixel_button(ui, &pixel::PLUS, "Connexion", &palette, scale)
                            .on_hover_text("Nouvelle connexion")
                            .clicked()
                        {
                            actions.push(Action::NewConnection(None));
                        }
                        if ui::pixel_button(ui, &pixel::FOLDER, "Dossier", &palette, scale)
                            .on_hover_text("Nouveau dossier")
                            .clicked()
                        {
                            actions.push(Action::NewFolder(String::new()));
                        }
                    });
                });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let needle = search.clone();

                    let favorites: Vec<&Connection> = config
                        .connections
                        .iter()
                        .filter(|c| c.favorite && c.matches(&needle))
                        .collect();
                    if !favorites.is_empty() {
                        ui::section_title(ui, "Favoris", &palette);
                        for connection in favorites {
                            connection_row(
                                ui, connection, config, &palette, scale, &opened, actions,
                            );
                        }
                    }

                    let mut folders: Vec<_> = config.folders.iter().collect();
                    folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    if !folders.is_empty() {
                        ui::section_title(ui, "Dossiers", &palette);
                    }
                    for folder in folders {
                        let children: Vec<&Connection> = config
                            .connections
                            .iter()
                            .filter(|c| c.folder.as_deref() == Some(folder.id.as_str()))
                            .filter(|c| c.matches(&needle))
                            .collect();
                        // Une recherche qui ne ramene rien masque le dossier, sauf
                        // s'il est vide et qu'aucune recherche n'est en cours.
                        if children.is_empty() && !needle.trim().is_empty() {
                            continue;
                        }
                        let expanded = expanded_folders.contains(&folder.id);
                        let toggled = folder_row(
                            ui,
                            &folder.name,
                            children.len(),
                            expanded,
                            &palette,
                            scale,
                            &folder.id,
                            actions,
                        );
                        if toggled {
                            if expanded {
                                expanded_folders.remove(&folder.id);
                            } else {
                                expanded_folders.insert(folder.id.clone());
                            }
                        }
                        // Une recherche active deplie tout: sinon les resultats
                        // seraient invisibles dans les dossiers replies.
                        if expanded || !needle.trim().is_empty() {
                            ui.indent(&folder.id, |ui| {
                                for connection in children {
                                    connection_row(
                                        ui, connection, config, &palette, scale, &opened, actions,
                                    );
                                }
                            });
                        }
                    }

                    let orphans: Vec<&Connection> = config
                        .connections
                        .iter()
                        .filter(|c| c.folder.is_none() && c.matches(&needle))
                        .collect();
                    if !orphans.is_empty() {
                        ui::section_title(ui, "Connexions", &palette);
                        for connection in orphans {
                            connection_row(
                                ui, connection, config, &palette, scale, &opened, actions,
                            );
                        }
                    }

                    if config.connections.is_empty() {
                        ui.add_space(12.0);
                        ui::hint(ui, "Aucune connexion enregistree.", &palette);
                        ui::hint(ui, "Utilisez « + Connexion » pour commencer.", &palette);
                    } else if config.connections.iter().all(|c| !c.matches(&needle)) {
                        ui.add_space(12.0);
                        ui::hint(ui, "Aucun resultat pour cette recherche.", &palette);
                    }
                });
        });
}

fn search_row(
    ui: &mut egui::Ui,
    search: &mut String,
    focus_search: &mut bool,
    palette: &Palette,
    scale: f32,
) {
    ui.horizontal(|ui| {
        pixel::icon(ui, &pixel::SEARCH, scale, palette.text_dim);
        let response = ui.add(
            egui::TextEdit::singleline(search)
                .hint_text("Rechercher (Ctrl+Maj+F)")
                .desired_width(f32::INFINITY),
        );
        if *focus_search {
            response.request_focus();
            *focus_search = false;
        }
        if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            search.clear();
        }
    });
}

/// Dessine l'en-tete d'un dossier. Renvoie `true` si l'utilisateur l'a plie
/// ou deplie.
#[allow(clippy::too_many_arguments)]
fn folder_row(
    ui: &mut egui::Ui,
    name: &str,
    count: usize,
    expanded: bool,
    palette: &Palette,
    scale: f32,
    folder_id: &str,
    actions: &mut Vec<Action>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::ZERO, palette.surface_high);
        }
        let chevron = if expanded {
            pixel::CHEVRON_DOWN
        } else {
            pixel::CHEVRON_RIGHT
        };
        let icon_y = rect.center().y - 4.0 * scale;
        pixel::draw(
            painter,
            egui::pos2(rect.left() + 2.0, icon_y),
            &chevron,
            scale,
            palette.text_dim,
            palette.text_dim,
        );
        pixel::draw(
            painter,
            egui::pos2(rect.left() + 6.0 + 8.0 * scale, icon_y),
            &pixel::FOLDER,
            scale,
            palette.accent_soft,
            palette.accent,
        );
        painter.text(
            egui::pos2(rect.left() + 14.0 + 16.0 * scale, rect.center().y),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(13.0),
            palette.text,
        );
        painter.text(
            egui::pos2(rect.right() - 4.0, rect.center().y),
            Align2::RIGHT_CENTER,
            count.to_string(),
            FontId::proportional(11.0),
            palette.text_dim,
        );
    }

    response.context_menu(|ui| {
        if ui.button("Nouvelle connexion ici").clicked() {
            actions.push(Action::NewConnection(Some(folder_id.to_string())));
            ui.close();
        }
        if ui.button("Supprimer le dossier").clicked() {
            actions.push(Action::DeleteFolder(folder_id.to_string()));
            ui.close();
        }
    });

    response.clicked()
}

fn connection_row(
    ui: &mut egui::Ui,
    connection: &Connection,
    config: &Config,
    palette: &Palette,
    scale: f32,
    opened: &HashSet<String>,
    actions: &mut Vec<Action>,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    let is_open = opened.contains(&connection.id);

    // Actions rapides revelees au survol, a la place de la cible: une barre
    // laterale etroite n'a pas la place d'afficher les deux en permanence.
    let icon = 8.0 * scale;
    let delete_rect = Rect::from_min_size(
        egui::pos2(rect.right() - icon - 6.0, rect.center().y - icon / 2.0),
        Vec2::splat(icon),
    );
    let edit_rect = Rect::from_min_size(
        egui::pos2(
            delete_rect.left() - icon - 8.0,
            rect.center().y - icon / 2.0,
        ),
        Vec2::splat(icon),
    );
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hovered = response.hovered();
    let on_edit = hovered && pointer.is_some_and(|p| edit_rect.expand(4.0).contains(p));
    let on_delete = hovered && pointer.is_some_and(|p| delete_rect.expand(4.0).contains(p));

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::ZERO, palette.surface_high);
        }
        if is_open {
            // Liseré gauche: la connexion a un onglet ouvert.
            let marker = Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + 2.0 * scale, rect.bottom()),
            );
            painter.rect_filled(marker, CornerRadius::ZERO, palette.accent);
        }

        let icon_y = rect.center().y - 4.0 * scale;
        let icon_x = rect.left() + 4.0 + 2.0 * scale;
        let (primary, secondary) = if connection.proton.is_some() {
            (palette.accent_soft, palette.success)
        } else {
            (palette.text_dim, palette.text_dim)
        };
        let sprite = if connection.proton.is_some() {
            pixel::KEY
        } else {
            pixel::SERVER
        };
        pixel::draw(
            painter,
            egui::pos2(icon_x, icon_y),
            &sprite,
            scale,
            primary,
            secondary,
        );

        let text_x = icon_x + 8.0 * scale + 8.0;
        let mut text_right = rect.right() - 4.0;
        if connection.favorite && !hovered {
            pixel::draw(
                painter,
                egui::pos2(text_right - 8.0 * scale, icon_y),
                &pixel::STAR,
                scale,
                palette.warning,
                palette.warning,
            );
            text_right -= 8.0 * scale + 4.0;
        }

        let galley_rect = painter.text(
            egui::pos2(text_x, rect.center().y - 6.0),
            Align2::LEFT_TOP,
            connection.display_name(),
            FontId::proportional(13.0),
            palette.text,
        );

        if hovered {
            pixel::draw(
                painter,
                edit_rect.min,
                &pixel::PENCIL,
                scale,
                if on_edit {
                    palette.accent_soft
                } else {
                    palette.text_dim
                },
                palette.warning,
            );
            pixel::draw(
                painter,
                delete_rect.min,
                &pixel::TRASH,
                scale,
                if on_delete {
                    palette.danger
                } else {
                    palette.text_dim
                },
                palette.surface,
            );
        } else {
            // Le `user@host` n'apparait que s'il reste de la place: dans une
            // barre laterale etroite, mieux vaut un nom lisible qu'une cible
            // tronquee. La largeur est mesuree, pas estimee: une estimation par
            // nombre de caracteres fait chevaucher les deux textes.
            let remaining = text_right - galley_rect.right() - 8.0;
            if remaining > 40.0 {
                let font = FontId::proportional(11.0);
                let target = connection.target();
                let mut budget = target.chars().count();
                let mut galley =
                    painter.layout_no_wrap(target.clone(), font.clone(), palette.text_dim);
                // On raccourcit jusqu'a tenir: la largeur moyenne par
                // caractere varie assez pour qu'une seule estimation rate.
                while galley.size().x > remaining && budget > 6 {
                    budget = budget.min((budget as f32 * remaining / galley.size().x) as usize) - 1;
                    galley = painter.layout_no_wrap(
                        truncate(&target, budget),
                        font.clone(),
                        palette.text_dim,
                    );
                }
                if galley.size().x <= remaining {
                    painter.galley(
                        egui::pos2(
                            text_right - galley.size().x,
                            rect.center().y - galley.size().y / 2.0,
                        ),
                        galley,
                        palette.text_dim,
                    );
                }
            }
        }
    }

    let tooltip = if on_delete {
        "Supprimer".to_string()
    } else if on_edit {
        "Modifier".to_string()
    } else {
        format!(
            "{}\n{}{}",
            connection.display_name(),
            connection.target(),
            connection
                .last_used
                .map(|ts| format!("\nOuverte {}", crate::config::relative_time(ts)))
                .unwrap_or_default()
        )
    };
    let response = response.on_hover_text(tooltip);

    if response.clicked() {
        if on_delete {
            actions.push(Action::AskDeleteConnection(connection.id.clone()));
        } else if on_edit {
            actions.push(Action::EditConnection(connection.id.clone()));
        } else {
            actions.push(Action::OpenConnection(connection.id.clone()));
        }
    }

    response.context_menu(|ui| {
        if ui.button("Ouvrir").clicked() {
            actions.push(Action::OpenConnection(connection.id.clone()));
            ui.close();
        }
        if ui.button("Modifier").clicked() {
            actions.push(Action::EditConnection(connection.id.clone()));
            ui.close();
        }
        let favorite = if connection.favorite {
            "Retirer des favoris"
        } else {
            "Ajouter aux favoris"
        };
        if ui.button(favorite).clicked() {
            actions.push(Action::ToggleFavorite(connection.id.clone()));
            ui.close();
        }
        ui.menu_button("Deplacer vers", |ui| {
            if ui.button("(racine)").clicked() {
                actions.push(Action::MoveConnection {
                    connection: connection.id.clone(),
                    folder: None,
                });
                ui.close();
            }
            for folder in &config.folders {
                if ui.button(&folder.name).clicked() {
                    actions.push(Action::MoveConnection {
                        connection: connection.id.clone(),
                        folder: Some(folder.id.clone()),
                    });
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui
            .button(RichText::new("Supprimer").color(Color32::from_rgb(0xF8, 0x71, 0x71)))
            .clicked()
        {
            actions.push(Action::AskDeleteConnection(connection.id.clone()));
            ui.close();
        }
    });
}

/// Tronque au milieu, pour garder le debut et la fin d'un `user@host`.
pub fn truncate(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars < 6 {
        return text.to_string();
    }
    let keep = max_chars - 1;
    let head = keep / 2;
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_both_ends() {
        assert_eq!(truncate("root@example.com", 40), "root@example.com");
        let short = truncate("root@tres-long-nom-de-machine.example.com", 12);
        assert_eq!(short.chars().count(), 12);
        assert!(short.starts_with("root@"));
        assert!(short.ends_with("com"));
        assert!(short.contains('…'));
    }

    #[test]
    fn truncate_refuses_absurd_widths() {
        assert_eq!(truncate("root@host", 3), "root@host");
    }
}
