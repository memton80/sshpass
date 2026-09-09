//! Zone centrale: barre d'onglets et contenu (accueil ou terminal).

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Sense, Vec2};

use crate::app::{Action, SshpassApp, TabState};
use crate::term::render;
use crate::ui::{anim, home, pixel};

const TAB_HEIGHT: f32 = 30.0;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let palette = app.palette;
    // Le terminal ne capture le clavier que si aucun champ de saisie ni
    // fenetre modale ne l'a deja.
    let interactive = ctx.memory(|m| m.focused()).is_none()
        && app.editor.is_none()
        && app.pending_delete.is_none()
        && app.new_folder_name.is_none()
        && !app.settings_open;

    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(palette.bg))
        .show(ui, |ui| {
            strip(app, ui);

            // Transition entre vues: la nouvelle apparait en fondu. La cle
            // identifie la vue affichee; un changement relance l'animation.
            let key = app
                .active_tab
                .and_then(|index| app.tabs.get(index))
                .map(|tab| view_key(&tab.id))
                .unwrap_or(0);
            let progress = anim::view_transition(ui, egui::Id::new("vue"), key, anim::VIEW);

            match app.active_tab {
                Some(index) if index < app.tabs.len() => {
                    // Seule l'opacite est animee ici: decaler le rectangle du
                    // terminal changerait le nombre de colonnes a chaque frame
                    // et declencherait une cascade de redimensionnements.
                    ui.multiply_opacity(progress);
                    terminal(app, ui, &ctx, index, interactive)
                }
                _ => {
                    app.active_tab = None;
                    ui.multiply_opacity(progress);
                    // L'accueil, lui, peut glisser: rien n'y depend de la
                    // hauteur exacte disponible.
                    let slide = anim::lerp(10.0, 0.0, progress) as i8;
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 8,
                            right: 8,
                            top: 8 + slide,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| home::show(app, ui));
                        });
                }
            }
        });
}

/// Barre d'onglets, avec le bouton d'accueil en tete.
fn strip(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;

    egui::Panel::top("tab_strip")
        .exact_size(TAB_HEIGHT + 8.0)
        .frame(
            Frame::new()
                .fill(palette.bg_deep)
                .inner_margin(Margin::symmetric(6, 4)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let home_active = app.active_tab.is_none();
                        if tab_button(ui, "Accueil", None, home_active, &palette, scale, false).0 {
                            app.actions.push(Action::ShowHome);
                        }

                        let entries: Vec<(String, String, bool, bool, bool)> = app
                            .tabs
                            .iter()
                            .enumerate()
                            .map(|(index, tab)| {
                                (
                                    tab.id.clone(),
                                    tab.title.clone(),
                                    app.active_tab == Some(index),
                                    tab.bell,
                                    matches!(tab.state, TabState::Running(_))
                                        && tab.session().is_some_and(|s| s.is_alive()),
                                )
                            })
                            .collect();

                        for (id, title, active, bell, alive) in entries {
                            let status = if bell {
                                Some(palette.warning)
                            } else if alive {
                                Some(palette.success)
                            } else {
                                Some(palette.text_dim)
                            };
                            let (clicked, closed) =
                                tab_button(ui, &title, status, active, &palette, scale, true);
                            if clicked {
                                app.actions.push(Action::SelectTab(id.clone()));
                            }
                            if closed {
                                app.actions.push(Action::CloseTab(id));
                            }
                        }
                    });
                });
        });
}

