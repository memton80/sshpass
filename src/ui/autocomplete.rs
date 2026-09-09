//! Champ de saisie avec suggestions.
//!
//! Il n'existe pas de crate d'autocompletion mure pour egui: le composant est
//! fait maison, mais il ne repose que sur des briques natives — un `TextEdit`
//! d'identifiant fixe, un `Popup` ancre sous lui, et la consommation des
//! touches avant que le champ ne les voie.
//!
//! Les suggestions viennent de l'historique des connexions deja enregistrees
//! et des elements Proton Pass deja charges; aucune requete n'est declenchee
//! par la frappe.

use egui::{Align2, CornerRadius, FontId, Key, Modifiers, Popup, RectAlign, Sense, Vec2};

use crate::theme::Palette;
use crate::ui::pixel::{self, Sprite};

/// Nombre maximal de propositions affichees.
pub const MAX_SUGGESTIONS: usize = 8;

/// Hauteur d'une ligne de proposition.
const ROW_HEIGHT: f32 = 26.0;

/// Une proposition: la valeur inseree, et une precision affichee a droite
/// (le nom d'un dossier, le type d'un item...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub value: String,
    pub detail: String,
}

impl Suggestion {
    pub fn new(value: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            detail: detail.into(),
        }
    }

    pub fn plain(value: impl Into<String>) -> Self {
        Self::new(value, String::new())
    }
}

/// Etat conserve entre deux frames, dans la memoire d'egui.
#[derive(Clone, Default)]
struct State {
    /// La liste etait ouverte a la frame precedente.
    open: bool,
    selected: usize,
    /// Nombre de propositions de la frame precedente, pour le bouclage.
    count: usize,
    /// L'utilisateur a ferme la liste avec Echap.
    dismissed: bool,
}

/// Retient les propositions qui correspondent a la saisie.
///
/// Une saisie vide propose tout l'historique: c'est le cas le plus utile, on
/// vient de cliquer dans un champ vide. Une valeur deja saisie a l'identique
/// n'est pas reproposee — il n'y aurait rien a completer.
pub fn filter(candidates: &[Suggestion], needle: &str, limit: usize) -> Vec<Suggestion> {
    let needle = needle.trim().to_lowercase();
    let mut seen: Vec<String> = Vec::new();
    let mut result = Vec::new();
    for candidate in candidates {
        let value = candidate.value.trim();
        if value.is_empty() {
            continue;
        }
        let lower = value.to_lowercase();
        if lower == needle {
            continue;
        }
        if !needle.is_empty() && !lower.contains(&needle) {
            continue;
        }
        if seen.contains(&lower) {
            continue;
        }
        seen.push(lower);
        result.push(candidate.clone());
        if result.len() >= limit {
            break;
        }
    }
    result
}

/// Index suivant, avec bouclage. `len` a zero renvoie zero.
pub fn next_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (current + 1) % len
    }
}

/// Index precedent, avec bouclage.
pub fn previous_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (current + len - 1) % len
    }
}

/// Champ de saisie avec liste de suggestions.
pub struct Autocomplete<'a> {
    salt: &'a str,
    hint: &'a str,
    width: f32,
    icon: &'a Sprite,
    candidates: &'a [Suggestion],
}

impl<'a> Autocomplete<'a> {
    pub fn new(salt: &'a str, candidates: &'a [Suggestion]) -> Self {
        Self {
            salt,
            hint: "",
            width: f32::INFINITY,
            icon: &pixel::CHEVRON_RIGHT,
            candidates,
        }
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn icon(mut self, icon: &'a Sprite) -> Self {
        self.icon = icon;
        self
    }

    pub fn show(
        self,
        ui: &mut egui::Ui,
        text: &mut String,
        palette: &Palette,
        scale: f32,
    ) -> egui::Response {
        let id = ui.make_persistent_id(self.salt);
        let mut state: State = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let focused = ui.memory(|m| m.has_focus(id));

        // Les touches sont consommees AVANT le champ: sinon le `TextEdit`
        // interprete les fleches comme un deplacement du curseur et Entree
        // comme une validation du formulaire.
        let mut accept: Option<usize> = None;
        if state.open && focused && state.count > 0 {
            ui.input_mut(|input| {
                if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    state.selected = next_index(state.selected, state.count);
                }
                if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    state.selected = previous_index(state.selected, state.count);
                }
                if input.consume_key(Modifiers::NONE, Key::Enter)
                    || input.consume_key(Modifiers::NONE, Key::Tab)
                {
                    accept = Some(state.selected);
                }
                if input.consume_key(Modifiers::NONE, Key::Escape) {
                    state.dismissed = true;
                }
            });
        }

        let mut response = ui.add(
            egui::TextEdit::singleline(text)
                .id(id)
                .hint_text(self.hint)
                .desired_width(self.width),
        );
        if response.changed() {
            // Une nouvelle saisie relance la proposition et repart du haut.
            state.dismissed = false;
            state.selected = 0;
        }
        if !focused {
            state.dismissed = false;
        }

        let matches = filter(self.candidates, text, MAX_SUGGESTIONS);
        state.selected = state.selected.min(matches.len().saturating_sub(1));
        let open = focused && !state.dismissed && !matches.is_empty();

        if let Some(index) = accept {
            if let Some(choice) = matches.get(index.min(matches.len().saturating_sub(1))) {
                *text = choice.value.clone();
                state.dismissed = true;
                response.mark_changed();
            }
        }

