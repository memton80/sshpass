//! Widgets animes: bouton, interrupteur, puce, selecteur segmente, champ a
//! halo, pastille, chargeur et jauge.
//!
//! Les widgets natifs d'egui changent d'apparence d'un coup, d'une frame a
//! l'autre: survol, appui, focus. Ceux-ci interpolent chaque transition
//! (`ui::anim`) et suivent l'allure courante (`theme::Look`): rayons, rebonds,
//! ondes au clic et halos. En retro, ils retrouvent les angles droits et les
//! liseres pixel art.
//!
//! Ils lisent palette et allure dans la memoire d'egui (`theme::palette`,
//! `theme::look`) plutot que de les recevoir en parametre: c'est ce qui leur
//! permet d'implementer `egui::Widget`, et donc de passer par
//! `Ui::add_enabled` et `on_disabled_hover_text` comme un bouton natif.

use egui::epaint::RectShape;
use egui::{
    Color32, CornerRadius, FontId, Margin, Painter, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, TextEdit, TextStyle, Ui, Vec2, Widget, WidgetInfo, WidgetType,
};

use crate::theme::{self, Look, Palette};
use crate::ui::anim;
use crate::ui::pixel::{self, Sprite};

// ---------------------------------------------------------------------------
// Primitives de dessin
// ---------------------------------------------------------------------------

/// Contour d'une surface: lisere aux coins evides en retro, trait arrondi
/// ailleurs.
pub fn outline(painter: &Painter, rect: Rect, radius: u8, color: Color32, look: &Look) {
    if look.pixel() {
        pixel::frame(painter, rect, color, 1.0);
    } else {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, color), StrokeKind::Inside);
    }
}

/// Halo flou autour d'un rectangle, a poser **sous** lui.
///
/// Le flou d'epaint est centre sur le bord: la moitie deborde, l'autre est
/// cachee par la surface. On elargit donc le rectangle de la moitie du flou.
pub fn glow(rect: Rect, radius: u8, color: Color32, strength: f32) -> Shape {
    let blur = 16.0;
    let spread = 1.0 + 2.0 * strength;
    RectShape::filled(
        rect.expand(spread + blur / 2.0 - 2.0),
        CornerRadius::same(radius.saturating_add(spread as u8 + 6)),
        anim::fade(color, 0.45 * strength),
    )
    .with_blur_width(blur)
    .into()
}

/// Ombre portee sous une surface soulevee de `lift` points.
pub fn shadow(rect: Rect, radius: u8, lift: f32, strength: f32) -> Shape {
    let blur = 8.0 + lift * 4.0;
    RectShape::filled(
        rect.translate(Vec2::new(0.0, 2.0 + lift)),
        CornerRadius::same(radius),
        Color32::from_black_alpha((90.0 * strength.clamp(0.0, 1.0)) as u8),
    )
    .with_blur_width(blur)
    .into()
}

/// Onde partie du point d'appui.
#[derive(Debug, Clone, Copy)]
struct Wave {
    origin: Pos2,
    start: f64,
}

/// Note l'appui sur `response` et dessine l'onde en cours dans `rect`.
///
/// L'onde est un rectangle arrondi qui grandit depuis le point d'appui jusqu'a
/// couvrir la surface: contenu dans elle et du meme rayon, il ne deborde jamais
/// de ses coins arrondis — un disque, lui, demanderait un decoupage arrondi
/// qu'egui ne sait pas faire.
pub fn ripple(ui: &Ui, response: &Response, rect: Rect, radius: u8, color: Color32) {
    if !theme::look(ui.ctx()).ripple {
        return;
    }
    let id = response.id.with("onde");
    let ctx = ui.ctx();
    let now = ctx.input(|i| i.time);
    let pressed = ctx.input(|i| i.pointer.any_pressed());
    if pressed && response.is_pointer_button_down_on() {
        if let Some(origin) = response.interact_pointer_pos() {
            ctx.data_mut(|d| d.insert_temp(id, Wave { origin, start: now }));
        }
    }
    let Some(wave) = ctx.data(|d| d.get_temp::<Wave>(id)) else {
        return;
    };
    let t = ((now - wave.start) as f32 / anim::RIPPLE).clamp(0.0, 1.0);
    if t >= 1.0 {
        ctx.data_mut(|d| d.remove::<Wave>(id));
        return;
    }
    let seed = Rect::from_center_size(wave.origin, Vec2::splat(6.0)).intersect(rect);
    let shape = seed.lerp_towards(&rect, anim::ease_out(t));
    let radius = (radius as f32)
        .min(shape.height() / 2.0)
        .min(shape.width() / 2.0);
    ui.painter().rect_filled(
        shape,
        CornerRadius::same(radius as u8),
        anim::fade(color, 0.35 * (1.0 - t)),
    );
    // L'onde suit l'horloge: personne d'autre ne redemandera la frame.
    ctx.request_repaint();
}

