/// Why online play stopped: the server refused this version.
#[cfg(not(target_arch = "wasm32"))]
pub const TOO_OLD: &str =
    "Cette version n'est plus compatible avec le serveur: mettez le jeu à jour pour jouer en ligne.";
#[cfg(target_arch = "wasm32")]
pub const TOO_OLD: &str = "Une nouvelle version du jeu est en ligne: rechargez la page pour jouer en ligne.";

#[cfg(target_arch = "wasm32")]
pub fn apply() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// A native build installs the newest signed release, looked up again on
/// the click: the one checked at start may not have been published yet.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply() {
    use crate::updater::{self, Status};
    match updater::status() {
        Status::Available(_) => updater::install(),
        Status::Checking | Status::Installing | Status::Done => {}
        Status::Idle | Status::Failed(_) => updater::update_now(),
    }
}

/// The releases page, for when GitHub cannot be asked for the binary.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_download_page() {
    use std::process::Command;
    const RELEASES: &str = "https://github.com/Priax/Rouillo/releases/latest";
    let opened = if cfg!(target_os = "windows") {
        Command::new("explorer").arg(RELEASES).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(RELEASES).spawn()
    } else {
        Command::new("xdg-open").arg(RELEASES).spawn()
    };
    if let Err(e) = opened {
        eprintln!("[update] impossible d'ouvrir {RELEASES}: {e}");
    }
}

/// What the update is doing, to show next to its button.
#[cfg(not(target_arch = "wasm32"))]
pub fn progress() -> Option<String> {
    match crate::updater::status() {
        crate::updater::Status::Checking => Some("Recherche de la mise à jour...".to_owned()),
        crate::updater::Status::Installing => Some("Téléchargement de la mise à jour...".to_owned()),
        crate::updater::Status::Failed(e) => Some(format!("Mise à jour impossible: {e}")),
        crate::updater::Status::Done => Some("Redémarrage...".to_owned()),
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn progress() -> Option<String> {
    None
}
