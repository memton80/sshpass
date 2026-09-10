//! Construction de la commande lancee dans le PTY d'un onglet.
//!
//! `ssh` est lance par `Command::new("ssh").args(...)`, jamais par `sh -c`:
//! rien de ce que l'utilisateur saisit n'est interprete par un shell, et une
//! option biscornue reste une option, pas une injection.
//!
//! Cela ne rend pas les options anodines pour autant: certaines directives de
//! `ssh` font executer des programmes **locaux** (`ProxyCommand`,
//! `LocalCommand`, `KnownHostsCommand`, `Match exec`...) et d'autres coupent
//! une verification (`StrictHostKeyChecking=no`). `dangerous_options` les
//! nomme, pour que l'interface puisse le dire au lieu de laisser croire qu'un
//! fichier de configuration est un simple reglage.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{AuthMethod, Connection};

/// Directives `ssh -o` qui font executer un programme local, redirigent la
/// connexion, ou desactivent une verification.
///
/// Comparees sans tenir compte de la casse: `ssh` lit ses mots-cles ainsi.
const POWERFUL_DIRECTIVES: [&str; 10] = [
    // Executent un programme local.
    "proxycommand",
    "localcommand",
    "permitlocalcommand",
    "knownhostscommand",
    "match",
    // Deplacent la confiance ailleurs.
    "proxyjump",
    "identityagent",
    "useknownhostsfile",
    "userknownhostsfile",
    // Coupe la verification d'empreinte.
    "stricthostkeychecking",
];

/// Parmi les options d'une connexion, celles qui meritent d'etre signalees.
///
/// Rend le mot-cle tel qu'il a ete saisi, pour que le message soit reconnu par
/// qui l'a ecrit. `StrictHostKeyChecking` n'est retenu que quand il *baisse*
/// la garde: `accept-new` et `yes` sont des reglages ordinaires.
pub fn dangerous_options(options: &[String]) -> Vec<String> {
    options
        .iter()
        .filter_map(|option| {
            let option = option.trim();
            let (keyword, value) = match option.split_once(['=', ' ']) {
                Some((keyword, value)) => (keyword.trim(), value.trim()),
                None => (option, ""),
            };
            let normalized = keyword.to_ascii_lowercase();
            if !POWERFUL_DIRECTIVES.contains(&normalized.as_str()) {
                return None;
            }
            if normalized == "stricthostkeychecking"
                && !matches!(value.to_ascii_lowercase().as_str(), "no" | "off")
            {
                return None;
            }
            Some(keyword.to_string())
        })
        .collect()
}

/// Programme, arguments et environnement d'une session terminal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_directory: Option<PathBuf>,
}

/// Elements fournis par l'application pour completer la commande.
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    /// Socket de l'agent Proton Pass a exporter dans `SSH_AUTH_SOCK`.
    pub agent_socket: Option<PathBuf>,
    /// Script `SSH_ASKPASS` qui lira le mot de passe dans Proton Pass.
    pub askpass_script: Option<PathBuf>,
}

