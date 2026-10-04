use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::{config, ClientMessage};

use crate::connection::now_secs;
use crate::http::{self, HttpSlot};
use crate::state::{ApiUserProfile, Screen, State};
use crate::theme;
use crate::ui::{self, Fonts, Persona, Pill, Rect, SharpText, Ui, View};

#[derive(Default)]
pub enum Search {
    #[default]
    Idle,
    Searching {
        since: f64,
    },
    Found {
        since: f64,
        opponent: String,
        opponent_elo: i32,
        opponent_avatar: Option<String>,
        deadline: f64,
        accepted: bool,
    },
}

#[derive(Default)]
pub struct RankedView {
    pub search: Search,
    cooldown_until: Option<f64>,
    stats: Option<ApiUserProfile>,
    stats_slot: Option<HttpSlot>,
}

impl RankedView {
    fn cooldown_left(&self) -> Option<u64> {
        let left = self.cooldown_until? - now_secs();
        (left > 0.0).then(|| left.ceil() as u64)
    }

    fn casual_missing(&self) -> Option<i64> {
        let played = self.stats.as_ref()?.casual_matches;
        (played < config::RANKED_MIN_CASUAL).then(|| config::RANKED_MIN_CASUAL - played)
    }

    fn since(&self) -> Option<f64> {
        match self.search {
            Search::Idle => None,
            Search::Searching { since } | Search::Found { since, .. } => Some(since),
        }
    }

    pub fn match_found(
        &mut self,
        opponent: String,
        opponent_elo: i32,
        opponent_avatar: Option<String>,
        secs: u32,
    ) -> bool {
        let Some(since) = self.since() else {
            return false;
        };
        self.search = Search::Found {
            since,
            opponent,
            opponent_elo,
            opponent_avatar,
            deadline: now_secs() + f64::from(secs),
            accepted: false,
        };
        true
    }

    pub fn match_cancelled(&mut self, requeued: bool) {
        self.search = match self.since() {
            Some(since) if requeued => Search::Searching { since },
            _ => Search::Idle,
        };
    }

    pub fn cooldown(&mut self, secs: u32) {
        self.search = Search::Idle;
        self.cooldown_until = Some(now_secs() + f64::from(secs));
    }

    pub fn stop(&mut self) {
        self.search = Search::Idle;
    }

    pub fn resume_search(&mut self) -> bool {
        let Some(since) = self.since() else {
            return false;
        };
        self.search = Search::Searching { since };
        true
    }
}

struct Buttons {
    main: Rect,
    second: Rect,
}

fn buttons(view: View) -> Buttons {
    let y = view.h / 2.0 + 120.0;
    Buttons {
        main: Rect::at(view.w / 2.0 - 230.0, y, 220.0, 54.0),
        second: Rect::at(view.w / 2.0 + 10.0, y, 220.0, 54.0),
    }
}

fn single_button(view: View) -> Rect {
    Rect::at(view.w / 2.0 - 140.0, view.h / 2.0 + 120.0, 280.0, 54.0)
}

pub fn enter(state: &mut State) {
    state.notice.clear();
    state.room = None;
    let ranked = &mut state.ranked;
    ranked.search = Search::Idle;
    ranked.stats_slot = state
        .auth
        .as_ref()
        .map(|auth| http::get_user(&auth.user_id, Some(auth.token.clone())));
    state.screen = Screen::Ranked;
}

fn exit(state: &mut State) {
    state.conn.disconnect();
    state.screen = Screen::PlayMenu;
}

fn start_search(state: &mut State) {
    state.notice.clear();
    state.ranked.search = Search::Searching { since: now_secs() };
    if state.conn.is_live() {
        state.conn.send(&ClientMessage::JoinQueue);
    } else {
        state.conn.connect(now_secs());
    }
}

fn stop_search(state: &mut State) {
    state.conn.send(&ClientMessage::LeaveQueue);
    state.ranked.search = Search::Idle;
}

fn matched(state: &State) -> bool {
    state.room.as_ref().is_some_and(|r| r.info.ranked.is_some())
}

fn poll_stats(state: &mut State) {
    if let Some(Ok(stats)) = http::take_json(&mut state.ranked.stats_slot) {
        state.ranked.stats = Some(stats);
    }
}

pub fn update(app: &App, state: &mut State) {
    poll_stats(state);
    if matched(state) {
        return;
    }
    let view = state.ui.view();
    let escape = app.keyboard.was_pressed(KeyCode::Escape);
    match state.ranked.search {
        Search::Idle => {
            let can_search = state.ranked.cooldown_left().is_none() && state.ranked.casual_missing().is_none();
            let b = buttons(view);
            if can_search && (state.ui.clicked(b.main) || app.keyboard.was_pressed(KeyCode::Enter)) {
                start_search(state);
            } else if state.ui.clicked(b.second) || escape {
                exit(state);
            }
        }
        Search::Searching { .. } => {
            if state.ui.clicked(single_button(view)) || escape {
                stop_search(state);
            }
        }
        Search::Found { accepted, .. } => {
            let b = buttons(view);
            let decline = if accepted { single_button(view) } else { b.second };
            if !accepted && (state.ui.clicked(b.main) || app.keyboard.was_pressed(KeyCode::Enter)) {
                if let Search::Found { accepted, .. } = &mut state.ranked.search {
                    *accepted = true;
                }
                state.conn.send(&ClientMessage::AcceptMatch);
            } else if state.ui.clicked(decline) || escape {
                stop_search(state);
            }
        }
    }
}

