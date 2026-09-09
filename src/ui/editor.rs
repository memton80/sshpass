//! Fenetre de creation et de modification d'une connexion.
//!
//! C'est ici que le secret d'une connexion **entre** dans Proton Pass: le mot
//! de passe saisi part vers le coffre et n'est jamais garde localement, et une
//! cle SSH y est importee ou generee. La fiche ne conserve ensuite qu'une
//! reference `pass://coffre/item`.

use egui::{ComboBox, Margin, RichText};

use crate::ui::autocomplete::{Autocomplete, Suggestion};

use crate::app::{Action, SshpassApp, ToastKind};
use crate::config::{AuthMethod, Connection, ProtonRef};
use crate::pass::secret::{self, Secret};
use crate::pass::{SshKeySource, SshKeyType};
use crate::ui::{self, pixel};

/// Etat de saisie. Les champs numeriques et les listes sont edites sous forme
/// de texte puis convertis a l'enregistrement: cela evite qu'un champ vide en
/// cours de frappe ne reinitialise la valeur.
pub struct ConnectionEditor {
    pub connection: Connection,
    pub is_new: bool,
    pub port_text: String,
    pub tags_text: String,
    pub options_text: String,
    pub vault_text: String,
    pub item_text: String,
    pub field_text: String,
    /// Mot de passe en attente d'ecriture dans le coffre.
    ///
    /// Volontairement absent de `Connection`: il ne doit ni etre enregistre
    /// dans le TOML ni survivre a la fermeture de la fiche (cf. `Drop`).
    pub password_text: String,
    /// Algorithme propose pour une cle generee par Proton Pass.
    pub key_type: SshKeyType,

    pub error: Option<String>,
}

/// A la fermeture de la fiche, le mot de passe encore saisi est ecrase.
///
/// `String::clear` ne ferait que remettre la longueur a zero; `secret::scrub`
/// remplit le tampon avant de le liberer.
impl Drop for ConnectionEditor {
    fn drop(&mut self) {
        secret::scrub(&mut self.password_text);
    }
}

impl ConnectionEditor {
    pub fn new(connection: Connection, is_new: bool) -> Self {
        let proton = connection.proton.clone().unwrap_or_default();
        Self {
            port_text: connection.port.to_string(),
            tags_text: connection.tags.join(", "),
            options_text: connection.ssh_options.join("\n"),
            vault_text: proton.vault,
            item_text: proton.item,
            field_text: proton.field.unwrap_or_default(),
            password_text: String::new(),
            key_type: SshKeyType::default(),

            error: None,
            connection,
            is_new,
        }
    }

    /// Titre sous lequel l'item sera ecrit dans le coffre.
    ///
    /// Celui saisi s'il y en a un, sinon le nom de la connexion, sinon l'hote:
    /// un item sans titre serait introuvable, et `pass://coffre/` invalide.
    pub fn item_title(&self) -> String {
        [
            &self.item_text,
            &self.connection.name,
            &self.connection.host,
        ]
        .into_iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
    }

