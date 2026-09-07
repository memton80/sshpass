//! Session terminal: PTY, emulation VT et etat partage avec l'interface.
//!
//! L'emulation est assuree par `alacritty_terminal` (pure Rust, aucune
//! liaison a libvte). Chaque session possede son PTY et sa boucle d'evenements
//! dans un thread dedie; le `Term` est partage avec le thread d'interface au
//! travers d'un `FairMutex`, verrouille uniquement le temps d'un rendu ou
//! d'une saisie.

pub mod colors;
pub mod command;
pub mod keys;
pub mod render;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::tty;

use crate::term::colors::{color32_to_rgb, TerminalPalette};
use crate::term::command::CommandSpec;

/// Dimensions de la grille, exprimees en cellules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub columns: usize,
    pub screen_lines: usize,
}

impl TermSize {
    pub fn new(columns: usize, screen_lines: usize) -> Self {
        // Une grille vide ferait paniquer le moteur d'emulation.
        Self {
            columns: columns.max(1),
            screen_lines: screen_lines.max(1),
        }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Relais des evenements du terminal vers le thread d'interface.
#[derive(Clone)]
pub(crate) struct EventProxy {
    sender: Sender<TermEvent>,
    ctx: egui::Context,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: TermEvent) {
        if self.sender.send(event).is_ok() {
            // Sans cela l'interface resterait endormie tant que l'utilisateur
            // ne bouge pas la souris, et la sortie du programme distant ne
            // s'afficherait pas.
            self.ctx.request_repaint();
        }
    }
}

/// Effets a traiter par l'interface apres consommation des evenements.
#[derive(Debug, Default)]
pub struct TerminalUpdate {
    /// Texte a placer dans le presse-papiers (OSC 52 ou copie implicite).
    pub copy_to_clipboard: Option<String>,
    /// Le titre a change.
    pub title_changed: bool,
    /// Le terminal a sonne.
    pub bell: bool,
}

/// Une session terminal vivante.
pub struct TerminalSession {
    term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    events: Receiver<TermEvent>,
    loop_handle: Option<JoinHandle<(EventLoop<tty::Pty, EventProxy>, State)>>,
    size: TermSize,
    cell: (u16, u16),
    palette: TerminalPalette,
    /// Copie locale du presse-papiers, pour repondre aux requetes OSC 52.
    clipboard: String,
    /// Fichiers temporaires a supprimer a la fermeture (scripts askpass).
    cleanup: Vec<PathBuf>,
    pub title: String,
    /// Renseigne quand le processus distant s'est termine.
    pub exit_status: Option<String>,
}

impl TerminalSession {
    /// Demarre une session. `cell` est la taille d'une cellule en pixels; elle
    /// est transmise au PTY pour que les applications distantes qui dessinent
    /// des images (sixel, kitty) connaissent la geometrie reelle.
    pub fn spawn(
        spec: &CommandSpec,
        size: TermSize,
        cell: (u16, u16),
        scrollback: usize,
        ctx: &egui::Context,
        cleanup: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
        let options = tty::Options {
            shell: Some(tty::Shell::new(spec.program.clone(), spec.args.clone())),
            working_directory: spec.working_directory.clone(),
            drain_on_exit: false,
            env: spec.env.clone(),
        };
        let window_size = WindowSize {
            num_lines: size.screen_lines as u16,
            num_cols: size.columns as u16,
            cell_width: cell.0.max(1),
            cell_height: cell.1.max(1),
        };
        let pty = tty::new(&options, window_size, 0)?;

        let (sender, events) = mpsc::channel();
        let proxy = EventProxy {
            sender,
            ctx: ctx.clone(),
        };

        let config = TermConfig {
            scrolling_history: scrollback,
            ..TermConfig::default()
        };
        let term = Term::new(config, &size, proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let event_loop = EventLoop::new(Arc::clone(&term), proxy, pty, false, false)?;
        let sender = event_loop.channel();
        let loop_handle = event_loop.spawn();

        Ok(Self {
            term,
            sender,
            events,
            loop_handle: Some(loop_handle),
            size,
            cell: (window_size.cell_width, window_size.cell_height),
            palette: TerminalPalette::default(),
            clipboard: String::new(),
            cleanup,
            title: spec.program.clone(),
            exit_status: None,
        })
    }

    pub fn size(&self) -> TermSize {
        self.size
    }

    pub fn palette(&self) -> &TerminalPalette {
        &self.palette
    }

    pub fn is_alive(&self) -> bool {
        self.exit_status.is_none()
    }

    /// Mode courant du terminal (curseur applicatif, collage encadre...).
    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Ecrit des octets bruts dans le PTY.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return;
        }
        if let Err(err) = self.sender.send(Msg::Input(bytes)) {
            log::warn!("ecriture PTY impossible: {err}");
        }
    }

    pub fn write_str(&self, text: &str) {
        self.write(text.as_bytes().to_vec());
    }

