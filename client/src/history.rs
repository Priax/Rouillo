use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http::{self, HttpSlot};
use crate::state::ApiMatchEntry;
use crate::ui::{page_count, EditKeys, Fonts, Pager, Rect, TextInput, Ui};

pub const PAGE_SIZE: usize = 7;

/// A player's matches, a page at a time. A page holds as many rows as the
/// screen fits, `PAGE_SIZE` at most: the first `update` sets it and loads.
pub struct History {
    user_id: String,
    token: Option<String>,
    pub entries: Vec<ApiMatchEntry>,
    pub slot: Option<HttpSlot>,
    pager: Pager,
    pub per_page: usize,
}

impl History {
    pub fn load(user_id: String, token: Option<String>) -> Self {
        Self {
            user_id,
            token,
            entries: Vec::new(),
            slot: None,
            pager: Pager::default(),
            per_page: 0,
        }
    }

    fn pages(&self, total: i64) -> usize {
        page_count(usize::try_from(total).unwrap_or(0), self.per_page.max(1))
    }

    fn fetch(&mut self) {
        let slot = http::new_slot();
        let url = format!(
            "users/{}/matches?limit={}&offset={}",
            self.user_id,
            self.per_page,
            self.pager.page() * self.per_page
        );
        http::get(http::api_url(&url), self.token.clone(), Arc::clone(&slot));
        self.slot = Some(slot);
    }

    pub fn poll(&mut self) {
        if let Some(Ok(matches)) = http::take_json::<Vec<ApiMatchEntry>>(&mut self.slot) {
            self.entries = matches;
        }
    }

    pub fn typing(&mut self) -> Option<&mut TextInput> {
        self.pager.typing()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, app: &App, ui: &Ui, fonts: &Fonts, keys: &EditKeys, area: Rect, total: i64, rows: usize) {
        let rows = rows.clamp(1, PAGE_SIZE);
        if rows != self.per_page {
            self.per_page = rows;
            self.pager = Pager::default();
            self.fetch();
        } else if self.pager.update(app, ui, fonts, keys, area, self.pages(total)) {
            self.fetch();
        }
    }

    pub fn draw_pager(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect, total: i64) {
        self.pager.draw(draw, ui, fonts, area, self.pages(total));
    }
}
