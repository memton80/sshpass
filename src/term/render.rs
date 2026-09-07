//! Widget egui de rendu du terminal.
//!
//! Le rendu est immediat: a chaque frame on parcourt les cellules visibles et
//! on les regroupe en segments de meme style, ce qui ramene une grille 80x24 a
//! quelques dizaines de primitives au lieu de deux mille.

use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{SelectionRange, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::term::{keys, TermSize, TerminalSession};
use crate::theme::MONO_BOLD;

/// Ce que le widget demande a l'application apres coup.
#[derive(Debug, Default)]
pub struct TerminalOutput {
    /// Texte a placer dans le presse-papiers.
    pub copy: Option<String>,
    /// L'utilisateur a demande la fermeture de l'onglet (Ctrl+Shift+W).
    pub close_requested: bool,
}

/// Taille d'une cellule pour une taille de police donnee.
pub fn cell_size(ctx: &egui::Context, font_size: f32) -> Vec2 {
    let font = FontId::new(font_size, FontFamily::Monospace);
    ctx.fonts_mut(|fonts| {
        // 'M' est la reference habituelle: en police monospace toutes les
        // avances sont egales, mais 'M' evite les glyphes a chasse nulle.
        let width = fonts.glyph_width(&font, 'M').max(1.0);
        let height = fonts.row_height(&font).max(1.0);
        Vec2::new(width, height)
    })
}

/// Vrai si le point fait partie de la selection.
pub fn in_selection(range: &SelectionRange, point: Point) -> bool {
    if range.is_block {
        point.line >= range.start.line
            && point.line <= range.end.line
            && point.column >= range.start.column
            && point.column <= range.end.column
    } else {
        let after_start = point.line > range.start.line
            || (point.line == range.start.line && point.column >= range.start.column);
        let before_end = point.line < range.end.line
            || (point.line == range.end.line && point.column <= range.end.column);
        after_start && before_end
    }
}

/// Convertit une position ecran en coordonnee de grille.
///
/// Renvoie la colonne et le cote de la cellule vise, ce dont a besoin la
/// selection pour savoir si le caractere sous le curseur est inclus.
pub fn grid_position(
    pos: Pos2,
    rect: Rect,
    cell: Vec2,
    size: TermSize,
    display_offset: usize,
) -> (Point, Side) {
    let relative = pos - rect.min;
    let row = (relative.y / cell.y).floor().max(0.0) as usize;
    let row = row.min(size.screen_lines.saturating_sub(1));

    let column_f = (relative.x / cell.x).max(0.0);
    let column = (column_f.floor() as usize).min(size.columns.saturating_sub(1));
    let side = if column_f.fract() > 0.5 {
        Side::Right
    } else {
        Side::Left
    };

    let line = alacritty_terminal::index::Line(row as i32 - display_offset as i32);
    (Point::new(line, Column(column)), side)
}

/// Segment de texte de style homogene, dessine en une seule primitive.
struct Run {
    row: usize,
    column: usize,
    text: String,
    color: Color32,
    bold: bool,
    underline: bool,
    strikeout: bool,
}

impl Run {
    fn style_matches(&self, color: Color32, bold: bool, underline: bool, strikeout: bool) -> bool {
        self.color == color
            && self.bold == bold
            && self.underline == underline
            && self.strikeout == strikeout
    }
}

/// Affiche le terminal et traite les entrees clavier/souris.
///
/// `interactive` est faux quand un champ de saisie ou une fenetre modale a le
/// focus: le terminal ne doit alors pas capturer les touches.
pub fn show(
    ui: &mut egui::Ui,
    session: &mut TerminalSession,
    font_size: f32,
    interactive: bool,
) -> TerminalOutput {
    let mut output = TerminalOutput::default();
    let cell = cell_size(ui.ctx(), font_size);
    let available = ui.available_size();
    let (rect, response) = ui.allocate_at_least(available, Sense::click_and_drag());

    let columns = (rect.width() / cell.x).floor().max(1.0) as usize;
    let rows = (rect.height() / cell.y).floor().max(1.0) as usize;
    session.resize(
        TermSize::new(columns, rows),
        (cell.x.round() as u16, cell.y.round() as u16),
    );
    let size = session.size();

    if interactive {
        handle_input(ui, session, &response, rect, cell, size, &mut output);
    }
    paint(ui, session, rect, cell, size, font_size, interactive);
    output
}

fn handle_input(
    ui: &mut egui::Ui,
    session: &mut TerminalSession,
    response: &egui::Response,
    rect: Rect,
    cell: Vec2,
    size: TermSize,
    output: &mut TerminalOutput,
) {
    let mode = session.mode();
    let display_offset = session.display_offset();

    // --- Souris: selection et defilement ---
    if let Some(pos) = response.interact_pointer_pos() {
        let (point, side) = grid_position(pos, rect, cell, size, display_offset);
        if response.triple_clicked() {
            session.start_selection(SelectionType::Lines, point, side);
        } else if response.double_clicked() {
            session.start_selection(SelectionType::Semantic, point, side);
        } else if response.drag_started() {
            session.start_selection(SelectionType::Simple, point, side);
        } else if response.dragged() {
            session.update_selection(point, side);
        } else if response.clicked() {
            session.clear_selection();
        }
    }
    // Copie implicite en fin de glisser, comme dans SSH Pilot.
    if response.drag_stopped() {
        if let Some(text) = session.selection_text() {
            output.copy = Some(text);
        }
    }

    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.5 {
            let lines = (scroll / cell.y).round() as i32;
            if lines != 0 {
                match keys::alternate_scroll(lines, mode) {
                    Some(bytes) => session.write(bytes),
                    None => session.scroll(lines),
                }
            }
        }
    }

    // --- Clavier ---
    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            egui::Event::Text(text) if !text.is_empty() => {
                session.scroll_to_bottom();
                session.write_str(&text);
            }
            egui::Event::Paste(text) => {
                session.scroll_to_bottom();
                session.write(keys::encode_paste(&text, mode));
            }
            egui::Event::Copy => {
                if let Some(text) = session.selection_text() {
                    output.copy = Some(text);
                }
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                // Raccourcis d'interface: ils ne descendent pas dans le PTY.
                if modifiers.ctrl && modifiers.shift {
                    match key {
                        egui::Key::C => {
                            if let Some(text) = session.selection_text() {
                                output.copy = Some(text);
                            }
                        }
                        egui::Key::A => session.select_all(),
                        egui::Key::W => output.close_requested = true,
                        _ => {}
                    }
                    continue;
                }
                if modifiers.shift {
                    // Maj+PagePrec/Suiv fait defiler l'historique local.
                    let page = size.screen_lines as i32;
                    match key {
                        egui::Key::PageUp => {
                            session.scroll(page);
                            continue;
                        }
                        egui::Key::PageDown => {
                            session.scroll(-page);
                            continue;
                        }
                        _ => {}
                    }
                }
                if let Some(bytes) = keys::encode(key, &modifiers, mode) {
                    session.scroll_to_bottom();
                    session.write(bytes);
                }
            }
            _ => {}
        }
    }
}

