//! Construction de la commande lancee dans le PTY d'un onglet.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{AuthMethod, Connection};

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
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        Self {
            program: shell,
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
        ("TERM_PROGRAM".to_string(), "sshpass".to_string()),
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
        args.push("-o".to_string());
        args.push("PreferredAuthentications=password,keyboard-interactive".to_string());
        args.push("-o".to_string());
        args.push("NumberOfPasswordPrompts=1".to_string());
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
    if connection.auth == AuthMethod::Password {
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
            agent_socket: Some(PathBuf::from("/run/sshpass/agent.sock")),
            ..Default::default()
        };
        let spec = build_ssh(&connection(), &context);
        assert_eq!(
            spec.env.get("SSH_AUTH_SOCK").map(String::as_str),
            Some("/run/sshpass/agent.sock")
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
            askpass_script: Some(PathBuf::from("/run/sshpass/askpass-1.sh")),
            ..Default::default()
        };
        let spec = build_ssh(&conn, &context);
        assert_eq!(
            spec.env.get("SSH_ASKPASS").map(String::as_str),
            Some("/run/sshpass/askpass-1.sh")
        );
        assert_eq!(
            spec.env.get("SSH_ASKPASS_REQUIRE").map(String::as_str),
            Some("force")
        );
        assert!(spec.args.contains(&"PubkeyAuthentication=no".to_string()));
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
