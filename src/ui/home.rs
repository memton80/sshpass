//! Ecran d'accueil: recherche et connexions recentes.

use egui::{Align, Align2, CornerRadius, FontId, Layout, RichText, Sense, Vec2};

use crate::app::{Action, SshpassApp};
use crate::config::relative_time;
use crate::ui::{self, anim, pixel};

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
        ui.label(RichText::new("sshpass").size(26.0).strong());
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
    for (id, name, target, when) in recents {
        if entry(app, ui, &name, &target, &when) {
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
    for (id, name, target) in matches {
        if entry(app, ui, &name, &target, "") {
            app.actions.push(Action::OpenConnection(id));
        }
    }
}

/// Une ligne cliquable. Renvoie `true` si elle a ete activee.
fn entry(app: &SshpassApp, ui: &mut egui::Ui, name: &str, target: &str, when: &str) -> bool {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());

    // Le survol fait glisser la carte vers la droite et allume sa bordure,
    // au lieu de la faire basculer d'un etat a l'autre.
    let t = anim::hover(ui, response.id.with("hover"), response.hovered());
    let shift = anim::lerp(0.0, 4.0, t);

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            anim::lerp_color(palette.surface, palette.surface_high, t),
        );
        if t > 0.0 {
            pixel::frame(painter, rect, anim::fade(palette.accent, t), scale);
        }
        pixel::draw(
            painter,
            egui::pos2(rect.left() + 10.0 + shift, rect.center().y - 4.0 * scale),
            &pixel::SERVER,
            scale,
            anim::lerp_color(palette.accent_soft, palette.accent, t),
            palette.accent,
        );
        let text_x = rect.left() + 18.0 + 8.0 * scale + shift;
        painter.text(
            egui::pos2(text_x, rect.center().y - 9.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(13.0),
            palette.text,
        );
        painter.text(
            egui::pos2(text_x, rect.center().y + 2.0),
            Align2::LEFT_TOP,
            target,
            FontId::proportional(11.0),
            palette.text_dim,
        );
        if !when.is_empty() {
            painter.text(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                when,
                FontId::proportional(11.0),
                palette.text_dim,
            );
        }
    }
    ui.add_space(4.0);
    response.clicked()
}