fn paint(
    ui: &egui::Ui,
    session: &TerminalSession,
    rect: Rect,
    cell: Vec2,
    size: TermSize,
    font_size: f32,
    focused: bool,
) {
    let painter = ui.painter_at(rect);
    let palette = session.palette().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, palette.background);

    let regular = FontId::new(font_size, FontFamily::Monospace);
    let bold_font = FontId::new(font_size, FontFamily::Name(MONO_BOLD.into()));

    let mut runs: Vec<Run> = Vec::new();
    let mut backgrounds: Vec<(usize, usize, usize, Color32)> = Vec::new(); // ligne, debut, fin, couleur
    let mut cursor: Option<(usize, usize, CursorShape, char, Color32)> = None;

    session.with_term(|term| {
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let overrides = content.colors;
        let selection = content.selection;

        let mut current: Option<Run> = None;
        let mut current_bg: Option<(usize, usize, usize, Color32)> = None;

        for indexed in content.display_iter {
            let point = indexed.point;
            let cell_data = indexed.cell;
            let flags = cell_data.flags;
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = (point.line.0 + display_offset as i32).max(0) as usize;
            let column = point.column.0;
            if row >= size.screen_lines || column >= size.columns {
                continue;
            }

            let mut foreground = palette.resolve(cell_data.fg, overrides, flags);
            let mut background = palette.resolve(cell_data.bg, overrides, flags);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            if selection.is_some_and(|range| in_selection(&range, point)) {
                background = palette.selection;
                foreground = palette.foreground;
            }
            if flags.contains(Flags::HIDDEN) {
                foreground = background;
            }

            // --- fonds ---
            match current_bg.as_mut() {
                Some(run) if run.0 == row && run.2 + 1 == column && run.3 == background => {
                    run.2 = column;
                }
                _ => {
                    if let Some(run) = current_bg.take() {
                        if run.3 != palette.background {
                            backgrounds.push(run);
                        }
                    }
                    current_bg = Some((row, column, column, background));
                }
            }

            // --- glyphes ---
            let bold = flags.contains(Flags::BOLD) || flags.contains(Flags::BOLD_ITALIC);
            let underline = flags.contains(Flags::UNDERLINE);
            let strikeout = flags.contains(Flags::STRIKEOUT);
            let wide = flags.contains(Flags::WIDE_CHAR);
            let glyph = cell_data.c;

            if glyph == ' ' && !underline && !strikeout {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
            } else {
                match current.as_mut() {
                    Some(run)
                        if run.row == row
                            && run.column + run.text.chars().count() == column
                            && run.style_matches(foreground, bold, underline, strikeout)
                            && !wide =>
                    {
                        run.text.push(glyph);
                    }
                    _ => {
                        if let Some(run) = current.take() {
                            runs.push(run);
                        }
                        current = Some(Run {
                            row,
                            column,
                            text: glyph.to_string(),
                            color: foreground,
                            bold,
                            underline,
                            strikeout,
                        });
                        // Un caractere large occupe deux cellules: il ferme le
                        // segment pour que la colonne suivante reste alignee.
                        if wide {
                            runs.push(current.take().expect("segment courant"));
                        }
                    }
                }
            }

            // --- curseur ---
            if point == content.cursor.point && content.cursor.shape != CursorShape::Hidden {
                cursor = Some((row, column, content.cursor.shape, glyph, background));
            }
        }
        if let Some(run) = current.take() {
            runs.push(run);
        }
        if let Some(run) = current_bg.take() {
            if run.3 != palette.background {
                backgrounds.push(run);
            }
        }
    });

    let cell_rect = |row: usize, from: usize, to: usize| {
        // Les bords sont arrondis au pixel: sur un fond pixel art, un demi
        // pixel de bavure se voit immediatement.
        let min = Pos2::new(
            (rect.left() + from as f32 * cell.x).round(),
            (rect.top() + row as f32 * cell.y).round(),
        );
        let max = Pos2::new(
            (rect.left() + (to + 1) as f32 * cell.x).round(),
            (rect.top() + (row + 1) as f32 * cell.y).round(),
        );
        Rect::from_min_max(min, max)
    };

    for (row, from, to, color) in backgrounds {
        painter.rect_filled(cell_rect(row, from, to), CornerRadius::ZERO, color);
    }

    for run in runs {
        let pos = Pos2::new(
            rect.left() + run.column as f32 * cell.x,
            rect.top() + run.row as f32 * cell.y,
        );
        let font = if run.bold {
            bold_font.clone()
        } else {
            regular.clone()
        };
        let width = run.text.chars().count() as f32 * cell.x;
        painter.text(pos, Align2::LEFT_TOP, &run.text, font, run.color);
        if run.underline {
            let y = (pos.y + cell.y - 1.5).round() + 0.5;
            painter.line_segment(
                [Pos2::new(pos.x, y), Pos2::new(pos.x + width, y)],
                Stroke::new(1.0, run.color),
            );
        }
        if run.strikeout {
            let y = (pos.y + cell.y * 0.5).round() + 0.5;
            painter.line_segment(
                [Pos2::new(pos.x, y), Pos2::new(pos.x + width, y)],
                Stroke::new(1.0, run.color),
            );
        }
    }

    if let Some((row, column, shape, glyph, _)) = cursor {
        let bounds = cell_rect(row, column, column);
        let color = palette.cursor;
        match shape {
            CursorShape::Block if focused => {
                painter.rect_filled(bounds, CornerRadius::ZERO, color);
                if glyph != ' ' {
                    painter.text(
                        bounds.min,
                        Align2::LEFT_TOP,
                        glyph,
                        regular.clone(),
                        palette.cursor_text,
                    );
                }
            }
            // Hors focus, le bloc devient un contour: on voit ou est le
            // curseur sans laisser croire que la saisie ira la.
            CursorShape::Block | CursorShape::HollowBlock => {
                painter.rect_stroke(
                    bounds,
                    CornerRadius::ZERO,
                    Stroke::new(1.0, color),
                    StrokeKind::Inside,
                );
            }
            CursorShape::Underline => {
                let bar =
                    Rect::from_min_max(Pos2::new(bounds.left(), bounds.bottom() - 2.0), bounds.max);
                painter.rect_filled(bar, CornerRadius::ZERO, color);
            }
            CursorShape::Beam => {
                let bar =
                    Rect::from_min_max(bounds.min, Pos2::new(bounds.left() + 2.0, bounds.bottom()));
                painter.rect_filled(bar, CornerRadius::ZERO, color);
            }
            CursorShape::Hidden => {}
        }
    }

    // Indicateur de defilement: on ne regarde plus le bas de l'historique.
    let offset = session.display_offset();
    if offset > 0 {
        let label = format!("historique -{offset}");
        let pos = Pos2::new(rect.right() - 8.0, rect.top() + 6.0);
        let text_rect = painter.text(
            pos,
            Align2::RIGHT_TOP,
            &label,
            FontId::new(font_size - 2.0, FontFamily::Proportional),
            palette.background,
        );
        painter.rect_filled(text_rect.expand(4.0), CornerRadius::ZERO, palette.cursor);
        painter.text(
            pos,
            Align2::RIGHT_TOP,
            &label,
            FontId::new(font_size - 2.0, FontFamily::Proportional),
            palette.background,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::Line;

    fn point(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    #[test]
    fn linear_selection_spans_lines() {
        let range = SelectionRange::new(point(0, 3), point(2, 5), false);
        assert!(!in_selection(&range, point(0, 2)));
        assert!(in_selection(&range, point(0, 3)));
        assert!(in_selection(&range, point(1, 0)));
        assert!(in_selection(&range, point(1, 99)));
        assert!(in_selection(&range, point(2, 5)));
        assert!(!in_selection(&range, point(2, 6)));
        assert!(!in_selection(&range, point(3, 0)));
    }

    #[test]
    fn block_selection_is_rectangular() {
        let range = SelectionRange::new(point(0, 2), point(3, 4), true);
        assert!(in_selection(&range, point(1, 3)));
        assert!(!in_selection(&range, point(1, 5)));
        assert!(!in_selection(&range, point(1, 1)));
        assert!(in_selection(&range, point(3, 4)));
    }

    #[test]
    fn selection_over_scrollback_uses_negative_lines() {
        let range = SelectionRange::new(point(-5, 0), point(-3, 10), false);
        assert!(in_selection(&range, point(-4, 0)));
        assert!(!in_selection(&range, point(-6, 0)));
    }

    #[test]
    fn grid_position_maps_pixels_to_cells() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 480.0));
        let cell = Vec2::new(10.0, 20.0);
        let size = TermSize::new(80, 24);

        let (p, side) = grid_position(Pos2::new(10.0, 20.0), rect, cell, size, 0);
        assert_eq!(p, point(0, 0));
        assert_eq!(side, Side::Left);

        let (p, side) = grid_position(Pos2::new(38.0, 61.0), rect, cell, size, 0);
        assert_eq!(p, point(2, 2));
        assert_eq!(side, Side::Right);
    }

    #[test]
    fn grid_position_is_clamped_to_the_grid() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 480.0));
        let cell = Vec2::new(10.0, 20.0);
        let size = TermSize::new(80, 24);
        let (p, _) = grid_position(Pos2::new(5000.0, 5000.0), rect, cell, size, 0);
        assert_eq!(p, point(23, 79));
        let (p, _) = grid_position(Pos2::new(-50.0, -50.0), rect, cell, size, 0);
        assert_eq!(p, point(0, 0));
    }

    #[test]
    fn grid_position_accounts_for_scrollback() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 480.0));
        let cell = Vec2::new(10.0, 20.0);
        let size = TermSize::new(80, 24);
        // Avec 10 lignes d'historique affichees, la ligne 0 de l'ecran est la
        // ligne -10 du buffer.
        let (p, _) = grid_position(Pos2::new(0.0, 0.0), rect, cell, size, 10);
        assert_eq!(p, point(-10, 0));
    }
}
