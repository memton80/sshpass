//! Zone centrale: barre d'onglets et contenu (accueil ou terminal).

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Sense, Vec2};

use crate::app::{Action, SshpassApp, TabState};
use crate::term::render;
use crate::ui::{anim, home, pixel};

const TAB_HEIGHT: f32 = 30.0;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let palette = app.palette;
    // Le terminal reclame le clavier des qu'aucune fenetre modale ne l'occupe:
    // il le prend alors pour de bon (focus egui), au lieu de se contenter des
    // touches dont personne ne veut. C'est ce qui lui rend `Tab`, les fleches
    // et `Echap`.
    let available = app.editor.is_none()
        && app.pending_delete.is_none()
        && app.new_folder_name.is_none()
        && !app.settings_open;

    // Un onglet qu'on quitte n'a plus le clavier: `vim` et `tmux`, qui
    // demandent a suivre le focus, doivent l'apprendre au changement d'onglet
    // comme ils l'apprennent au changement de fenetre.
    for (index, tab) in app.tabs.iter_mut().enumerate() {
        if Some(index) != app.active_tab {
            if let Some(session) = tab.session_mut() {
                session.set_focus(false);
            }
        }
    }

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
                    terminal(app, ui, &ctx, index, available)
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
                        let home = Look {
                            title: "Accueil",
                            status: None,
                            active: home_active,
                            closable: false,
                            busy: false,
                            pulse: false,
                        };
                        if tab_button(ui, home, &palette, scale).0 {
                            app.actions.push(Action::ShowHome);
                        }

                        let entries: Vec<Entry> = app
                            .tabs
                            .iter()
                            .enumerate()
                            .map(|(index, tab)| Entry {
                                id: tab.id.clone(),
                                title: tab.title.clone(),
                                active: app.active_tab == Some(index),
                                bell: tab.bell,
                                alive: matches!(tab.state, TabState::Running(_))
                                    && tab.session().is_some_and(|s| s.is_alive()),
                                connecting: tab.is_connecting(),
                            })
                            .collect();

                        for entry in entries {
                            let status = if entry.bell {
                                palette.warning
                            } else if entry.connecting {
                                palette.accent_soft
                            } else if entry.alive {
                                palette.success
                            } else {
                                palette.text_dim
                            };
                            let (clicked, closed) = tab_button(
                                ui,
                                Look {
                                    title: &entry.title,
                                    status: Some(status),
                                    active: entry.active,
                                    closable: true,
                                    busy: entry.connecting,
                                    // Seule l'attente bat. La cloche, elle,
                                    // reste allumee jusqu'a ce qu'on ouvre
                                    // l'onglet: la faire clignoter ferait
                                    // redessiner l'application sans fin.
                                    pulse: entry.connecting,
                                },
                                &palette,
                                scale,
                            );
                            if clicked {
                                app.actions.push(Action::SelectTab(entry.id.clone()));
                            }
                            if closed {
                                app.actions.push(Action::CloseTab(entry.id));
                            }
                        }
                    });
                });
        });
}

/// Ce que la barre doit savoir d'un onglet pour le dessiner.
struct Entry {
    id: String,
    title: String,
    active: bool,
    bell: bool,
    alive: bool,
    connecting: bool,
}

/// Apparence d'un onglet.
struct Look<'a> {
    title: &'a str,
    status: Option<Color32>,
    active: bool,
    closable: bool,
    /// Connexion en cours d'etablissement: le bandeau du bas devient une barre
    /// de chargement.
    busy: bool,
    /// La pastille bat au lieu de rester fixe.
    pulse: bool,
}

