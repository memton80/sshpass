//! Barre laterale: recherche, favoris, dossiers et connexions.

use std::collections::HashSet;

use egui::collapsing_header::CollapsingState;
use egui::{Align2, FontId, Frame, Margin, Rect, RichText, Sense, Vec2};

use crate::app::{Action, SshpassApp};
use crate::config::{Config, Connection};
use crate::theme::{self, Look, Palette};
use crate::ui::widgets::{self, Button};
use crate::ui::{self, anim, pixel};

/// Hauteur d'une ligne de la liste.
const ROW_HEIGHT: f32 = 26.0;

/// Arrivee des lignes l'une apres l'autre.
///
/// La progression est commune a toute la liste, seul le rang change: chaque
/// ligne demande la sienne a son tour, quel que soit le dossier ou elle est.
/// Elle repart de zero quand la recherche change, pour que les resultats se
/// posent au lieu d'apparaitre.
struct Cascade {
    progress: f32,
    rank: usize,
}

impl Cascade {
    fn new(ctx: &egui::Context, search: &str, look: &Look) -> Self {
        let id = egui::Id::new("barre_cascade").with(search.trim());
        Self {
            progress: anim::appear(ctx, id, look.duration(0.45)),
            rank: 0,
        }
    }

    /// Avancement de la ligne suivante.
    fn next(&mut self) -> f32 {
        let arrival = anim::stagger(self.progress, self.rank, 0.05);
        self.rank += 1;
        arrival
    }
}

