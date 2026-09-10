//! Fenetres modales: ouverture animee, fermeture animee.
//!
//! Les quatre fenetres de l'application (fiche de connexion, confirmation de
//! suppression, nouveau dossier, reglages) partagent le meme habillage: ancrees
//! au centre, ni repliables ni redimensionnables, fond `surface` et marge de
//! 14 points. Elles sont donc construites ici, une seule fois.
//!
//! L'ouverture est facile: egui fait deja le fondu d'entree d'une `Area`, on y
//! ajoute une legere montee en echelle.
//!
//! La fermeture, elle, demande un detour. En mode immediat, une fenetre
//! disparait parce que l'etat qui la decrivait n'existe plus — la fiche de
//! connexion, par exemple, efface le mot de passe saisi des sa fermeture, et il
//! n'est pas question de la garder vivante quelques frames de plus juste pour
//! l'animer. On garde donc, non pas la fenetre, mais sa **trace**: rectangle et
//! titre. Quand une modale cesse d'etre dessinee, sa trace se retire en fondu a
//! sa place, puis est oubliee.

use std::collections::HashMap;

use egui::emath::TSTransform;
use egui::{
    Align2, Context, CornerRadius, FontId, Frame, Id, InnerResponse, LayerId, Margin, Order, Rect,
    Ui, Vec2,
};

use crate::theme::Palette;
use crate::ui::{anim, pixel};

/// Ce qui reste d'une fenetre une fois qu'elle n'est plus dessinee.
#[derive(Clone)]
struct Trace {
    rect: Rect,
    title: String,
    /// Derniere passe de rendu ou la fenetre a ete dessinee.
    pass: u64,
    /// Instant ou sa disparition a ete constatee.
    gone_at: Option<f64>,
}

/// Registre des fenetres suivies, range dans la memoire d'egui.
#[derive(Clone, Default)]
struct Traces(HashMap<Id, Trace>);

fn traces_id() -> Id {
    Id::new("modales_tracees")
}

/// Une fenetre modale de l'application.
pub struct Modal<'a> {
    title: &'a str,
    /// Bouton de fermeture de la barre de titre, comme `Window::open`.
    open: Option<&'a mut bool>,
    width: Option<f32>,
}

impl<'a> Modal<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            open: None,
            width: None,
        }
    }

    /// Ajoute la croix de fermeture, pilotee par le booleen donne.
    pub fn closable(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Largeur imposee au contenu.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn show<R>(
        self,
        ctx: &Context,
        palette: &Palette,
        contents: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<Option<R>>> {
        let Self { title, open, width } = self;
        let id = Id::new(("modale", title));
        let progress = anim::ease_out(anim::appear(ctx, id.with("apparition"), anim::MODAL));

        let mut window = egui::Window::new(title)
            .id(id)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                Frame::window(&ctx.style_of(egui::Theme::Dark))
                    .fill(palette.surface)
                    .inner_margin(Margin::same(14)),
            );
        if let Some(open) = open {
            window = window.open(open);
        }

        let response = window.show(ctx, |ui| {
            if let Some(width) = width {
                ui.set_width(width);
            }
            contents(ui)
        });

        if let Some(response) = &response {
            let rect = response.response.rect;
            // Montee en echelle depuis le centre. `transform_layer_shapes` ne
            // vaut que pour la frame en cours et ne touche pas aux entrees: la
            // fenetre se pose sans jamais decaler ses zones cliquables.
            if progress < 1.0 {
                let scaling = anim::lerp(0.96, 1.0, progress);
                let pivot = rect.center().to_vec2();
                ctx.transform_layer_shapes(
                    response.response.layer_id,
                    TSTransform::new(pivot * (1.0 - scaling), scaling),
                );
            }
            track(ctx, id, title, rect);
        }
        response
    }
}

/// Note qu'une fenetre est bien la, a cette place, a cette passe de rendu.
fn track(ctx: &Context, id: Id, title: &str, rect: Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        let traces: &mut Traces = data.get_temp_mut_or_default(traces_id());
        traces.0.insert(
            id,
            Trace {
                rect,
                title: title.to_string(),
                pass,
                gone_at: None,
            },
        );
    });
}

