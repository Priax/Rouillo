#[cfg(target_arch = "wasm32")]
pub const NOTICE: &str = "Une nouvelle version est disponible.";
#[cfg(not(target_arch = "wasm32"))]
pub const NOTICE: &str = "Une nouvelle version est disponible sur GitHub.";

#[cfg(target_arch = "wasm32")]
pub fn apply() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

// A native build cannot replace itself (yet): open the download page instead.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply() {
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
