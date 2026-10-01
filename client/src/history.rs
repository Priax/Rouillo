use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http::{self, HttpSlot};
use crate::state::ApiMatchEntry;
use crate::ui::{page_count, EditKeys, Fonts, Pager, Rect, Ui};

pub const PAGE_SIZE: usize = 7;

pub struct History {
    user_id: String,
    token: Option<String>,
    pub entries: Vec<ApiMatchEntry>,
    pub slot: Option<HttpSlot>,
    pager: Pager,
}

fn pages(total: i64) -> usize {
    page_count(usize::try_from(total).unwrap_or(0), PAGE_SIZE)
}

impl History {
    pub fn load(user_id: String, token: Option<String>) -> Self {
        let mut history = Self {
            user_id,
            token,
            entries: Vec::new(),
            slot: None,
            pager: Pager::default(),
        };
        history.fetch();
        history
    }

    fn fetch(&mut self) {
        let slot = http::new_slot();
        let url = format!(
            "users/{}/matches?limit={PAGE_SIZE}&offset={}",
            self.user_id,
            self.pager.page() * PAGE_SIZE
        );
        http::get(http::api_url(&url), self.token.clone(), Arc::clone(&slot));
        self.slot = Some(slot);
    }

    pub fn poll(&mut self) {
        let Some(Ok(resp)) = http::take(&mut self.slot) else {
            return;
        };
        if let Some(matches) = http::json::<Vec<ApiMatchEntry>>(&resp) {
            self.entries = matches;
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.pager.type_char(c);
    }

    pub fn update(&mut self, app: &App, ui: &Ui, fonts: &Fonts, keys: &EditKeys, area: Rect, total: i64) {
        if self.pager.update(app, ui, fonts, keys, area, pages(total)) {
            self.fetch();
        }
    }

    pub fn draw_pager(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect, total: i64) {
        self.pager.draw(draw, ui, fonts, area, pages(total));
    }
}