    /// Redimensionne la grille. Sans effet si la taille n'a pas change.
    pub fn resize(&mut self, size: TermSize, cell: (u16, u16)) {
        let cell = (cell.0.max(1), cell.1.max(1));
        if size == self.size && cell == self.cell {
            return;
        }
        self.size = size;
        self.cell = cell;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(WindowSize {
            num_lines: size.screen_lines as u16,
            num_cols: size.columns as u16,
            cell_width: cell.0,
            cell_height: cell.1,
        }));
    }

    /// Fait defiler l'historique de `lines` lignes (positif = vers le passe).
    pub fn scroll(&self, lines: i32) {
        if lines != 0 {
            self.term.lock().scroll_display(Scroll::Delta(lines));
        }
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    pub fn display_offset(&self) -> usize {
        self.term.lock().grid().display_offset()
    }

    /// Acces en lecture au terminal, pour le rendu.
    pub fn with_term<R>(&self, f: impl FnOnce(&Term<EventProxy>) -> R) -> R {
        f(&self.term.lock())
    }

    pub fn start_selection(&self, ty: SelectionType, point: Point, side: Side) {
        self.term.lock().selection = Some(Selection::new(ty, point, side));
    }

    pub fn update_selection(&self, point: Point, side: Side) {
        let mut term = self.term.lock();
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    pub fn select_all(&self) {
        let mut term = self.term.lock();
        let start = Point::new(term.topmost_line(), alacritty_terminal::index::Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    /// Memorise le presse-papiers pour repondre aux requetes OSC 52.
    pub fn set_clipboard(&mut self, text: String) {
        self.clipboard = text;
    }

    /// Consomme les evenements emis par la boucle terminal.
    pub fn pump(&mut self) -> TerminalUpdate {
        let mut update = TerminalUpdate::default();
        loop {
            let event = match self.events.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.exit_status.is_none() {
                        self.exit_status = Some("session terminee".to_string());
                    }
                    break;
                }
            };
            match event {
                TermEvent::Title(title) => {
                    self.title = title;
                    update.title_changed = true;
                }
                TermEvent::ResetTitle => {
                    self.title.clear();
                    update.title_changed = true;
                }
                TermEvent::ClipboardStore(_, text) => {
                    self.clipboard = text.clone();
                    update.copy_to_clipboard = Some(text);
                }
                TermEvent::ClipboardLoad(_, format) => {
                    let reply = format(&self.clipboard);
                    self.write(reply.into_bytes());
                }
                TermEvent::ColorRequest(index, format) => {
                    let reply = format(color32_to_rgb(self.color_at(index)));
                    self.write(reply.into_bytes());
                }
                TermEvent::TextAreaSizeRequest(format) => {
                    let reply = format(WindowSize {
                        num_lines: self.size.screen_lines as u16,
                        num_cols: self.size.columns as u16,
                        cell_width: self.cell.0,
                        cell_height: self.cell.1,
                    });
                    self.write(reply.into_bytes());
                }
                TermEvent::PtyWrite(text) => self.write(text.into_bytes()),
                TermEvent::Bell => update.bell = true,
                TermEvent::ChildExit(status) => {
                    self.exit_status = Some(match status.code() {
                        Some(0) => "session fermee".to_string(),
                        Some(code) => format!("session terminee (code {code})"),
                        None => "session interrompue".to_string(),
                    });
                }
                TermEvent::Exit => {
                    if self.exit_status.is_none() {
                        self.exit_status = Some("session fermee".to_string());
                    }
                }
                TermEvent::Wakeup
                | TermEvent::MouseCursorDirty
                | TermEvent::CursorBlinkingChange => {}
            }
        }
        update
    }

    /// Couleur associee a un index de la table `Colors` d'alacritty.
    fn color_at(&self, index: usize) -> egui::Color32 {
        use alacritty_terminal::vte::ansi::NamedColor;
        let overrides = alacritty_terminal::term::color::Colors::default();
        match index {
            0..=255 => self.palette.indexed_color(index as u8, &overrides),
            i if i == NamedColor::Foreground as usize => self.palette.foreground,
            i if i == NamedColor::Background as usize => self.palette.background,
            i if i == NamedColor::Cursor as usize => self.palette.cursor,
            _ => self.palette.foreground,
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
        if let Some(handle) = self.loop_handle.take() {
            // La boucle libere le PTY, qui envoie SIGHUP au processus fils.
            let _ = handle.join();
        }
        for path in &self.cleanup {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_size_never_degenerates() {
        let size = TermSize::new(0, 0);
        assert_eq!(size.columns, 1);
        assert_eq!(size.screen_lines, 1);
        assert_eq!(size.total_lines(), 1);
    }

    #[test]
    fn term_size_reports_dimensions() {
        let size = TermSize::new(80, 24);
        assert_eq!(size.columns(), 80);
        assert_eq!(size.screen_lines(), 24);
        assert_eq!(size.last_column().0, 79);
        assert_eq!(size.bottommost_line().0, 23);
    }
}
