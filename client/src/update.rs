pub const NOTICE: &str = "Une nouvelle version est disponible.";

#[cfg(target_arch = "wasm32")]
pub fn apply() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// A native build installs the signed release it found; without one (GitHub
/// unreachable, or a release still building) it opens the download page.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply() {
    if matches!(crate::updater::status(), crate::updater::Status::Available(_)) {
        crate::updater::install();
        return;
    }
    use std::process::Command;
    const RELEASES: &str = "https://github.com/Priax/Rouillo/releases/latest";
    let opened = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", RELEASES]).spawn()
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