impl CommandSpec {
    /// Shell de connexion local, pour un onglet sans connexion distante.
    pub fn login_shell() -> Self {
        Self {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            args: Vec::new(),
            env: base_env(),
            working_directory: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    /// Ligne de commande affichable, a titre informatif.
    pub fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

/// Variables communes a toutes les sessions.
fn base_env() -> HashMap<String, String> {
    HashMap::from([
        // alacritty_terminal implemente xterm-256color; annoncer autre chose
        // ferait mal afficher les applications distantes.
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "sshpass-gui".to_string()),
    ])
}

/// Construit la commande `ssh` correspondant a une connexion.
pub fn build_ssh(connection: &Connection, context: &SessionContext) -> CommandSpec {
    let mut args: Vec<String> = Vec::new();

    args.push("-p".to_string());
    args.push(connection.port.to_string());

    if connection.auth == AuthMethod::KeyFile {
        if let Some(path) = connection
            .key_file
            .as_ref()
            .filter(|p| !p.trim().is_empty())
        {
            args.push("-i".to_string());
            args.push(path.clone());
            // Sans cela ssh essaierait d'abord toutes les cles de l'agent.
            args.push("-o".to_string());
            args.push("IdentitiesOnly=yes".to_string());
        }
    }

    if connection.auth == AuthMethod::Password {
        // On force l'authentification par mot de passe: sinon ssh epuise
        // d'abord les cles de l'agent et le script askpass n'est jamais appele.
        args.push("-o".to_string());
        args.push("PubkeyAuthentication=no".to_string());
        // `password` **seul**, jamais `keyboard-interactive`.
        //
        // Dans `keyboard-interactive`, c'est le serveur qui redige les
        // questions, et `ssh` les transmet telles quelles a SSH_ASKPASS. Un
        // serveur hostile n'a alors qu'a demander « Password: », ou n'importe
        // quoi d'autre, pour que le pont askpass lui serve le mot de passe du
        // coffre. Avec `password`, l'invite est fabriquee par `ssh` a partir
        // de l'hote et de l'utilisateur: le distant n'a plus la main dessus.
        //
        // Les serveurs qui exigent reellement PAM ou un second facteur
        // relevent d'`AuthMethod::KeyboardInteractive`, ou la reponse est
        // tapee par un humain.
        args.push("-o".to_string());
        args.push("PreferredAuthentications=password".to_string());
        args.push("-o".to_string());
        args.push("NumberOfPasswordPrompts=1".to_string());
    }

    if connection.auth == AuthMethod::KeyboardInteractive {
        args.push("-o".to_string());
        args.push("PubkeyAuthentication=no".to_string());
        args.push("-o".to_string());
        args.push("PreferredAuthentications=keyboard-interactive".to_string());
    }

    for option in &connection.ssh_options {
        let option = option.trim();
        if option.is_empty() {
            continue;
        }
        args.push("-o".to_string());
        args.push(option.to_string());
    }

    let command = connection
        .command
        .as_ref()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty());
    if command.is_some() {
        // Forcer le PTY: sans `-t`, une commande distante n'a pas de terminal
        // et les applications plein ecran (htop, vim) ne fonctionnent pas.
        args.push("-t".to_string());
    }

    args.push(connection.target());
    if let Some(command) = command {
        args.push(command.to_string());
    }

    let mut env = base_env();
    if let Some(socket) = &context.agent_socket {
        env.insert(
            "SSH_AUTH_SOCK".to_string(),
            socket.to_string_lossy().into_owned(),
        );
    }
    if connection.auth.uses_stored_password() {
        if let Some(script) = &context.askpass_script {
            env.insert(
                "SSH_ASKPASS".to_string(),
                script.to_string_lossy().into_owned(),
            );
            // Requis depuis OpenSSH 8.4 pour utiliser askpass alors qu'un
            // terminal est disponible; sans cela ssh demanderait sur le tty.
            env.insert("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string());
        }
    }

    if connection.auth == AuthMethod::KeyboardInteractive {
        // Le mode existe pour qu'un humain reponde. Un `SSH_ASKPASS` herite de
        // l'environnement — le notre ou celui du bureau — repondrait a sa
        // place, a des questions ecrites par le serveur: on l'ecarte.
        env.insert("SSH_ASKPASS_REQUIRE".to_string(), "never".to_string());
    }

    CommandSpec {
        program: "ssh".to_string(),
        args,
        env,
        working_directory: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProtonRef;

    fn connection() -> Connection {
        Connection {
            name: "web".into(),
            host: "example.com".into(),
            user: "alex".into(),
            port: 22,
            ..Default::default()
        }
    }

    #[test]
    fn minimal_command() {
        let spec = build_ssh(&connection(), &SessionContext::default());
        assert_eq!(spec.program, "ssh");
        assert_eq!(spec.args, vec!["-p", "22", "alex@example.com"]);
        assert_eq!(
            spec.env.get("TERM").map(String::as_str),
            Some("xterm-256color")
        );
        assert_eq!(
            spec.env.get("TERM_PROGRAM").map(String::as_str),
            Some("sshpass-gui")
        );
        assert!(!spec.env.contains_key("SSH_AUTH_SOCK"));
    }

    #[test]
    fn custom_port_and_no_user() {
        let mut conn = connection();
        conn.port = 2222;
        conn.user.clear();
        let spec = build_ssh(&conn, &SessionContext::default());
        assert_eq!(spec.args, vec!["-p", "2222", "example.com"]);
    }

    #[test]
    fn agent_socket_is_exported() {
        let context = SessionContext {
            agent_socket: Some(PathBuf::from("/run/sshpass-gui/agent.sock")),
            ..Default::default()
        };
        let spec = build_ssh(&connection(), &context);
        assert_eq!(
            spec.env.get("SSH_AUTH_SOCK").map(String::as_str),
            Some("/run/sshpass-gui/agent.sock")
        );
    }

    #[test]
    fn key_file_adds_identity_options() {
        let mut conn = connection();
        conn.auth = AuthMethod::KeyFile;
        conn.key_file = Some("/home/alex/.ssh/id_ed25519".into());
        let spec = build_ssh(&conn, &SessionContext::default());
        assert!(spec
            .args
            .windows(2)
            .any(|w| w == ["-i", "/home/alex/.ssh/id_ed25519"]));
        assert!(spec.args.contains(&"IdentitiesOnly=yes".to_string()));
    }

    #[test]
    fn key_file_without_path_is_ignored() {
        let mut conn = connection();
        conn.auth = AuthMethod::KeyFile;
        conn.key_file = Some("   ".into());
        let spec = build_ssh(&conn, &SessionContext::default());
        assert!(!spec.args.contains(&"-i".to_string()));
    }

    #[test]
    fn password_auth_wires_askpass() {
        let mut conn = connection();
        conn.auth = AuthMethod::Password;
        conn.proton = Some(ProtonRef {
            vault: "V".into(),
            item: "I".into(),
            field: Some("password".into()),
        });
        let context = SessionContext {
            askpass_script: Some(PathBuf::from("/run/sshpass-gui/askpass-1.sh")),
            ..Default::default()
        };
        let spec = build_ssh(&conn, &context);
        assert_eq!(
            spec.env.get("SSH_ASKPASS").map(String::as_str),
            Some("/run/sshpass-gui/askpass-1.sh")
        );
        assert_eq!(
            spec.env.get("SSH_ASKPASS_REQUIRE").map(String::as_str),
            Some("force")
        );
        assert!(spec.args.contains(&"PubkeyAuthentication=no".to_string()));
    }

    #[test]
    fn stored_password_never_meets_keyboard_interactive() {
        let mut conn = connection();
        conn.auth = AuthMethod::Password;
        let spec = build_ssh(&conn, &SessionContext::default());
        // Le serveur redige les questions de keyboard-interactive: le pont
        // askpass ne doit jamais pouvoir y repondre tout seul.
        assert!(spec
            .args
            .contains(&"PreferredAuthentications=password".to_string()));
        assert!(
            !spec.args.iter().any(|a| a.contains("keyboard-interactive")),
            "keyboard-interactive negocie avec le secret du coffre: {:?}",
            spec.args
        );
    }

    #[test]
    fn interactive_mode_asks_the_human_and_nothing_else() {
        let mut conn = connection();
        conn.auth = AuthMethod::KeyboardInteractive;
        conn.proton = Some(ProtonRef {
            vault: "V".into(),
            item: "I".into(),
            field: Some("password".into()),
        });
        // Meme si un script trainait dans le contexte, il n'est pas branche.
        let context = SessionContext {
            askpass_script: Some(PathBuf::from("/run/sshpass-gui/askpass-1.sh")),
            ..Default::default()
        };
        let spec = build_ssh(&conn, &context);
        assert!(spec
            .args
            .contains(&"PreferredAuthentications=keyboard-interactive".to_string()));
        assert!(!spec.env.contains_key("SSH_ASKPASS"));
        // Un askpass herite de l'environnement repondrait a la place de
        // l'utilisateur, a des questions ecrites par le serveur.
        assert_eq!(
            spec.env.get("SSH_ASKPASS_REQUIRE").map(String::as_str),
            Some("never")
        );
    }

    #[test]
    fn powerful_directives_are_named() {
        let flagged = dangerous_options(&[
            "ServerAliveInterval=30".into(),
            "ProxyCommand=nc %h %p".into(),
            "PermitLocalCommand=yes".into(),
            "StrictHostKeyChecking=no".into(),
            "Compression yes".into(),
        ]);
        assert_eq!(
            flagged,
            vec![
                "ProxyCommand",
                "PermitLocalCommand",
                "StrictHostKeyChecking"
            ]
        );
    }

    #[test]
    fn ordinary_options_are_left_alone() {
        // Renforcer la verification d'empreinte n'est pas un avertissement.
        assert!(dangerous_options(&[
            "StrictHostKeyChecking=yes".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "ServerAliveInterval=30".into(),
            "Compression=yes".into(),
        ])
        .is_empty());
        // La casse et l'espace comme separateur sont ceux de ssh_config.
        assert_eq!(
            dangerous_options(&["proxyjump bastion".into()]),
            ["proxyjump"]
        );
    }

    #[test]
    fn password_auth_without_script_does_not_set_env() {
        let mut conn = connection();
        conn.auth = AuthMethod::Password;
        let spec = build_ssh(&conn, &SessionContext::default());
        assert!(!spec.env.contains_key("SSH_ASKPASS"));
    }

    #[test]
    fn remote_command_forces_pty() {
        let mut conn = connection();
        conn.command = Some("htop".into());
        let spec = build_ssh(&conn, &SessionContext::default());
        assert_eq!(
            spec.args,
            vec!["-p", "22", "-t", "alex@example.com", "htop"]
        );
    }

    #[test]
    fn extra_options_are_prefixed() {
        let mut conn = connection();
        conn.ssh_options = vec!["ServerAliveInterval=30".into(), "  ".into()];
        let spec = build_ssh(&conn, &SessionContext::default());
        assert!(spec
            .args
            .windows(2)
            .any(|w| w == ["-o", "ServerAliveInterval=30"]));
        assert_eq!(spec.args.iter().filter(|a| *a == "-o").count(), 1);
    }

    #[test]
    fn target_comes_last_before_command() {
        let mut conn = connection();
        conn.ssh_options = vec!["StrictHostKeyChecking=accept-new".into()];
        let spec = build_ssh(&conn, &SessionContext::default());
        assert_eq!(
            spec.args.last().map(String::as_str),
            Some("alex@example.com")
        );
    }

    #[test]
    fn login_shell_has_terminal_env() {
        let spec = CommandSpec::login_shell();
        assert!(!spec.program.is_empty());
        assert_eq!(
            spec.env.get("TERM").map(String::as_str),
            Some("xterm-256color")
        );
    }
}
