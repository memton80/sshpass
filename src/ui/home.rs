//! Ecran d'accueil: recherche et connexions recentes.

use egui::{Align, Align2, CornerRadius, FontId, Layout, Pos2, Sense, Vec2};

use crate::app::{Action, SshpassApp};
use crate::config::relative_time;
use crate::theme;
use crate::ui::widgets::{self, Button};
use crate::ui::{self, anim, pixel};

/// Duree totale de la cascade d'arrivee des lignes de l'accueil.
const CASCADE: f32 = 0.5;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let look = theme::look(ui.ctx());

    // Entree en scene: logo, titre, sous-titre et recherche se posent l'un
    // apres l'autre. Elle se rejoue a chaque retour sur l'accueil, et ne coute
    // rien une fois terminee.
    let intro = anim::appear(
        ui.ctx(),
        egui::Id::new("accueil_intro"),
        look.duration(anim::INTRO),
    );
    let step = |rank: usize| anim::stagger(intro, rank, 0.14);

    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        logo(ui, &palette, scale, step(0));

        ui.add_space(12.0);
        rising_text(
            ui,
            "sshpass-gui",
            FontId::proportional(26.0),
            palette.text,
            step(1),
        );
        rising_text(
            ui,
            "Connexions SSH et secrets Proton Pass",
            FontId::proportional(13.0),
            palette.text_dim,
            step(2),
        );
        ui.add_space(24.0);

        ui.allocate_ui_with_layout(Vec2::new(460.0, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.scope(|ui| {
                ui.multiply_opacity(step(3).clamp(0.0, 1.0));
                ui.horizontal(|ui| {
                    pixel::icon(ui, &pixel::SEARCH, scale, palette.text_dim);
                    let response = widgets::field(
                        ui,
                        egui::TextEdit::singleline(&mut app.search)
                            .hint_text("Rechercher une connexion")
                            .desired_width(f32::INFINITY),
                    );
                    if app.focus_search {
                        response.request_focus();
                        app.focus_search = false;
                    }
                });
            });

            ui.add_space(20.0);
            let needle = app.search.clone();

            if needle.trim().is_empty() {
                recents(app, ui);
            } else {
                results(app, ui, &needle);
            }
        });
    });
}

/// Logo de l'accueil: il tombe a sa place, puis se souleve au survol. Hors
/// retro, un halo violet l'eclaire par en dessous.
fn logo(ui: &mut egui::Ui, palette: &theme::Palette, scale: f32, arrival: f32) {
    let look = theme::look(ui.ctx());
    let logo_scale = scale * 3.0;
    let (rect, response) = ui.allocate_exact_size(pixel::TERMINAL.size(logo_scale), Sense::hover());
    let hover = anim::hover(ui, response.id.with("survol"), response.hovered());
    let drop = anim::lerp_unclamped(-24.0, 0.0, look.arrive(arrival));
    let rise = look.lift * 2.0 * look.arrive(hover);
    let rect = rect.translate(Vec2::new(0.0, drop - rise));
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(arrival.clamp(0.0, 1.0));
    if look.glow {
        painter.add(widgets::glow(
            rect.shrink(2.0),
            (rect.width() / 2.0) as u8,
            palette.accent,
            1.2 + 0.6 * hover,
        ));
    }
    pixel::draw(
        &painter,
        rect.min,
        &pixel::TERMINAL,
        logo_scale,
        palette.accent,
        palette.accent_soft,
    );
}

/// Ligne de texte centree qui monte a sa place en apparaissant.
fn rising_text(ui: &mut egui::Ui, text: &str, font: FontId, color: egui::Color32, arrival: f32) {
    let look = theme::look(ui.ctx());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    if ui.is_rect_visible(rect) {
        let rise = anim::lerp_unclamped(12.0, 0.0, look.arrive(arrival));
        ui.painter().galley(
            Pos2::new(rect.left(), rect.top() + rise),
            galley,
            anim::fade(color, arrival),
        );
    }
}

fn recents(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let recents: Vec<(String, String, String, String)> = app
        .config
        .recents(8)
        .into_iter()
        .map(|c| {
            (
                c.id.clone(),
                c.display_name(),
                c.target(),
                c.last_used.map(relative_time).unwrap_or_default(),
            )
        })
        .collect();

    if recents.is_empty() {
        ui::section_title(ui, "Pour commencer", &palette);
        ui::hint(ui, "Aucune connexion ouverte pour l'instant.", &palette);
        ui.add_space(8.0);
        if ui
            .add(
                Button::new("Creer une connexion")
                    .icon(&pixel::PLUS, app.config.ui.pixel_scale as f32)
                    .primary(),
            )
            .clicked()
        {
            app.actions.push(Action::NewConnection(None));
        }
        return;
    }

    ui::section_title(ui, "Recentes", &palette);
    for (rank, (id, name, target, when)) in recents.into_iter().enumerate() {
        if entry(app, ui, &name, &target, &when, rank) {
            app.actions.push(Action::OpenConnection(id));
        }
    }
}

