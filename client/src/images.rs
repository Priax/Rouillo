use std::collections::HashMap;
use std::sync::Arc;

use aero::image::{Filter, Image};

use crate::http::{self, HttpSlot};

enum Entry {
    Loading(HttpSlot),
    /// The encoded picture is kept: a GPU lost takes the image with it.
    Ready(Image, Vec<u8>),
    Failed,
}

/// Pictures fetched from the server once each, by URL. A new upload gets a
/// new URL, so an entry never goes stale.
#[derive(Default)]
pub struct Images {
    entries: HashMap<String, Entry>,
}

impl Images {
    /// The picture at `url`, a path on the API server, once it has loaded.
    pub fn get(&mut self, url: &str) -> Option<Image> {
        let entries = &mut self.entries;
        let entry = entries.entry(url.to_owned()).or_insert_with(|| {
            let slot = http::new_slot();
            http::get(http::resource_url(url), None, Arc::clone(&slot));
            Entry::Loading(slot)
        });
        if let Entry::Loading(slot) = entry {
            match http::poll(slot) {
                Some(Ok(resp)) if resp.status == 200 => {
                    *entry = picture(&resp.bytes).map_or(Entry::Failed, |image| Entry::Ready(image, resp.bytes));
                }
                Some(_) => *entry = Entry::Failed,
                None => {}
            }
        }
        match entry {
            Entry::Ready(image, _) => Some(image.clone()),
            Entry::Loading(_) | Entry::Failed => None,
        }
    }

    pub fn get_opt(&mut self, url: Option<&str>) -> Option<Image> {
        url.and_then(|u| self.get(u))
    }

    /// Makes the pictures again after the GPU was lost.
    pub fn rebuild(&mut self) {
        for entry in self.entries.values_mut() {
            if let Entry::Ready(image, bytes) = entry {
                match picture(bytes) {
                    Ok(again) => *image = again,
                    Err(_) => *entry = Entry::Failed,
                }
            }
        }
    }
}

/// Width, height and RGBA pixels of an encoded picture: PNG, JPEG or ICO, the largest image of an ICO.
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let picture = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
    Ok((picture.width(), picture.height(), picture.into_raw()))
}

fn picture(bytes: &[u8]) -> Result<Image, String> {
    let (width, height, rgba) = decode(bytes)?;
    Image::from_rgba(width, height, rgba, Filter::Linear).map_err(|e| e.to_string())
}