    /// Valide la saisie et renvoie la connexion prete a etre enregistree.
    pub fn build(&self) -> Result<Connection, String> {
        let mut connection = self.connection.clone();
        connection.host = connection.host.trim().to_string();
        if connection.host.is_empty() {
            return Err("L'hote est obligatoire.".into());
        }
        connection.name = connection.name.trim().to_string();
        if connection.name.is_empty() {
            connection.name = connection.host.clone();
        }
        connection.user = connection.user.trim().to_string();
        connection.port = self
            .port_text
            .trim()
            .parse::<u16>()
            .map_err(|_| "Le port doit etre un entier entre 1 et 65535.".to_string())?;
        if connection.port == 0 {
            return Err("Le port doit etre superieur a 0.".into());
        }
        connection.tags = self
            .tags_text
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        connection.ssh_options = self
            .options_text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        connection.command = connection
            .command
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty());
        connection.key_file = connection
            .key_file
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty());

        let vault = self.vault_text.trim();
        let item = self.item_text.trim();
        connection.proton = if vault.is_empty() && item.is_empty() {
            None
        } else {
            Some(ProtonRef {
                vault: vault.to_string(),
                item: item.to_string(),
                field: Some(self.field_text.trim().to_string()).filter(|f| !f.is_empty()),
            })
        };

        // Un mot de passe encore dans le champ n'a pas ete ecrit dans le
        // coffre: enregistrer la fiche le detruirait sans prevenir.
        if !self.password_text.trim().is_empty() {
            return Err(
                "Le mot de passe saisi n'est pas encore dans Proton Pass: cliquez sur \
                 « Enregistrer dans Proton Pass », ou videz le champ."
                    .into(),
            );
        }
        if connection.auth == AuthMethod::Password
            && !connection.proton.as_ref().is_some_and(|p| p.is_complete())
        {
            return Err(
                "L'authentification par mot de passe demande un coffre et un item Proton Pass: \
                 saisissez le mot de passe et cliquez sur « Enregistrer dans Proton Pass »."
                    .into(),
            );
        }
        if connection.auth == AuthMethod::KeyFile && connection.key_file.is_none() {
            return Err("Indiquez le chemin du fichier de cle privee.".into());
        }
        Ok(connection)
    }
}