fn results(app: &mut SshpassApp, ui: &mut egui::Ui, needle: &str) {
    let palette = app.palette;
    let matches: Vec<(String, String, String)> = app
        .config
        .connections
        .iter()
        .filter(|c| c.matches(needle))
        .take(12)
        .map(|c| (c.id.clone(), c.display_name(), c.target()))
        .collect();

    ui::section_title(ui, "Resultats", &palette);
    if matches.is_empty() {
        ui::hint(ui, "Aucune connexion ne correspond.", &palette);
        return;
    }
    for (rank, (id, name, target)) in matches.into_iter().enumerate() {
        if entry(app, ui, &name, &target, "", rank) {
            app.actions.push(Action::OpenConnection(id));
        }
    }
}

/// Une ligne cliquable. Renvoie `true` si elle a ete activee.
///
/// `rank` est sa place dans la liste: les lignes n'apparaissent pas toutes en
/// meme temps, elles se posent l'une apres l'autre.
fn entry(
    app: &SshpassApp,
    ui: &mut egui::Ui,
    name: &str,
    target: &str,
    when: &str,
    rank: usize,
) -> bool {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let look = theme::look(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());

    // Le survol fait glisser le contenu vers la droite, souleve la carte et
    // allume sa bordure, au lieu de la faire basculer d'un etat a l'autre.
    let t = anim::hover(ui, response.id.with("hover"), response.hovered());
    let press = anim::toggle(
        ui,
        response.id.with("appui"),
        response.is_pointer_button_down_on(),
        0.06,
    );
    let shift = anim::lerp(0.0, 4.0, t);

    // Cascade d'arrivee: l'accueil se remplit ligne a ligne. La progression est
    // commune a toute la liste, seul le retard change d'une ligne a l'autre.
    let listing = anim::appear(
        ui.ctx(),
        egui::Id::new("accueil_liste"),
        look.duration(CASCADE),
    );
    let arrival = anim::stagger(listing, rank, 0.08);
    let rise = anim::lerp_unclamped(14.0, 0.0, look.arrive(arrival));

    if ui.is_rect_visible(rect) {
        // L'opacite passe par un pinceau a part: `multiply_opacity` sur la
        // `Ui` vaudrait pour tout ce qui suit, donc pour toutes les lignes
        // suivantes, et se cumulerait a chacune.
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(arrival.clamp(0.0, 1.0));
        let painter = &painter;
        let lift = look.lift * t;
        let rect = rect
            .translate(Vec2::new(0.0, rise - lift))
            .shrink(if look.pixel() { 0.0 } else { press * 1.5 });
        let radius = look.medium;
        if !look.pixel() {
            painter.add(widgets::shadow(rect, radius, lift, 0.4 + 0.6 * t));
        }
        if look.glow && t > 0.0 {
            painter.add(widgets::glow(rect, radius, palette.accent, 0.5 * t));
        }
        painter.rect_filled(
            rect,
            CornerRadius::same(radius),
            anim::lerp_color(palette.surface, palette.surface_high, t),
        );
        if t > 0.0 {
            widgets::outline(painter, rect, radius, anim::fade(palette.accent, t), &look);
        }
        widgets::ripple(ui, &response, rect, radius, palette.accent_soft);
        pixel::draw(
            painter,
            egui::pos2(rect.left() + 12.0 + shift, rect.center().y - 4.0 * scale),
            &pixel::SERVER,
            scale,
            anim::lerp_color(palette.accent_soft, palette.accent, t),
            palette.accent,
        );
        let text_x = rect.left() + 20.0 + 8.0 * scale + shift;
        painter.text(
            egui::pos2(text_x, rect.center().y - 10.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(13.0),
            palette.text,
        );
        painter.text(
            egui::pos2(text_x, rect.center().y + 3.0),
            Align2::LEFT_TOP,
            target,
            FontId::proportional(11.0),
            palette.text_dim,
        );
        if !when.is_empty() {
            painter.text(
                egui::pos2(rect.right() - 12.0, rect.center().y),
                Align2::RIGHT_CENTER,
                when,
                FontId::proportional(11.0),
                anim::lerp_color(palette.text_dim, palette.text, t),
            );
        }
    }
    ui.add_space(6.0);
    response.clicked()
}
