//! sshpass — gestionnaire de connexions SSH avec terminal integre.
//!
//! Pile technique: `egui`/`eframe` pour l'interface (aucune dependance GTK,
//! Qt, Electron ou webview), `alacritty_terminal` pour l'emulation VT, et
//! `pass-cli` en sous-processus pour Proton Pass.

// sshpass ne cible que les systemes Unix: PTY, sockets d'agent et script
// SSH_ASKPASS reposent tous sur des mecanismes POSIX. Le message est explicite
// plutot que de laisser une cascade d'erreurs incomprehensibles.
#[cfg(not(unix))]
compile_error!("sshpass ne cible que les systemes Unix (Linux, *BSD, macOS).");

mod app;
mod config;
mod pass;
mod term;
mod theme;
mod ui;

use app::SshpassApp;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("sshpass=info,warn"),
    )
    .init();

    let config = match config::load() {
        Ok(config) => config,
        Err(err) => {
            // Une configuration illisible ne doit pas empecher de demarrer:
            // on repart d'une configuration vide sans ecraser le fichier.
            log::error!("configuration illisible ({err}); demarrage a vide");
            config::Config::default()
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("sshpass")
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([760.0, 460.0])
            .with_app_id("sshpass"),
        ..Default::default()
    };

    eframe::run_native(
        "sshpass",
        options,
        Box::new(|cc| Ok(Box::new(SshpassApp::new(cc, config)))),
    )
}
