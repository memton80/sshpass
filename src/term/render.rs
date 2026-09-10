//! Widget egui de rendu du terminal.
//!
//! Le rendu est immediat: a chaque frame on parcourt les cellules visibles et
//! on les regroupe en segments de meme style, ce qui ramene une grille 80x24 a
//! quelques dizaines de primitives au lieu de deux mille.

use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{SelectionRange, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::CursorShape;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, EventFilter, FontFamily, FontId, Modifiers, Pos2,
    Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::term::keys::{self, MouseAction, MouseButton};
use crate::term::{TermSize, TerminalSession};
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

/// Geometrie de la grille pour une frame.
///
/// Les quatre valeurs vont toujours ensemble: les passer separement a chaque
/// fonction d'entree donnait des signatures que plus personne ne relisait.
#[derive(Clone, Copy)]
struct Grid {
    rect: Rect,
    cell: Vec2,
    size: TermSize,
    display_offset: usize,
}

impl Grid {
    /// Cellule visee par un point de l'ecran.
    fn at(&self, pos: Pos2) -> (Point, Side) {
        grid_position(pos, self.rect, self.cell, self.size, self.display_offset)
    }
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
/// `available` est faux quand une fenetre modale occupe l'ecran: le terminal
/// rend alors le clavier au lieu de le disputer au champ de saisie.
pub fn show(
    ui: &mut egui::Ui,
    session: &mut TerminalSession,
    font_size: f32,
    available: bool,
) -> TerminalOutput {
    let mut output = TerminalOutput::default();
    let cell = cell_size(ui.ctx(), font_size);
    let space = ui.available_size();
    let (rect, response) = ui.allocate_at_least(space, Sense::click_and_drag());

    let columns = (rect.width() / cell.x).floor().max(1.0) as usize;
    let rows = (rect.height() / cell.y).floor().max(1.0) as usize;
    session.resize(
        TermSize::new(columns, rows),
        (cell.x.round() as u16, cell.y.round() as u16),
    );
    let size = session.size();

    let focused = claim_keyboard(ui.ctx(), &response, available);
    // Le focus de la fenetre compte autant que celui du widget: une fenetre
    // passee en arriere-plan n'a plus le clavier, et les programmes qui
    // suivent le focus doivent l'apprendre.
    session.set_focus(focused && ui.input(|i| i.focused));

    let mode = session.mode();
    let modifiers = ui.input(|i| i.modifiers);
    let grid = Grid {
        rect,
        cell,
        size,
        display_offset: session.display_offset(),
    };
    mouse(ui, session, &response, grid, &modifiers, mode, &mut output);
    if focused {
        keyboard(ui, session, size, mode, &modifiers, &mut output);
    }
    paint(ui, session, rect, cell, size, font_size, focused);
    output
}

/// Donne, garde ou rend le clavier au terminal. Renvoie vrai s'il l'a.
///
/// Un terminal doit posseder le focus egui, et pas seulement profiter de ce
/// que personne d'autre ne l'a: sans cela `Tab` promene le focus dans les
/// boutons de l'interface, les fleches le deplacent de widget en widget et
/// `Echap` l'abandonne — trois touches qui appartiennent au shell.
fn claim_keyboard(ctx: &egui::Context, response: &egui::Response, available: bool) -> bool {
    let id = response.id;
    if !available {
        if ctx.memory(|m| m.has_focus(id)) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        return false;
    }

    // Le clic prend le clavier; a defaut le terminal le prend d'office tant
    // qu'aucun champ ne le reclame, pour qu'un onglet frais soit utilisable
    // sans cliquer dedans.
    if response.clicked() || response.drag_started() || ctx.memory(|m| m.focused()).is_none() {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    if !ctx.memory(|m| m.has_focus(id)) {
        return false;
    }

    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });
    true
}

/// Souris: selection locale, ou suivi par le programme distant.
fn mouse(
    ui: &mut egui::Ui,
    session: &mut TerminalSession,
    response: &egui::Response,
    grid: Grid,
    mods: &Modifiers,
    mode: TermMode,
    output: &mut TerminalOutput,
) {
    // `Maj` passe outre le suivi de souris: c'est la convention de tous les
    // terminaux pour selectionner du texte malgre `htop`, `vim` ou `tmux`.
    let to_remote = keys::wants_mouse(mode) && !mods.shift;

    if response.hovered() {
        // Curseur en I sur du texte selectionnable, fleche quand c'est le
        // programme distant qui recoit les clics.
        ui.ctx().set_cursor_icon(match to_remote {
            true => CursorIcon::Default,
            false => CursorIcon::Text,
        });
    }

    if to_remote {
        report_mouse(ui, session, response, grid, mods, mode);
    } else {
        select(session, response, grid, mods, output);
    }

    wheel(ui, session, response, grid, mods, mode, to_remote);
}

/// Selection a la souris et copie implicite en fin de glisser.
fn select(
    session: &mut TerminalSession,
    response: &egui::Response,
    grid: Grid,
    mods: &Modifiers,
    output: &mut TerminalOutput,
) {
    if let Some(pos) = response.interact_pointer_pos() {
        let (point, side) = grid.at(pos);
        if response.triple_clicked() {
            session.start_selection(SelectionType::Lines, point, side);
        } else if response.double_clicked() {
            session.start_selection(SelectionType::Semantic, point, side);
        } else if response.drag_started() {
            // `Ctrl` selectionne en colonnes, comme dans alacritty: c'est ce
            // qu'il faut pour extraire une colonne de `docker ps`.
            let kind = match mods.ctrl {
                true => SelectionType::Block,
                false => SelectionType::Simple,
            };
            session.start_selection(kind, point, side);
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
}

/// Boutons de souris, dans l'ordre ou xterm les numerote.
const BUTTONS: [(egui::PointerButton, MouseButton); 3] = [
    (egui::PointerButton::Primary, MouseButton::Left),
    (egui::PointerButton::Middle, MouseButton::Middle),
    (egui::PointerButton::Secondary, MouseButton::Right),
];

/// Transmet clics, relachements et deplacements au programme distant.
fn report_mouse(
    ui: &egui::Ui,
    session: &mut TerminalSession,
    response: &egui::Response,
    grid: Grid,
    mods: &Modifiers,
    mode: TermMode,
) {
    let Some(pos) = ui.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    let (point, _) = grid.at(pos);
    // Le programme distant ne connait que l'ecran: une position prise dans
    // l'historique ne lui veut rien dire.
    let Ok(line) = usize::try_from(point.line.0) else {
        return;
    };
    let column = point.column.0;

    let over = response.contains_pointer();
    let (pressed, released, held) = ui.input(|i| {
        let pressed: Vec<MouseButton> = BUTTONS
            .iter()
            .filter(|(egui_button, _)| i.pointer.button_pressed(*egui_button))
            .map(|(_, button)| *button)
            .collect();
        let released: Vec<MouseButton> = BUTTONS
            .iter()
            .filter(|(egui_button, _)| i.pointer.button_released(*egui_button))
            .map(|(_, button)| *button)
            .collect();
        let held = BUTTONS
            .iter()
            .find(|(egui_button, _)| i.pointer.button_down(*egui_button))
            .map(|(_, button)| *button);
        (pressed, released, held)
    });

    for button in pressed {
        if !over {
            continue;
        }
        // Une selection locale n'aurait plus de sens: c'est le programme
        // distant qui dessine desormais la sienne.
        session.clear_selection();
        report(
            session,
            button,
            MouseAction::Press,
            column,
            line,
            mods,
            mode,
        );
    }
    for button in released {
        report(
            session,
            button,
            MouseAction::Release,
            column,
            line,
            mods,
            mode,
        );
    }

    // --- deplacement ---
    let motion_wanted = mode.contains(TermMode::MOUSE_MOTION)
        || (mode.contains(TermMode::MOUSE_DRAG) && held.is_some());
    let id = response.id.with("derniere_cellule");
    let previous: Option<(usize, usize)> = ui.data(|d| d.get_temp(id));
    if previous != Some((column, line)) {
        ui.data_mut(|d| d.insert_temp(id, (column, line)));
        if motion_wanted && (over || held.is_some()) {
            let button = held.unwrap_or(MouseButton::None);
            report(
                session,
                button,
                MouseAction::Motion,
                column,
                line,
                mods,
                mode,
            );
        }
    }
}

/// Ecrit un rapport de souris, s'il y a quelque chose a ecrire.
fn report(
    session: &TerminalSession,
    button: MouseButton,
    action: MouseAction,
    column: usize,
    line: usize,
    mods: &Modifiers,
    mode: TermMode,
) {
    if let Some(bytes) = keys::mouse_report(button, action, column, line, mods, mode) {
        session.write(bytes);
    }
}

/// Molette: historique local, fleches de substitution ou boutons 4 et 5.
fn wheel(
    ui: &egui::Ui,
    session: &mut TerminalSession,
    response: &egui::Response,
    grid: Grid,
    mods: &Modifiers,
    mode: TermMode,
    to_remote: bool,
) {
    if !response.hovered() {
        return;
    }
    let delta = ui.input(|i| i.smooth_scroll_delta.y);
    // Le reste d'une ligne est reporte d'une frame a l'autre: sans cela un
    // pave tactile, qui avance par fractions de ligne, ne ferait jamais rien.
    let id = response.id.with("residu_molette");
    let carried: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
    let total = carried + delta / grid.cell.y;
    let whole = total.trunc();
    ui.data_mut(|d| d.insert_temp(id, total - whole));
    let lines = whole as i32;
    if lines == 0 {
        return;
    }

    if to_remote {
        // Pour le programme distant, la molette est un bouton de plus.
        let Some(pos) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let (point, _) = grid.at(pos);
        let Ok(line) = usize::try_from(point.line.0) else {
            return;
        };
        let button = match lines > 0 {
            true => MouseButton::WheelUp,
            false => MouseButton::WheelDown,
        };
        for _ in 0..lines.unsigned_abs() {
            report(
                session,
                button,
                MouseAction::Press,
                point.column.0,
                line,
                mods,
                mode,
            );
        }
        return;
    }

    match keys::alternate_scroll(lines, mode) {
        Some(bytes) => session.write(bytes),
        None => session.scroll(lines),
    }
}

/// Ce qu'un evenement clavier demande au terminal.
///
/// Le passage par cette enumeration n'est pas de la ceremonie: c'est ce qui
/// rend la traduction verifiable sans PTY ni fenetre, et `Ctrl+C` est
/// precisement le genre de touche qu'on ne veut pas voir regresser.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Input {
    /// Octets a envoyer au programme distant.
    Write(Vec<u8>),
    /// Copier la selection dans le presse-papiers.
    Copy,
    /// Selectionner tout l'historique.
    SelectAll,
    /// Fermer l'onglet.
    Close,
    /// Faire defiler l'historique local, en lignes.
    Scroll(i32),
    /// Rien a faire.
    Ignore,
}

/// Traduit les evenements d'une frame.
///
/// `modifiers` est l'etat des modificateurs de la frame et non celui d'une
/// touche: `egui` convertit `Ctrl+C`, `Ctrl+X` et `Ctrl+V` en evenements de
/// presse-papiers sans jamais livrer la touche, et c'est le seul endroit ou
/// retrouver le `Maj` qui distingue « interrompre » de « copier ».
fn translate(
    events: &[egui::Event],
    modifiers: &Modifiers,
    mode: TermMode,
    page: i32,
) -> Vec<Input> {
    let mut inputs = Vec::new();
    // `Alt+b` arrive en deux morceaux: la touche, encodee en `ESC b`, puis le
    // texte « b » que winit produit quand meme. Sans ce drapeau, le shell
    // reculerait d'un mot *et* taperait un « b ».
    let mut alt_encoded = false;

    for event in events {
        let follows_alt = std::mem::take(&mut alt_encoded);
        if follows_alt && matches!(event, egui::Event::Text(_)) {
            continue;
        }
        let input = translate_one(event, modifiers, mode, page);
        alt_encoded = matches!(input, Input::Write(_))
            && matches!(event, egui::Event::Key { modifiers, .. } if modifiers.alt);
        if input != Input::Ignore {
            inputs.push(input);
        }
    }
    inputs
}

fn translate_one(event: &egui::Event, modifiers: &Modifiers, mode: TermMode, page: i32) -> Input {
    // Vrai pour `Ctrl+…` sans `Maj`: la combinaison appartient alors au
    // programme distant, pas au presse-papiers.
    let to_remote = modifiers.ctrl && !modifiers.shift;

    match event {
        egui::Event::Text(text) if !text.is_empty() => Input::Write(text.as_bytes().to_vec()),
        // Saisie composee (methode d'entree, touches mortes enchainees).
        egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
            Input::Write(text.as_bytes().to_vec())
        }
        // `Ctrl+C` interrompt la commande en cours, comme dans tout terminal;
        // c'est `Ctrl+Maj+C` qui copie.
        egui::Event::Copy if to_remote => Input::Write(vec![0x03]),
        egui::Event::Copy => Input::Copy,
        egui::Event::Cut if to_remote => Input::Write(vec![0x18]),
        egui::Event::Cut => Input::Copy,
        // `Ctrl+V` prend le caractere suivant au pied de la lettre (`^V` de
        // readline); le collage, c'est `Ctrl+Maj+V`.
        egui::Event::Paste(_) if to_remote => Input::Write(vec![0x16]),
        egui::Event::Paste(text) => Input::Write(keys::encode_paste(text, mode)),
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            // Raccourcis d'interface: ils ne descendent pas dans le PTY.
            if modifiers.ctrl && modifiers.shift {
                match key {
                    egui::Key::A => return Input::SelectAll,
                    egui::Key::W => return Input::Close,
                    _ => {}
                }
            }
            if modifiers.shift {
                // Maj+PagePrec/Suiv fait defiler l'historique local.
                match key {
                    egui::Key::PageUp => return Input::Scroll(page),
                    egui::Key::PageDown => return Input::Scroll(-page),
                    _ => {}
                }
            }
            match keys::encode(*key, modifiers, mode) {
                Some(bytes) => Input::Write(bytes),
                None => Input::Ignore,
            }
        }
        _ => Input::Ignore,
    }
}

