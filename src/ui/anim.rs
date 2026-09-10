//! Helpers d'animation.
//!
//! Tout passe par les animateurs natifs d'egui (`Context::animate_bool_with_time`
//! et `animate_value_with_time`), qui interpolent une valeur par identifiant
//! entre deux frames et demandent eux-memes le rafraichissement tant que
//! l'animation court. Aucune dependance externe, aucun timer a gerer.
//!
//! Deux familles cohabitent:
//!
//! * les animations **d'etat**, pilotees par egui (survol, pliage, apparition);
//! * les animations **perpetuelles** (chargement, battement), calculees a
//!   partir de l'horloge de la frame — elles n'ont pas d'etat a memoriser mais
//!   doivent reclamer elles-memes le rafraichissement suivant.

use egui::{Color32, Context, Id, Ui};

/// Duree d'une reaction au survol. Assez court pour rester nerveux, assez long
/// pour qu'on percoive le mouvement.
pub const HOVER: f32 = 0.12;

/// Duree d'une transition de vue (changement d'onglet, ouverture de panneau).
pub const VIEW: f32 = 0.16;

/// Duree d'ouverture — et de fermeture — d'une fenetre modale.
pub const MODAL: f32 = 0.14;

/// Periode d'un battement: pastille d'etat qui respire pendant une attente.
pub const PULSE: f32 = 1.2;

/// Periode d'un aller-retour de barre de chargement indeterminee.
pub const SWEEP: f32 = 1.5;

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

/// Depart vif puis freinage (cubique). C'est la courbe des apparitions: on voit
/// le mouvement tout de suite, il se pose sans rebond.
pub fn ease_out(t: f32) -> f32 {
    let t = 1.0 - t.clamp(0.0, 1.0);
    1.0 - t * t * t
}

/// Demarrage et arrivee amortis, pleine vitesse au milieu. C'est la courbe des
/// va-et-vient: sans elle, la barre de chargement rebondirait sur ses bords.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Aller-retour: 0 aux extremites, 1 au milieu.
pub fn ping_pong(t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 0.5 {
        t * 2.0
    } else {
        (1.0 - t) * 2.0
    }
}

/// Position 0→1 dans un cycle, pour un instant et une periode donnes.
///
/// Fonction pure: c'est elle qui est testee, `cycle` n'y ajoute que l'horloge
/// et la demande de rafraichissement.
pub fn phase(time: f64, period: f32) -> f32 {
    if period <= 0.0 {
        return 0.0;
    }
    (time.rem_euclid(period as f64) / period as f64) as f32
}

/// Position 0→1 dans un cycle perpetuel, cale sur l'horloge de la frame.
///
/// Reclame le rafraichissement suivant: une animation qui ne depend d'aucune
/// interaction ne repartirait pas toute seule, egui laissant dormir une fenetre
/// inerte.
pub fn cycle(ctx: &Context, period: f32) -> f32 {
    ctx.request_repaint();
    phase(ctx.input(|i| i.time), period)
}

/// Battement doux 0→1→0 (sinusoide), pour ce qui doit respirer sans clignoter.
pub fn breathe(ctx: &Context, period: f32) -> f32 {
    let phase = cycle(ctx, period);
    0.5 - 0.5 * (std::f32::consts::TAU * phase).cos()
}

/// Va-et-vient amorti d'une barre de chargement indeterminee: 0 a gauche, 1 a
/// droite, sans a-coup aux extremites.
pub fn sweep(phase: f32) -> f32 {
    ease_in_out(ping_pong(phase))
}

/// Intensite d'une case de chenillard: pleine sur la tete, decroissante sur la
/// traine, jamais tout a fait eteinte pour que la forme reste lisible.
pub fn chase(phase: f32, index: usize, count: usize) -> f32 {
    let count = count.max(1) as f32;
    let head = phase.rem_euclid(1.0) * count;
    let distance = (head - index as f32).rem_euclid(count);
    (1.0 - distance / 2.0).clamp(0.15, 1.0)
}

/// Avancement d'un element d'une cascade: chaque rang demarre avec un retard
/// proportionnel, et tous arrivent avant la fin de l'animation.
pub fn stagger(progress: f32, index: usize, delay: f32) -> f32 {
    let start = (index as f32 * delay).clamp(0.0, 0.8);
    ((progress.clamp(0.0, 1.0) - start) / (1.0 - start)).clamp(0.0, 1.0)
}