const PORTRAIT_R: f32 = 56.0;

/// Who the series is against: their portrait, name and ELO, centred on `cx`
/// around `cy`.
fn draw_opponent(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &Fonts,
    (cx, cy): (f32, f32),
    name: &str,
    elo: i32,
    picture: Option<&Texture>,
) {
    let pal = ui.palette();
    let who = Persona {
        name,
        glow: 0.0,
        picture,
    };
    ui::portrait(draw, &pal, fonts, (cx, cy - 105.0), PORTRAIT_R, &who);
    draw.sharp_text(&fonts.display, name)
        .position(cx, cy - 25.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(pal.text);
    let elo = format!("ELO {elo}");
    let pill = Pill {
        text: &elo,
        color: theme::GOLD,
        size: theme::size::LABEL,
    };
    pill.draw(draw, fonts, (cx - pill.width(fonts) / 2.0, cy + 15.0));
}

fn opponent_avatar(state: &State) -> Option<&str> {
    let in_series = state.room.as_ref().and_then(|r| r.info.ranked.as_ref());
    match (in_series, &state.ranked.search) {
        (Some(ranked), _) => ranked.opponent_avatar.as_deref(),
        (None, Search::Found { opponent_avatar, .. }) => opponent_avatar.as_deref(),
        _ => None,
    }
}

fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn draw(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let (cx, cy) = (view.w / 2.0, view.h / 2.0);
    let fonts = &state.fonts;
    let picture = state.images.get_opt(gfx, opponent_avatar(state));
    let mut draw = state.ui.screen_canvas(gfx);

    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&fonts.display, "Classé")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    let line = |draw: &mut Draw, text: &str, y: f32, size: f32, color: Color| {
        draw.sharp_text(&fonts.text, text)
            .position(cx, y)
            .size(size)
            .h_align_center()
            .v_align_middle()
            .color(color);
    };
    let first_to = format!("Premier à {} manches", config::RANKED_WINS);

    if let Some((lobby, ranked)) = state
        .room
        .as_ref()
        .and_then(|r| Some((&r.info, r.info.ranked.as_ref()?)))
    {
        line(
            &mut draw,
            "Adversaire trouvé",
            cy - 200.0,
            theme::size::HEADING,
            pal.text_dim,
        );
        let at = (cx, cy);
        draw_opponent(
            &mut draw,
            &state.ui,
            fonts,
            at,
            &ranked.opponent,
            ranked.opponent_elo,
            picture.as_ref(),
        );
        line(&mut draw, &first_to, cy + 55.0, theme::size::LABEL, pal.text_muted);
        if let Some(n) = lobby.countdown {
            draw.sharp_text(&fonts.display, &n.to_string())
                .position(cx, cy + 135.0)
                .size(theme::size::HERO)
                .h_align_center()
                .v_align_middle()
                .color(pal.title);
        }
        state.ui.render(gfx, &draw);
        return;
    }

    let ranked = &state.ranked;
    match &ranked.search {
        Search::Idle => {
            let elo = state.auth.as_ref().map_or(0, |a| a.elo);
            line(&mut draw, "Votre ELO", cy - 170.0, theme::size::LABEL, pal.text_muted);
            draw.sharp_text(&fonts.display, &elo.to_string())
                .position(cx, cy - 115.0)
                .size(theme::size::HERO)
                .h_align_center()
                .v_align_middle()
                .color(pal.title);
            if let Some(stats) = &ranked.stats {
                let record = format!(
                    "Séries: {} gagnées sur {}",
                    stats.ranked_series_won, stats.ranked_series
                );
                line(&mut draw, &record, cy - 50.0, theme::size::EMPHASIS, pal.text);
            }
            line(&mut draw, &first_to, cy, theme::size::LABEL, pal.text_dim);
            let rule = format!(
                "Quand une partie sera trouvée, vous aurez {} secondes pour l'accepter.",
                config::MATCH_ACCEPT_SECS,
            );
            line(&mut draw, &rule, cy + 35.0, theme::size::BODY, pal.text_muted);

            let (label, enabled) = match (ranked.casual_missing(), ranked.cooldown_left()) {
                (Some(n), _) => {
                    let plural = if n > 1 { "s" } else { "" };
                    let why = format!("Jouez encore {n} partie{plural} amicale{plural} pour débloquer le classé.");
                    line(&mut draw, &why, cy + 75.0, theme::size::BODY, theme::WARNING_TEXT);
                    ("Rechercher une partie".to_string(), false)
                }
                (None, Some(left)) => (format!("Disponible dans {}", clock(left)), false),
                (None, None) => ("Rechercher une partie".to_string(), true),
            };
            let b = buttons(view);
            state
                .ui
                .button_enabled(&mut draw, fonts, b.main, label.as_str(), enabled);
            state.ui.button(&mut draw, fonts, b.second, "Retour");
        }
        Search::Searching { since } => {
            let text = format!("Recherche d'un adversaire{}", state.ui.waiting_dots());
            line(&mut draw, &text, cy - 40.0, theme::size::HEADING, pal.text);
            let waited = clock((now_secs() - since).max(0.0) as u64);
            line(&mut draw, &waited, cy + 10.0, theme::size::EMPHASIS, pal.text_dim);
            state.ui.button(&mut draw, fonts, single_button(view), "Annuler");
        }
        Search::Found {
            opponent,
            opponent_elo,
            deadline,
            accepted,
            ..
        } => {
            line(
                &mut draw,
                "Adversaire trouvé",
                cy - 200.0,
                theme::size::HEADING,
                pal.text_dim,
            );
            let at = (cx, cy);
            draw_opponent(
                &mut draw,
                &state.ui,
                fonts,
                at,
                opponent,
                *opponent_elo,
                picture.as_ref(),
            );
            let total = f64::from(config::MATCH_ACCEPT_SECS);
            let left = (deadline - now_secs()).clamp(0.0, total);
            let bar = Rect::at(cx - 200.0, cy + 48.0, 400.0, 10.0);
            draw.rect((bar.x, bar.y), (bar.w, bar.h))
                .corner_radius(5.0)
                .color(pal.surface);
            draw.rect((bar.x, bar.y), (bar.w * (left / total) as f32, bar.h))
                .corner_radius(5.0)
                .color(if left < 3.0 { theme::DANGER } else { pal.accent });
            line(
                &mut draw,
                &format!("{} s", left.ceil() as u64),
                cy + 80.0,
                theme::size::EMPHASIS,
                pal.text_dim,
            );
            if *accepted {
                line(
                    &mut draw,
                    "En attente de l'adversaire...",
                    cy + 104.0,
                    theme::size::LABEL,
                    pal.text_muted,
                );
                state.ui.button(&mut draw, fonts, single_button(view), "Annuler");
            } else {
                let b = buttons(view);
                state.ui.button(&mut draw, fonts, b.main, "Accepter");
                state.ui.button(&mut draw, fonts, b.second, "Refuser");
            }
        }
    }

    if let Some((msg, color)) = state.notice.shown(&pal) {
        line(&mut draw, msg, view.h - 60.0, theme::size::EMPHASIS, color);
    }
    state.ui.render(gfx, &draw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn searching(since: f64) -> RankedView {
        RankedView {
            search: Search::Searching { since },
            ..RankedView::default()
        }
    }

    #[test]
    fn a_requeue_keeps_the_time_already_waited() {
        let mut view = searching(5.0);
        view.match_found("o".into(), 1000, None, 10);
        assert!(matches!(
            view.search,
            Search::Found {
                since: 5.0,
                accepted: false,
                ..
            }
        ));
        view.match_cancelled(true);
        assert!(matches!(view.search, Search::Searching { since: 5.0 }));
    }

    #[test]
    fn a_missed_match_stops_the_search_and_waits_out_the_cooldown() {
        let mut view = searching(5.0);
        view.match_found("o".into(), 1000, None, 10);
        view.match_cancelled(false);
        view.cooldown(60);
        assert!(matches!(view.search, Search::Idle));
        assert!(view.cooldown_left().is_some_and(|s| (59..=60).contains(&s)));
    }

    #[test]
    fn a_match_found_after_cancelling_is_ignored() {
        let mut view = RankedView::default();
        assert!(!view.match_found("o".into(), 1000, None, 10));
        assert!(matches!(view.search, Search::Idle));
    }

    #[test]
    fn a_reconnection_resumes_only_an_ongoing_search() {
        let mut view = RankedView::default();
        assert!(!view.resume_search());
        let mut view = searching(5.0);
        view.match_found("o".into(), 1000, None, 10);
        assert!(view.resume_search());
        assert!(matches!(view.search, Search::Searching { since: 5.0 }));
    }

    #[test]
    fn too_few_casual_games_block_the_search() {
        let mut view = RankedView::default();
        assert_eq!(view.casual_missing(), None, "unknown until the profile loads");
        view.stats = Some(ApiUserProfile {
            casual_matches: config::RANKED_MIN_CASUAL - 2,
            ..ApiUserProfile::default()
        });
        assert_eq!(view.casual_missing(), Some(2));
    }
}
