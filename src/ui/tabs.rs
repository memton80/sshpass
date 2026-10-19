//! Zone centrale: barre d'onglets et contenu (accueil ou terminal).

use egui::{Align2, Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, Vec2};

use crate::app::{Action, SshpassApp, TabState};
use crate::term::render;
use crate::theme;
use crate::ui::widgets::{self, Button};
use crate::ui::{anim, home, pixel};

const TAB_HEIGHT: f32 = 30.0;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let palette = app.palette;
    let look = theme::look(&ctx);
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

    // En retro, la zone centrale est un aplat colle aux panneaux. Ailleurs, elle
    // laisse voir le fond de la fenetre, et le contenu y est pose en carte.
    let frame = if look.pixel() {
        Frame::new().fill(palette.bg)
    } else {
        Frame::new().outer_margin(Margin {
            left: look.gap,
            right: look.gap,
            top: 0,
            bottom: look.gap,
        })
    };

    egui::CentralPanel::no_frame().frame(frame).show(ui, |ui| {
        strip(app, ui);

        // Transition entre vues: la nouvelle apparait en fondu. La cle
        // identifie la vue affichee; un changement relance l'animation.
        let key = app
            .active_tab
            .and_then(|index| app.tabs.get(index))
            .map(|tab| view_key(&tab.id))
            .unwrap_or(0);
        let progress =
            anim::view_transition(ui, egui::Id::new("vue"), key, look.duration(anim::VIEW));

        match app.active_tab {
            Some(index) if index < app.tabs.len() => {
                // Seule l'opacite est animee ici: decaler le rectangle du
                // terminal changerait le nombre de colonnes a chaque frame
                // et declencherait une cascade de redimensionnements.
                ui.multiply_opacity(progress);
                let card = if look.pixel() {
                    Frame::new()
                } else {
                    // Le fond du terminal deborde dans la marge de la carte:
                    // ses angles droits restent caches sous l'arrondi.
                    Frame::new()
                        .fill(palette.bg_deep)
                        .stroke(Stroke::new(1.0, palette.border))
                        .corner_radius(look.large)
                        .inner_margin(Margin::same(look.large as i8 / 2 + 2))
                };
                card.show(ui, |ui| terminal(app, ui, &ctx, index, available));
            }
            _ => {
                app.active_tab = None;
                ui.multiply_opacity(progress);
                // L'accueil, lui, peut glisser: rien n'y depend de la
                // hauteur exacte disponible.
                let slide = anim::lerp(10.0, 0.0, progress) as i8;
                let card = if look.pixel() {
                    Frame::new()
                } else {
                    Frame::new().fill(palette.bg).corner_radius(look.large)
                };
                card.show(ui, |ui| {
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
                });
            }
        }
    });
}

/// Barre d'onglets, avec le bouton d'accueil en tete.
fn strip(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let look = theme::look(ui.ctx());

    egui::Panel::top("tab_strip")
        .exact_size(TAB_HEIGHT + 8.0)
        .show_separator_line(look.pixel())
        .frame(
            Frame::new()
                .fill(palette.bg_deep)
                .inner_margin(Margin::symmetric(if look.pixel() { 6 } else { 0 }, 4)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if !look.pixel() {
                            ui.spacing_mut().item_spacing.x = 4.0;
                        }
                        let home_active = app.active_tab.is_none();
                        let home = TabLook {
                            id: egui::Id::new("onglet_accueil"),
                            title: "Accueil",
                            status: None,
                            active: home_active,
                            closable: false,
                            busy: false,
                            pulse: false,
                        };
                        let home = tab_button(ui, home, &palette, scale);
                        if home.clicked {
                            app.actions.push(Action::ShowHome);
                        }
                        // Rectangle de l'onglet courant, et part de son bandeau
                        // deja prise par la barre de chargement.
                        let mut current = home_active.then_some((home.rect, home.loading));

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
                            let tab = tab_button(
                                ui,
                                TabLook {
                                    id: egui::Id::new(("onglet", &entry.id)),
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
                            if entry.active {
                                current = Some((tab.rect, tab.loading));
                            }
                            if tab.clicked {
                                app.actions.push(Action::SelectTab(entry.id.clone()));
                            }
                            if tab.closed {
                                app.actions.push(Action::CloseTab(entry.id));
                            }
                        }

                        if let Some((rect, loading)) = current {
                            indicator(ui, rect, 1.0 - loading, &palette);
                        }
                    });
                });
        });
}

