//! Primitives de dessin pixel art.
//!
//! Les icones sont decrites par des grilles de caracteres et rasterisees a
//! l'echelle demandee. C'est volontaire: une grille dessinee reste nette a
//! n'importe quel facteur entier, ne demande aucun fichier d'asset et evite
//! d'embarquer une police bitmap juste pour des pictogrammes. Le texte, lui,
//! reste en police systeme (cf. `theme::install_system_fonts`).
//!
//! Convention de la grille:
//! * `.` ou espace — transparent
//! * `#` — couleur principale
//! * `o` — couleur secondaire

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};

/// Une icone pixel art.
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    pub rows: &'static [&'static str],
}

impl Sprite {
    pub const fn new(rows: &'static [&'static str]) -> Self {
        Self { rows }
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn width(&self) -> usize {
        let mut width = 0;
        let mut index = 0;
        while index < self.rows.len() {
            let len = self.rows[index].len();
            if len > width {
                width = len;
            }
            index += 1;
        }
        width
    }

    /// Taille en points pour un facteur d'echelle donne.
    pub fn size(&self, scale: f32) -> Vec2 {
        Vec2::new(self.width() as f32 * scale, self.height() as f32 * scale)
    }
}

pub const SERVER: Sprite = Sprite::new(&[
    ".######.", ".#....#.", ".#.oo.#.", ".#....#.", ".######.", ".#....#.", ".#.oo.#.", ".######.",
]);

pub const FOLDER: Sprite = Sprite::new(&[
    "..##....", ".####...", "########", "#......#", "#......#", "#......#", "#......#", "########",
]);

pub const STAR: Sprite = Sprite::new(&[
    "....#...", "...###..", ".#######", "..#####.", "...###..", "..##.##.", ".##...##", "........",
]);

pub const KEY: Sprite = Sprite::new(&[
    ".####...", "#....#..", "#.oo.#..", "#....#..", ".####...", "..#####.", "..#.#...", "..#.##..",
]);

pub const PLUG: Sprite = Sprite::new(&[
    "..#..#..", "..#..#..", ".######.", ".#....#.", ".######.", "...##...", "...##...", "..oooo..",
]);

pub const SEARCH: Sprite = Sprite::new(&[
    ".####...", "#....#..", "#....#..", "#....#..", ".####...", "...#.#..", "....###.", ".....##.",
]);

pub const PLUS: Sprite = Sprite::new(&[
    "........", "...##...", "...##...", ".######.", ".######.", "...##...", "...##...", "........",
]);

pub const PENCIL: Sprite = Sprite::new(&[
    ".....##.", "....###.", "...###..", "..###...", ".###....", "###.....", "##o.....", "#.......",
]);

pub const TRASH: Sprite = Sprite::new(&[
    "..####..", ".######.", "########", ".#....#.", ".#.oo.#.", ".#.oo.#.", ".#....#.", ".######.",
]);

pub const TERMINAL: Sprite = Sprite::new(&[
    "########", "#......#", "#.##...#", "#...#..#", "#.##...#", "#......#", "#.oooo.#", "########",
]);

pub const CHEVRON_RIGHT: Sprite = Sprite::new(&[
    "..#.....", "..##....", "..###...", "..####..", "..###...", "..##....", "..#.....", "........",
]);

pub const CHEVRON_DOWN: Sprite = Sprite::new(&[
    "........", "########", ".######.", ".######.", "..####..", "...##...", "........", "........",
]);

pub const CLOSE: Sprite = Sprite::new(&[
    "##....##", "###..###", ".######.", "..####..", "..####..", ".######.", "###..###", "##....##",
]);

pub const DOT: Sprite = Sprite::new(&[
    "........", "..####..", ".######.", ".######.", ".######.", ".######.", "..####..", "........",
]);

/// Dessine un sprite. Les pixels contigus d'une meme ligne sont fusionnes en
/// un seul rectangle.
pub fn draw(
    painter: &Painter,
    top_left: Pos2,
    sprite: &Sprite,
    scale: f32,
    primary: Color32,
    secondary: Color32,
) {
    // L'origine est arrondie au pixel: un demi-pixel de decalage suffit a
    // rendre floue une icone pixel art.
    let origin = Pos2::new(top_left.x.round(), top_left.y.round());

    for (y, row) in sprite.rows.iter().enumerate() {
        let mut run: Option<(usize, usize, Color32)> = None;
        for (x, symbol) in row.chars().enumerate() {
            let color = match symbol {
                '#' => Some(primary),
                'o' => Some(secondary),
                _ => None,
            };
            match (run.as_mut(), color) {
                (Some(current), Some(color)) if current.2 == color && current.1 + 1 == x => {
                    current.1 = x;
                }
                (_, Some(color)) => {
                    flush_run(painter, origin, scale, y, run.take());
                    run = Some((x, x, color));
                }
                (_, None) => flush_run(painter, origin, scale, y, run.take()),
            }
        }
        flush_run(painter, origin, scale, y, run.take());
    }
}

