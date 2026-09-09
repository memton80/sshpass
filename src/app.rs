//! Etat global de l'application et boucle eframe.

use std::collections::{HashMap, HashSet};

use crate::config::{self, AgentMode, AuthMethod, Config, Connection, Folder, ProtonRef};
use crate::pass::{
    AgentManager, AgentState, Item, PassCli, PassRequest, PassResponse, PassWorker, Vault,
};
use crate::term::command::{self, CommandSpec, SessionContext};
use crate::term::{TermSize, TerminalSession};
use crate::theme::{self, Palette};
use crate::ui;

/// Delai au-dela duquel on cesse d'attendre l'agent Proton Pass.
const AGENT_WAIT_TIMEOUT: f64 = 20.0;
/// Duree d'affichage d'une notification.
const TOAST_DURATION: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub expires_at: f64,
}

/// Disponibilite de `pass-cli`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassStatus {
    Probing,
    Available(String),
    Missing(String),
}

impl PassStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, PassStatus::Available(_))
    }
}

/// Etat d'un onglet.
pub enum TabState {
    /// L'agent du coffre demarre; la session attend sa socket.
    WaitingAgent {
        vault: String,
        since: f64,
    },
    Running(Box<TerminalSession>),
    Failed(String),
}

pub struct Tab {
    pub id: String,
    /// Connexion d'origine, absente pour un shell local.
    pub connection: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub state: TabState,
    pub bell: bool,
}

impl Tab {
    pub fn session(&self) -> Option<&TerminalSession> {
        match &self.state {
            TabState::Running(session) => Some(session),
            _ => None,
        }
    }
}

/// Actions emises par l'interface et appliquees en fin de frame.
///
/// Ce report evite d'emprunter l'application en mutable pendant qu'on la
/// dessine, sans avoir a decouper l'etat en cellules partagees.
pub enum Action {
    OpenConnection(String),
    OpenLocalShell,
    NewConnection(Option<String>),
    EditConnection(String),
    AskDeleteConnection(String),
    DeleteConnection(String),
    MoveConnection {
        connection: String,
        folder: Option<String>,
    },
    ToggleFavorite(String),
    NewFolder(String),
    DeleteFolder(String),
    // Les onglets sont designes par leur identifiant, pas par leur indice:
    // deux actions dans la meme frame (fermer puis selectionner) decaleraient
    // les indices et agiraient sur le mauvais onglet.
    CloseTab(String),
    SelectTab(String),
    ShowHome,
    RefreshVaults,
    LoadItems(String),
    StartAgent(String),
    StopAgent(String),
    LoadIntoExistingAgent(String),
    AssignItem {
        connection: String,
        vault: String,
        item: String,
    },
    ConnectWithoutAgent(String),
    Toast(String, ToastKind),
}

pub struct SshpassApp {
    pub config: Config,
    pub palette: Palette,
    pub agents: AgentManager,
    pub pass: PassWorker,
    pub pass_status: PassStatus,
    pub vaults: Vec<Vault>,
    pub items: HashMap<String, Vec<Item>>,
    pub loading_items: HashSet<String>,

    pub tabs: Vec<Tab>,
    pub active_tab: Option<usize>,

    pub search: String,
    pub focus_search: bool,
    pub expanded_folders: HashSet<String>,

    pub editor: Option<ui::editor::ConnectionEditor>,
    pub pending_delete: Option<String>,
    pub new_folder_name: Option<String>,
    pub show_vault_panel: bool,
    pub vault_search: String,
    pub selected_vault: Option<String>,
    pub settings_open: bool,

    pub toasts: Vec<Toast>,
    pub actions: Vec<Action>,
}

