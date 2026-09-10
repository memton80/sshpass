//! Etat global de l'application et boucle eframe.

use std::collections::{HashMap, HashSet};

use crate::config::{self, AgentMode, AuthMethod, Config, Connection, Folder, ProtonRef};
use crate::pass::{
    AgentManager, AgentState, Item, LoginDraft, LoginManager, LoginOutcome, PassCli, PassFailure,
    PassRequest, PassResponse, PassWorker, Secret, Session, SshKeySource, Vault,
};
use crate::term::command::{self, CommandSpec, SessionContext};
use crate::term::{TermSize, TerminalSession};
use crate::theme::{self, Palette};
use crate::ui;

/// Delai au-dela duquel on cesse d'attendre l'agent Proton Pass.
///
/// Public: l'ecran d'attente d'un onglet en fait une jauge, pour que le delai
/// affiche soit celui reellement applique.
pub const AGENT_WAIT_TIMEOUT: f64 = 20.0;
/// Duree d'affichage d'une notification.
const TOAST_DURATION: f64 = 5.0;
/// Intervalle entre deux verifications spontanees de la session Proton Pass.
///
/// Une session expire sans prevenir personne. Sans cette ronde, l'application
/// ne s'en apercevrait qu'au premier appel qui echoue — c'est-a-dire au pire
/// moment, celui ou l'on essaie d'ouvrir une connexion.
const SESSION_CHECK_INTERVAL: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    /// Identifiant propre a la notification.
    ///
    /// Son animation d'entree doit suivre la notification, pas sa position
    /// dans la pile: sans identifiant, refermer la premiere ferait rejouer
    /// l'animation de toutes celles qui remontent d'un cran.
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
    pub expires_at: f64,
}

impl Toast {
    pub fn new(message: impl Into<String>, kind: ToastKind, expires_at: f64) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            message: message.into(),
            kind,
            expires_at,
        }
    }
}

/// Disponibilite de `pass-cli`, binaire **et** session.
///
/// Les deux sont distingues parce que les remedes n'ont rien a voir: un
/// binaire absent s'installe, une session fermee se rouvre toute seule
/// (cf. `pass::login`), une session verrouillee reclame un code que seul
/// l'utilisateur connait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassStatus {
    Probing,
    /// Binaire present et session ouverte: tout est utilisable.
    Ready {
        version: String,
        account: String,
    },
    /// Binaire present, session fermee ou expiree.
    LoggedOut {
        detail: String,
    },
    /// Session authentifiee mais verrouillee par un code.
    Locked {
        detail: String,
    },
    /// Binaire introuvable, ou injoignable.
    Missing(String),
}

impl PassStatus {
    /// Vrai quand une commande Proton Pass a une chance d'aboutir.
    pub fn is_available(&self) -> bool {
        matches!(self, PassStatus::Ready { .. })
    }