/// Clavier: raccourcis de l'interface, presse-papiers, puis PTY.
fn keyboard(
    ui: &egui::Ui,
    session: &mut TerminalSession,
    size: TermSize,
    mode: TermMode,
    modifiers: &Modifiers,
    output: &mut TerminalOutput,
) {
    let events = ui.input(|i| i.events.clone());
    for input in translate(&events, modifiers, mode, size.screen_lines as i32) {
        match input {
            Input::Write(bytes) => {
                // Taper ramene toujours au bas de l'historique: c'est la que
                // le programme distant va repondre.
                session.scroll_to_bottom();
                session.write(bytes);
            }
            Input::Copy => {
                if let Some(text) = session.selection_text() {
                    output.copy = Some(text);
                }
            }
            Input::SelectAll => session.select_all(),
            Input::Close => output.close_requested = true,
            Input::Scroll(lines) => session.scroll(lines),
            Input::Ignore => {}
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
        let font = FontId::new(font_size - 2.0, FontFamily::Proportional);
        let galley =
            painter.layout_no_wrap(format!("historique -{offset}"), font, palette.background);
        let size = galley.size();
        let origin = egui::pos2(rect.right() - size.x - 12.0, rect.top() + 6.0);
        painter.rect_filled(
            Rect::from_min_size(origin, size).expand(4.0),
            CornerRadius::ZERO,
            palette.cursor,
        );
        painter.galley(origin, galley, palette.background);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::Line;

    const NO_MODS: Modifiers = Modifiers::NONE;

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        }
    }

    fn key(key: egui::Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Traduction d'un evenement isole, terminal ordinaire, 24 lignes.
    fn one(event: egui::Event, modifiers: Modifiers) -> Input {
        let mut inputs = translate(&[event], &modifiers, TermMode::NONE, 24);
        assert!(inputs.len() <= 1, "un seul effet attendu: {inputs:?}");
        inputs.pop().unwrap_or(Input::Ignore)
    }

    #[test]
    fn ctrl_c_interrupts_and_ctrl_shift_c_copies() {
        // egui livre `Ctrl+C` comme une demande de copie, sans la touche:
        // c'est la que se jouait l'impossibilite d'interrompre une commande.
        assert_eq!(one(egui::Event::Copy, ctrl()), Input::Write(vec![0x03]));
        assert_eq!(one(egui::Event::Copy, ctrl_shift()), Input::Copy);
        // La touche « Copier » d'un clavier multimedia copie, elle aussi.
        assert_eq!(one(egui::Event::Copy, NO_MODS), Input::Copy);
    }

    #[test]
    fn ctrl_x_cuts_nothing_and_reaches_the_shell() {
        assert_eq!(one(egui::Event::Cut, ctrl()), Input::Write(vec![0x18]));
        assert_eq!(one(egui::Event::Cut, ctrl_shift()), Input::Copy);
    }

    #[test]
    fn only_ctrl_shift_v_pastes() {
        let paste = egui::Event::Paste("ls\n".to_string());
        // `Ctrl+V` est le « caractere suivant, litteralement » de readline.
        assert_eq!(one(paste.clone(), ctrl()), Input::Write(vec![0x16]));
        assert_eq!(
            one(paste, ctrl_shift()),
            Input::Write(b"ls\r".to_vec()),
            "collage attendu, fins de ligne normalisees"
        );
    }

    #[test]
    fn the_shell_keys_reach_the_shell() {
        // Les trois touches que l'interface detournait: completion,
        // historique, sortie de `vim`.
        assert_eq!(
            one(key(egui::Key::Tab, NO_MODS), NO_MODS),
            Input::Write(b"\t".to_vec())
        );
        assert_eq!(
            one(key(egui::Key::ArrowUp, NO_MODS), NO_MODS),
            Input::Write(b"\x1b[A".to_vec())
        );
        assert_eq!(
            one(key(egui::Key::Escape, NO_MODS), NO_MODS),
            Input::Write(vec![0x1b])
        );
    }

    #[test]
    fn interface_shortcuts_stay_in_the_interface() {
        assert_eq!(
            one(key(egui::Key::A, ctrl_shift()), ctrl_shift()),
            Input::SelectAll
        );
        assert_eq!(
            one(key(egui::Key::W, ctrl_shift()), ctrl_shift()),
            Input::Close
        );
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            one(key(egui::Key::PageUp, shift), shift),
            Input::Scroll(24),
            "Maj+PagePrec defile l'historique local"
        );
    }

    #[test]
    fn an_alt_combination_is_not_typed_twice() {
        // winit livre la touche *et* le texte pour `Alt+b`: sans filtre, le
        // shell reculerait d'un mot puis taperait un « b ».
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        let events = vec![key(egui::Key::B, alt), egui::Event::Text("b".to_string())];
        assert_eq!(
            translate(&events, &alt, TermMode::NONE, 24),
            vec![Input::Write(b"\x1bb".to_vec())]
        );

        // Un caractere que `egui` ne sait pas nommer n'arrive que par le
        // flux de texte: il doit passer, meme avec Alt enfonce, sans quoi les
        // claviers a troisieme niveau perdraient des caracteres.
        let events = vec![egui::Event::Text("e".to_string())];
        assert_eq!(
            translate(&events, &alt, TermMode::NONE, 24),
            vec![Input::Write(b"e".to_vec())]
        );
    }

    /// Une frame de terminal, avec un bouton derriere lui pour que `Tab` ait
    /// une cible s'il devait s'echapper.
    fn terminal_frame(ctx: &egui::Context, events: Vec<egui::Event>) -> egui::Id {
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(400.0, 200.0),
            )),
            ..Default::default()
        };
        let mut id = egui::Id::NULL;
        let _ = ctx.run_ui(input, |ui| {
            let (_, response) =
                ui.allocate_at_least(egui::vec2(300.0, 150.0), Sense::click_and_drag());
            id = response.id;
            claim_keyboard(ui.ctx(), &response, true);
            let _ = ui.button("ailleurs");
        });
        id
    }

    #[test]
    fn the_terminal_takes_the_keyboard_and_keeps_it_on_tab() {
        let ctx = egui::Context::default();
        let id = terminal_frame(&ctx, Vec::new());
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(id),
            "le terminal doit prendre le clavier des qu'il est libre"
        );
        // Le filtre d'evenements ne prend effet qu'une fois le focus acquis
        // depuis une frame: on laisse passer celle-la.
        terminal_frame(&ctx, Vec::new());
        terminal_frame(&ctx, vec![key(egui::Key::Tab, Modifiers::NONE)]);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(id),
            "Tab est parti promener le focus au lieu d'aller au shell"
        );
    }

    /// Le chemin complet d'un `Ctrl+C`, PTY compris.
    ///
    /// Les tests precedents s'arretent a l'octet produit; celui-ci verifie que
    /// cet octet interrompt bien un programme — c'est la ligne de discipline
    /// du PTY qui transforme le `0x03` en signal, et c'est cette derniere
    /// marche qui manquait a l'utilisateur.
    #[test]
    fn ctrl_c_interrupts_a_real_process() {
        use crate::term::command::CommandSpec;
        use crate::term::{ClipboardPolicy, TerminalSession};

        let ctx = egui::Context::default();
        let spec = CommandSpec {
            program: "/bin/cat".to_string(),
            args: Vec::new(),
            env: std::collections::HashMap::new(),
            working_directory: None,
        };
        let mut session = TerminalSession::spawn(
            &spec,
            TermSize::new(80, 24),
            (8, 16),
            100,
            &ctx,
            Vec::new(),
            ClipboardPolicy::default(),
        )
        .expect("session terminal");

        match one(egui::Event::Copy, ctrl()) {
            Input::Write(bytes) => session.write(bytes),
            other => panic!("interruption attendue, recu {other:?}"),
        }

        // `cat` attend son entree: sans le signal, il attendrait indefiniment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while session.exit_status.is_none() && std::time::Instant::now() < deadline {
            session.pump();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            session.exit_status.as_deref(),
            Some("session interrompue"),
            "le ^C n'a pas atteint le processus"
        );
    }

    #[test]
    fn a_modal_window_gets_the_keyboard_back() {
        let ctx = egui::Context::default();
        let id = terminal_frame(&ctx, Vec::new());
        assert_eq!(ctx.memory(|m| m.focused()), Some(id));

        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(400.0, 200.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let (_, response) =
                ui.allocate_at_least(egui::vec2(300.0, 150.0), Sense::click_and_drag());
            // Une fenetre modale est ouverte: le terminal rend le clavier.
            assert!(!claim_keyboard(ui.ctx(), &response, false));
        });
        assert_eq!(ctx.memory(|m| m.focused()), None);
    }

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