pub fn show(app: &mut SshpassApp, ctx: &egui::Context) {
    if app.editor.is_none() {
        return;
    }
    let palette = app.palette;
    let scale = app.config.ui.pixel_scale as f32;
    let folders: Vec<(String, String)> = app
        .config
        .folders
        .iter()
        .map(|f| (f.id.clone(), f.name.clone()))
        .collect();
    let vaults: Vec<String> = app.vaults.iter().map(|v| v.name.clone()).collect();

    let mut close = false;
    let mut save: Option<Connection> = None;
    let mut delete: Option<String> = None;
    let mut load_items: Option<String> = None;
    // Les ecritures dans le coffre partent par la file d'actions, comme le
    // reste: la fenetre emprunte `app.editor` en mutable et ne peut pas
    // toucher a `app` tant qu'elle est dessinee.
    let mut write_secret: Option<Action> = None;

    // Propositions tirees de ce qui est deja connu: la frappe ne declenche
    // aucune requete.
    let known_hosts = host_suggestions(app);
    let known_users = user_suggestions(app);
    let vault_suggestions: Vec<Suggestion> = app
        .vaults
        .iter()
        .map(|vault| {
            let detail = vault
                .item_count
                .map(|n| format!("{n} items"))
                .unwrap_or_default();
            Suggestion::new(vault.name.clone(), detail)
        })
        .collect();

    let pass_ready = app.pass_status.is_available();

    let editor = app.editor.as_mut().expect("editeur ouvert");
    let title = if editor.is_new {
        "Nouvelle connexion"
    } else {
        "Modifier la connexion"
    };
    let vault_key = editor.vault_text.trim().to_string();
    let items: Vec<crate::pass::Item> = app.items.get(&vault_key).cloned().unwrap_or_default();
    let item_suggestions: Vec<Suggestion> = items
        .iter()
        .map(|item| Suggestion::new(item.title.clone(), item.kind.label().to_string()))
        .collect();

    // Trois etats distincts pour le coffre courant: jamais lu, en cours de
    // lecture, lu (fut-il vide). Le premier ne doit pas etre confondu avec le
    // dernier: sans la liste, impossible de savoir si un enregistrement doit
    // creer un item ou en mettre un a jour.
    let items_known = app.items.contains_key(&vault_key);
    let items_loading = app.loading_items.contains(&vault_key);
    let writing = app.writing_secret.as_deref() == Some(editor.connection.id.as_str());
    let item_title = editor.item_title();
    let item_exists = items.iter().any(|item| item.title.trim() == item_title);

    // Lecture spontanee du coffre a l'ouverture d'une fiche deja remplie:
    // l'utilisateur n'a pas a cliquer sur « Charger » pour que la fiche sache
    // ce que le coffre contient deja.
    if !vault_key.is_empty()
        && !items_known
        && !items_loading
        && vaults.iter().any(|known| known == &vault_key)
    {
        load_items = Some(vault_key.clone());
    }

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(palette.surface)
                .inner_margin(Margin::same(14)),
        )
        .show(ctx, |ui| {
            ui.set_width(520.0);

            // La fiche s'est allongee avec le bloc d'ecriture dans le coffre:
            // le corps defile, mais les boutons d'action restent hors du
            // defilement, toujours atteignables sur un petit ecran.
            let max_height = (ctx.content_rect().height() - 200.0).max(240.0);
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                egui::Grid::new("connection_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Nom");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.connection.name)
                                .hint_text("web-01")
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();

                        ui.label("Hote");
                        Autocomplete::new("editor_host", &known_hosts)
                            .hint("10.0.0.4 ou example.com")
                            .icon(&pixel::SERVER)
                            .show(ui, &mut editor.connection.host, &palette, scale);
                        ui.end_row();

                        ui.label("Utilisateur");
                        Autocomplete::new("editor_user", &known_users)
                            .hint("root")
                            .icon(&pixel::TERMINAL)
                            .show(ui, &mut editor.connection.user, &palette, scale);
                        ui.end_row();

                        ui.label("Port");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.port_text).desired_width(70.0),
                            );
                            ui.add_space(12.0);
                            ui.checkbox(&mut editor.connection.favorite, "Favori");
                        });
                        ui.end_row();

                        ui.label("Dossier");
                        let current = editor
                            .connection
                            .folder
                            .as_ref()
                            .and_then(|id| folders.iter().find(|(fid, _)| fid == id))
                            .map(|(_, name)| name.clone())
                            .unwrap_or_else(|| "(racine)".to_string());
                        ComboBox::from_id_salt("folder")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut editor.connection.folder, None, "(racine)");
                                for (id, name) in &folders {
                                    ui.selectable_value(
                                        &mut editor.connection.folder,
                                        Some(id.clone()),
                                        name,
                                    );
                                }
                            });
                        ui.end_row();

                        ui.label("Tags");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.tags_text)
                                .hint_text("prod, web")
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                    });

                ui::separator(ui, &palette);
                ui::section_title(ui, "Authentification", &palette);
                ui.horizontal(|ui| {
                    for method in AuthMethod::ALL {
                        ui.selectable_value(&mut editor.connection.auth, method, method.label());
                    }
                });
                    // Le champ de mot de passe n'est montre que pour cette
                // methode: change-t-on d'avis qu'il est efface aussitot,
                // plutot que de garder un secret dans un champ invisible.
                if editor.connection.auth != AuthMethod::Password {
                    secret::scrub(&mut editor.password_text);
                }
                if editor.connection.auth == AuthMethod::KeyFile {
                    ui.add_space(6.0);
                    let mut key = editor.connection.key_file.clone().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label("Cle privee");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut key)
                                    .hint_text("~/.ssh/id_ed25519")
                                    .desired_width(f32::INFINITY),
                            )
                            .changed()
                        {
                            editor.connection.key_file = Some(key);
                        }
                    });
                }

                ui::separator(ui, &palette);
                ui.horizontal(|ui| {
                    pixel::icon_two_tone(ui, &pixel::KEY, scale, palette.accent_soft, palette.success);
                    ui.label(
                        RichText::new("PROTON PASS")
                            .color(palette.text_dim)
                            .size(11.0)
                            .strong(),
                    );
                });
                ui.add_space(4.0);

                egui::Grid::new("proton_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Coffre");
                        let response = Autocomplete::new("editor_vault", &vault_suggestions)
                            .hint("SSH Keys")
                            .icon(&pixel::FOLDER)
                            .width(220.0)
                            .show(ui, &mut editor.vault_text, &palette, scale);
                        if response.changed() {
                            // Changer de coffre invalide l'item choisi, et charge la
                            // liste du nouveau coffre des qu'on en reconnait le nom.
                            editor.item_text.clear();
                            let vault = editor.vault_text.trim().to_string();
                            if vaults.iter().any(|known| known == &vault) {
                                load_items = Some(vault);
                            }
                        }
                        ui.end_row();

                        ui.label("Item");
                        ui.horizontal(|ui| {
                            Autocomplete::new("editor_item", &item_suggestions)
                                .hint("Titre de l'item")
                                .icon(&pixel::KEY)
                                .width(220.0)
                                .show(ui, &mut editor.item_text, &palette, scale);
                            if !editor.vault_text.trim().is_empty()
                                && ui
                                    .small_button("Charger")
                                    .on_hover_text("Lire le coffre pour alimenter les suggestions")
                                    .clicked()
                            {
                                load_items = Some(editor.vault_text.trim().to_string());
                            }
                        });
                        ui.end_row();

                        ui.label("Champ");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.field_text)
                                .hint_text("password (par defaut)")
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                    });

                // Les items charges alimentent directement les suggestions du champ
                // ci-dessus: plus besoin d'une liste separee sous la fiche.
                if !item_suggestions.is_empty() {
                    ui.add_space(2.0);
                    ui::hint(
                        ui,
                        &format!("{} items proposes pour ce coffre.", item_suggestions.len()),
                        &palette,
                    );
                }

                // --- Ecriture du secret dans le coffre -------------------------
                //
                // Sans ce bloc, creer une connexion ne remplissait rien du tout
                // dans Proton Pass: la fiche ne savait que pointer vers un item
                // deja existant. C'est ici que le secret y entre.
                ui.add_space(6.0);
                let blocker = write_blocker(
                    pass_ready,
                    &vault_key,
                    &item_title,
                    items_known,
                    items_loading,
                );
                match editor.connection.auth {
                    AuthMethod::Password => {
                        ui::section_title(ui, "Mot de passe", &palette);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut editor.password_text)
                                    .password(true)
                                    .hint_text("mot de passe du serveur")
                                    .desired_width(220.0),
                            );
                            let label = if item_exists {
                                "Mettre a jour dans Proton Pass"
                            } else {
                                "Enregistrer dans Proton Pass"
                            };
                            let ready =
                                blocker.is_none() && !writing && !editor.password_text.is_empty();
                            if ui
                                .add_enabled(ready, egui::Button::new(label))
                                .on_disabled_hover_text(blocker.clone().unwrap_or_else(|| {
                                    "Saisissez le mot de passe a ranger dans le coffre.".to_string()
                                }))
                                .clicked()
                            {
                                // `Secret::take` vide le champ au passage: la
                                // fiche ne garde pas la valeur qu'elle envoie.
                                write_secret = Some(Action::SaveProtonLogin {
                                    connection: editor.connection.id.clone(),
                                    vault: vault_key.clone(),
                                    item: item_title.clone(),
                                    user: editor.connection.user.trim().to_string(),
                                    host: editor.connection.host.trim().to_string(),
                                    port: editor.port_text.trim().parse().unwrap_or(22),
                                    password: Secret::take(&mut editor.password_text),
                                    replace: item_exists,
                                });
                            }
                        });
                        write_status(ui, &palette, blocker.as_deref(), writing);
                        if blocker.is_none() && !writing {
                            let action = if item_exists { "mis a jour" } else { "cree" };
                            ui::hint(
                                ui,
                                &format!(
                                    "« {item_title} » sera {action} dans « {vault_key} », avec \
                                     l'utilisateur et l'URL ssh:// de cette connexion."
                                ),
                                &palette,
                            );
                            if item_exists {
                                // Honnetete sur la seule fuite possible: la mise a
                                // jour n'a pas d'equivalent par entree standard.
                                ui.label(
                                    RichText::new(
                                        "Mise a jour: pass-cli n'accepte la valeur que sur sa ligne \
                                         de commande, brievement visible des autres processus. Une \
                                         creation, elle, passe par l'entree standard.",
                                    )
                                    .size(10.0)
                                    .color(palette.warning),
                                );
                            }
                        }
                    }
                    AuthMethod::Agent | AuthMethod::KeyFile => {
                        ui::section_title(ui, "Cle SSH", &palette);
                        let key_file = editor
                            .connection
                            .key_file
                            .clone()
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        // Une cle ne se met pas a jour: `pass-cli` n'a que
                        // `create`. Reutiliser un titre existant ferait donc un
                        // doublon, et `pass://coffre/titre` ne saurait plus lequel
                        // designer — mieux vaut refuser et demander un autre titre.
                        let blocker = blocker.clone().or_else(|| {
                            item_exists.then(|| {
                                format!(
                                    "« {item_title} » existe deja dans « {vault_key} »: donnez un \
                                     autre titre d'item pour y ranger une nouvelle cle."
                                )
                            })
                        });
                        ui.horizontal(|ui| {
                            ComboBox::from_id_salt("ssh_key_type")
                                .selected_text(editor.key_type.label())
                                .width(110.0)
                                .show_ui(ui, |ui| {
                                    for kind in SshKeyType::ALL {
                                        ui.selectable_value(&mut editor.key_type, kind, kind.label());
                                    }
                                });
                            let ready = blocker.is_none() && !writing;
                            if ui
                                .add_enabled(ready, egui::Button::new("Generer dans Proton Pass"))
                                .on_hover_text(
                                    "La cle privee nait dans le coffre: elle ne touche pas le disque.",
                                )
                                .on_disabled_hover_text(blocker.clone().unwrap_or_default())
                                .clicked()
                            {
                                write_secret = Some(Action::SaveProtonSshKey {
                                    connection: editor.connection.id.clone(),
                                    vault: vault_key.clone(),
                                    item: item_title.clone(),
                                    source: SshKeySource::Generate {
                                        key_type: editor.key_type,
                                        comment: editor.connection.target(),
                                    },
                                });
                            }
                        });
                        if !key_file.is_empty() {
                            let ready = blocker.is_none() && !writing;
                            if ui
                                .add_enabled(
                                    ready,
                                    egui::Button::new(format!("Importer {key_file} dans Proton Pass")),
                                )
                                .on_hover_text(
                                    "sshpass-gui ne lit pas le fichier: pass-cli en recoit le chemin.",
                                )
                                .on_disabled_hover_text(blocker.clone().unwrap_or_default())
                                .clicked()
                            {
                                write_secret = Some(Action::SaveProtonSshKey {
                                    connection: editor.connection.id.clone(),
                                    vault: vault_key.clone(),
                                    item: item_title.clone(),
                                    source: SshKeySource::Import(std::path::PathBuf::from(
                                        expand_home(&key_file),
                                    )),
                                });
                            }
                        }
                        write_status(ui, &palette, blocker.as_deref(), writing);
                        if blocker.is_none() && !writing {
                            ui::hint(
                                ui,
                                &format!(
                                    "La cle sera rangee dans « {vault_key} » sous « {item_title} », \
                                     puis servie par l'agent du coffre. Sa cle publique est a \
                                     deposer sur le serveur.",
                                ),
                                &palette,
                            );
                        }
                    }
                }

                ui::separator(ui, &palette);
                // Repliees par defaut: la fiche tient ainsi entierement dans la
                // fenetre, boutons d'action compris.
                egui::CollapsingHeader::new("Options avancees")
                    .default_open(false)
                    .show(ui, |ui| {
                        let mut remote = editor.connection.command.clone().unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label("Commande");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut remote)
                                        .hint_text("laisser vide pour un shell")
                                        .desired_width(f32::INFINITY),
                                )
                                .changed()
                            {
                                editor.connection.command = Some(remote);
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Options ssh (-o), une par ligne")
                                .size(11.0)
                                .color(palette.text_dim),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut editor.options_text)
                                .hint_text("ServerAliveInterval=30")
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        );
                    });
                });

            if let Some(error) = &editor.error {
                ui.add_space(8.0);
                ui.label(RichText::new(error).color(palette.danger).size(12.0));
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Enregistrer").clicked() {
                    match editor.build() {
                        Ok(connection) => {
                            save = Some(connection);
                            close = true;
                        }
                        Err(message) => editor.error = Some(message),
                    }
                }
                if ui.button("Annuler").clicked() {
                    close = true;
                }
                if !editor.is_new {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Supprimer").color(palette.danger))
                            .clicked()
                        {
                            delete = Some(editor.connection.id.clone());
                            close = true;
                        }
                    });
                }
            });
        });

    if let Some(action) = write_secret {
        app.actions.push(action);
    }
    if let Some(vault) = load_items {
        app.actions.push(Action::LoadItems(vault));
    }
    if let Some(connection) = save {
        let is_new = app.config.connection(&connection.id).is_none();
        app.config.upsert_connection(connection);
        app.save_config();
        let message = if is_new {
            "Connexion creee"
        } else {
            "Connexion enregistree"
        };
        app.actions
            .push(Action::Toast(message.into(), ToastKind::Success));
    }
    if let Some(id) = delete {
        app.actions.push(Action::AskDeleteConnection(id));
    }
    if close {
        app.editor = None;
    }
}