impl SshpassApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        theme::install_system_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, &theme::DARK, config.ui.font_size);

        let agents = AgentManager::new(
            &config.proton_pass.binary,
            config.proton_pass.agent_mode,
            config.proton_pass.refresh_interval,
        );
        let mut pass = PassWorker::spawn();
        pass.send(PassRequest::Probe(PassCli::new(&config.proton_pass.binary)));

        Self {
            config,
            palette: theme::DARK,
            agents,
            pass,
            pass_status: PassStatus::Probing,
            vaults: Vec::new(),
            items: HashMap::new(),
            loading_items: HashSet::new(),
            tabs: Vec::new(),
            active_tab: None,
            search: String::new(),
            focus_search: false,
            expanded_folders: HashSet::new(),
            editor: None,
            pending_delete: None,
            new_folder_name: None,
            show_vault_panel: false,
            vault_search: String::new(),
            selected_vault: None,
            settings_open: false,
            toasts: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub fn cli(&self) -> PassCli {
        PassCli::new(&self.config.proton_pass.binary)
    }

    pub fn toast(&mut self, message: impl Into<String>, kind: ToastKind, now: f64) {
        self.toasts.push(Toast {
            message: message.into(),
            kind,
            expires_at: now + TOAST_DURATION,
        });
    }

    pub fn save_config(&mut self) {
        if let Err(err) = config::save(&self.config) {
            log::error!("sauvegarde impossible: {err}");
            self.toasts.push(Toast {
                message: format!("Sauvegarde impossible: {err}"),
                kind: ToastKind::Error,
                expires_at: f64::MAX,
            });
        }
    }

    /// Coffre Proton Pass associe a une connexion, s'il y en a un.
    fn vault_of(&self, connection: &Connection) -> Option<String> {
        connection
            .proton
            .as_ref()
            .map(|p| p.vault.clone())
            .filter(|vault| !vault.is_empty())
    }

    /// Ouvre une connexion dans un nouvel onglet.
    fn open_connection(&mut self, id: &str, ctx: &egui::Context, now: f64) {
        let Some(connection) = self.config.connection(id).cloned() else {
            return;
        };
        let vault = self.vault_of(&connection);

        // En mode agent dedie, la session doit attendre que la socket existe:
        // ssh lit SSH_AUTH_SOCK au demarrage et ne le relira pas.
        if let Some(vault) = vault.clone() {
            match self.config.proton_pass.agent_mode {
                AgentMode::OwnAgent => {
                    let state = self.agents.ensure(&vault);
                    if !state.is_running() {
                        if let AgentState::Failed(err) = &state {
                            self.toast(
                                format!("Agent Proton Pass indisponible: {err}"),
                                ToastKind::Error,
                                now,
                            );
                        }
                        self.push_tab(Tab {
                            id: config::new_id(),
                            connection: Some(connection.id.clone()),
                            title: connection.display_name(),
                            subtitle: connection.target(),
                            state: TabState::WaitingAgent { vault, since: now },
                            bell: false,
                        });
                        self.mark_used(id);
                        return;
                    }
                }
                AgentMode::LoadIntoExisting => {
                    let cli = self.cli();
                    self.pass.send(PassRequest::LoadAgent(cli, vault));
                }
                AgentMode::Disabled => {}
            }
        }

        self.mark_used(id);
        let tab = self.build_tab(&connection, ctx, now);
        self.push_tab(tab);
    }

    /// Construit l'onglet et demarre reellement la session.
    fn build_tab(&mut self, connection: &Connection, ctx: &egui::Context, now: f64) -> Tab {
        let mut cleanup = Vec::new();
        let mut context = SessionContext::default();

        if let Some(vault) = self.vault_of(connection) {
            context.agent_socket = self.agents.socket_for(&vault);
        }

        if connection.auth == AuthMethod::Password {
            match connection.proton.as_ref().filter(|p| p.is_complete()) {
                Some(reference) => {
                    let uri = uri_with_default_field(reference);
                    match crate::pass::write_askpass_script(
                        &self.config.proton_pass.binary,
                        &uri,
                        &connection.id,
                    ) {
                        Ok(path) => {
                            cleanup.push(path.clone());
                            context.askpass_script = Some(path);
                        }
                        Err(err) => {
                            log::error!("script askpass impossible: {err}");
                            self.toast(
                                format!("Script askpass impossible: {err}"),
                                ToastKind::Error,
                                now,
                            );
                        }
                    }
                }
                None => self.toast(
                    "Authentification par mot de passe sans item Proton Pass associe",
                    ToastKind::Error,
                    now,
                ),
            }
        }

        let spec = command::build_ssh(connection, &context);
        self.spawn_tab(
            connection.display_name(),
            connection.target(),
            Some(connection.id.clone()),
            spec,
            cleanup,
            ctx,
        )
    }

    fn spawn_tab(
        &mut self,
        title: String,
        subtitle: String,
        connection: Option<String>,
        spec: CommandSpec,
        cleanup: Vec<std::path::PathBuf>,
        ctx: &egui::Context,
    ) -> Tab {
        let cell = crate::term::render::cell_size(ctx, self.config.ui.terminal_font_size);
        let state = match TerminalSession::spawn(
            &spec,
            TermSize::new(80, 24),
            (cell.x.round() as u16, cell.y.round() as u16),
            self.config.ui.scrollback_lines,
            ctx,
            cleanup,
        ) {
            Ok(session) => TabState::Running(Box::new(session)),
            Err(err) => {
                log::error!("demarrage de session impossible: {err}");
                TabState::Failed(format!("{err}\n\nCommande: {}", spec.display()))
            }
        };
        Tab {
            id: config::new_id(),
            connection,
            title,
            subtitle,
            state,
            bell: false,
        }
    }

    fn push_tab(&mut self, tab: Tab) {
        self.tabs.push(tab);
        self.active_tab = Some(self.tabs.len() - 1);
    }

    fn mark_used(&mut self, id: &str) {
        if let Some(connection) = self.config.connection_mut(id) {
            connection.touch();
            self.save_config();
        }
    }

    fn tab_index(&self, id: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        self.active_tab = match self.tabs.is_empty() {
            true => None,
            false => Some(index.min(self.tabs.len() - 1)),
        };
    }

    /// Fait avancer les onglets qui attendent leur agent.
    fn advance_waiting_tabs(&mut self, ctx: &egui::Context, now: f64) {
        let ready: Vec<(usize, bool)> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| match &tab.state {
                TabState::WaitingAgent { vault, since } => {
                    let running = self.agents.state_of(vault).is_running();
                    let expired = now - since > AGENT_WAIT_TIMEOUT;
                    (running || expired).then_some((index, running))
                }
                _ => None,
            })
            .collect();

        for (index, running) in ready {
            let Some(connection_id) = self.tabs[index].connection.clone() else {
                continue;
            };
            let Some(connection) = self.config.connection(&connection_id).cloned() else {
                continue;
            };
            if running {
                let tab = self.build_tab(&connection, ctx, now);
                self.tabs[index].state = tab.state;
            } else {
                self.tabs[index].state = TabState::Failed(format!(
                    "L'agent Proton Pass n'a pas demarre en {AGENT_WAIT_TIMEOUT:.0} s.\n\
                     Verifiez que `{}` est installe et que la session est deverrouillee.",
                    self.config.proton_pass.binary
                ));
            }
        }
    }

    /// Traite les reponses du thread Proton Pass.
    fn poll_pass(&mut self, now: f64) {
        while let Some(response) = self.pass.try_recv() {
            match response {
                PassResponse::Probe(Ok(version)) => {
                    self.pass_status = PassStatus::Available(version);
                    let cli = self.cli();
                    self.pass.send(PassRequest::Vaults(cli));
                }
                PassResponse::Probe(Err(err)) => {
                    self.pass_status = PassStatus::Missing(err);
                }
                PassResponse::Vaults(Ok(vaults)) => {
                    if self.selected_vault.is_none() {
                        self.selected_vault = self
                            .config
                            .proton_pass
                            .default_vault
                            .clone()
                            .or_else(|| vaults.first().map(|v| v.name.clone()));
                    }
                    self.vaults = vaults;
                }
                PassResponse::Vaults(Err(err)) => {
                    self.toast(format!("Coffres illisibles: {err}"), ToastKind::Error, now);
                }
                PassResponse::Items(vault, result) => {
                    self.loading_items.remove(&vault);
                    match result {
                        Ok(items) => {
                            self.items.insert(vault, items);
                        }
                        Err(err) => self.toast(
                            format!("Items de « {vault} » illisibles: {err}"),
                            ToastKind::Error,
                            now,
                        ),
                    }
                }
                PassResponse::LoadAgent(vault, Ok(summary)) => {
                    let summary = summary
                        .lines()
                        .next()
                        .unwrap_or("cles chargees")
                        .to_string();
                    self.toast(format!("{vault}: {summary}"), ToastKind::Success, now);
                }
                PassResponse::LoadAgent(vault, Err(err)) => {
                    self.toast(format!("{vault}: {err}"), ToastKind::Error, now);
                }
            }
        }
    }

    /// Consomme les evenements des sessions terminal.
    fn poll_terminals(&mut self, ctx: &egui::Context) {
        let mut clipboard: Option<String> = None;
        for tab in &mut self.tabs {
            if let TabState::Running(session) = &mut tab.state {
                let update = session.pump();
                if update.title_changed && !session.title.is_empty() {
                    tab.subtitle = session.title.clone();
                }
                if let Some(text) = update.copy_to_clipboard {
                    clipboard = Some(text);
                }
                if update.bell {
                    tab.bell = true;
                }
            }
        }
        if let Some(text) = clipboard {
            ctx.copy_text(text);
        }
    }

    fn apply_actions(&mut self, ctx: &egui::Context, now: f64) {
        for action in std::mem::take(&mut self.actions) {
            match action {
                Action::OpenConnection(id) => self.open_connection(&id, ctx, now),
                Action::OpenLocalShell => {
                    let spec = CommandSpec::login_shell();
                    let title = "Terminal local".to_string();
                    let subtitle = spec.program.clone();
                    let tab = self.spawn_tab(title, subtitle, None, spec, Vec::new(), ctx);
                    self.push_tab(tab);
                }
                Action::NewConnection(folder) => {
                    let mut connection = Connection {
                        folder,
                        ..Default::default()
                    };
                    connection.proton =
                        self.config
                            .proton_pass
                            .default_vault
                            .clone()
                            .map(|vault| ProtonRef {
                                vault,
                                ..Default::default()
                            });
                    self.editor = Some(ui::editor::ConnectionEditor::new(connection, true));
                }
                Action::EditConnection(id) => {
                    if let Some(connection) = self.config.connection(&id).cloned() {
                        self.editor = Some(ui::editor::ConnectionEditor::new(connection, false));
                    }
                }
                Action::AskDeleteConnection(id) => self.pending_delete = Some(id),
                Action::DeleteConnection(id) => {
                    self.config.remove_connection(&id);
                    self.save_config();
                    self.pending_delete = None;
                    self.toast("Connexion supprimee", ToastKind::Info, now);
                }
                Action::MoveConnection { connection, folder } => {
                    if let Some(slot) = self.config.connection_mut(&connection) {
                        slot.folder = folder;
                        self.save_config();
                    }
                }
                Action::ToggleFavorite(id) => {
                    if let Some(connection) = self.config.connection_mut(&id) {
                        connection.favorite = !connection.favorite;
                        self.save_config();
                    }
                }
                Action::NewFolder(name) => {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        // Nom vide: c'est le bouton de la barre laterale, on
                        // ouvre la boite de dialogue de saisie.
                        self.new_folder_name = Some(String::new());
                    } else {
                        let folder = Folder::new(name);
                        self.expanded_folders.insert(folder.id.clone());
                        self.config.folders.push(folder);
                        self.save_config();
                    }
                }
                Action::DeleteFolder(id) => {
                    self.config.remove_folder(&id);
                    self.save_config();
                }
                Action::CloseTab(id) => {
                    if let Some(index) = self.tab_index(&id) {
                        self.close_tab(index);
                    }
                }
                Action::SelectTab(id) => {
                    if let Some(index) = self.tab_index(&id) {
                        self.active_tab = Some(index);
                        self.tabs[index].bell = false;
                    }
                }
                Action::ShowHome => self.active_tab = None,
                Action::RefreshVaults => {
                    let cli = self.cli();
                    self.pass_status = PassStatus::Probing;
                    self.pass.send(PassRequest::Probe(cli));
                }
                Action::LoadItems(vault) => {
                    if !vault.is_empty() && self.loading_items.insert(vault.clone()) {
                        let cli = self.cli();
                        self.pass.send(PassRequest::Items(cli, vault));
                    }
                }
                Action::StartAgent(vault) => match self.agents.ensure(&vault) {
                    AgentState::Failed(err) => {
                        self.toast(err, ToastKind::Error, now);
                    }
                    _ => self.toast(
                        format!("Agent « {vault} » en cours de demarrage"),
                        ToastKind::Info,
                        now,
                    ),
                },
                Action::StopAgent(vault) => {
                    self.agents.stop(&vault);
                    self.toast(format!("Agent « {vault} » arrete"), ToastKind::Info, now);
                }
                Action::LoadIntoExistingAgent(vault) => {
                    let cli = self.cli();
                    self.pass.send(PassRequest::LoadAgent(cli, vault));
                }
                Action::AssignItem {
                    connection,
                    vault,
                    item,
                } => {
                    if let Some(slot) = self.config.connection_mut(&connection) {
                        let field = slot.proton.as_ref().and_then(|p| p.field.clone());
                        slot.proton = Some(ProtonRef { vault, item, field });
                        self.save_config();
                        self.toast("Item Proton Pass associe", ToastKind::Success, now);
                    }
                }
                Action::ConnectWithoutAgent(tab_id) => {
                    let target = self
                        .tab_index(&tab_id)
                        .and_then(|index| self.tabs[index].connection.clone().map(|c| (index, c)));
                    if let Some((index, connection_id)) = target {
                        if let Some(connection) = self.config.connection(&connection_id).cloned() {
                            let tab = self.build_tab(&connection, ctx, now);
                            self.tabs[index].state = tab.state;
                        }
                    }
                }
                Action::Toast(message, kind) => self.toast(message, kind, now),
            }
        }
    }

    /// Raccourcis globaux, actifs quand aucune fenetre modale n'est ouverte.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.editor.is_some() || self.pending_delete.is_some() {
            return;
        }
        let terminal_focused = self.active_tab.is_some();
        ctx.input_mut(|input| {
            use egui::{Key, Modifiers};
            let ctrl_shift = Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            };
            if input.consume_key(ctrl_shift, Key::F) {
                self.focus_search = true;
            }
            // Ctrl+F seul ne serait pas transmis au shell distant: on ne le
            // capture que hors terminal.
            if !terminal_focused && input.consume_key(Modifiers::CTRL, Key::F) {
                self.focus_search = true;
            }
            if input.consume_key(ctrl_shift, Key::T) {
                self.actions.push(Action::OpenLocalShell);
            }
            if input.consume_key(ctrl_shift, Key::P) {
                self.show_vault_panel = !self.show_vault_panel;
            }
            if input.consume_key(ctrl_shift, Key::Tab) && !self.tabs.is_empty() {
                let next = self
                    .active_tab
                    .map(|i| (i + 1) % self.tabs.len())
                    .unwrap_or(0);
                self.actions
                    .push(Action::SelectTab(self.tabs[next].id.clone()));
            }
        });
    }

    fn prune_toasts(&mut self, now: f64) {
        self.toasts.retain(|toast| toast.expires_at > now);
    }
}