// ---------------------------------------------------------------------------
// Bouton
// ---------------------------------------------------------------------------

/// Role d'un bouton, qui decide de ses couleurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// L'action principale d'un ecran: remplie de violet.
    Primary,
    /// Toutes les autres.
    Secondary,
    /// Ce qui supprime ou arrete.
    Danger,
}

/// Bouton anime: survol, appui, onde, halo.
pub struct Button<'a> {
    label: String,
    icon: Option<(&'a Sprite, f32)>,
    tone: Tone,
    small: bool,
    selected: bool,
}

impl<'a> Button<'a> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            tone: Tone::Secondary,
            small: false,
            selected: false,
        }
    }

    pub fn primary(mut self) -> Self {
        self.tone = Tone::Primary;
        self
    }

    pub fn danger(mut self) -> Self {
        self.tone = Tone::Danger;
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    /// Sprite pixel art avant le libelle, a l'echelle donnee.
    pub fn icon(mut self, sprite: &'a Sprite, scale: f32) -> Self {
        self.icon = Some((sprite, scale));
        self
    }

    /// Etat enclenche, pour un bouton qui ouvre et ferme quelque chose.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            label,
            icon,
            tone,
            small,
            selected,
        } = self;
        let palette = theme::palette(ui.ctx());
        let look = theme::look(ui.ctx());

        let font = if small {
            let body = TextStyle::Body.resolve(ui.style());
            FontId::new(body.size - 1.0, body.family)
        } else {
            TextStyle::Button.resolve(ui.style())
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label.clone(), font, Color32::PLACEHOLDER);
        let padding = if small { 8.0 } else { 12.0 };
        let height = if small { 22.0 } else { 28.0 };
        let icon_width = icon.map_or(0.0, |(sprite, scale)| sprite.size(scale).x + 6.0);
        let size = Vec2::new(galley.size().x + icon_width + 2.0 * padding, height);

        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), &label));
        let enabled = ui.is_enabled();
        let hover = anim::hover(
            ui,
            response.id.with("survol"),
            enabled && response.hovered(),
        );
        // L'appui est bref (0,06 s): un relachement mou donnerait
        // l'impression que le clic n'a pas ete pris.
        let press = anim::toggle(
            ui,
            response.id.with("appui"),
            response.is_pointer_button_down_on(),
            0.06,
        );
        let on = anim::toggle(ui, response.id.with("choisi"), selected, anim::HOVER);

        if !ui.is_rect_visible(rect) {
            return response;
        }
        let radius = look.small;
        let (fill, border, text, wave) = colors(&palette, tone, hover.max(on), press);
        let veil = |color: Color32| {
            if enabled {
                color
            } else {
                anim::fade(color, 0.45)
            }
        };

        // Ecrasement a l'appui: le bouton se resserre d'un point. En retro le
        // contenu descend d'un pixel, comme un bouton 8 bits.
        let (rect, sink) = if look.pixel() {
            (rect, press.round())
        } else {
            (rect.shrink(press), 0.0)
        };
        let painter = ui.painter();
        if look.glow && enabled {
            let strength = match tone {
                Tone::Primary => 0.35 + 0.65 * hover,
                _ => hover.max(on) * 0.5,
            };
            if strength > 0.0 {
                painter.add(glow(rect, radius, border, strength));
            }
        }
        painter.rect_filled(rect, radius, veil(fill));
        outline(painter, rect, radius, veil(border), &look);
        ripple(ui, &response, rect, radius, wave);

        let mut x = rect.left() + padding;
        let center_y = rect.center().y + sink;
        if let Some((sprite, scale)) = icon {
            let color = veil(anim::lerp_color(
                palette.text_dim,
                text,
                hover.max(on).max(0.4),
            ));
            pixel::draw(
                painter,
                Pos2::new(x, center_y - sprite.size(scale).y / 2.0),
                sprite,
                scale,
                color,
                color,
            );
            x += sprite.size(scale).x + 6.0;
        }
        painter.galley(
            Pos2::new(x, center_y - galley.size().y / 2.0),
            galley,
            veil(text),
        );
        response
    }
}