/// Hotes deja utilises, les plus recemment ouverts en tete.
fn host_suggestions(app: &SshpassApp) -> Vec<Suggestion> {
    let mut connections: Vec<&Connection> = app.config.connections.iter().collect();
    connections.sort_by_key(|c| std::cmp::Reverse(c.last_used));
    connections
        .iter()
        .filter(|c| !c.host.trim().is_empty())
        .map(|c| {
            let detail = c
                .folder
                .as_ref()
                .and_then(|id| app.config.folders.iter().find(|f| &f.id == id))
                .map(|f| f.name.clone())
                .unwrap_or_default();
            Suggestion::new(c.host.clone(), detail)
        })
        .collect()
}

/// Utilisateurs deja employes, les plus frequents en tete.
fn user_suggestions(app: &SshpassApp) -> Vec<Suggestion> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for connection in &app.config.connections {
        let user = connection.user.trim();
        if user.is_empty() {
            continue;
        }
        match counts.iter_mut().find(|(name, _)| name == user) {
            Some((_, count)) => *count += 1,
            None => counts.push((user.to_string(), 1)),
        }
    }
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    counts
        .into_iter()
        .map(|(user, count)| {
            if count > 1 {
                Suggestion::new(user, format!("{count} connexions"))
            } else {
                Suggestion::plain(user)
            }
        })
        .collect()
}

