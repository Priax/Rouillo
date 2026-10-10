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
        Status::Checking | Status::Downloading { .. } | Status::Verifying | Status::Installing | Status::Done => {}
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

/// What the update is doing, to show next to its button, and how much of
/// the download is done when that is known.
#[cfg(not(target_arch = "wasm32"))]
pub fn progress() -> Option<(String, Option<f32>)> {
    use crate::updater::Status;
    let text = match crate::updater::status() {
        Status::Checking => "Recherche de la mise à jour...".to_owned(),
        Status::Downloading {
            done,
            total: Some(total),
        } if total > 0 => {
            let text = format!("Téléchargement: {} / {} Mo", megabytes(done), megabytes(total));
            return Some((text, Some((done as f32 / total as f32).min(1.0))));
        }
        Status::Downloading { done, .. } => format!("Téléchargement: {} Mo", megabytes(done)),
        Status::Verifying => "Vérification de la signature...".to_owned(),
        Status::Installing => "Installation...".to_owned(),
        Status::Failed(e) => format!("Mise à jour impossible: {e}"),
        Status::Done => "Redémarrage...".to_owned(),
        Status::Idle | Status::Available(_) => return None,
    };
    Some((text, None))
}

#[cfg(not(target_arch = "wasm32"))]
fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0).replace('.', ",")
}

#[cfg(target_arch = "wasm32")]
pub fn progress() -> Option<(String, Option<f32>)> {
    None
}