        if open {
            let chosen = self.popup(&response, &matches, state.selected, palette, scale);
            if let Some(value) = chosen {
                *text = value;
                state.dismissed = true;
                response.mark_changed();
                // Le clic dans la liste a retire le focus du champ: on le rend
                // pour que la saisie puisse continuer sans re-cliquer.
                response.request_focus();
            }
        }

        state.open = open;
        state.count = matches.len();
        ui.ctx().data_mut(|d| d.insert_temp(id, state));
        response
    }

    /// Affiche la liste et renvoie la valeur choisie a la souris.
    fn popup(
        &self,
        anchor: &egui::Response,
        matches: &[Suggestion],
        selected: usize,
        palette: &Palette,
        scale: f32,
    ) -> Option<String> {
        let width = anchor.rect.width().max(160.0);
        Popup::from_response(anchor)
            .id(anchor.id.with("suggestions"))
            .open(true)
            .align(RectAlign::BOTTOM_START)
            .gap(2.0)
            .width(width)
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_deep)
                    .stroke(egui::Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::same(2)),
            )
            .show(|ui| {
                let mut chosen = None;
                for (index, suggestion) in matches.iter().enumerate() {
                    if self.row(ui, suggestion, index == selected, palette, scale) {
                        chosen = Some(suggestion.value.clone());
                    }
                }
                chosen
            })
            .and_then(|inner| inner.inner)
    }

    /// Dessine une proposition. Renvoie `true` si elle a ete choisie.
    fn row(
        &self,
        ui: &mut egui::Ui,
        suggestion: &Suggestion,
        selected: bool,
        palette: &Palette,
        scale: f32,
    ) -> bool {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
        let highlight =
            crate::ui::anim::hover(ui, response.id.with("hl"), selected || response.hovered());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let background =
                crate::ui::anim::lerp_color(palette.bg_deep, palette.accent_dim, highlight);
            painter.rect_filled(rect, CornerRadius::ZERO, background);
            let icon_color =
                crate::ui::anim::lerp_color(palette.text_dim, palette.accent_soft, highlight);
            pixel::draw(
                painter,
                egui::pos2(rect.left() + 6.0, rect.center().y - 4.0 * scale),
                self.icon,
                scale,
                icon_color,
                icon_color,
            );
            painter.text(
                egui::pos2(rect.left() + 12.0 + 8.0 * scale, rect.center().y),
                Align2::LEFT_CENTER,
                &suggestion.value,
                FontId::proportional(12.0),
                palette.text,
            );
            if !suggestion.detail.is_empty() {
                painter.text(
                    egui::pos2(rect.right() - 6.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &suggestion.detail,
                    FontId::proportional(10.0),
                    palette.text_dim,
                );
            }
        }
        response.clicked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<Suggestion> {
        vec![
            Suggestion::new("web-01.example.com", "prod"),
            Suggestion::new("web-02.example.com", "prod"),
            Suggestion::new("db.example.com", "prod"),
            Suggestion::plain("10.0.0.4"),
        ]
    }

    #[test]
    fn empty_input_proposes_everything() {
        let result = filter(&candidates(), "", MAX_SUGGESTIONS);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn filter_is_case_insensitive_and_substring() {
        let result = filter(&candidates(), "WEB", MAX_SUGGESTIONS);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].value, "web-01.example.com");

        let result = filter(&candidates(), "example", MAX_SUGGESTIONS);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn exact_match_is_not_proposed() {
        // Rien a completer: la valeur est deja celle du champ.
        let result = filter(&candidates(), "db.example.com", MAX_SUGGESTIONS);
        assert!(result.iter().all(|s| s.value != "db.example.com"));
    }

    #[test]
    fn exact_match_ignores_case_and_spaces() {
        let result = filter(&candidates(), "  DB.Example.COM  ", MAX_SUGGESTIONS);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicates_are_collapsed() {
        let doubled = vec![
            Suggestion::plain("root"),
            Suggestion::plain("root"),
            Suggestion::plain("ROOT"),
            Suggestion::plain("alex"),
        ];
        let result = filter(&doubled, "", MAX_SUGGESTIONS);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn blank_candidates_are_skipped() {
        let list = vec![Suggestion::plain("  "), Suggestion::plain("ok")];
        let result = filter(&list, "", MAX_SUGGESTIONS);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].value, "ok");
    }

    #[test]
    fn limit_is_respected() {
        let many: Vec<Suggestion> = (0..50)
            .map(|i| Suggestion::plain(format!("hote-{i}")))
            .collect();
        assert_eq!(filter(&many, "", MAX_SUGGESTIONS).len(), MAX_SUGGESTIONS);
        assert_eq!(filter(&many, "", 3).len(), 3);
    }

    #[test]
    fn no_match_gives_empty_list() {
        assert!(filter(&candidates(), "mysql", MAX_SUGGESTIONS).is_empty());
    }

    #[test]
    fn index_wraps_in_both_directions() {
        assert_eq!(next_index(0, 3), 1);
        assert_eq!(next_index(2, 3), 0);
        assert_eq!(previous_index(0, 3), 2);
        assert_eq!(previous_index(2, 3), 1);
    }

    #[test]
    fn index_helpers_tolerate_empty_list() {
        assert_eq!(next_index(0, 0), 0);
        assert_eq!(previous_index(0, 0), 0);
    }
}