/// Ce qui empeche d'ecrire dans le coffre, ou `None` si la voie est libre.
///
/// Un seul message a la fois, dans l'ordre ou l'utilisateur peut y remedier:
/// installer `pass-cli`, choisir un coffre, nommer l'item, attendre la lecture
/// du coffre. La lecture est une condition et non un detail: sans la liste des
/// items, un enregistrement creerait un doublon la ou il fallait une mise a
/// jour, et l'URI `pass://coffre/titre` deviendrait ambigue.
fn write_blocker(
    pass_ready: bool,
    vault: &str,
    item_title: &str,
    items_known: bool,
    items_loading: bool,
) -> Option<String> {
    if !pass_ready {
        return Some("pass-cli n'est pas detecte: rien ne peut etre ecrit.".into());
    }
    if vault.is_empty() {
        return Some("Choisissez d'abord un coffre.".into());
    }
    if item_title.is_empty() {
        return Some("Donnez un titre d'item, un nom ou un hote a la connexion.".into());
    }
    if items_loading {
        return Some("Lecture du coffre en cours...".into());
    }
    if !items_known {
        return Some(format!(
            "Contenu de « {vault} » inconnu: chargez le coffre avec « Charger »."
        ));
    }
    None
}

/// Ligne d'etat sous les boutons d'ecriture.
fn write_status(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    blocker: Option<&str>,
    writing: bool,
) {
    if writing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui::hint(ui, "Ecriture dans Proton Pass...", palette);
        });
        return;
    }
    if let Some(blocker) = blocker {
        ui.label(RichText::new(blocker).size(11.0).color(palette.text_dim));
    }
}

