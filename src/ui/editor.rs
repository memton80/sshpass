//! Fenetre de creation et de modification d'une connexion.

use egui::{ComboBox, Margin, RichText};

use crate::app::{Action, SshpassApp, ToastKind};
use crate::config::{AuthMethod, Connection, ProtonRef};
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
    pub item_filter: String,
    pub error: Option<String>,
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
            item_filter: String::new(),
            error: None,
            connection,
            is_new,
        }
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

        if connection.auth == AuthMethod::Password
            && !connection.proton.as_ref().is_some_and(|p| p.is_complete())
        {
            return Err(
                "L'authentification par mot de passe demande un coffre et un item Proton Pass."
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

    let editor = app.editor.as_mut().expect("editeur ouvert");
    let title = if editor.is_new {
        "Nouvelle connexion"
    } else {
        "Modifier la connexion"
    };
    let items: Vec<crate::pass::Item> = app
        .items
        .get(editor.vault_text.trim())
        .cloned()
        .unwrap_or_default();

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
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.connection.host)
                            .hint_text("10.0.0.4 ou example.com")
                            .desired_width(f32::INFINITY),
                    );
                    ui.end_row();

                    ui.label("Utilisateur");
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.connection.user)
                            .hint_text("root")
                            .desired_width(f32::INFINITY),
                    );
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
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut editor.vault_text)
                                .hint_text("SSH Keys")
                                .desired_width(220.0),
                        );
                        if response.changed() {
                            editor.item_text.clear();
                        }
                        if !vaults.is_empty() {
                            ComboBox::from_id_salt("vault_pick")
                                .selected_text("Choisir")
                                .width(110.0)
                                .show_ui(ui, |ui| {
                                    for vault in &vaults {
                                        if ui.selectable_label(false, vault).clicked() {
                                            editor.vault_text = vault.clone();
                                            editor.item_text.clear();
                                            load_items = Some(vault.clone());
                                        }
                                    }
                                });
                        }
                    });
                    ui.end_row();

                    ui.label("Item");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.item_text)
                                .hint_text("Titre de l'item")
                                .desired_width(220.0),
                        );
                        if !editor.vault_text.trim().is_empty()
                            && ui.small_button("Parcourir").clicked()
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

            // La liste n'apparait qu'apres « Parcourir »: elle est haute et
            // n'a pas de raison d'occuper la fiche en permanence.
            if !items.is_empty() {
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut editor.item_filter)
                        .hint_text("Filtrer les items")
                        .desired_width(f32::INFINITY),
                );
                egui::ScrollArea::vertical()
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for item in items.iter().filter(|i| i.matches(&editor.item_filter)) {
                            let selected = editor.item_text == item.title;
                            let label = format!("{}  ·  {}", item.title, item.kind.label());
                            if ui.selectable_label(selected, label).clicked() {
                                editor.item_text = item.title.clone();
                            }
                        }
                    });
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