/// Decalage et opacite d'une ligne qui arrive.
fn arriving(
    painter: &egui::Painter,
    rect: Rect,
    arrival: f32,
    look: &Look,
) -> (egui::Painter, Rect) {
    let mut painter = painter.clone();
    painter.multiply_opacity(arrival.clamp(0.0, 1.0));
    let shift = anim::lerp_unclamped(-12.0, 0.0, look.arrive(arrival));
    (painter, rect.translate(Vec2::new(shift, 0.0)))
}

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let look = theme::look(ui.ctx());
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
        .show_separator_line(look.pixel())
        .default_size(default_width)
        .size_range(210.0..=460.0)
        .frame(
            // Dans les allures arrondies, le panneau est une carte posee sur le
            // fond de la fenetre, a distance de ses bords.
            Frame::new()
                .fill(palette.surface)
                .inner_margin(Margin::same(8))
                .corner_radius(look.large)
                .outer_margin(Margin {
                    left: look.gap,
                    right: 0,
                    top: 0,
                    bottom: look.gap,
                }),
        )
        .show(ui, |ui| {
            search_row(ui, search, focus_search, &palette, scale);
            ui::separator(ui, &palette);

            egui::Panel::bottom("sidebar_footer")
                .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(Button::new("Connexion").icon(&pixel::PLUS, scale).small())
                            .on_hover_text("Nouvelle connexion")
                            .clicked()
                        {
                            actions.push(Action::NewConnection(None));
                        }
                        if ui
                            .add(Button::new("Dossier").icon(&pixel::FOLDER, scale).small())
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
                    let mut cascade = Cascade::new(ui.ctx(), &needle, &look);

                    let favorites: Vec<&Connection> = config
                        .connections
                        .iter()
                        .filter(|c| c.favorite && c.matches(&needle))
                        .collect();
                    if !favorites.is_empty() {
                        ui::section_title(ui, "Favoris", &palette);
                        for connection in favorites {
                            connection_row(
                                ui,
                                connection,
                                config,
                                &palette,
                                scale,
                                &opened,
                                actions,
                                &mut cascade,
                            );
                        }
                    }

                    let mut folders: Vec<_> = config.folders.iter().collect();
                    folders.sort_by_key(|f| f.name.to_lowercase());
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
                        // Une recherche active deplie tout: sinon les resultats
                        // seraient invisibles dans les dossiers replies.
                        let searching = !needle.trim().is_empty();
                        let expanded = expanded_folders.contains(&folder.id) || searching;

                        // Le pliage passe par `CollapsingState`, l'etat natif
                        // d'egui: c'est lui qui interpole la hauteur et coupe
                        // le contenu debordant. La verite reste
                        // `expanded_folders`; l'etat egui n'anime que le
                        // mouvement.
                        let mut collapsing = CollapsingState::load_with_default_open(
                            ui.ctx(),
                            ui.make_persistent_id(("dossier", &folder.id)),
                            expanded,
                        );
                        collapsing.set_open(expanded);
                        let openness = collapsing.openness(ui.ctx());

                        let toggled = folder_row(
                            ui,
                            &folder.name,
                            children.len(),
                            openness,
                            &palette,
                            scale,
                            &folder.id,
                            actions,
                            cascade.next(),
                        );
                        if toggled {
                            if expanded_folders.contains(&folder.id) {
                                expanded_folders.remove(&folder.id);
                            } else {
                                expanded_folders.insert(folder.id.clone());
                            }
                        }
                        collapsing.show_body_unindented(ui, |ui| {
                            ui.indent(&folder.id, |ui| {
                                for connection in children {
                                    connection_row(
                                        ui,
                                        connection,
                                        config,
                                        &palette,
                                        scale,
                                        &opened,
                                        actions,
                                        &mut cascade,
                                    );
                                }
                            });
                        });
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
                                ui,
                                connection,
                                config,
                                &palette,
                                scale,
                                &opened,
                                actions,
                                &mut cascade,
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
        let response = widgets::field(
            ui,
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
///
/// `openness` est l'avancement du depliage (0 replie, 1 ouvert): le chevron
/// suit le mouvement du contenu au lieu de basculer avant lui.
#[allow(clippy::too_many_arguments)]
fn folder_row(
    ui: &mut egui::Ui,
    name: &str,
    count: usize,
    openness: f32,
    palette: &Palette,
    scale: f32,
    folder_id: &str,
    actions: &mut Vec<Action>,
    arrival: f32,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    let t = anim::hover(ui, response.id.with("hover"), response.hovered());
    let look = theme::look(ui.ctx());

    if ui.is_rect_visible(rect) {
        let (painter, rect) = arriving(ui.painter(), rect, arrival, &look);
        let painter = &painter;
        if t > 0.0 {
            painter.rect_filled(
                rect,
                look.small,
                anim::lerp_color(palette.surface, palette.surface_high, t),
            );
        }
        widgets::ripple(ui, &response, rect, look.small, palette.accent_soft);
        let icon_y = rect.center().y - 4.0 * scale;
        // Deux sprites superposes en fondu croise: une rotation de chevron
        // serait floutee a chaque angle intermediaire, la ou un fondu reste
        // net a tous les stades.
        let chevron_color = anim::lerp_color(palette.text_dim, palette.text, t);
        for (sprite, share) in [
            (&pixel::CHEVRON_RIGHT, 1.0 - openness),
            (&pixel::CHEVRON_DOWN, openness),
        ] {
            if share <= 0.0 {
                continue;
            }
            let color = anim::fade(chevron_color, share);
            pixel::draw(
                painter,
                egui::pos2(rect.left() + 2.0, icon_y),
                sprite,
                scale,
                color,
                color,
            );
        }
        pixel::draw(
            painter,
            egui::pos2(rect.left() + 6.0 + 8.0 * scale, icon_y),
            &pixel::FOLDER,
            scale,
            anim::lerp_color(palette.accent_soft, palette.accent, t),
            palette.accent,
        );
        painter.text(
            egui::pos2(
                rect.left() + 14.0 + 16.0 * scale + look.lift * t,
                rect.center().y,
            ),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(13.0),
            palette.text,
        );
        // Le compte devient une petite capsule hors retro.
        let count_galley = painter.layout_no_wrap(
            count.to_string(),
            FontId::proportional(11.0),
            palette.text_dim,
        );
        let badge = Rect::from_min_size(
            egui::pos2(
                rect.right() - 6.0 - count_galley.size().x - 12.0,
                rect.center().y - 8.0,
            ),
            Vec2::new(count_galley.size().x + 12.0, 16.0),
        );
        if !look.pixel() {
            painter.rect_filled(
                badge,
                look.pill(badge.height()),
                anim::lerp_color(palette.bg_deep, palette.accent_dim, t),
            );
        }
        painter.galley(
            egui::pos2(
                badge.center().x - count_galley.size().x / 2.0,
                badge.center().y - count_galley.size().y / 2.0,
            ),
            count_galley,
            anim::lerp_color(palette.text_dim, palette.text, t),
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

#[allow(clippy::too_many_arguments)]
fn connection_row(
    ui: &mut egui::Ui,
    connection: &Connection,
    config: &Config,
    palette: &Palette,
    scale: f32,
    opened: &HashSet<String>,
    actions: &mut Vec<Action>,
    cascade: &mut Cascade,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    let is_open = opened.contains(&connection.id);
    let look = theme::look(ui.ctx());
    let arrival = cascade.next();
    // Le liseré d'onglet ouvert se deploie depuis le milieu de la ligne.
    let marked = anim::toggle(
        ui,
        response.id.with("ouverte"),
        is_open,
        look.duration(anim::VIEW),
    );

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
    // Avancement du survol: la ligne ne bascule pas d'un etat a l'autre, elle y
    // glisse, et la cible croise les actions rapides en fondu plutot que de
    // disparaitre d'un coup.
    let t = anim::hover(ui, response.id.with("hover"), hovered);

    if ui.is_rect_visible(rect) {
        let (painter, rect) = arriving(ui.painter(), rect, arrival, &look);
        let painter = &painter;
        if t > 0.0 {
            painter.rect_filled(
                rect,
                look.small,
                anim::lerp_color(palette.surface, palette.surface_high, t),
            );
        }
        widgets::ripple(ui, &response, rect, look.small, palette.accent_soft);
        if marked > 0.0 {
            // Liseré gauche: la connexion a un onglet ouvert. Il s'epaissit au
            // survol pour repondre au pointeur; hors retro, c'est une capsule
            // qui grandit depuis le milieu de la ligne.
            let width = anim::lerp(2.0, 3.0, t) * scale;
            let marker = if look.pixel() {
                Rect::from_min_max(rect.min, egui::pos2(rect.left() + width, rect.bottom()))
            } else {
                let height = (rect.height() - 8.0) * look.arrive(marked);
                Rect::from_center_size(
                    egui::pos2(rect.left() + width / 2.0 + 1.0, rect.center().y),
                    Vec2::new(width, height.max(0.0)),
                )
            };
            if look.glow {
                painter.add(widgets::glow(marker, 2, palette.accent, 0.4 * marked));
            }
            painter.rect_filled(
                marker,
                look.pill(marker.width()),
                anim::fade(palette.accent, marked),
            );
        }

        let icon_y = rect.center().y - 4.0 * scale;
        let icon_x = rect.left() + 4.0 + 2.0 * scale + look.lift * t;
        let (base_primary, secondary) = if connection.proton.is_some() {
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
            anim::lerp_color(base_primary, palette.accent, t),
            secondary,
        );

        let text_x = icon_x + 8.0 * scale + 8.0;
        let mut text_right = rect.right() - 4.0;
        if connection.favorite {
            // L'etoile s'efface au profit des actions rapides.
            let star = anim::fade(palette.warning, 1.0 - t);
            pixel::draw(
                painter,
                egui::pos2(text_right - 8.0 * scale, icon_y),
                &pixel::STAR,
                scale,
                star,
                star,
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

        if t > 0.0 {
            let edit_color = if on_edit {
                palette.accent_soft
            } else {
                palette.text_dim
            };
            pixel::draw(
                painter,
                edit_rect.min,
                &pixel::PENCIL,
                scale,
                anim::fade(edit_color, t),
                anim::fade(palette.warning, t),
            );
            let delete_color = if on_delete {
                palette.danger
            } else {
                palette.text_dim
            };
            pixel::draw(
                painter,
                delete_rect.min,
                &pixel::TRASH,
                scale,
                anim::fade(delete_color, t),
                anim::fade(palette.surface, t),
            );
        }
        if t < 1.0 {
            // Le `user@host` n'apparait que s'il reste de la place: dans une
            // barre laterale etroite, mieux vaut un nom lisible qu'une cible
            // tronquee. La largeur est mesuree, pas estimee: une estimation par
            // nombre de caracteres fait chevaucher les deux textes.
            let remaining = text_right - galley_rect.right() - 8.0;
            if remaining > 40.0 {
                let font = FontId::proportional(11.0);
                let target = connection.target();
                let faded = anim::fade(palette.text_dim, 1.0 - t);
                let mut budget = target.chars().count();
                let mut galley = painter.layout_no_wrap(target.clone(), font.clone(), faded);
                // On raccourcit jusqu'a tenir: la largeur moyenne par
                // caractere varie assez pour qu'une seule estimation rate.
                while galley.size().x > remaining && budget > 6 {
                    budget = budget.min((budget as f32 * remaining / galley.size().x) as usize) - 1;
                    galley = painter.layout_no_wrap(truncate(&target, budget), font.clone(), faded);
                }
                if galley.size().x <= remaining {
                    painter.galley(
                        egui::pos2(
                            text_right - galley.size().x,
                            rect.center().y - galley.size().y / 2.0,
                        ),
                        galley,
                        faded,
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
            .button(RichText::new("Supprimer").color(palette.danger))
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
