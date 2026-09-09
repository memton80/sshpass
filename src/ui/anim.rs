//! Helpers d'animation.
//!
//! Tout passe par les animateurs natifs d'egui (`Context::animate_bool_with_time`
//! et `animate_value_with_time`), qui interpolent une valeur par identifiant
//! entre deux frames et demandent eux-memes le rafraichissement tant que
//! l'animation court. Aucune dependance externe, aucun timer a gerer.

use egui::{Color32, Id, Ui};

/// Duree d'une reaction au survol. Assez court pour rester nerveux, assez long
/// pour qu'on percoive le mouvement.
pub const HOVER: f32 = 0.12;

/// Duree d'une transition de vue (changement d'onglet, ouverture de panneau).
pub const VIEW: f32 = 0.16;

/// Avancement 0→1 de l'animation de survol d'un element.
pub fn hover(ui: &Ui, id: Id, hovered: bool) -> f32 {
    ui.ctx().animate_bool_with_time(id, hovered, HOVER)
}

/// Avancement 0→1 d'un etat binaire quelconque, sur une duree donnee.
pub fn toggle(ui: &Ui, id: Id, active: bool, seconds: f32) -> f32 {
    ui.ctx().animate_bool_with_time(id, active, seconds)
}

/// Interpolation de couleur en espace gamma (celui de l'affichage).
pub fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    from.lerp_to_gamma(to, t.clamp(0.0, 1.0))
}

/// Interpolation lineaire d'un scalaire.
pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t.clamp(0.0, 1.0)
}

/// Couleur attenuee par un facteur d'opacite, pour les fondus croises.
pub fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

/// Relance une transition quand la vue affichee change.
///
/// Renvoie l'avancement 0→1 de la transition en cours. La cle identifie la vue:
/// des qu'elle change, l'animation repart de zero.
pub fn view_transition(ui: &Ui, id: Id, key: u64, seconds: f32) -> f32 {
    let ctx = ui.ctx();
    let previous: Option<u64> = ctx.data(|d| d.get_temp(id));
    if previous != Some(key) {
        ctx.data_mut(|d| d.insert_temp(id, key));
        // Duree nulle: force la valeur a 0 immediatement, sans interpoler
        // depuis l'etat de la vue precedente.
        ctx.animate_value_with_time(id.with("progress"), 0.0, 0.0);
    }
    ctx.animate_value_with_time(id.with("progress"), 1.0, seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_reaches_both_ends() {
        assert_eq!(lerp(10.0, 20.0, 0.0), 10.0);
        assert_eq!(lerp(10.0, 20.0, 1.0), 20.0);
        assert_eq!(lerp(10.0, 20.0, 0.5), 15.0);
    }

    #[test]
    fn lerp_clamps_out_of_range() {
        assert_eq!(lerp(0.0, 10.0, -5.0), 0.0);
        assert_eq!(lerp(0.0, 10.0, 5.0), 10.0);
    }

    #[test]
    fn color_lerp_reaches_both_ends() {
        let from = Color32::from_rgb(0, 0, 0);
        let to = Color32::from_rgb(255, 255, 255);
        assert_eq!(lerp_color(from, to, 0.0), from);
        assert_eq!(lerp_color(from, to, 1.0), to);
        let middle = lerp_color(from, to, 0.5);
        assert!(middle.r() > 0 && middle.r() < 255);
    }

    #[test]
    fn fade_scales_alpha() {
        let color = Color32::from_rgb(200, 100, 50);
        assert_eq!(fade(color, 0.0).a(), 0);
        assert_eq!(fade(color, 1.0), color);
    }
}
