use aero::draw::Draw;
use aero::geom::Color;
use aero::image::Image;
use aero::input::KeyCode;
use shared::{config, ClientMessage};

use crate::connection::{now_secs, Connection};
use crate::http::{self, HttpSlot};
use crate::nav::Transition;
use crate::state::{ApiUserProfile, Room, Screen, State};
use crate::theme::{self, Palette};
use crate::tick::Tick;
use crate::ui::{self, widgets, Face, Fonts, Gui, Persona, Pill, Rect, Typeset, View, FILL};

const PORTRAIT_R: f32 = 56.0;

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
    let row = Rect::centered_on(view.w / 2.0, view.h / 2.0 + 120.0, 460.0, 54.0);
    let [main, second] = row.columns(20.0, [FILL, FILL]);
    Buttons { main, second }
}

fn single_button(view: View) -> Rect {
    Rect::centered_on(view.w / 2.0, view.h / 2.0 + 120.0, 280.0, 54.0)
}

pub fn enter(state: &mut State) -> Transition {
    state.notice.clear();
    state.room = None;
    let ranked = &mut state.ranked;
    ranked.search = Search::Idle;
    ranked.stats_slot = state.auth.as_ref().map(|auth| http::get_user(&auth.user_id, Some(auth.token.clone())));

    Transition::To(Screen::Ranked)
}

fn exit(state: &mut State) -> Transition {
    state.net.conn.disconnect();

    Transition::To(Screen::PlayMenu)
}

fn start_search(state: &mut State) {
    state.notice.clear();
    state.ranked.search = Search::Searching { since: now_secs() };

    if state.net.conn.is_live() {
        state.net.conn.send(&ClientMessage::JoinQueue);
    } else {
        state.net.conn.connect(now_secs());
    }
}

fn stop_search(conn: &mut Connection, ranked: &mut RankedView) {
    conn.send(&ClientMessage::LeaveQueue);
    ranked.search = Search::Idle;
}

fn poll_stats(ranked: &mut RankedView) {
    if let Some(Ok(stats)) = http::take_json(&mut ranked.stats_slot) {
        ranked.stats = Some(stats);
    }
}

/// The screen's buttons, as they stand for the search's state.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pressed {
    /// Search, or accept the match found.
    Main,
    /// Back, or refuse the match found.
    Second,
    /// Cancel the search or the accepted match.
    Single,
}

/// What Enter and Escape stand for; the clicks are in [`show`].
pub fn update(tick: &Tick, state: &mut State) -> Transition {
    poll_stats(&mut state.ranked);
    if state.room.as_ref().is_some_and(|r| r.info.ranked.is_some()) {
        return Transition::Stay;
    }
    let escape = match state.ranked.search {
        Search::Idle => Pressed::Second,
        Search::Searching { .. } | Search::Found { accepted: true, .. } => Pressed::Single,
        Search::Found { accepted: false, .. } => Pressed::Second,
    };
    if tick.pressed(KeyCode::Enter) {
        act(state, Pressed::Main)
    } else if tick.back() {
        act(state, escape)
    } else {
        Transition::Stay
    }
}

fn act(state: &mut State, pressed: Pressed) -> Transition {
    match (&state.ranked.search, pressed) {
        (Search::Idle, Pressed::Main) => {
            if state.ranked.cooldown_left().is_none() && state.ranked.casual_missing().is_none() {
                start_search(state);
            }
        }
        (Search::Idle, Pressed::Second) => return exit(state),
        (Search::Found { accepted: false, .. }, Pressed::Main) => {
            if let Search::Found { accepted, .. } = &mut state.ranked.search {
                *accepted = true;
            }
            state.net.conn.send(&ClientMessage::AcceptMatch);
        }
        (Search::Searching { .. } | Search::Found { .. }, Pressed::Second | Pressed::Single) => {
            stop_search(&mut state.net.conn, &mut state.ranked);
        }
        _ => {}
    }
    Transition::Stay
}