    /// Libelle court pour l'infobulle de la barre d'outils.
    pub fn summary(&self) -> String {
        match self {
            PassStatus::Probing => "Detection en cours".into(),
            PassStatus::Ready { version, account } if account.is_empty() => {
                format!("Session ouverte — {version}")
            }
            PassStatus::Ready { version, account } => {
                format!("Session ouverte: {account} — {version}")
            }
            PassStatus::LoggedOut { detail } => format!("Session fermee: {detail}"),
            PassStatus::Locked { detail } => format!("Session verrouillee: {detail}"),
            PassStatus::Missing(err) => err.clone(),
        }
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

    pub fn session_mut(&mut self) -> Option<&mut TerminalSession> {
        match &mut self.state {
            TabState::Running(session) => Some(session),
            _ => None,
        }
    }

    /// Vrai tant que la connexion n'a rien donne a voir.
    ///
    /// Couvre les deux attentes que l'utilisateur subit sans rien pouvoir
    /// faire: l'agent Proton Pass qui demarre, puis `ssh` qui negocie — ce
    /// dernier reste muet jusqu'a la premiere sortie du distant. C'est ce que
    /// signale l'animation de chargement de l'onglet.
    pub fn is_connecting(&self) -> bool {
        match &self.state {
            TabState::WaitingAgent { .. } => true,
            TabState::Running(session) => session.is_connecting(),
            TabState::Failed(_) => false,
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
    /// Relance `pass-cli login` a la demande, meme si une tentative a deja
    /// echoue: c'est le bouton de rattrapage du panneau lateral.
    ReconnectPass,
    /// Ecrit dans un coffre le mot de passe saisi dans la fiche de connexion.
    ///
    /// Le secret est porte par un `Secret`: la variante n'est ni clonable ni
    /// affichable, et la valeur est effacee des que la requete est consommee.
    SaveProtonLogin {
        connection: String,
        vault: String,
        item: String,
        user: String,
        host: String,
        port: u16,
        password: Secret,
        /// L'item existe deja: mettre a jour au lieu d'en creer un doublon.
        replace: bool,
    },
    /// Range une cle SSH dans un coffre: import d'un fichier, ou generation.
    SaveProtonSshKey {
        connection: String,
        vault: String,
        item: String,
        source: SshKeySource,
    },
    Toast(String, ToastKind),
}

pub struct SshpassApp {
    pub config: Config,
    pub palette: Palette,
    pub agents: AgentManager,
    pub pass: PassWorker,
    pub pass_status: PassStatus,
    /// Flux `pass-cli login` en cours, s'il y en a un.
    pub login: LoginManager,
    /// Horodatage de la derniere verification de session.
    pub last_session_check: f64,
    /// Une reconnexion automatique reste permise.
    ///
    /// Desarme des qu'on en lance une, et rearme quand la session est
    /// confirmee ouverte: une tentative abandonnee ne doit pas rouvrir un
    /// onglet de navigateur a chaque ronde.
    pub auto_login_armed: bool,
    pub vaults: Vec<Vault>,
    pub items: HashMap<String, Vec<Item>>,
    pub loading_items: HashSet<String>,
    /// Connexion dont un secret est en cours d'ecriture dans un coffre.
    /// Sert a neutraliser les boutons: deux envois de suite creeraient deux
    /// items.
    pub writing_secret: Option<String>,

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

        // Une session ferme son script askpass en partant; un plantage, non.
        crate::pass::sweep_stale_askpass_scripts();

        let agents = AgentManager::new(
            &config.proton_pass.binary,
            config.proton_pass.agent_mode,
            config.proton_pass.refresh_interval,
            config.security.allow_temp_runtime_dir,
        );
        let mut pass = PassWorker::spawn();
        pass.send(PassRequest::Probe(PassCli::new(&config.proton_pass.binary)));

        Self {
            config,
            palette: theme::DARK,
            agents,
            pass,
            pass_status: PassStatus::Probing,
            login: LoginManager::new(),
            last_session_check: 0.0,
            auto_login_armed: true,
            vaults: Vec::new(),
            items: HashMap::new(),
            loading_items: HashSet::new(),
            writing_secret: None,
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
        self.toasts
            .push(Toast::new(message, kind, now + TOAST_DURATION));
    }

    pub fn save_config(&mut self) {
        if let Err(err) = config::save(&self.config) {
            log::error!("sauvegarde impossible: {err}");
            self.toasts.push(Toast::new(
                format!("Sauvegarde impossible: {err}"),
                ToastKind::Error,
                f64::MAX,
            ));
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

        // Une connexion adossee au coffre n'a aucune chance d'aboutir sans
        // session: l'agent ne demarrerait pas, ou le script askpass ne
        // trouverait rien. Autant le dire tout de suite et lancer la
        // reconnexion, plutot que d'ouvrir un onglet condamne a echouer.
        if vault.is_some() {
            match &self.pass_status {
                PassStatus::LoggedOut { .. } => {
                    // L'utilisateur vient d'agir: la reconnexion est de
                    // nouveau permise, meme apres une tentative abandonnee.
                    self.auto_login_armed = true;
                    self.reconnect_if_allowed(now);
                    self.toast(
                        format!(
                            "Session Proton Pass fermee: reconnectez-vous, puis rouvrez « {} ».",
                            connection.display_name()
                        ),
                        ToastKind::Error,
                        now,
                    );
                    return;
                }
                PassStatus::Locked { .. } => {
                    self.toast(
                        "Session Proton Pass verrouillee: `pass-cli session unlock`, \
                         puis reessayez.",
                        ToastKind::Error,
                        now,
                    );
                    return;
                }
                _ => {}
            }
        }

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

        if connection.auth.uses_stored_password() {
            match connection.proton.as_ref().filter(|p| p.is_complete()) {
                Some(reference) => {
                    let uri = uri_with_default_field(reference);
                    // L'identifiant est tire ici, et pas repris de la
                    // connexion: deux onglets ouverts sur la meme machine
                    // doivent avoir chacun leur script, sinon le second
                    // ecrase la reference du premier (cf. `write_askpass_script`).
                    let session_id = config::new_id();
                    match crate::pass::write_askpass_script(
                        &self.config.proton_pass.binary,
                        &uri,
                        &session_id,
                        self.config.security.allow_temp_runtime_dir,
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
            crate::term::ClipboardPolicy::from_config(&self.config.security),
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
                PassResponse::Probe(Ok(probe)) => match probe.session {
                    Session::Open { account } => {
                        self.pass_status = PassStatus::Ready {
                            version: probe.version,
                            account,
                        };
                        // Session confirmee: une future coupure aura de
                        // nouveau droit a une reconnexion automatique.
                        self.auto_login_armed = true;
                        let cli = self.cli();
                        self.pass.send(PassRequest::Vaults(cli));
                    }
                    Session::Closed(detail) => {
                        self.pass_status = PassStatus::LoggedOut { detail };
                        self.reconnect_if_allowed(now);
                    }
                    Session::Locked(detail) => {
                        // Rien d'automatique n'est possible: le code de
                        // verrouillage n'est connu que de l'utilisateur.
                        self.pass_status = PassStatus::Locked { detail };
                        self.toast(
                            "Session Proton Pass verrouillee: `pass-cli session unlock` \
                             dans un terminal.",
                            ToastKind::Error,
                            now,
                        );
                    }
                },
                PassResponse::Probe(Err(failure)) => {
                    self.pass_status = PassStatus::Missing(failure.message);
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
                PassResponse::Vaults(Err(failure)) => {
                    let message = format!("Coffres illisibles: {failure}");
                    self.note_failure(&failure, now);
                    self.toast(message, ToastKind::Error, now);
                }
                PassResponse::Items(vault, result) => {
                    self.loading_items.remove(&vault);
                    match result {
                        Ok(items) => {
                            self.items.insert(vault, items);
                        }
                        Err(failure) => {
                            // Le coffre est marque comme lu, meme vide: la
                            // fiche de connexion redemande la lecture tant
                            // qu'elle n'a rien, et un echec relancerait sinon
                            // `pass-cli` a chaque frame.
                            self.items.entry(vault.clone()).or_default();
                            let message = format!("Items de « {vault} » illisibles: {failure}");
                            self.note_failure(&failure, now);
                            self.toast(message, ToastKind::Error, now);
                        }
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
                PassResponse::LoadAgent(vault, Err(failure)) => {
                    let message = format!("{vault}: {failure}");
                    self.note_failure(&failure, now);
                    self.toast(message, ToastKind::Error, now);
                }
                PassResponse::SavedLogin {
                    connection,
                    vault,
                    item,
                    result,
                } => {
                    self.writing_secret = None;
                    match result {
                        Ok(summary) => {
                            // Le champ reste vide: `pass-cli item view` lit
                            // `password` par defaut, l'imposer n'apporterait rien.
                            self.attach_item(&connection, &vault, &item, None);
                            self.toast(summary, ToastKind::Success, now);
                            self.refresh_vault_items(vault);
                        }
                        Err(failure) => {
                            let message =
                                format!("Enregistrement dans « {vault} » impossible: {failure}");
                            self.note_failure(&failure, now);
                            self.toast(message, ToastKind::Error, now);
                        }
                    }
                }
                PassResponse::SavedSshKey {
                    connection,
                    vault,
                    item,
                    result,
                } => {
                    self.writing_secret = None;
                    match result {
                        Ok(summary) => {
                            // La cle vit desormais dans le coffre: c'est l'agent
                            // Proton Pass qui la sert, plus un fichier local.
                            self.attach_item(&connection, &vault, &item, Some(AuthMethod::Agent));
                            self.toast(summary, ToastKind::Success, now);
                            self.toast(
                                format!(
                                    "Ajoutez la cle publique de « {item} » sur le serveur \
                                     (Proton Pass, onglet Cle SSH) avant de vous connecter."
                                ),
                                ToastKind::Info,
                                now,
                            );
                            self.refresh_vault_items(vault);
                        }
                        Err(failure) => {
                            let message =
                                format!("Cle SSH non enregistree dans « {vault} »: {failure}");
                            self.note_failure(&failure, now);
                            self.toast(message, ToastKind::Error, now);
                        }
                    }
                }
            }
        }
    }

    /// Tire les consequences d'un appel `pass-cli` qui a echoue.
    ///
    /// Une session fermee ne se voit pas toujours a la ronde suivante: c'est
    /// souvent un appel ordinaire qui la revele en premier. On corrige alors
    /// l'etat affiche et on enclenche la reconnexion sans attendre.
    fn note_failure(&mut self, failure: &PassFailure, now: f64) {
        if !failure.session_closed {
            return;
        }
        self.pass_status = PassStatus::LoggedOut {
            detail: failure.message.clone(),
        };
        self.reconnect_if_allowed(now);
    }

    /// Lance une reconnexion si la configuration l'autorise et qu'aucune
    /// tentative n'a deja eu lieu.
    fn reconnect_if_allowed(&mut self, now: f64) {
        if !self.config.proton_pass.auto_login || !self.auto_login_armed {
            return;
        }
        self.begin_login(now);
    }

    /// Demarre `pass-cli login` et previent l'utilisateur.
    ///
    /// Le flux est web: `pass-cli` imprime une adresse, que `pass::login`
    /// ouvre dans le navigateur des qu'elle parait. Aucun identifiant ne passe
    /// par sshpass-gui.
    fn begin_login(&mut self, now: f64) {
        if self.login.is_running() {
            return;
        }
        self.auto_login_armed = false;
        let cli = self.cli();
        match self.login.start(&cli) {
            Ok(()) => self.toast(
                "Session Proton Pass fermee: reconnexion, le navigateur va s'ouvrir.",
                ToastKind::Info,
                now,
            ),
            Err(err) => {
                log::error!("reconnexion Proton Pass impossible: {err}");
                self.toast(
                    format!("Reconnexion impossible: {err}"),
                    ToastKind::Error,
                    now,
                );
            }
        }
    }

    /// Fait avancer le flux de reconnexion et sonde de nouveau a la fin.
    fn poll_login(&mut self, now: f64) {
        match self.login.poll() {
            Some(LoginOutcome::Succeeded) => {
                self.toast("Session Proton Pass rouverte.", ToastKind::Success, now);
                // La reussite du processus ne prouve pas que la session soit
                // exploitable: on la resonde plutot que de l'affirmer.
                self.probe_session(now);
            }
            Some(LoginOutcome::Failed(detail)) => {
                self.pass_status = PassStatus::LoggedOut {
                    detail: detail.clone(),
                };
                self.toast(
                    format!("Reconnexion Proton Pass abandonnee: {detail}"),
                    ToastKind::Error,
                    now,
                );
            }
            None => {}
        }
    }

    /// Verifie periodiquement que la session tient toujours.
    fn check_session(&mut self, now: f64) {
        let idle = now - self.last_session_check >= SESSION_CHECK_INTERVAL;
        let busy = self.login.is_running()
            || matches!(self.pass_status, PassStatus::Probing)
            || self.pass.is_busy();
        if idle && !busy {
            self.probe_session(now);
        }
    }

    /// Envoie une detection complete (binaire + session).
    fn probe_session(&mut self, now: f64) {
        self.last_session_check = now;
        let cli = self.cli();
        self.pass.send(PassRequest::Probe(cli));
    }

    /// Relit un coffre dont le contenu vient de changer.
    ///
    /// Sans cela l'item tout juste cree n'apparaitrait ni dans les suggestions
    /// de la fiche ni dans le panneau lateral, et un second enregistrement
    /// creerait un doublon au lieu d'une mise a jour.
    fn refresh_vault_items(&mut self, vault: String) {
        self.items.remove(&vault);
        self.actions.push(Action::LoadItems(vault));
    }

    /// Rattache une connexion a un item qui vient d'etre ecrit dans un coffre.
    ///
    /// La fiche ouverte est servie en premier: c'est elle que l'utilisateur a
    /// sous les yeux et c'est elle qui sera enregistree. La connexion deja
    /// persistee est mise a jour elle aussi, pour qu'un abandon de la fiche ne
    /// perde pas le lien vers un item pourtant bien cree.
    fn attach_item(&mut self, connection: &str, vault: &str, item: &str, auth: Option<AuthMethod>) {
        if let Some(editor) = self
            .editor
            .as_mut()
            .filter(|editor| editor.connection.id == connection)
        {
            editor.vault_text = vault.to_string();
            editor.item_text = item.to_string();
            if let Some(auth) = auth {
                editor.connection.auth = auth;
            }
            editor.error = None;
        }

        let mut changed = false;
        if let Some(slot) = self.config.connection_mut(connection) {
            let field = slot.proton.as_ref().and_then(|p| p.field.clone());
            slot.proton = Some(ProtonRef {
                vault: vault.to_string(),
                item: item.to_string(),
                field,
            });
            if let Some(auth) = auth {
                slot.auth = auth;
            }
            changed = true;
        }
        if changed {
            self.save_config();
        }
    }

    /// Consomme les evenements des sessions terminal.
    fn poll_terminals(&mut self, ctx: &egui::Context, now: f64) {
        let mut clipboard: Option<String> = None;
        let mut blocked_reads: Vec<String> = Vec::new();
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
                if update.blocked_clipboard_read {
                    blocked_reads.push(tab.title.clone());
                }
            }
        }
        // Une seule fois par session: la sequence peut etre reemise en boucle,
        // et une notification par tentative noierait l'information.
        for title in blocked_reads {
            self.toast(
                format!("« {title} » a demande a lire le presse-papiers: refuse"),
                ToastKind::Error,
                now,
            );
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
                    self.pass_status = PassStatus::Probing;
                    // Un rafraichissement manuel rouvre le droit a une
                    // reconnexion: c'est justement ce que l'utilisateur
                    // demande en cliquant.
                    self.auto_login_armed = true;
                    self.probe_session(now);
                }
                Action::ReconnectPass => self.begin_login(now),
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
                Action::SaveProtonLogin {
                    connection,
                    vault,
                    item,
                    user,
                    host,
                    port,
                    password,
                    replace,
                } => {
                    let draft = LoginDraft::for_ssh(&item, &user, &host, port, password);
                    let cli = self.cli();
                    self.writing_secret = Some(connection.clone());
                    self.pass.send(PassRequest::SaveLogin {
                        cli,
                        vault,
                        draft,
                        connection,
                        replace,
                        allow_argv_fallback: self.config.security.allow_argv_fallback,
                    });
                }
                Action::SaveProtonSshKey {
                    connection,
                    vault,
                    item,
                    source,
                } => {
                    let cli = self.cli();
                    self.writing_secret = Some(connection.clone());
                    self.pass.send(PassRequest::SaveSshKey {
                        cli,
                        vault,
                        title: item,
                        source,
                        connection,
                    });
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
        // Passe par `ProtonRef::uri` pour beneficier du meme encodage des
        // composants, plutot que de recomposer l'URI a la main ici.
        None => ProtonRef {
            vault: reference.vault.clone(),
            item: reference.item.clone(),
            field: Some("password".to_string()),
        }
        .uri(),
    }
}

impl eframe::App for SshpassApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);

        self.agents.poll();
        self.poll_login(now);
        self.check_session(now);
        self.poll_pass(now);
        self.poll_terminals(&ctx, now);
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
            || self.login.is_running()
            || self
                .tabs
                .iter()
                .any(|t| matches!(t.state, TabState::WaitingAgent { .. }));
        if waiting || !self.toasts.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        } else {
            // egui ne redessine pas une fenetre inerte: sans reveil programme,
            // la ronde de session n'aurait lieu qu'au gre des interactions et
            // une session expirant la nuit ne serait vue qu'au matin.
            let remaining = SESSION_CHECK_INTERVAL - (now - self.last_session_check);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                remaining.clamp(1.0, SESSION_CHECK_INTERVAL),
            ));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tabs.clear();
        self.agents.stop_all();
        self.login.cancel();
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
    fn only_an_open_session_counts_as_available() {
        let ready = PassStatus::Ready {
            version: "1.0".into(),
            account: "alex@proton.me".into(),
        };
        assert!(ready.is_available());
        assert!(ready.summary().contains("alex@proton.me"));

        // Le binaire repond, mais rien n'est utilisable pour autant.
        assert!(!PassStatus::LoggedOut {
            detail: "not logged in".into()
        }
        .is_available());
        assert!(!PassStatus::Locked {
            detail: "session is locked".into()
        }
        .is_available());
        assert!(!PassStatus::Probing.is_available());
        assert!(!PassStatus::Missing("boum".into()).is_available());
    }

    #[test]
    fn summary_stays_readable_without_an_account() {
        let anonymous = PassStatus::Ready {
            version: "pass-cli 1.2".into(),
            account: String::new(),
        };
        assert_eq!(anonymous.summary(), "Session ouverte — pass-cli 1.2");
    }
}
