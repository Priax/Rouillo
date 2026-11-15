#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::{Mutex, OnceLock};

    use arboard::Clipboard;

    // On Linux the copied text is served by this object: it must outlive the copy.
    fn clipboard() -> Option<std::sync::MutexGuard<'static, Option<Clipboard>>> {
        static CLIPBOARD: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();
        CLIPBOARD.get_or_init(|| Mutex::new(Clipboard::new().ok())).lock().ok()
    }

    pub fn set(text: &str) {
        if let Some(Some(c)) = clipboard().as_deref_mut().map(Option::as_mut) {
            let _ = c.set_text(text);
        }
    }

    pub fn get() -> Option<String> {
        clipboard()?.as_mut()?.get_text().ok()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{get, set};

/// The browser copies through its own events, which read what `publish` left.
#[cfg(target_arch = "wasm32")]
pub fn set(_: &str) {}

/// The browser pastes into the hidden input, which types the text.
#[cfg(target_arch = "wasm32")]
pub fn get() -> Option<String> {
    None
}

pub fn publish(selected: Option<&str>) {
    #[cfg(target_arch = "wasm32")]
    crate::web::set_selection(selected.unwrap_or_default());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = selected;
}
