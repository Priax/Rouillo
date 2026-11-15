use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use notan::prelude::*;

use crate::http::{self, HttpSlot};

enum Entry {
    Loading(HttpSlot),
    Ready(Texture),
    Failed,
}

/// Pictures fetched from the server once each, by URL. A new upload gets a
/// new URL, so an entry never goes stale.
#[derive(Default)]
pub struct Images {
    entries: RefCell<HashMap<String, Entry>>,
}

impl Images {
    /// The picture at `url`, a path on the API server, once it has loaded.
    pub fn get(&self, gfx: &mut Graphics, url: &str) -> Option<Texture> {
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(url.to_owned()).or_insert_with(|| {
            let slot = http::new_slot();
            http::get(http::resource_url(url), None, Arc::clone(&slot));
            Entry::Loading(slot)
        });
        if let Entry::Loading(slot) = entry {
            match http::poll(slot) {
                Some(Ok(resp)) if resp.status == 200 => {
                    *entry = gfx
                        .create_texture()
                        .from_image(&resp.bytes)
                        .with_filter(TextureFilter::Linear, TextureFilter::Linear)
                        .build()
                        .map_or(Entry::Failed, Entry::Ready);
                }
                Some(_) => *entry = Entry::Failed,
                None => {}
            }
        }
        match entry {
            Entry::Ready(texture) => Some(texture.clone()),
            Entry::Loading(_) | Entry::Failed => None,
        }
    }

    pub fn get_opt(&self, gfx: &mut Graphics, url: Option<&str>) -> Option<Texture> {
        url.and_then(|u| self.get(gfx, u))
    }
}