/// Developpe un `~` initial: `pass-cli` recoit le chemin tel quel, sans shell
/// pour l'interpreter a sa place.
fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_string();
    };
    // `~utilisateur/...` ne nous concerne pas: seul `~` ou `~/` est developpe.
    if !rest.is_empty() && !rest.starts_with('/') {
        return path.to_string();
    }
    match std::env::var_os("HOME") {
        Some(home) => format!("{}{rest}", home.to_string_lossy()),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(connection: Connection) -> ConnectionEditor {
        ConnectionEditor::new(connection, true)
    }

    #[test]
    fn host_is_required() {
        let error = editor(Connection::default())
            .build()
            .expect_err("doit echouer");
        assert!(error.contains("hote"), "message inattendu: {error}");
    }

    #[test]
    fn name_defaults_to_host() {
        let connection = Connection {
            host: "example.com".into(),
            ..Default::default()
        };
        let built = editor(connection).build().expect("valide");
        assert_eq!(built.name, "example.com");
    }

    #[test]
    fn port_must_be_numeric() {
        let connection = Connection {
            host: "h".into(),
            ..Default::default()
        };
        let mut editor = editor(connection);
        editor.port_text = "abc".into();
        assert!(editor.build().is_err());
        editor.port_text = "70000".into();
        assert!(editor.build().is_err());
        editor.port_text = "2222".into();
        assert_eq!(editor.build().expect("valide").port, 2222);
    }

    #[test]
    fn tags_and_options_are_split_and_trimmed() {
        let connection = Connection {
            host: "h".into(),
            ..Default::default()
        };
        let mut editor = editor(connection);
        editor.tags_text = " prod ,, web ".into();
        editor.options_text = "  ServerAliveInterval=30\n\n  Compression=yes ".into();
        let built = editor.build().expect("valide");
        assert_eq!(built.tags, vec!["prod", "web"]);
        assert_eq!(
            built.ssh_options,
            vec!["ServerAliveInterval=30", "Compression=yes"]
        );
    }

    #[test]
    fn proton_reference_is_optional() {
        let connection = Connection {
            host: "h".into(),
            ..Default::default()
        };
        assert!(editor(connection).build().expect("valide").proton.is_none());
    }

    #[test]
    fn a_typed_password_blocks_saving_until_it_reaches_the_vault() {
        let mut editor = editor(Connection {
            host: "h".into(),
            auth: AuthMethod::Password,
            ..Default::default()
        });
        editor.vault_text = "Coffre".into();
        editor.item_text = "Item".into();
        editor.password_text = "hunter2".into();
        let error = editor.build().expect_err("doit refuser");
        assert!(error.contains("Proton Pass"), "message inattendu: {error}");

        // Une fois parti dans le coffre, le champ est vide et la fiche passe.
        editor.password_text.clear();
        assert!(editor.build().is_ok());
    }

    #[test]
    fn password_auth_requires_an_item() {
        let connection = Connection {
            host: "h".into(),
            auth: AuthMethod::Password,
            ..Default::default()
        };
        let mut editor = editor(connection);
        assert!(editor.build().is_err());
        editor.vault_text = "Vault".into();
        editor.item_text = "Item".into();
        let built = editor.build().expect("valide");
        assert_eq!(built.proton.as_ref().expect("reference").vault, "Vault");
    }

    #[test]
    fn key_file_auth_requires_a_path() {
        let connection = Connection {
            host: "h".into(),
            auth: AuthMethod::KeyFile,
            ..Default::default()
        };
        let mut editor = editor(connection);
        assert!(editor.build().is_err());
        editor.connection.key_file = Some("~/.ssh/id_ed25519".into());
        assert!(editor.build().is_ok());
    }

    #[test]
    fn item_title_falls_back_to_name_then_host() {
        let connection = Connection {
            name: "web-01".into(),
            host: "10.0.0.4".into(),
            ..Default::default()
        };
        let mut editor = editor(connection);
        assert_eq!(editor.item_title(), "web-01");

        editor.item_text = "  Item choisi  ".into();
        assert_eq!(editor.item_title(), "Item choisi");

        editor.item_text.clear();
        editor.connection.name.clear();
        assert_eq!(editor.item_title(), "10.0.0.4");

        editor.connection.host.clear();
        assert!(editor.item_title().is_empty());
    }

    #[test]
    fn closing_the_editor_wipes_the_password() {
        let mut editor = editor(Connection {
            host: "h".into(),
            ..Default::default()
        });
        editor.password_text = "hunter2".into();
        drop(editor);
        // Rien a observer directement: le test documente le contrat, et
        // `secret::scrub` est couvert par ses propres tests.
    }

    #[test]
    fn password_is_never_part_of_the_saved_connection() {
        let mut editor = editor(Connection {
            host: "h".into(),
            auth: AuthMethod::Password,
            ..Default::default()
        });
        editor.vault_text = "Coffre".into();
        editor.item_text = "Item".into();
        // Le mot de passe vit dans l'editeur, pas dans la connexion: meme
        // saisi, il n'a aucun champ ou atterrir a la serialisation.
        editor.password_text = "hunter2".into();
        secret::scrub(&mut editor.password_text);

        let built = editor.build().expect("valide");
        let text = toml::to_string_pretty(&built).expect("serialisation");
        assert!(!text.contains("hunter2"), "mot de passe serialise: {text}");
        assert!(text.contains("Coffre"), "reference perdue: {text}");
    }

    #[test]
    fn blocker_names_the_next_thing_to_fix() {
        // `pass-cli` absent passe avant tout le reste.
        assert!(write_blocker(false, "V", "I", true, false)
            .expect("bloque")
            .contains("pass-cli"));
        assert!(write_blocker(true, "", "I", true, false)
            .expect("bloque")
            .contains("coffre"));
        assert!(write_blocker(true, "V", "", true, false)
            .expect("bloque")
            .contains("titre"));
        assert!(write_blocker(true, "V", "I", false, true)
            .expect("bloque")
            .contains("Lecture"));
        // Coffre jamais lu: creer a l'aveugle ferait un doublon.
        assert!(write_blocker(true, "V", "I", false, false)
            .expect("bloque")
            .contains("Charger"));
        // Un coffre lu mais vide n'est pas un obstacle.
        assert_eq!(write_blocker(true, "V", "I", true, false), None);
    }

    #[test]
    fn home_is_expanded_for_the_key_path() {
        std::env::set_var("HOME", "/home/essai");
        assert_eq!(
            expand_home("~/.ssh/id_ed25519"),
            "/home/essai/.ssh/id_ed25519"
        );
        assert_eq!(expand_home("~"), "/home/essai");
        // `~autre/...` designe le foyer d'un autre compte: on n'y touche pas.
        assert_eq!(expand_home("~root/.ssh/id"), "~root/.ssh/id");
        assert_eq!(expand_home("/tmp/id"), "/tmp/id");
    }

    #[test]
    fn existing_reference_is_loaded_into_fields() {
        let connection = Connection {
            host: "h".into(),
            proton: Some(ProtonRef {
                vault: "V".into(),
                item: "I".into(),
                field: Some("secret".into()),
            }),
            ..Default::default()
        };
        let editor = ConnectionEditor::new(connection, false);
        assert_eq!(editor.vault_text, "V");
        assert_eq!(editor.item_text, "I");
        assert_eq!(editor.field_text, "secret");
    }
}