/// Avancement 0→1 depuis l'apparition d'un element.
///
/// `animate_value_with_time` renvoie la valeur cible des le premier appel: sans
/// precaution, un element qui apparait serait deja a 1 et n'aurait aucune
/// animation d'entree. On detecte donc l'apparition — l'identifiant n'a pas ete
/// dessine a la passe precedente — pour remettre la valeur a zero d'abord.
pub fn appear(ctx: &Context, id: Id, seconds: f32) -> f32 {
    let pass = ctx.cumulative_pass_nr();
    let seen = id.with("passe");
    let previous: Option<u64> = ctx.data(|d| d.get_temp(seen));
    ctx.data_mut(|d| d.insert_temp(seen, pass));
    if previous.is_none_or(|last| last + 1 < pass) {
        // Duree nulle: la valeur tombe a zero immediatement, sans interpoler
        // depuis ce qu'elle valait la derniere fois que l'element existait.
        ctx.animate_value_with_time(id, 0.0, 0.0);
    }
    ctx.animate_value_with_time(id, 1.0, seconds)
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

    /// Fait tourner un contexte egui de test, une frame par instant donne.
    fn frames(times: &[f64], mut frame: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        for time in times {
            let input = egui::RawInput {
                time: Some(*time),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| frame(ui));
        }
    }

    #[test]
    fn appear_starts_from_zero_and_climbs() {
        let mut seen = Vec::new();
        frames(&[0.0, 0.05, 0.10, 0.30], |ui| {
            seen.push(appear(ui.ctx(), Id::new("essai"), 0.16));
        });
        // Sans la remise a zero, egui renverrait 1 des la premiere frame et il
        // n'y aurait aucune animation d'entree a voir.
        assert_eq!(seen[0], 0.0);
        assert!(seen[1] > 0.0 && seen[1] < 1.0, "{seen:?}");
        assert!(seen[2] > seen[1], "{seen:?}");
        assert_eq!(*seen.last().expect("frames"), 1.0);
    }

    #[test]
    fn appear_restarts_after_a_disparition() {
        let mut seen = Vec::new();
        // L'element est dessine, disparait une frame, puis revient.
        frames(&[0.0, 0.5, 1.0, 1.5], |ui| {
            let drawn = ui.ctx().input(|i| i.time) != 1.0;
            if drawn {
                seen.push(appear(ui.ctx(), Id::new("essai"), 0.16));
            }
        });
        assert_eq!(seen[0], 0.0);
        assert_eq!(seen[1], 1.0);
        // Retour apres absence: l'animation repart de zero.
        assert_eq!(seen[2], 0.0);
    }

    #[test]
    fn easings_stay_in_range_and_reach_both_ends() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        // Un freinage passe au-dessus de la diagonale, un amorti la croise au
        // milieu: c'est ce qui distingue les deux courbes.
        assert!(ease_out(0.25) > 0.25);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-6);
        for step in 0..=20 {
            let t = step as f32 / 20.0;
            assert!((0.0..=1.0).contains(&ease_out(t)));
            assert!((0.0..=1.0).contains(&ease_in_out(t)));
        }
    }

    #[test]
    fn ping_pong_goes_back_and_forth() {
        assert_eq!(ping_pong(0.0), 0.0);
        assert_eq!(ping_pong(0.5), 1.0);
        assert!(ping_pong(1.0) < 1e-6);
        // Le cycle se repete a l'identique.
        assert!((ping_pong(0.25) - ping_pong(1.25)).abs() < 1e-6);
    }

    #[test]
    fn phase_wraps_around_the_period() {
        assert_eq!(phase(0.0, 2.0), 0.0);
        assert_eq!(phase(1.0, 2.0), 0.5);
        assert_eq!(phase(2.0, 2.0), 0.0);
        assert_eq!(phase(5.0, 2.0), 0.5);
        // Une periode absurde ne doit pas diviser par zero.
        assert_eq!(phase(3.0, 0.0), 0.0);
    }

    #[test]
    fn sweep_touches_both_edges() {
        assert!(sweep(0.0) < 1e-6);
        assert!((sweep(0.5) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn chase_lights_the_head_and_dims_the_tail() {
        let count = 4;
        // Tete du chenillard sur la premiere case.
        assert!((chase(0.0, 0, count) - 1.0).abs() < 1e-6);
        // Les cases derriere sont de moins en moins vives, sans s'eteindre.
        assert!(chase(0.0, 3, count) > chase(0.0, 2, count));
        assert!(chase(0.0, 2, count) >= 0.15);
        // Un compte nul ne doit pas faire diviser par zero.
        assert!(chase(0.3, 0, 0) > 0.0);
    }

    #[test]
    fn stagger_delays_each_rank() {
        // Le premier suit l'animation, les suivants demarrent plus tard.
        assert_eq!(stagger(0.5, 0, 0.1), 0.5);
        assert!(stagger(0.5, 1, 0.1) < 0.5);
        assert_eq!(stagger(0.05, 2, 0.1), 0.0);
        // Tout le monde est arrive a la fin, meme le dernier rang.
        assert_eq!(stagger(1.0, 50, 0.1), 1.0);
    }

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