/// Fond, bordure, texte et onde d'un bouton, pour un survol et un appui donnes.
fn colors(
    palette: &Palette,
    tone: Tone,
    hover: f32,
    press: f32,
) -> (Color32, Color32, Color32, Color32) {
    match tone {
        Tone::Primary => (
            anim::lerp_color(
                anim::lerp_color(palette.accent_dim, palette.accent, hover),
                palette.accent_dim,
                press,
            ),
            anim::lerp_color(palette.accent, palette.accent_soft, hover),
            palette.text,
            Color32::WHITE,
        ),
        Tone::Secondary => (
            anim::lerp_color(
                anim::lerp_color(palette.surface_high, palette.border, hover),
                palette.accent_dim,
                press,
            ),
            anim::lerp_color(palette.border, palette.accent, hover.max(press)),
            anim::lerp_color(palette.text, Color32::WHITE, hover),
            palette.accent_soft,
        ),
        Tone::Danger => (
            anim::lerp_color(
                palette.surface_high,
                anim::lerp_color(palette.surface_high, palette.danger, 0.25),
                hover.max(press),
            ),
            anim::lerp_color(anim::fade(palette.danger, 0.5), palette.danger, hover),
            palette.danger,
            palette.danger,
        ),
    }
}

// ---------------------------------------------------------------------------
// Interrupteur
// ---------------------------------------------------------------------------

/// Interrupteur a la place d'une case a cocher: le bouton glisse d'un bord a
/// l'autre, et depasse un peu sa place dans l'allure « Vivant ».
pub struct Toggle<'a> {
    on: &'a mut bool,
    label: String,
}

impl<'a> Toggle<'a> {
    pub fn new(on: &'a mut bool, label: impl Into<String>) -> Self {
        Self {
            on,
            label: label.into(),
        }
    }
}

impl Widget for Toggle<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self { on, label } = self;
        let palette = theme::palette(ui.ctx());
        let look = theme::look(ui.ctx());

        let track = Vec2::new(32.0, 18.0);
        let galley = ui.painter().layout_no_wrap(
            label.clone(),
            TextStyle::Body.resolve(ui.style()),
            Color32::PLACEHOLDER,
        );
        let gap = if label.is_empty() { 0.0 } else { 8.0 };
        let size = Vec2::new(
            track.x + gap + galley.size().x,
            track.y.max(galley.size().y),
        );
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        if response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, &label)
        });

        if !ui.is_rect_visible(rect) {
            return response;
        }
        let hover = anim::hover(ui, response.id.with("survol"), response.hovered());
        let curve = |t: f32| look.arrive(t);
        let travel = anim::glide(
            ui.ctx(),
            response.id.with("bouton"),
            if *on { 1.0 } else { 0.0 },
            look.duration(0.18),
            curve,
        );
        let lit = travel.clamp(0.0, 1.0);

        let track_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.center().y - track.y / 2.0),
            track,
        );
        let radius = look.pill(track.y);
        let painter = ui.painter();
        if look.glow && lit > 0.0 {
            painter.add(glow(
                track_rect,
                radius.nw,
                palette.accent,
                lit * (0.5 + 0.5 * hover),
            ));
        }
        painter.rect_filled(
            track_rect,
            radius,
            anim::lerp_color(palette.bg_deep, palette.accent, lit),
        );
        outline(
            painter,
            track_rect,
            radius.nw,
            anim::lerp_color(
                anim::lerp_color(palette.border, palette.accent_soft, hover),
                palette.accent,
                lit,
            ),
            &look,
        );

        let knob = track.y - 6.0 + hover;
        let left = track_rect.left() + 3.0 + knob / 2.0;
        let right = track_rect.right() - 3.0 - knob / 2.0;
        let center = Pos2::new(
            anim::lerp_unclamped(left, right, travel),
            track_rect.center().y,
        );
        let knob_color = anim::lerp_color(palette.text_dim, Color32::WHITE, lit.max(hover));
        if look.pixel() {
            let square = Rect::from_center_size(center, Vec2::splat(knob.round()));
            painter.rect_filled(square, CornerRadius::ZERO, knob_color);
        } else {
            painter.circle_filled(center, knob / 2.0, knob_color);
        }

        if !label.is_empty() {
            painter.galley(
                Pos2::new(
                    track_rect.right() + gap,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                if ui.is_enabled() {
                    palette.text
                } else {
                    palette.text_dim
                },
            );
        }
        response
    }
}

