use crate::http::{self, Json};
use crate::state::ApiMatchEntry;
use crate::ui::{page_count, Gui, Pager, Rect};

pub const PAGE_SIZE: usize = 7;

/// A player's matches, a page at a time. A page holds as many rows as the
/// screen fits, `PAGE_SIZE` at most: the first `update` sets it and loads.
pub struct History {
    user_id: String,
    token: Option<String>,
    pub entries: Vec<ApiMatchEntry>,
    pub slot: Option<Json<Vec<ApiMatchEntry>>>,
    pager: Pager,
    pub per_page: usize,
}

impl History {
    pub fn load(user_id: String, token: Option<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use crate::state::ApiMatchEntry;

    const ENTRY: &str = r#"{"id":"x","played_at":"t","duration_secs":12.5,"ranked":false,"players":[
        {"user_id":"me","username":"moi","slot":1,"team":1,"place":2,"won":false,"max_chain":3,"total_chains":1,
         "nuisance_sent":10,"nuisance_received":4,"all_clears":0,"pieces_placed":9},
        {"user_id":null,"username":null,"slot":2,"team":2,"place":1,"won":true,"max_chain":1,"total_chains":1,
         "nuisance_sent":4,"nuisance_received":10,"all_clears":0,"pieces_placed":8}]}"#;

    #[test]
    fn a_match_from_the_server_reads_with_its_players() {
        let m: ApiMatchEntry = serde_json::from_str(ENTRY).expect("parses");
        assert_eq!(m.me("me").map(|p| (p.place, p.won, p.max_chain)), Some((2, false, 3)));
        assert_eq!(m.opponent("me").map(|p| p.won), Some(true));
        assert!(!m.drawn());
    }

    #[test]
    fn a_game_of_four_has_no_single_opponent() {
        let mut m: ApiMatchEntry = serde_json::from_str(ENTRY).expect("parses");
        let more: ApiMatchEntry = serde_json::from_str(ENTRY).expect("parses");
        m.players.extend(more.players);
        assert!(m.opponent("me").is_none());
        assert!(m.me("me").is_some());
    }
}