/// Who the series is against: their portrait, name and ELO, centred on `cx`
/// around `cy`.
fn draw_opponent(
    draw: &mut Draw,
    pal: &Palette,
    fonts: &mut Fonts,
    (cx, cy): (f32, f32),
    name: &str,
    elo: i32,
    picture: Option<&Image>,
) {
    let who = Persona { name, glow: 0.0, picture };
    let elo = format!("ELO {elo}");
    let pill = Pill { text: &elo, color: theme::GOLD, size: theme::size::LABEL };

    ui::portrait(draw, pal, fonts, (cx, cy - 105.0), PORTRAIT_R, &who);
    fonts.write(draw, Face::Display, name).center(cx, cy - 25.0).size(theme::size::TITLE).color(pal.text);
    let left = cx - pill.width(fonts) / 2.0;
    pill.draw(draw, fonts, (left, cy + 15.0));
}

fn opponent_avatar<'a>(room: Option<&'a Room>, ranked: &'a RankedView) -> Option<&'a str> {
    let in_series = room.and_then(|r| r.info.ranked.as_ref());
    match (in_series, &ranked.search) {
        (Some(ranked), _) => ranked.opponent_avatar.as_deref(),
        (None, Search::Found { opponent_avatar, .. }) => opponent_avatar.as_deref(),
        _ => None,
    }
}

fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    match paint(ui, gui, state) {
        Some(pressed) => act(state, pressed),
        None => Transition::Stay,
    }
}

/// Draws the screen; returns the button clicked.
fn paint(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &State) -> Option<Pressed> {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let (cx, cy) = (view.w / 2.0, view.h / 2.0);
    let picture = gui.images.get_opt(opponent_avatar(state.room.as_ref(), &state.ranked));
    let b = buttons(view);
    let line = |draw: &mut Draw, fonts: &mut Fonts, text: &str, y: f32, size: f32, color: Color| {
        fonts.write(draw, Face::Text, text).center(cx, y).size(size).color(color);
    };
    let first_to = format!("Premier à {} manches", config::RANKED_WINS);
    let found = |draw: &mut Draw, fonts: &mut Fonts, opponent: &str, elo: i32| {
        line(draw, fonts, "Adversaire trouvé", cy - 200.0, theme::size::HEADING, pal.text_dim);
        draw_opponent(draw, &pal, fonts, (cx, cy), opponent, elo, picture.as_ref());
    };

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    gui.ui.page_header(ui.draw, &mut ui.cx.fonts, "Classé");
    let mut pressed = None;

    if let Some((lobby, ranked)) = state.room.as_ref().and_then(|r| Some((&r.info, r.info.ranked.as_ref()?))) {
        found(ui.draw, &mut ui.cx.fonts, &ranked.opponent, ranked.opponent_elo);
        line(ui.draw, &mut ui.cx.fonts, &first_to, cy + 55.0, theme::size::LABEL, pal.text_muted);
        if let Some(n) = lobby.countdown {
            ui.cx
                .fonts
                .write(ui.draw, Face::Display, &n.to_string())
                .center(cx, cy + 135.0)
                .size(theme::size::HERO)
                .color(pal.title);
        }
        return None;
    }

    let ranked = &state.ranked;
    match &ranked.search {
        Search::Idle => pressed = paint_idle(ui, gui, state),
        Search::Searching { since } => {
            let text = format!("Recherche d'un adversaire{}", gui.ui.waiting_dots());
            gui.ui.next_second();
            let waited = clock((now_secs() - since).max(0.0) as u64);

            line(ui.draw, &mut ui.cx.fonts, &text, cy - 40.0, theme::size::HEADING, pal.text);
            line(ui.draw, &mut ui.cx.fonts, &waited, cy + 10.0, theme::size::EMPHASIS, pal.text_dim);
            if widgets::button(ui, "single", single_button(view), "Annuler", true) {
                pressed = Some(Pressed::Single);
            }
        }
        Search::Found { opponent, opponent_elo, deadline, accepted, .. } => {
            let total = f64::from(config::MATCH_ACCEPT_SECS);
            let left = (deadline - now_secs()).clamp(0.0, total);
            let bar = Rect::at(cx - 200.0, cy + 48.0, 400.0, 10.0);
            let urgency = if left < 3.0 { theme::DANGER } else { pal.accent };
            gui.ui.keep_moving();

            found(ui.draw, &mut ui.cx.fonts, opponent, *opponent_elo);
            ui.draw.rect(bar).radius(5.0).color(pal.surface);
            ui.draw
                .rect(Rect::at(bar.x(), bar.y(), bar.w() * (left / total) as f32, bar.h()))
                .radius(5.0)
                .color(urgency);
            line(
                ui.draw,
                &mut ui.cx.fonts,
                &format!("{} s", left.ceil() as u64),
                cy + 80.0,
                theme::size::EMPHASIS,
                pal.text_dim,
            );
            if *accepted {
                line(
                    ui.draw,
                    &mut ui.cx.fonts,
                    "En attente de l'adversaire...",
                    cy + 104.0,
                    theme::size::LABEL,
                    pal.text_muted,
                );
                if widgets::button(ui, "single", single_button(view), "Annuler", true) {
                    pressed = Some(Pressed::Single);
                }
            } else {
                if widgets::button(ui, "main", b.main, "Accepter", true) {
                    pressed = Some(Pressed::Main);
                }
                if widgets::button(ui, "second", b.second, "Refuser", true) {
                    pressed = Some(Pressed::Second);
                }
            }
        }
    }

    if let Some((msg, color)) = state.notice.shown(&pal) {
        line(ui.draw, &mut ui.cx.fonts, msg, view.h - 60.0, theme::size::EMPHASIS, color);
    }
    pressed
}