/// Dessine un onglet. Renvoie (active, ferme).
fn tab_button(
    ui: &mut egui::Ui,
    title: &str,
    status: Option<Color32>,
    active: bool,
    palette: &crate::theme::Palette,
    scale: f32,
    closable: bool,
) -> (bool, bool) {
    let label = crate::ui::sidebar::truncate(title, 24);
    let text_width = label.chars().count() as f32 * 7.0;
    let width = text_width + 40.0 + if closable { 20.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, TAB_HEIGHT), Sense::click());

    let close_rect = Rect::from_min_size(
        egui::pos2(rect.right() - 22.0, rect.center().y - 6.0),
        Vec2::splat(12.0),
    );
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let on_close = closable && pointer.is_some_and(|p| close_rect.contains(p));
    let hover = anim::hover(ui, response.id.with("hover"), response.hovered());
    // Le soulignement de l'onglet courant se deplie depuis la gauche.
    let selected = anim::toggle(ui, response.id.with("active"), active, anim::VIEW);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let idle = anim::lerp_color(palette.bg_deep, palette.surface_high, hover);
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            anim::lerp_color(idle, palette.surface, selected),
        );
        if selected > 0.0 {
            // Souligne l'onglet courant plutot que de le cerner: la barre
            // reste lisible meme avec beaucoup d'onglets.
            let underline = Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 2.0),
                egui::pos2(rect.left() + rect.width() * selected, rect.bottom()),
            );
            painter.rect_filled(underline, CornerRadius::ZERO, palette.accent);
        }

        let mut text_x = rect.left() + 8.0;
        if let Some(color) = status {
            pixel::draw(
                painter,
                egui::pos2(text_x, rect.center().y - 4.0 * scale),
                &pixel::DOT,
                scale,
                color,
                color,
            );
            text_x += 8.0 * scale + 4.0;
        } else {
            pixel::draw(
                painter,
                egui::pos2(text_x, rect.center().y - 4.0 * scale),
                &pixel::TERMINAL,
                scale,
                palette.text_dim,
                palette.text_dim,
            );
            text_x += 8.0 * scale + 4.0;
        }

        let idle_text = anim::lerp_color(palette.text_dim, palette.text, hover);
        painter.text(
            egui::pos2(text_x, rect.center().y),
            Align2::LEFT_CENTER,
            &label,
            FontId::proportional(12.0),
            anim::lerp_color(idle_text, palette.text, selected),
        );

        if closable {
            // La croix s'affirme au survol de l'onglet et vire au rouge quand
            // le pointeur l'atteint, au lieu de basculer d'un coup.
            let base = anim::lerp_color(palette.text_dim, palette.danger, f32::from(on_close));
            let visible = hover.max(selected).max(0.5);
            let color = anim::fade(base, visible);
            pixel::draw(painter, close_rect.min, &pixel::CLOSE, 1.5, color, color);
        }
    }

    let clicked = response.clicked();
    (clicked && !on_close, clicked && on_close)
}

fn terminal(
    app: &mut SshpassApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    index: usize,
    interactive: bool,
) {
    let palette = app.palette;
    let font_size = app.config.ui.terminal_font_size;
    let binary = app.config.proton_pass.binary.clone();
    let mut queued: Vec<Action> = Vec::new();
    let mut clipboard: Option<String> = None;

    let tab = &mut app.tabs[index];
    let id = tab.id.clone();
    // L'identite egui suit l'onglet, pas sa position: fermer un onglet ne doit
    // pas transferer l'etat des widgets (selection, defilement) a son voisin.
    ui.push_id(egui::Id::new(&id), |ui| match &mut tab.state {
        TabState::WaitingAgent { vault, .. } => {
            let vault = vault.clone();
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.spinner();
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Demarrage de l'agent « {vault} »")).strong());
                ui.label(
                    RichText::new(format!("{binary} ssh-agent start --vault-name {vault}"))
                        .size(11.0)
                        .color(palette.text_dim),
                );
                ui.add_space(16.0);
                if ui.button("Connecter sans attendre l'agent").clicked() {
                    queued.push(Action::ConnectWithoutAgent(id.clone()));
                }
            });
        }
        TabState::Failed(message) => {
            let message = message.clone();
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("La session n'a pas pu demarrer")
                        .strong()
                        .size(15.0),
                );
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(palette.surface)
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.label(RichText::new(&message).size(12.0).color(palette.text_dim));
                    });
                ui.add_space(12.0);
                if ui.button("Fermer l'onglet").clicked() {
                    queued.push(Action::CloseTab(id.clone()));
                }
            });
        }
        TabState::Running(session) => {
            if let Some(status) = session.exit_status.clone() {
                egui::Panel::bottom("session_status")
                    .frame(
                        Frame::new()
                            .fill(palette.surface)
                            .inner_margin(Margin::symmetric(10, 6)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            pixel::status_dot(
                                ui,
                                palette.danger,
                                app.config.ui.pixel_scale as f32,
                                "",
                            );
                            ui.label(RichText::new(status).size(12.0).color(palette.text_dim));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Fermer").clicked() {
                                        queued.push(Action::CloseTab(id.clone()));
                                    }
                                },
                            );
                        });
                    });
            }
            let output = render::show(ui, session, font_size, interactive);
            if let Some(text) = output.copy {
                session.set_clipboard(text.clone());
                clipboard = Some(text);
            }
            if output.close_requested {
                queued.push(Action::CloseTab(id.clone()));
            }
        }
    });

    if let Some(text) = clipboard {
        ctx.copy_text(text);
    }
    app.actions.extend(queued);
}

/// Cle stable d'une vue, pour detecter un changement d'onglet.
fn view_key(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    // Zero est reserve a l'accueil.
    hasher.finish() | 1
}
