pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
pub const TOO_HEAVY: &str = "Image trop lourde (5 Mo au plus).";

pub enum Picked {
    File(Vec<u8>),
    Failed(&'static str),
    Cancelled,
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::Mutex;

    use super::{Picked, MAX_BYTES, TOO_HEAVY};

    static PICKED: Mutex<Option<Picked>> = Mutex::new(None);

    pub fn open() {
        let picked = match rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
            .pick_file()
        {
            None => Picked::Cancelled,
            Some(path) => match std::fs::metadata(&path) {
                Ok(meta) if meta.len() > MAX_BYTES => Picked::Failed(TOO_HEAVY),
                _ => std::fs::read(&path).map_or(Picked::Failed("Fichier illisible."), Picked::File),
            },
        };
        if let Ok(mut slot) = PICKED.lock() {
            *slot = Some(picked);
        }
    }

    pub fn take() -> Option<Picked> {
        PICKED.lock().ok()?.take()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{open, take};

#[cfg(target_arch = "wasm32")]
pub use crate::web::{pick_file as open, take_picked as take};