/// The screen before any search: the player's ELO and record, and the button to search.
fn paint_idle(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &State) -> Option<Pressed> {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let (cx, cy) = (view.w / 2.0, view.h / 2.0);
    let b = buttons(view);
    let ranked = &state.ranked;
    let line = |draw: &mut Draw, fonts: &mut Fonts, text: &str, y: f32, size: f32, color: Color| {
        fonts.write(draw, Face::Text, text).center(cx, y).size(size).color(color);
    };
    let first_to = format!("Premier à {} manches", config::RANKED_WINS);
    let mut pressed = None;
    let elo = state.auth.as_ref().map_or(0, |a| a.elo);
    let rule =
        format!("Quand une partie sera trouvée, vous aurez {} secondes pour l'accepter.", config::MATCH_ACCEPT_SECS);

    line(ui.draw, &mut ui.cx.fonts, "Votre ELO", cy - 170.0, theme::size::LABEL, pal.text_muted);
    ui.cx
        .fonts
        .write(ui.draw, Face::Display, &elo.to_string())
        .center(cx, cy - 115.0)
        .size(theme::size::HERO)
        .color(pal.title);
    if let Some(stats) = &ranked.stats {
        let record = format!("Séries: {} gagnées sur {}", stats.ranked_series_won, stats.ranked_series);
        line(ui.draw, &mut ui.cx.fonts, &record, cy - 50.0, theme::size::EMPHASIS, pal.text);
    }
    line(ui.draw, &mut ui.cx.fonts, &first_to, cy, theme::size::LABEL, pal.text_dim);
    line(ui.draw, &mut ui.cx.fonts, &rule, cy + 35.0, theme::size::BODY, pal.text_muted);

    let (label, enabled) = match (ranked.casual_missing(), ranked.cooldown_left()) {
        (Some(n), _) => {
            let plural = if n > 1 { "s" } else { "" };
            let why = format!("Jouez encore {n} partie{plural} amicale{plural} pour débloquer le classé.");
            line(ui.draw, &mut ui.cx.fonts, &why, cy + 75.0, theme::size::BODY, theme::WARNING_TEXT);
            ("Rechercher une partie".to_string(), false)
        }
        (None, Some(left)) => {
            gui.ui.next_second();
            (format!("Disponible dans {}", clock(left)), false)
        }
        (None, None) => ("Rechercher une partie".to_string(), true),
    };
    if widgets::button(ui, "main", b.main, label.as_str(), enabled) {
        pressed = Some(Pressed::Main);
    }
    if widgets::button(ui, "second", b.second, "Retour", true) {
        pressed = Some(Pressed::Second);
    }
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn searching(since: f64) -> RankedView {
        RankedView { search: Search::Searching { since }, ..RankedView::default() }
    }

    #[test]
    fn a_requeue_keeps_the_time_already_waited() {
        let mut view = searching(5.0);
        view.match_found("o".into(), 1000, None, 10);
        assert!(matches!(view.search, Search::Found { since: 5.0, accepted: false, .. }));
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
        view.stats =
            Some(ApiUserProfile { casual_matches: config::RANKED_MIN_CASUAL - 2, ..ApiUserProfile::default() });
        assert_eq!(view.casual_missing(), Some(2));
    }
}