fn flush_run(
    painter: &Painter,
    origin: Pos2,
    scale: f32,
    y: usize,
    run: Option<(usize, usize, Color32)>,
) {
    let Some((from, to, color)) = run else { return };
    let rect = Rect::from_min_max(
        Pos2::new(origin.x + from as f32 * scale, origin.y + y as f32 * scale),
        Pos2::new(
            origin.x + (to + 1) as f32 * scale,
            origin.y + (y + 1) as f32 * scale,
        ),
    );
    painter.rect_filled(rect, CornerRadius::ZERO, color);
}

/// Reserve la place d'une icone et la dessine, sans interaction.
pub fn icon(ui: &mut Ui, sprite: &Sprite, scale: f32, color: Color32) -> Response {
    icon_two_tone(ui, sprite, scale, color, color)
}

pub fn icon_two_tone(
    ui: &mut Ui,
    sprite: &Sprite,
    scale: f32,
    primary: Color32,
    secondary: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(sprite.size(scale), Sense::hover());
    if ui.is_rect_visible(rect) {
        draw(ui.painter(), rect.min, sprite, scale, primary, secondary);
    }
    response
}

/// Cadre pixel art: un liseré d'un pixel avec les quatre coins evides, motif
/// caracteristique des interfaces 8 bits.
pub fn frame(painter: &Painter, rect: Rect, color: Color32, scale: f32) {
    let rect = Rect::from_min_max(
        Pos2::new(rect.left().round(), rect.top().round()),
        Pos2::new(rect.right().round(), rect.bottom().round()),
    );
    let bar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1)),
            CornerRadius::ZERO,
            color,
        );
    };
    // Horizontales et verticales raccourcies de `scale` a chaque extremite:
    // c'est ce retrait qui evide les coins.
    bar(
        rect.left() + scale,
        rect.top(),
        rect.right() - scale,
        rect.top() + scale,
    );
    bar(
        rect.left() + scale,
        rect.bottom() - scale,
        rect.right() - scale,
        rect.bottom(),
    );
    bar(
        rect.left(),
        rect.top() + scale,
        rect.left() + scale,
        rect.bottom() - scale,
    );
    bar(
        rect.right() - scale,
        rect.top() + scale,
        rect.right(),
        rect.bottom() - scale,
    );
}

/// Pastille d'etat (agent actif, connexion ouverte...).
pub fn status_dot(ui: &mut Ui, color: Color32, scale: f32, tooltip: &str) -> Response {
    let response = icon(ui, &DOT, scale, color);
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [(&str, Sprite); 13] = [
        ("SERVER", SERVER),
        ("FOLDER", FOLDER),
        ("STAR", STAR),
        ("KEY", KEY),
        ("PLUG", PLUG),
        ("SEARCH", SEARCH),
        ("PLUS", PLUS),
        ("PENCIL", PENCIL),
        ("TRASH", TRASH),
        ("TERMINAL", TERMINAL),
        ("CHEVRON_RIGHT", CHEVRON_RIGHT),
        ("CHEVRON_DOWN", CHEVRON_DOWN),
        ("CLOSE", CLOSE),
    ];

    #[test]
    fn sprites_are_square_grids() {
        for (name, sprite) in ALL {
            assert_eq!(sprite.height(), 8, "{name} n'a pas 8 lignes");
            for (index, row) in sprite.rows.iter().enumerate() {
                assert_eq!(row.len(), 8, "{name} ligne {index} n'a pas 8 colonnes");
            }
        }
    }

    #[test]
    fn sprites_only_use_known_symbols() {
        for (name, sprite) in ALL {
            for row in sprite.rows {
                for symbol in row.chars() {
                    assert!(
                        matches!(symbol, '.' | ' ' | '#' | 'o'),
                        "{name} contient le symbole inattendu {symbol:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn sprites_are_not_blank() {
        for (name, sprite) in ALL {
            let filled = sprite
                .rows
                .iter()
                .flat_map(|r| r.chars())
                .filter(|c| *c == '#')
                .count();
            assert!(filled > 4, "{name} est presque vide");
        }
    }

    #[test]
    fn size_scales_linearly() {
        assert_eq!(SERVER.size(1.0), Vec2::new(8.0, 8.0));
        assert_eq!(SERVER.size(2.0), Vec2::new(16.0, 16.0));
        assert_eq!(SERVER.size(3.0), Vec2::new(24.0, 24.0));
    }
}
