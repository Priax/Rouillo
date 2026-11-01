use std::sync::Arc;

use notan::prelude::*;
use shared::config::CLIENT_SIM_DT;
use shared::{Board, Playback, Replay};

use crate::draw::{self, Hud};
use crate::http::{self, HttpSlot};
use crate::state::{GameSession, ProfileCore, Rival, Screen, State};
use crate::ui::{Icon, Rect, Status, View};
use crate::{logic, theme};

const MAX_REPLAY_SIZE: usize = 64 << 20;
const SPEEDS: [f32; 3] = [1.0, 2.0, 4.0];
const MAX_STEPS_PER_FRAME: u32 = 40;

/// A replay on its way from the server: whose board to show large, and
/// where to go back to.
pub struct Loading {
    slot: HttpSlot,
    focus: u8,
    back: Screen,
}

/// Once a game is saved, the choice of keeping its replay.
pub struct KeepOffer {
    id: String,
    game: u32,
    slot: Option<HttpSlot>,
    status: Status,
    kept: bool,
}

impl KeepOffer {
    pub const fn new(id: String, game: u32) -> Self {
        Self {
            id,
            game,
            slot: None,
            status: Status::Empty,
            kept: false,
        }
    }

    pub const fn is_for(&self, game: u32) -> bool {
        self.game == game
    }
}

fn favorite_url(match_id: &str) -> String {
    http::api_url(&format!("matches/{match_id}/favorite"))
}

/// Asks the server to keep the replay of a match, or to let it go.
pub fn set_favorite(match_id: &str, on: bool, token: String) -> HttpSlot {
    let slot = http::new_slot();
    if on {
        http::post_empty(favorite_url(match_id), Some(token), Arc::clone(&slot));
    } else {
        http::delete_req(favorite_url(match_id), Some(token), Arc::clone(&slot));
    }
    slot
}

fn keep_button(view: View) -> Rect {
    Rect::at(view.w - 236.0, 56.0, 220.0, 44.0)
}

fn game_finished(state: &State) -> bool {
    state.session().is_some_and(GameSession::decided) || state.room.as_ref().is_some_and(|r| r.series_over.is_some())
}

pub fn update_keep(state: &mut State) {
    let finished = game_finished(state);
    let view = state.ui.view();
    let Some(offer) = state.room.as_mut().and_then(|r| r.keep.as_mut()) else {
        return;
    };
    match http::take_done(&mut offer.slot) {
        Some(Ok(())) => offer.kept = true,
        Some(Err(e)) => offer.status = Status::error(e),
        None => {}
    }
    if finished && !offer.kept && offer.slot.is_none() && state.ui.clicked(keep_button(view)) {
        if let Some(auth) = &state.auth {
            offer.status = Status::Empty;
            offer.slot = Some(set_favorite(&offer.id, true, auth.token.clone()));
        }
    }
}

pub fn draw_keep(d: &mut notan::draw::Draw, state: &State) {
    use crate::ui::SharpText;
    let Some(offer) = state.room.as_ref().and_then(|r| r.keep.as_ref()) else {
        return;
    };
    if !game_finished(state) || state.auth.is_none() {
        return;
    }
    let rect = keep_button(state.ui.view());
    let label = if offer.kept {
        "Replay gardé"
    } else {
        "Garder le replay"
    };
    let enabled = !offer.kept && offer.slot.is_none();
    state.ui.button_enabled(d, &state.fonts, rect, label, enabled);
    if let Some((msg, color)) = offer.status.shown(&state.ui.palette()) {
        d.sharp_text(&state.fonts.text, msg)
            .position(rect.x + rect.w, rect.y + rect.h + 16.0)
            .size(theme::size::SMALL)
            .h_align_right()
            .v_align_middle()
            .color(color);
    }
}

/// A finished game played again, as a spectator sees it.
pub struct ReplayViewer {
    pub session: GameSession,
    start: Playback,
    playback: Playback,
    paused: bool,
    speed: usize,
    /// Time not played yet, in seconds of the game.
    behind: f32,
    back: Screen,
}