/// URI d'un item, en visant le champ `password` par defaut.
fn uri_with_default_field(reference: &ProtonRef) -> String {
    match reference
        .field
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        Some(_) => reference.uri(),
        None => format!("pass://{}/{}/password", reference.vault, reference.item),
    }
}

impl eframe::App for SshpassApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);

        self.agents.poll();
        self.poll_pass(now);
        self.poll_terminals(&ctx);
        self.advance_waiting_tabs(&ctx, now);
        self.prune_toasts(now);
        self.handle_shortcuts(&ctx);

        // Les panneaux se posent dans l'ordre: barres, puis zone centrale.
        ui::toolbar::show(self, ui);
        ui::sidebar::show(self, ui);
        // Appele meme referme: c'est le panneau lui-meme qui anime sa sortie
        // et sa rentree, il doit donc rester dans la boucle de rendu.
        ui::vault::show(self, ui);
        ui::tabs::show(self, ui);
        // Fenetres et calques flottants: toujours rattaches au contexte.
        ui::editor::show(self, &ctx);
        ui::dialogs::show(self, &ctx, now);
        ui::toasts::show(self, &ctx);

        self.apply_actions(&ctx, now);

        // Un appel `pass-cli` ou un agent en cours de demarrage n'emet aucun
        // evenement egui: on programme un reveil pour ne pas figer l'affichage.
        let waiting = self.pass.is_busy()
            || self
                .tabs
                .iter()
                .any(|t| matches!(t.state, TabState::WaitingAgent { .. }));
        if waiting || !self.toasts.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tabs.clear();
        self.agents.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_uri_defaults_to_password_field() {
        let reference = ProtonRef {
            vault: "V".into(),
            item: "I".into(),
            field: None,
        };
        assert_eq!(uri_with_default_field(&reference), "pass://V/I/password");

        let explicit = ProtonRef {
            vault: "V".into(),
            item: "I".into(),
            field: Some("secret".into()),
        };
        assert_eq!(uri_with_default_field(&explicit), "pass://V/I/secret");

        let blank = ProtonRef {
            vault: "V".into(),
            item: "I".into(),
            field: Some("  ".into()),
        };
        assert_eq!(uri_with_default_field(&blank), "pass://V/I/password");
    }

    #[test]
    fn pass_status_availability() {
        assert!(PassStatus::Available("1.0".into()).is_available());
        assert!(!PassStatus::Probing.is_available());
        assert!(!PassStatus::Missing("boum".into()).is_available());
    }
}