/// Dessine un onglet. Renvoie (active, ferme).
fn tab_button(
    ui: &mut egui::Ui,
    look: Look<'_>,
    palette: &crate::theme::Palette,
    scale: f32,
) -> (bool, bool) {
    let Look {
        title,
        status,
        active,
        closable,
        busy,
        pulse,
    } = look;
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
    // La barre de chargement prend et rend la place du soulignement en fondu,
    // pour qu'une connexion etablie ne fasse pas sauter le bandeau.
    let loading = anim::toggle(ui, response.id.with("chargement"), busy, anim::VIEW);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let idle = anim::lerp_color(palette.bg_deep, palette.surface_high, hover);
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            anim::lerp_color(idle, palette.surface, selected),
        );
        // Le bandeau de deux pixels du bas sert a deux choses, jamais en meme
        // temps: signaler l'onglet courant, ou montrer que la connexion
        // travaille encore.
        let strip_top = rect.bottom() - 2.0;
        if loading > 0.0 {
            // Rail attenue, puis navette qui le parcourt: la duree d'un
            // `ssh` etant inconnue, la barre ne peut qu'etre indeterminee.
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(rect.left(), strip_top),
                    egui::pos2(rect.right(), rect.bottom()),
                ),
                CornerRadius::ZERO,
                anim::fade(palette.accent_dim, loading),
            );
            let shuttle = rect.width() * 0.35;
            let travel = anim::sweep(anim::cycle(ui.ctx(), anim::SWEEP));
            let left = anim::lerp(rect.left(), rect.right() - shuttle, travel);
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(left, strip_top),
                    egui::pos2(left + shuttle, rect.bottom()),
                ),
                CornerRadius::ZERO,
                anim::fade(palette.accent_soft, loading),
            );
        }
        if selected > 0.0 && loading < 1.0 {
            // Souligne l'onglet courant plutot que de le cerner: la barre
            // reste lisible meme avec beaucoup d'onglets.
            let underline = Rect::from_min_max(
                egui::pos2(rect.left(), strip_top),
                egui::pos2(rect.left() + rect.width() * selected, rect.bottom()),
            );
            painter.rect_filled(
                underline,
                CornerRadius::ZERO,
                anim::fade(palette.accent, 1.0 - loading),
            );
        }

        let mut text_x = rect.left() + 8.0;
        if let Some(color) = status {
            // Une attente se voit mieux qu'elle ne se lit: la pastille respire
            // tant que la connexion n'a pas abouti.
            let color = match pulse {
                true => anim::fade(
                    color,
                    anim::lerp(0.3, 1.0, anim::breathe(ui.ctx(), anim::PULSE)),
                ),
                false => color,
            };
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
    available: bool,
) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let font_size = app.config.ui.terminal_font_size;
    let binary = app.config.proton_pass.binary.clone();
    let now = ctx.input(|i| i.time);
    let mut queued: Vec<Action> = Vec::new();
    let mut clipboard: Option<String> = None;

    let tab = &mut app.tabs[index];
    let id = tab.id.clone();
    // L'identite egui suit l'onglet, pas sa position: fermer un onglet ne doit
    // pas transferer l'etat des widgets (selection, defilement) a son voisin.
    ui.push_id(egui::Id::new(&id), |ui| match &mut tab.state {
        TabState::WaitingAgent { vault, since } => {
            let vault = vault.clone();
            let waited = (now - *since).max(0.0);
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                // Chenillard pixel plutot que le `spinner` d'egui: meme role,
                // mais dans l'habillage du reste de l'interface.
                pixel::loader(ui, scale * 1.5, palette.accent);
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Demarrage de l'agent « {vault} »")).strong());
                ui.label(
                    RichText::new(format!("{binary} ssh-agent start --vault-name {vault}"))
                        .size(11.0)
                        .color(palette.text_dim),
                );
                ui.add_space(12.0);
                // L'attente a une fin connue: autant la montrer plutot que de
                // laisser croire que l'onglet est fige.
                let fraction = (waited / crate::app::AGENT_WAIT_TIMEOUT).clamp(0.0, 1.0) as f32;
                let (gauge, _) = ui.allocate_exact_size(Vec2::new(260.0, 10.0), Sense::hover());
                pixel::progress(
                    ui.painter(),
                    gauge,
                    fraction,
                    anim::lerp_color(palette.accent, palette.warning, fraction),
                    palette.border,
                );
                let remaining = (crate::app::AGENT_WAIT_TIMEOUT - waited).max(0.0);
                ui.label(
                    RichText::new(format!("abandon dans {remaining:.0} s"))
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
            let output = render::show(ui, session, font_size, available);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_input(time: f64) -> egui::RawInput {
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        }
    }

    /// Rectangles du bandeau de deux points du bas d'un onglet, pour une frame.
    ///
    /// L'echelle pixel est volontairement a 3: les carres des sprites font
    /// alors trois points de haut et ne se confondent pas avec le bandeau.
    fn strip_rects(ctx: &egui::Context, time: f64, busy: bool, active: bool) -> Vec<Rect> {
        let output = ctx.run_ui(raw_input(time), |ui| {
            tab_button(
                ui,
                Look {
                    title: "web-01",
                    status: Some(crate::theme::DARK.success),
                    active,
                    closable: true,
                    busy,
                    pulse: busy,
                },
                &crate::theme::DARK,
                3.0,
            );
        });
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if (shape.rect.height() - 2.0).abs() < 0.01 => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_connecting_tab_runs_a_shuttle_along_its_strip() {
        let ctx = egui::Context::default();
        // Premiere frame: l'animation de prise en charge du bandeau demarre a
        // zero, la navette n'est pas encore visible.
        strip_rects(&ctx, 0.0, true, false);
        let started = strip_rects(&ctx, 0.2, true, false);
        assert_eq!(started.len(), 2, "rail et navette attendus: {started:?}");

        let rail = started.iter().copied().fold(started[0], |wide, rect| {
            if rect.width() > wide.width() {
                rect
            } else {
                wide
            }
        });
        let shuttle = started
            .iter()
            .copied()
            .find(|rect| rect.width() < rail.width())
            .expect("navette");
        assert!(rail.contains_rect(shuttle), "navette hors du rail");

        // Un quart de periode plus tard, elle a avance sans sortir du rail.
        let later = strip_rects(&ctx, 0.2 + (anim::SWEEP / 4.0) as f64, true, false);
        let moved = later
            .iter()
            .copied()
            .find(|rect| rect.width() < rail.width())
            .expect("navette");
        assert!(moved.left() > shuttle.left(), "navette immobile");
        assert!(rail.contains_rect(moved), "navette hors du rail");
    }

    #[test]
    fn an_idle_tab_only_underlines_the_current_one() {
        let ctx = egui::Context::default();
        // Onglet courant, connexion etablie: un seul bandeau, le soulignement.
        strip_rects(&ctx, 0.0, false, true);
        let settled = strip_rects(&ctx, 1.0, false, true);
        assert_eq!(settled.len(), 1, "soulignement attendu seul: {settled:?}");

        // Onglet ni courant ni en connexion: rien du tout.
        let ctx = egui::Context::default();
        strip_rects(&ctx, 0.0, false, false);
        assert!(strip_rects(&ctx, 1.0, false, false).is_empty());
    }
}
