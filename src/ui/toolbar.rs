//! Barre d'outils superieure.

use egui::{Align, Frame, Layout, Margin, Pos2, RichText, Sense};

use crate::app::{Action, PassStatus, SshpassApp, ToastKind};
use crate::theme;
use crate::ui::widgets::{self, Button};
use crate::ui::{anim, pixel};

pub fn show(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let look = theme::look(ui.ctx());

    egui::Panel::top("toolbar")
        .exact_size(44.0)
        // Hors retro, ce sont les cartes qui separent les zones, pas des traits.
        .show_separator_line(look.pixel())
        .frame(
            Frame::new()
                .fill(palette.bg_deep)
                .inner_margin(Margin::symmetric(10, 6)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                logo(ui, &palette, scale);
                ui.add_space(6.0);
                ui.label(RichText::new("sshpass-gui").strong().size(15.0));
                ui.add_space(12.0);

                if ui
                    .add(Button::new("Connexion").icon(&pixel::PLUS, scale).primary())
                    .on_hover_text("Nouvelle connexion SSH")
                    .clicked()
                {
                    app.actions.push(Action::NewConnection(None));
                }
                if ui
                    .add(Button::new("Terminal local").icon(&pixel::TERMINAL, scale))
                    .on_hover_text("Ouvrir un shell local (Ctrl+Maj+T)")
                    .clicked()
                {
                    app.actions.push(Action::OpenLocalShell);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Le bouton reste en place et s'allume quand le panneau est
                    // ouvert: changer son libelle le ferait sauter de largeur.
                    if ui
                        .add(
                            Button::new("Coffre")
                                .icon(&pixel::KEY, scale)
                                .selected(app.show_vault_panel),
                        )
                        .on_hover_text("Panneau Proton Pass (Ctrl+Maj+P)")
                        .clicked()
                    {
                        app.show_vault_panel = !app.show_vault_panel;
                    }
                    ui.add_space(2.0);
                    if ui
                        .add(Button::new("Reglages").selected(app.settings_open))
                        .clicked()
                    {
                        app.settings_open = true;
                    }
                    ui.add_space(6.0);
                    status_chip(app, ui);
                });
            });
        });
}

/// Logo de la barre: il tombe a sa place au demarrage, puis ne bouge plus.
fn logo(ui: &mut egui::Ui, palette: &theme::Palette, scale: f32) {
    let look = theme::look(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(pixel::TERMINAL.size(scale), Sense::hover());
    let arrival = anim::appear(
        ui.ctx(),
        egui::Id::new("logo_barre"),
        look.duration(anim::INTRO),
    );
    let drop = anim::lerp_unclamped(-12.0, 0.0, look.arrive(arrival));
    let alpha = arrival.min(1.0);
    pixel::draw(
        ui.painter(),
        Pos2::new(rect.left(), rect.top() + drop),
        &pixel::TERMINAL,
        scale,
        anim::fade(palette.accent, alpha),
        anim::fade(palette.accent_soft, alpha),
    );
}

/// Pastille d'etat de Proton Pass, cliquable pour relancer la detection.
///
/// La couleur distingue le binaire de la session: une session fermee n'est pas
/// une panne, seulement une reconnexion a faire — d'ou l'orange plutot que le
/// rouge, reserve a ce qui demande une intervention.
fn status_chip(app: &mut SshpassApp, ui: &mut egui::Ui) {
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let look = theme::look(ui.ctx());
    let reconnecting = app.login.is_running();
    let (color, label) = match &app.pass_status {
        PassStatus::Probing => (palette.warning, "Proton Pass"),
        PassStatus::Ready { .. } => (palette.success, "Proton Pass"),
        PassStatus::LoggedOut { .. } => (palette.warning, "Session fermee"),
        PassStatus::Locked { .. } => (palette.danger, "Session verrouillee"),
        PassStatus::Missing(_) => (palette.danger, "Proton Pass"),
    };
    let (color, label) = if reconnecting {
        (palette.warning, "Reconnexion...")
    } else {
        (color, label)
    };
    let tooltip = if reconnecting {
        match app.login.state().url() {
            Some(url) => format!("Terminez la connexion dans le navigateur:\n{url}"),
            None => "Ouverture du lien de connexion Proton Pass...".to_string(),
        }
    } else {
        app.pass_status.summary()
    };

    // La pastille est une capsule: on reserve sa place sous le contenu, puis on
    // la remplit une fois sa taille et son survol connus.
    let background = ui.painter().add(egui::Shape::Noop);
    let probing = matches!(app.pass_status, PassStatus::Probing);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Une detection ou une reconnexion en cours se voit: chargeur
                // pendant que `pass-cli login` tourne, pastille qui bat ou
                // emet une onde pendant la detection.
                if reconnecting {
                    widgets::loader(ui, scale, color);
                } else if probing {
                    let beat = anim::lerp(0.3, 1.0, anim::breathe(ui.ctx(), anim::PULSE));
                    let dot = widgets::status_dot(ui, anim::fade(color, beat), scale, "");
                    widgets::ping(ui, dot.rect, color, scale);
                } else {
                    widgets::status_dot(ui, color, scale, "");
                }
                ui.label(RichText::new(label).color(palette.text_dim).size(12.0));
            });
        });

    let response = inner
        .response
        .interact(egui::Sense::click())
        .on_hover_text(&tooltip);
    let rect = inner.response.rect;
    let hover = anim::hover(ui, response.id.with("survol"), response.hovered());
    // La couleur d'etat glisse d'une teinte a l'autre au lieu de basculer.
    let tint = anim::lerp_color(palette.border, color, anim::lerp(0.35, 0.8, hover));
    let radius = look.pill(rect.height());
    if look.pixel() {
        ui.painter().set(
            background,
            egui::Shape::rect_filled(
                rect,
                radius,
                anim::lerp_color(palette.bg_deep, palette.surface, hover),
            ),
        );
    } else {
        ui.painter().set(
            background,
            egui::Shape::rect_filled(
                rect,
                radius,
                anim::lerp_color(palette.surface, palette.surface_high, hover),
            ),
        );
        widgets::outline(ui.painter(), rect, radius.nw, tint, &look);
    }
    widgets::ripple(ui, &response, rect, radius.nw, color);

    if response.clicked() && !reconnecting {
        app.actions.push(Action::RefreshVaults);
        app.actions.push(Action::Toast(
            "Verification de la session Proton Pass...".into(),
            ToastKind::Info,
        ));
    }
}