/// Efface en fondu les fenetres qui viennent de disparaitre.
///
/// A appeler une fois par frame, **apres** toutes les modales: c'est l'absence
/// d'une fenetre a la passe courante qui declenche son animation de sortie.
pub fn fade_out_closed(ctx: &Context, palette: &Palette) {
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let Some(traces) = ctx.data(|data| data.get_temp::<Traces>(traces_id())) else {
        return;
    };

    let mut remaining: HashMap<Id, Trace> = HashMap::new();
    for (id, mut trace) in traces.0 {
        if trace.pass >= pass {
            // Toujours a l'ecran: rien a animer.
            remaining.insert(id, trace);
            continue;
        }
        let gone_at = *trace.gone_at.get_or_insert(now);
        let progress = ((now - gone_at) as f32 / anim::MODAL).clamp(0.0, 1.0);
        if progress >= 1.0 {
            // Fondu termine: la trace n'a plus de raison d'etre.
            continue;
        }
        paint(ctx, palette, id, &trace, progress);
        remaining.insert(id, trace);
    }
    ctx.data_mut(|data| data.insert_temp(traces_id(), Traces(remaining)));
}

/// Dessine la trace d'une fenetre fermee: elle s'efface en se resserrant.
fn paint(ctx: &Context, palette: &Palette, id: Id, trace: &Trace, progress: f32) {
    let eased = anim::ease_out(progress);
    let alpha = 1.0 - eased;
    let rect = Rect::from_center_size(
        trace.rect.center(),
        trace.rect.size() * anim::lerp(1.0, 0.96, eased),
    );

    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, id.with("trace")));
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        anim::fade(palette.surface, alpha * 0.9),
    );
    pixel::frame(&painter, rect, anim::fade(palette.border, alpha), 1.0);
    painter.text(
        egui::pos2(rect.left() + 14.0, rect.top() + 12.0),
        Align2::LEFT_TOP,
        &trace.title,
        FontId::proportional(13.0),
        anim::fade(palette.text_dim, alpha),
    );
    // Un fondu ne depend d'aucune interaction: sans reveil programme, il se
    // figerait des que la souris s'arrete.
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vrai tant qu'une fenetre est suivie: affichee, ou en train de s'effacer.
    fn has_trace(ctx: &Context) -> bool {
        ctx.data(|data| data.get_temp::<Traces>(traces_id()))
            .is_some_and(|traces| !traces.0.is_empty())
    }

    fn input(time: f64) -> egui::RawInput {
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn a_closed_window_leaves_a_trace_then_is_forgotten() {
        let ctx = Context::default();
        let palette = crate::theme::DARK;

        // Deux frames avec la fenetre a l'ecran.
        for time in [0.0, 0.02] {
            let _ = ctx.run_ui(input(time), |ui| {
                let ctx = ui.ctx().clone();
                Modal::new("Essai").show(&ctx, &palette, |ui| {
                    ui.label("contenu");
                });
                fade_out_closed(&ctx, &palette);
            });
        }
        assert!(has_trace(&ctx), "fenetre affichee mais pas suivie");

        // La fenetre disparait: sa trace reste, le temps du fondu.
        let _ = ctx.run_ui(input(0.04), |ui| {
            fade_out_closed(&ui.ctx().clone(), &palette)
        });
        assert!(has_trace(&ctx), "fermeture sans animation de sortie");

        // Une fois le fondu termine, plus rien n'est dessine ni retenu.
        let _ = ctx.run_ui(input(0.04 + anim::MODAL as f64 + 0.01), |ui| {
            fade_out_closed(&ui.ctx().clone(), &palette)
        });
        assert!(!has_trace(&ctx), "trace jamais oubliee");
    }

    #[test]
    fn a_reopened_window_drops_its_pending_trace() {
        let ctx = Context::default();
        let palette = crate::theme::DARK;
        let show = |time: f64, visible: bool| {
            let _ = ctx.run_ui(input(time), |ui| {
                let ctx = ui.ctx().clone();
                if visible {
                    Modal::new("Essai").show(&ctx, &palette, |ui| {
                        ui.label("contenu");
                    });
                }
                fade_out_closed(&ctx, &palette);
            });
        };

        show(0.0, true);
        show(0.02, false);
        // Rouverte pendant son propre fondu de sortie: la trace redevient une
        // fenetre vivante, sans quoi le fantome resterait affiche par-dessus.
        show(0.04, true);
        let pending = ctx.data(|data| {
            data.get_temp::<Traces>(traces_id())
                .map(|traces| traces.0.values().any(|trace| trace.gone_at.is_some()))
        });
        assert_eq!(pending, Some(false));
    }
}
