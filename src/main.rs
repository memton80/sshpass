//! sshpass-gui — gestionnaire de connexions SSH avec terminal integre.
//!
//! Pile technique: `egui`/`eframe` pour l'interface (aucune dependance GTK,
//! Qt, Electron ou webview), `alacritty_terminal` pour l'emulation VT, et
//! `pass-cli` en sous-processus pour Proton Pass.

// sshpass-gui ne cible que les systemes Unix: PTY, sockets d'agent et script
// SSH_ASKPASS reposent tous sur des mecanismes POSIX. Le message est explicite
// plutot que de laisser une cascade d'erreurs incomprehensibles.
#[cfg(not(unix))]
compile_error!("sshpass-gui ne cible que les systemes Unix (Linux, *BSD, macOS).");

mod app;
mod config;
mod pass;
mod term;
mod theme;
mod ui;

use app::SshpassApp;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        // Le nom de cible des journaux est celui de la caisse, tirets
        // convertis en soulignes.
        env_logger::Env::default().default_filter_or("sshpass_gui=info,warn"),
    )
    .init();

    match config::adopt_legacy_config() {
        Ok(Some(source)) => log::info!(
            "configuration reprise depuis {} vers {}",
            source.display(),
            config::config_path().display()
        ),
        Ok(None) => {}
        Err(err) => log::warn!("reprise de l'ancienne configuration impossible: {err}"),
    }

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
            .with_title("sshpass-gui")
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([760.0, 460.0])
            // Doit correspondre a `StartupWMClass` du fichier .desktop,
            // sinon l'icone de la fenetre n'est pas retrouvee sous Wayland.
            .with_app_id("sshpass-gui"),
        ..Default::default()
    };

    eframe::run_native(
        "sshpass-gui",
        options,
        Box::new(|cc| Ok(Box::new(SshpassApp::new(cc, config)))),
    )
}
