use shared::api::{MatchEntry, Uuid};

use crate::http::{self, Json};
use crate::ui::{page_count, Gui, Pager, Rect};

pub const PAGE_SIZE: usize = 7;

/// A player's matches, a page at a time. A page holds as many rows as the
/// screen fits, `PAGE_SIZE` at most: the first `update` sets it and loads.
pub struct History {
    user_id: Uuid,
    token: Option<String>,
    pub entries: Vec<MatchEntry>,
    pub slot: Option<Json<Vec<MatchEntry>>>,
    pager: Pager,
    pub per_page: usize,
}

impl History {
    pub fn load(user_id: Uuid, token: Option<String>) -> Self {
        Self { user_id, token, entries: Vec::new(), slot: None, pager: Pager::default(), per_page: 0 }
    }

    fn pages(&self, total: i64) -> usize {
        page_count(usize::try_from(total).unwrap_or(0), self.per_page.max(1))
    }

    pub fn fetch(&mut self) {
        let url = format!(
            "users/{}/matches?limit={}&offset={}",
            self.user_id,
            self.per_page,
            self.pager.page() * self.per_page
        );
        self.slot = Some(http::get(http::api_url(&url), self.token.clone()).json());
    }

    pub fn poll(&mut self) {
        if let Some(Ok(matches)) = http::take(&mut self.slot) {
            self.entries = matches;
        }
    }

    /// Fits a page to `rows`, loading it again when that changes.
    pub fn fit(&mut self, rows: usize) {
        let rows = rows.clamp(1, PAGE_SIZE);
        if rows != self.per_page {
            self.per_page = rows;
            self.pager = Pager::default();
            self.fetch();
        }
    }

    pub fn show_pager(&mut self, ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, area: Rect, total: i64) {
        if self.pager.show(ui, gui, "matches", area, self.pages(total)) {
            self.fetch();
        }
    }
}