impl ReplayViewer {
    pub fn new(replay: Replay, focus: u8, back: Screen) -> Self {
        let start = replay.play();
        let mut viewer = Self {
            session: GameSession::new(0, Vec::new()),
            playback: start.clone(),
            start,
            paused: false,
            speed: 0,
            behind: 0.0,
            back,
        };
        viewer.watch(focus);
        viewer
    }

    /// Shows `focus`'s board large and the others beside it.
    fn watch(&mut self, focus: u8) {
        let players = &self.playback.replay().players;
        let focus = focus.clamp(1, u8::try_from(players.len()).unwrap_or(u8::MAX).max(1));
        let mut session = GameSession::new(0, players.iter().map(|p| p.name.clone()).collect());
        session.focus = focus;
        session.team = players.get(usize::from(focus) - 1).map_or(0, |p| p.team);
        for (i, player) in players.iter().enumerate() {
            let slot = u8::try_from(i + 1).unwrap_or(u8::MAX);
            if slot != focus {
                session.add_rival(Rival::new(slot, player.team, Board::new(0, &shared::Rules::default())));
            }
        }
        self.session = session;
        self.show();
    }

    fn show(&mut self) {
        let boards = self.playback.boards();
        let focus = usize::from(self.session.focus) - 1;
        if let Some(board) = boards.get(focus) {
            self.session.predicted_board = board.clone();
            self.session.board = board.clone();
        }
        for rival in &mut self.session.rivals {
            if let Some(board) = boards.get(usize::from(rival.slot) - 1) {
                rival.board = board.clone();
            }
        }
    }

    fn advance(&mut self) {
        let events = self.playback.step();
        self.show();
        if events
            .get(usize::from(self.session.focus) - 1)
            .is_some_and(|e| e.locked)
        {
            crate::audio::play_lock();
        }
        logic::announce_events(&mut self.session);
    }

    pub fn restart(&mut self) {
        self.playback = self.start.clone();
        let focus = self.session.focus;
        self.watch(focus);
        self.paused = false;
    }

    /// Shows the next player's board large.
    fn next_player(&mut self) {
        let players = u8::try_from(self.playback.replay().players.len()).unwrap_or(u8::MAX);
        let next = self.session.focus % players.max(1) + 1;
        let (paused, speed) = (self.paused, self.speed);
        self.watch(next);
        (self.paused, self.speed) = (paused, speed);
    }

    fn over(&self) -> bool {
        self.playback.done()
    }

    pub fn step(&mut self, dt: f32) {
        if !self.paused && !self.over() {
            self.behind += dt * SPEEDS[self.speed];
            let mut steps = 0;
            while self.behind >= CLIENT_SIM_DT && steps < MAX_STEPS_PER_FRAME && !self.over() {
                self.behind -= CLIENT_SIM_DT;
                self.advance();
                steps += 1;
            }
            if steps == MAX_STEPS_PER_FRAME {
                self.behind = 0.0;
            }
        }
        logic::animate(&mut self.session, dt);
    }

    /// The time played and the game's length, as `m:ss / m:ss`.
    fn clock(&self) -> String {
        let secs = |ticks: u32| ticks / shared::config::SERVER_TICK_HZ as u32;
        let (now, total) = (secs(self.playback.tick()), secs(self.playback.replay().ticks));
        format!("{}:{:02} / {}:{:02}", now / 60, now % 60, total / 60, total % 60)
    }
}

/// Asks the server for a match's replay, to show `focus`'s board large.
pub fn open(state: &mut State, match_id: &str, focus: u8, back: Screen) {
    if let Some(core) = asked_from(state, back) {
        core.replay_status.clear();
    }
    let slot = http::new_slot();
    let url = http::api_url(&format!("matches/{match_id}/replay"));
    http::get(url, None, Arc::clone(&slot));
    state.replay_loading = Some(Loading { slot, focus, back });
}