/// Soulignement de l'onglet courant. Il n'y en a qu'un: il glisse d'un onglet
/// a l'autre au lieu de s'eteindre sous l'un et de s'allumer sous l'autre.
///
/// `visible` est la part du bandeau qui lui revient: une connexion en cours le
/// remplace par sa barre de chargement. Renvoie le rectangle dessine.
fn indicator(ui: &egui::Ui, target: Rect, visible: f32, palette: &theme::Palette) -> Rect {
    let look = theme::look(ui.ctx());
    let target = if look.pixel() {
        // Bandeau de deux pixels sur toute la largeur, comme a l'origine.
        Rect::from_min_max(egui::pos2(target.left(), target.bottom() - 2.0), target.max)
    } else {
        // Capsule courte sous le libelle.
        Rect::from_min_max(
            egui::pos2(target.left() + 10.0, target.bottom() - 3.0),
            egui::pos2(target.right() - 10.0, target.bottom()),
        )
    };
    let seconds = look.duration(anim::VIEW);
    let curve = |t: f32| look.arrive(t);
    let id = egui::Id::new("indicateur_onglet");
    let left = anim::glide(ui.ctx(), id.with("gauche"), target.left(), seconds, curve);
    let right = anim::glide(ui.ctx(), id.with("droite"), target.right(), seconds, curve);
    let rect = Rect::from_min_max(
        egui::pos2(left, target.top()),
        egui::pos2(right.max(left + 2.0), target.bottom()),
    );
    if visible <= 0.0 {
        return rect;
    }
    let painter = ui.painter();
    if look.glow {
        painter.add(widgets::glow(rect, 2, palette.accent, 0.6 * visible));
    }
    painter.rect_filled(
        rect,
        look.pill(rect.height()),
        anim::fade(palette.accent, visible),
    );
    rect
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
struct TabLook<'a> {
    /// Identite stable: suit l'onglet, pas sa place dans la barre.
    id: egui::Id,
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

/// Ce que la barre doit savoir d'un onglet une fois dessine.
struct TabResponse {
    clicked: bool,
    closed: bool,
    rect: Rect,
    /// Part du bandeau prise par la barre de chargement (0 a 1).
    loading: f32,
}

/// Dessine un onglet.
fn tab_button(
    ui: &mut egui::Ui,
    look: TabLook<'_>,
    palette: &theme::Palette,
    scale: f32,
) -> TabResponse {
    let TabLook {
        id,
        title,
        status,
        active,
        closable,
        busy,
        pulse,
    } = look;
    let allure = theme::look(ui.ctx());
    let label = crate::ui::sidebar::truncate(title, 24);
    let text_width = label.chars().count() as f32 * 7.0;
    let width = text_width + 40.0 + if closable { 20.0 } else { 0.0 };
    // Un nouvel onglet s'ouvre en s'elargissant: les suivants s'ecartent pour
    // lui faire place au lieu d'etre pousses d'un coup.
    let grow = if closable {
        allure.arrive(anim::appear(
            ui.ctx(),
            id.with("ouverture"),
            allure.duration(anim::VIEW),
        ))
    } else {
        1.0
    };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new((width * grow).max(0.0), TAB_HEIGHT),
        Sense::click(),
    );
    let content = Rect::from_min_size(rect.min, Vec2::new(width, TAB_HEIGHT));

    let close_rect = Rect::from_min_size(
        egui::pos2(content.right() - 22.0, content.center().y - 6.0),
        Vec2::splat(12.0),
    );
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let on_close = closable && pointer.is_some_and(|p| close_rect.expand(3.0).contains(p));
    let hover = anim::hover(ui, response.id.with("hover"), response.hovered());
    let close_hover = anim::hover(
        ui,
        response.id.with("croix"),
        response.hovered() && on_close,
    );
    let selected = anim::toggle(
        ui,
        response.id.with("active"),
        active,
        allure.duration(anim::VIEW),
    );
    // La barre de chargement prend et rend la place du soulignement en fondu,
    // pour qu'une connexion etablie ne fasse pas sauter le bandeau.
    let loading = anim::toggle(ui, response.id.with("chargement"), busy, anim::VIEW);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let painter = &painter;
        let radius = allure.small;
        let idle = anim::lerp_color(palette.bg_deep, palette.surface_high, hover);
        let fill = anim::lerp_color(idle, palette.surface, selected);
        painter.rect_filled(rect, radius, fill);
        widgets::ripple(ui, &response, rect, radius, palette.accent_soft);
        // Le bandeau de deux pixels du bas sert a deux choses, jamais en meme
        // temps: signaler l'onglet courant (cf. `indicator`), ou montrer que la
        // connexion travaille encore.
        if loading > 0.0 {
            // Rail attenue, puis navette qui le parcourt: la duree d'un
            // `ssh` etant inconnue, la barre ne peut qu'etre indeterminee.
            let inset = if allure.pixel() { 0.0 } else { 8.0 };
            let rail = Rect::from_min_max(
                egui::pos2(rect.left() + inset, rect.bottom() - 2.0),
                egui::pos2(rect.right() - inset, rect.bottom()),
            );
            let round = allure.pill(rail.height());
            painter.rect_filled(rail, round, anim::fade(palette.accent_dim, loading));
            let shuttle = rail.width() * 0.35;
            let travel = anim::sweep(anim::cycle(ui.ctx(), anim::SWEEP));
            let left = anim::lerp(rail.left(), rail.right() - shuttle, travel);
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(left, rail.top()),
                    egui::pos2(left + shuttle, rail.bottom()),
                ),
                round,
                anim::fade(palette.accent_soft, loading),
            );
        }

        let mut text_x = content.left() + 8.0;
        let icon = Rect::from_min_size(
            egui::pos2(text_x, content.center().y - 4.0 * scale),
            pixel::DOT.size(scale),
        );
        if let Some(color) = status {
            // Une attente se voit mieux qu'elle ne se lit: la pastille respire
            // tant que la connexion n'a pas abouti, et emet une onde hors retro.
            let shown = if pulse {
                widgets::ping(ui, icon, color, scale);
                anim::fade(
                    color,
                    anim::lerp(0.3, 1.0, anim::breathe(ui.ctx(), anim::PULSE)),
                )
            } else {
                color
            };
            widgets::paint_dot(painter, icon, shown, scale, &allure);
        } else {
            let color = anim::lerp_color(palette.text_dim, palette.accent_soft, selected);
            pixel::draw(painter, icon.min, &pixel::TERMINAL, scale, color, color);
        }
        text_x += 8.0 * scale + 4.0;

        let idle_text = anim::lerp_color(palette.text_dim, palette.text, hover);
        painter.text(
            egui::pos2(text_x, content.center().y),
            Align2::LEFT_CENTER,
            &label,
            FontId::proportional(12.0),
            anim::lerp_color(idle_text, palette.text, selected),
        );

        if closable {
            // La croix s'affirme au survol de l'onglet et vire au rouge quand
            // le pointeur l'atteint, sur un disque qui grandit derriere elle.
            if close_hover > 0.0 && !allure.pixel() {
                painter.circle_filled(
                    close_rect.center(),
                    anim::lerp(4.0, 10.0, allure.arrive(close_hover)),
                    anim::fade(palette.danger, 0.22 * close_hover),
                );
            }
            let base = anim::lerp_color(palette.text_dim, palette.danger, close_hover);
            let visible = hover.max(selected).max(0.5);
            let color = anim::fade(base, visible);
            pixel::draw(painter, close_rect.min, &pixel::CLOSE, 1.5, color, color);
        }
    }

    let clicked = response.clicked();
    TabResponse {
        clicked: clicked && !on_close,
        closed: clicked && on_close,
        rect,
        loading,
    }
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
                // Chargeur maison plutot que le `spinner` d'egui: meme role,
                // mais dans l'habillage du reste de l'interface.
                widgets::loader(ui, scale * 1.5, palette.accent);
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
                widgets::progress(
                    ui.painter(),
                    gauge,
                    fraction,
                    anim::lerp_color(palette.accent, palette.warning, fraction),
                    palette.border,
                    &theme::look(ui.ctx()),
                );
                let remaining = (crate::app::AGENT_WAIT_TIMEOUT - waited).max(0.0);
                ui.label(
                    RichText::new(format!("abandon dans {remaining:.0} s"))
                        .size(11.0)
                        .color(palette.text_dim),
                );
                ui.add_space(16.0);
                if ui
                    .add(Button::new("Connecter sans attendre l'agent").primary())
                    .clicked()
                {
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
                    .corner_radius(theme::look(ui.ctx()).medium)
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.label(RichText::new(&message).size(12.0).color(palette.text_dim));
                    });
                ui.add_space(12.0);
                if ui.add(Button::new("Fermer l'onglet")).clicked() {
                    queued.push(Action::CloseTab(id.clone()));
                }
            });
        }
        TabState::Running(session) => {
            if let Some(status) = session.exit_status.clone() {
                egui::Panel::bottom("session_status")
                    .show_separator_line(theme::look(ui.ctx()).pixel())
                    .frame(
                        Frame::new()
                            .fill(palette.surface)
                            .corner_radius(theme::look(ui.ctx()).medium)
                            .inner_margin(Margin::symmetric(10, 6)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            widgets::status_dot(
                                ui,
                                palette.danger,
                                app.config.ui.pixel_scale as f32,
                                "",
                            );
                            ui.label(RichText::new(status).size(12.0).color(palette.text_dim));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.add(Button::new("Fermer").small()).clicked() {
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
    use crate::config::UiStyle;

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

    fn context(style: UiStyle) -> egui::Context {
        let ctx = egui::Context::default();
        theme::apply(&ctx, &theme::DARK, 14.0, style);
        ctx
    }

    /// Rectangles du bandeau de deux points du bas d'un onglet, pour une frame.
    ///
    /// L'echelle pixel est volontairement a 3: les carres des sprites font
    /// alors trois points de haut et ne se confondent pas avec le bandeau.
    fn strip_rects(ctx: &egui::Context, time: f64, busy: bool, active: bool) -> Vec<Rect> {
        let output = ctx.run_ui(raw_input(time), |ui| {
            tab_button(
                ui,
                TabLook {
                    id: egui::Id::new("essai"),
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
        for style in UiStyle::ALL {
            let ctx = context(style);
            // Premiere frame: l'onglet s'ouvre et l'animation de prise en
            // charge du bandeau demarre a zero. Une seconde plus tard, tout
            // est en place.
            strip_rects(&ctx, 0.0, true, false);
            let started = strip_rects(&ctx, 1.0, true, false);
            assert_eq!(
                started.len(),
                2,
                "{style:?}: rail et navette attendus: {started:?}"
            );

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
            assert!(
                rail.contains_rect(shuttle),
                "{style:?}: navette hors du rail"
            );

            // Un quart de periode plus tard, elle a avance sans sortir du rail.
            let later = strip_rects(&ctx, 1.0 + (anim::SWEEP / 4.0) as f64, true, false);
            let moved = later
                .iter()
                .copied()
                .find(|rect| rect.width() < rail.width())
                .expect("navette");
            assert_ne!(moved.left(), shuttle.left(), "{style:?}: navette immobile");
            assert!(rail.contains_rect(moved), "{style:?}: navette hors du rail");
        }
    }

    #[test]
    fn an_idle_tab_leaves_its_strip_to_the_indicator() {
        // Le soulignement de l'onglet courant n'est plus dessine par l'onglet:
        // c'est l'indicateur de la barre qui glisse sous lui.
        for active in [true, false] {
            let ctx = context(UiStyle::Lively);
            strip_rects(&ctx, 0.0, false, active);
            assert!(strip_rects(&ctx, 1.0, false, active).is_empty());
        }
    }

    #[test]
    fn a_new_tab_opens_by_widening() {
        let ctx = context(UiStyle::Soft);
        let mut widths = Vec::new();
        for time in [0.0, 0.08, 1.0] {
            let _ = ctx.run_ui(raw_input(time), |ui| {
                let tab = tab_button(
                    ui,
                    TabLook {
                        id: egui::Id::new("nouveau"),
                        title: "web-01",
                        status: None,
                        active: false,
                        closable: true,
                        busy: false,
                        pulse: false,
                    },
                    &theme::DARK,
                    2.0,
                );
                widths.push(tab.rect.width());
            });
        }
        assert_eq!(widths[0], 0.0, "{widths:?}");
        assert!(widths[1] > 0.0 && widths[1] < widths[2], "{widths:?}");
    }

    #[test]
    fn the_indicator_glides_from_one_tab_to_the_next() {
        let ctx = context(UiStyle::Lively);
        let first = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 30.0));
        let second = Rect::from_min_size(egui::pos2(200.0, 0.0), egui::vec2(100.0, 30.0));
        let mut lefts = Vec::new();
        for (time, target) in [(0.0, first), (0.5, second), (0.55, second), (2.0, second)] {
            let _ = ctx.run_ui(raw_input(time), |ui| {
                lefts.push(indicator(ui, target, 1.0, &theme::DARK).left());
            });
        }
        // Pose d'emblee sous le premier onglet, puis en route vers le second
        // sans y sauter, et arrive a la fin.
        assert!((lefts[0] - lefts[1]).abs() < 0.01, "{lefts:?}");
        assert!(lefts[2] > lefts[1] && lefts[2] < lefts[3], "{lefts:?}");
        assert!((lefts[3] - (second.left() + 10.0)).abs() < 0.5, "{lefts:?}");
    }
}
