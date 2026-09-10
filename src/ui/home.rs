//! Ecran d'accueil: recherche et connexions recentes.

use egui::{Align, Align2, CornerRadius, FontId, Layout, RichText, Sense, Vec2};

use crate::app::{Action, SshpassApp};
use crate::config::relative_time;
use crate::ui::{self, anim, pixel};

/// Duree totale de la cascade d'arrivee des lignes de l'accueil.
const CASCADE: f32 = 0.5;

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;

    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        let logo_scale = scale * 3.0;
        let (rect, _) = ui.allocate_exact_size(pixel::TERMINAL.size(logo_scale), Sense::hover());
        pixel::draw(
            ui.painter(),
            rect.min,
            &pixel::TERMINAL,
            logo_scale,
            palette.accent,
            palette.accent_soft,
        );

        ui.add_space(12.0);
        ui.label(RichText::new("sshpass-gui").size(26.0).strong());
        ui.label(
            RichText::new("Connexions SSH et secrets Proton Pass")
                .size(13.0)
                .color(palette.text_dim),
        );
        ui.add_space(24.0);

        ui.allocate_ui_with_layout(Vec2::new(460.0, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.horizontal(|ui| {
                pixel::icon(ui, &pixel::SEARCH, scale, palette.text_dim);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut app.search)
                        .hint_text("Rechercher une connexion")
                        .desired_width(f32::INFINITY),
                );
                if app.focus_search {
                    response.request_focus();
                    app.focus_search = false;
                }
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
        if ui.button("+ Creer une connexion").clicked() {
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
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());

    // Le survol fait glisser la carte vers la droite et allume sa bordure,
    // au lieu de la faire basculer d'un etat a l'autre.
    let t = anim::hover(ui, response.id.with("hover"), response.hovered());
    let shift = anim::lerp(0.0, 4.0, t);

    // Cascade d'arrivee: l'accueil se remplit ligne a ligne. La progression est
    // commune a toute la liste, seul le retard change d'une ligne a l'autre.
    let listing = anim::appear(ui.ctx(), egui::Id::new("accueil_liste"), CASCADE);
    let arrival = anim::ease_out(anim::stagger(listing, rank, 0.08));
    let rise = anim::lerp(10.0, 0.0, arrival);

    if ui.is_rect_visible(rect) {
        // L'opacite est appliquee couleur par couleur: `multiply_opacity`
        // vaudrait pour tout ce qui suit dans la meme `Ui`, donc pour toutes
        // les lignes suivantes, et se cumulerait a chacune.
        let veil = |color: egui::Color32| anim::fade(color, arrival);
        let rect = rect.translate(Vec2::new(0.0, rise));
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            veil(anim::lerp_color(palette.surface, palette.surface_high, t)),
        );
        if t > 0.0 {
            pixel::frame(painter, rect, veil(anim::fade(palette.accent, t)), scale);
        }
        pixel::draw(
            painter,
            egui::pos2(rect.left() + 10.0 + shift, rect.center().y - 4.0 * scale),
            &pixel::SERVER,
            scale,
            veil(anim::lerp_color(palette.accent_soft, palette.accent, t)),
            veil(palette.accent),
        );
        let text_x = rect.left() + 18.0 + 8.0 * scale + shift;
        painter.text(
            egui::pos2(text_x, rect.center().y - 9.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(13.0),
            veil(palette.text),
        );
        painter.text(
            egui::pos2(text_x, rect.center().y + 2.0),
            Align2::LEFT_TOP,
            target,
            FontId::proportional(11.0),
            veil(palette.text_dim),
        );
        if !when.is_empty() {
            painter.text(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                when,
                FontId::proportional(11.0),
                veil(palette.text_dim),
            );
        }
    }
    ui.add_space(4.0);
    response.clicked()
}