/// Starts the replay once it has come: an error stays on the screen it was
/// asked from.
pub fn poll(state: &mut State) {
    let Some(loading) = state.replay_loading.as_mut() else {
        return;
    };
    let Some(result) = http::poll(&loading.slot) else {
        return;
    };
    let Some(Loading { focus, back, .. }) = state.replay_loading.take() else {
        return;
    };
    let replay = match result {
        Ok(resp) if resp.ok => shared::decode_bounded::<Replay>(&resp.bytes, MAX_REPLAY_SIZE),
        _ => None,
    };
    match replay {
        Some(replay) => {
            state.replay = Some(ReplayViewer::new(replay, focus, back));
            state.screen = Screen::Replay;
        }
        None => {
            if let Some(core) = asked_from(state, back) {
                core.replay_status = Status::error("Impossible de charger ce replay.");
            }
        }
    }
}

/// The profile whose history a replay was asked from.
fn asked_from(state: &mut State, back: Screen) -> Option<&mut ProfileCore> {
    match back {
        Screen::OtherProfile => state.other_profile.as_mut().map(|p| &mut p.core),
        _ => state.profile.as_mut().map(|p| &mut p.core),
    }
}

/// The controls under the boards: play or pause, speed, whose board is
/// large, and the way out.
fn controls(view: View) -> [Rect; 4] {
    const W: f32 = 150.0;
    const H: f32 = 46.0;
    const GAP: f32 = 12.0;
    let total = 4.0 * W + 3.0 * GAP;
    let (x, y) = ((view.w - total) / 2.0, view.h - H - 14.0);
    std::array::from_fn(|i| Rect::at(x + i as f32 * (W + GAP), y, W, H))
}

pub fn update(app: &mut App, state: &mut State) {
    let dt = app.timer.delta_f32();
    let view = state.ui.view();
    let Some(viewer) = state.replay.as_mut() else {
        state.screen = Screen::Menu;
        return;
    };
    let [play, speed, player, quit] = controls(view);
    let (leave_row, again_row) = draw::exit_rows(view, true);
    let over = viewer.over();
    let keys = &app.keyboard;
    if (over && state.ui.bar_clicked(again_row)) || keys.was_pressed(KeyCode::KeyR) {
        viewer.restart();
    } else if state.ui.clicked(play) || keys.was_pressed(KeyCode::Space) {
        viewer.paused = !viewer.paused;
    } else if state.ui.clicked(speed) || keys.was_pressed(KeyCode::ArrowRight) {
        viewer.speed = (viewer.speed + 1) % SPEEDS.len();
    } else if state.ui.clicked(player) || keys.was_pressed(KeyCode::Tab) {
        viewer.next_player();
    }
    let leave = state.ui.clicked(quit) || (over && state.ui.bar_clicked(leave_row));
    if leave || keys.was_pressed(KeyCode::Escape) {
        state.screen = viewer.back;
        state.replay = None;
        return;
    }
    viewer.step(dt);
}

pub fn draw(app: &mut App, gfx: &mut Graphics, state: &State) {
    let Some(viewer) = state.replay.as_ref() else {
        return;
    };
    draw::draw_game(app, gfx, &viewer.session, &state.ui, &state.fonts, Hud::Replay);
    let view = state.ui.view();
    let mut d = state.ui.canvas(gfx);
    if !viewer.over() {
        let [play, speed, player, quit] = controls(view);
        let fonts = &state.fonts;
        let icon = if viewer.paused { Icon::Play } else { Icon::Pause };
        state.ui.button(&mut d, fonts, play, icon);
        let label = format!("Vitesse x{}", SPEEDS[viewer.speed]);
        state.ui.button(&mut d, fonts, speed, label.as_str());
        let multi = viewer.playback.replay().players.len() > 1;
        state.ui.button_enabled(&mut d, fonts, player, "Joueur suivant", multi);
        state.ui.button(&mut d, fonts, quit, "Quitter");
        draw_clock(&mut d, state, &viewer.clock(), play);
    }
    state.ui.render(gfx, &d);
}

fn draw_clock(d: &mut notan::draw::Draw, state: &State, clock: &str, play: Rect) {
    use crate::ui::SharpText;
    d.sharp_text(&state.fonts.display, clock)
        .position(play.x - 20.0, play.y + play.h / 2.0)
        .size(theme::size::LABEL)
        .h_align_right()
        .v_align_middle()
        .color(state.ui.palette().text_dim);
}