// ---------------------------------------------------------------------------
// Puce et selecteur segmente
// ---------------------------------------------------------------------------

/// Puce selectionnable: une pastille qui se remplit quand on la choisit.
pub fn chip(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let palette = theme::palette(ui.ctx());
    let look = theme::look(ui.ctx());
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        TextStyle::Body.resolve(ui.style()),
        Color32::PLACEHOLDER,
    );
    let size = Vec2::new(galley.size().x + 22.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = anim::hover(ui, response.id.with("survol"), response.hovered());
    let on = anim::toggle(
        ui,
        response.id.with("choisie"),
        selected,
        look.duration(anim::HOVER),
    );
    if ui.is_rect_visible(rect) {
        let radius = look.pill(rect.height());
        let painter = ui.painter();
        if look.glow && on > 0.0 {
            painter.add(glow(rect, radius.nw, palette.accent, on * 0.6));
        }
        painter.rect_filled(
            rect,
            radius,
            anim::lerp_color(
                anim::lerp_color(palette.bg_deep, palette.surface_high, hover),
                palette.accent_dim,
                on,
            ),
        );
        outline(
            painter,
            rect,
            radius.nw,
            anim::lerp_color(
                anim::lerp_color(palette.border, palette.accent_soft, hover),
                palette.accent,
                on,
            ),
            &look,
        );
        ripple(ui, &response, rect, radius.nw, palette.accent_soft);
        painter.galley(
            Pos2::new(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0),
            galley,
            anim::lerp_color(palette.text_dim, palette.text, hover.max(on)),
        );
    }
    response
}

/// Selecteur a segments: un indicateur glisse d'une option a l'autre.
///
/// Renvoie la reponse de l'ensemble, marquee `changed` quand le choix change.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    current: &mut T,
    options: &[(T, &str)],
) -> Response {
    let palette = theme::palette(ui.ctx());
    let look = theme::look(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let font = TextStyle::Body.resolve(ui.style());
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap(label.to_string(), font.clone(), Color32::PLACEHOLDER)
        })
        .collect();
    let inset = 3.0;
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 28.0).collect();
    let size = Vec2::new(widths.iter().sum::<f32>() + 2.0 * inset, 30.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::hover());

    let mut segments = Vec::with_capacity(options.len());
    let mut x = rect.left() + inset;
    for width in &widths {
        segments.push(Rect::from_min_size(
            Pos2::new(x, rect.top() + inset),
            Vec2::new(*width, rect.height() - 2.0 * inset),
        ));
        x += width;
    }

    let mut responses = Vec::with_capacity(options.len());
    for (index, segment) in segments.iter().enumerate() {
        let segment_response = ui.interact(*segment, id.with(index), Sense::click());
        if segment_response.clicked() && options[index].0 != *current {
            *current = options[index].0;
            response.mark_changed();
        }
        responses.push(segment_response);
    }

    if !ui.is_rect_visible(rect) {
        return response;
    }
    let selected = options
        .iter()
        .position(|(value, _)| value == current)
        .unwrap_or(0);
    let target = segments[selected];
    let seconds = look.duration(0.2);
    let curve = |t: f32| look.arrive(t);
    let left = anim::glide(ui.ctx(), id.with("gauche"), target.left(), seconds, curve);
    let right = anim::glide(ui.ctx(), id.with("droite"), target.right(), seconds, curve);
    let indicator = Rect::from_min_max(
        Pos2::new(left, target.top()),
        Pos2::new(right.max(left + 4.0), target.bottom()),
    );

    let painter = ui.painter();
    let outer = look.pill(rect.height());
    painter.rect_filled(rect, outer, palette.bg_deep);
    outline(painter, rect, outer.nw, palette.border, &look);
    let inner = look.pill(indicator.height());
    if look.glow {
        painter.add(glow(indicator, inner.nw, palette.accent, 0.5));
    }
    painter.rect_filled(indicator, inner, palette.accent_dim);
    outline(painter, indicator, inner.nw, palette.accent, &look);

    for (index, (segment, galley)) in segments.iter().zip(galleys).enumerate() {
        let hover = anim::hover(ui, id.with(("survol", index)), responses[index].hovered());
        ripple(
            ui,
            &responses[index],
            *segment,
            inner.nw,
            palette.accent_soft,
        );
        let color = if index == selected {
            palette.text
        } else {
            anim::lerp_color(palette.text_dim, palette.text, hover)
        };
        painter.galley(
            Pos2::new(
                segment.center().x - galley.size().x / 2.0,
                segment.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response
}

// ---------------------------------------------------------------------------
// Champ de saisie
// ---------------------------------------------------------------------------

/// Champ de saisie avec halo de focus.
///
/// Le halo doit passer **sous** le champ, qu'egui dessine lui-meme: on reserve
/// sa place dans la liste des formes avant le champ, et on la remplit apres,
/// une fois le focus connu.
pub fn field(ui: &mut Ui, edit: TextEdit<'_>) -> Response {
    let look = theme::look(ui.ctx());
    let edit = if look.pixel() {
        edit
    } else {
        edit.margin(Margin::symmetric(8, 5))
    };
    let slot = ui.painter().add(Shape::Noop);
    let response = ui.add(edit);
    focus_halo(ui, slot, &response);
    response
}

/// Remplit la place reservee `slot` avec le halo de focus de `response`.
pub fn focus_halo(ui: &Ui, slot: egui::layers::ShapeIdx, response: &Response) {
    let look = theme::look(ui.ctx());
    if look.pixel() {
        return;
    }
    let palette = theme::palette(ui.ctx());
    let focus = anim::toggle(
        ui,
        response.id.with("halo"),
        response.has_focus(),
        look.duration(anim::HOVER),
    );
    if focus <= 0.0 {
        return;
    }
    let rect = response.rect;
    let shape = if look.glow {
        glow(rect, look.small, palette.accent, focus * 0.8)
    } else {
        RectShape::stroke(
            rect.expand(2.0 * focus),
            CornerRadius::same(look.small.saturating_add(2)),
            Stroke::new(2.0, anim::fade(palette.accent, 0.45 * focus)),
            StrokeKind::Outside,
        )
        .into()
    };
    ui.painter().set(slot, shape);
}

// ---------------------------------------------------------------------------
// Pastille, chargeur, jauge
// ---------------------------------------------------------------------------

/// Pastille d'etat: un disque, ou le sprite pixel en retro.
pub fn status_dot(ui: &mut Ui, color: Color32, scale: f32, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(pixel::DOT.size(scale), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_dot(ui.painter(), rect, color, scale, &theme::look(ui.ctx()));
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// Dessine une pastille dans `rect` (la place d'un sprite 8x8).
pub fn paint_dot(painter: &Painter, rect: Rect, color: Color32, scale: f32, look: &Look) {
    if look.pixel() {
        pixel::draw(painter, rect.min, &pixel::DOT, scale, color, color);
    } else {
        painter.circle_filled(rect.center(), 3.0 * scale, color);
    }
}

/// Onde radar autour d'une pastille: un anneau qui s'elargit et s'eteint.
///
/// Perpetuelle: a n'appeler que pendant une attente reelle (cf. ADR 0005).
pub fn ping(ui: &Ui, rect: Rect, color: Color32, scale: f32) {
    let look = theme::look(ui.ctx());
    if look.pixel() {
        return;
    }
    let phase = anim::cycle(ui.ctx(), anim::PULSE);
    let eased = anim::ease_out(phase);
    let radius = anim::lerp(3.0 * scale, 7.0 * scale, eased);
    ui.painter().circle_stroke(
        rect.center(),
        radius,
        Stroke::new(1.5, anim::fade(color, 0.8 * (1.0 - eased))),
    );
}

/// Chargeur: points qui sautent l'un apres l'autre, ou le chenillard pixel en
/// retro. Meme encombrement dans les deux cas.
pub fn loader(ui: &mut Ui, scale: f32, color: Color32) -> Response {
    if theme::look(ui.ctx()).pixel() {
        return pixel::loader(ui, scale, color);
    }
    let size = pixel::loader_size(scale);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let phase = anim::cycle(ui.ctx(), 0.9);
        draw_dots(ui.painter(), rect, scale, color, phase);
    }
    response
}

/// Points du chargeur, dans le rectangle du chenillard pixel.
pub fn draw_dots(painter: &Painter, rect: Rect, scale: f32, color: Color32, phase: f32) {
    const DOTS: usize = 3;
    let radius = 2.0 * scale;
    let step = (rect.width() - 2.0 * radius) / (DOTS - 1) as f32;
    for index in 0..DOTS {
        // Chaque point saute a son tour: une demi-sinusoide decalee d'un tiers
        // de cycle, a plat le reste du temps.
        let local = (phase - index as f32 / DOTS as f32 * 0.6).rem_euclid(1.0);
        let hop = if local < 0.5 {
            (local * 2.0 * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        let center = Pos2::new(
            rect.left() + radius + index as f32 * step,
            rect.center().y - hop * 2.5 * scale,
        );
        painter.circle_filled(center, radius, anim::fade(color, 0.4 + 0.6 * hop));
    }
}

/// Jauge: piste arrondie remplie de gauche a droite, ou jauge pixel en retro.
pub fn progress(
    painter: &Painter,
    rect: Rect,
    fraction: f32,
    fill: Color32,
    track: Color32,
    look: &Look,
) {
    if look.pixel() {
        pixel::progress(painter, rect, fraction, fill, track);
        return;
    }
    let radius = look.pill(rect.height());
    painter.rect_filled(rect, radius, anim::fade(track, 0.6));
    let width = rect.width() * fraction.clamp(0.0, 1.0);
    if width <= 0.0 {
        return;
    }
    let filled = Rect::from_min_size(rect.min, Vec2::new(width.max(rect.height()), rect.height()));
    if look.glow {
        painter.add(glow(filled, radius.nw, fill, 0.5));
    }
    painter.rect_filled(filled, radius, fill);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UiStyle;

    fn input(time: f64) -> egui::RawInput {
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
            ..Default::default()
        }
    }

    #[test]
    fn toggle_flips_on_click_and_reports_a_change() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, &theme::DARK, 14.0, UiStyle::Lively);
        let mut on = false;
        let mut changed = false;
        let mut frame = |events: Vec<egui::Event>, time: f64| {
            let mut raw = input(time);
            raw.events = events;
            let mut rect = Rect::NOTHING;
            let _ = ctx.run_ui(raw, |ui| {
                let response = ui.add(Toggle::new(&mut on, "Essai"));
                rect = response.rect;
                changed |= response.changed();
            });
            rect
        };
        let rect = frame(Vec::new(), 0.0);
        let at = rect.left_center() + Vec2::new(8.0, 0.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(at)], 0.1);
        frame(vec![press(true)], 0.2);
        frame(vec![press(false)], 0.25);
        assert!(on, "le clic n'a pas bascule l'interrupteur");
        assert!(changed, "le changement n'est pas signale");
    }

    #[test]
    fn segmented_slides_its_indicator_toward_the_choice() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, &theme::DARK, 14.0, UiStyle::Soft);
        let options = [(1, "Un"), (2, "Deux"), (3, "Trois")];
        // Bord gauche de l'indicateur: le seul rectangle rempli de violet sombre.
        let indicator = |time: f64, value: i32| {
            let mut value = value;
            let output = ctx.run_ui(input(time), |ui| {
                segmented(ui, "essai", &mut value, &options);
            });
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    Shape::Rect(rect) if rect.fill == theme::DARK.accent_dim => {
                        Some(rect.rect.left())
                    }
                    _ => None,
                })
                .expect("indicateur")
        };
        let first = indicator(0.0, 1);
        // Le choix passe au troisieme: l'indicateur part, sans sauter.
        let start = indicator(0.1, 3);
        let middle = indicator(0.2, 3);
        let end = indicator(2.0, 3);
        assert_eq!(first, start, "l'indicateur a saute");
        assert!(middle > start && middle < end, "{start} {middle} {end}");
    }

    #[test]
    fn dots_stay_near_their_rectangle() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), pixel::loader_size(2.0));
        for step in 0..10 {
            let output = ctx.run_ui(input(step as f64 * 0.1), |ui| {
                draw_dots(ui.painter(), rect, 2.0, Color32::WHITE, step as f32 / 10.0);
            });
            for clipped in output.shapes {
                if let Shape::Circle(circle) = clipped.shape {
                    assert!(rect.expand(8.0).contains(circle.center), "{circle:?}");
                }
            }
        }
    }
}
