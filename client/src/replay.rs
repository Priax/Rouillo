use std::sync::Arc;

use aero::draw::Draw;
use aero::input::KeyCode;
use shared::config::CLIENT_SIM_DT;
use shared::{Board, Playback, Replay};

use crate::draw::{self, Hud};
use crate::http::{self, HttpSlot};
use crate::nav::{Nav, Transition};
use crate::state::{GameSession, Player, Rival, Room, Screen, State};
use crate::tick::Tick;
use crate::ui::{widgets, Face, Fonts, Gui, Icon, Rect, Status, Typeset, Ui, View};
use crate::{logic, theme};

const MAX_REPLAY_SIZE: usize = 64 << 20;
const SPEEDS: [f32; 3] = [1.0, 2.0, 4.0];
const MAX_STEPS_PER_FRAME: u32 = 40;

/// A replay on its way from the server, and whose board to show large.
pub struct Loading {
    slot: HttpSlot,
    focus: u8,
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
        Self { id, game, slot: None, status: Status::Empty, kept: false }
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

/// The answer to keeping the replay; the click is in [`show_keep`].
pub fn update_keep(state: &mut State) {
    let Some(offer) = state.room.as_mut().and_then(|r| r.keep.as_mut()) else {
        return;
    };
    match http::take_done(&mut offer.slot) {
        Some(Ok(())) => offer.kept = true,
        Some(Err(e)) => offer.status = Status::error(e),
        None => {}
    }
}

/// The button that keeps a finished game's replay.
pub fn show_keep(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) {
    let finished = state.room.as_ref().is_some_and(Room::finished);
    let State { room, auth, .. } = state;
    let Some(offer) = room.as_mut().and_then(|r| r.keep.as_mut()) else {
        return;
    };
    let Some(auth) = auth.as_ref().filter(|_| finished) else {
        return;
    };

    let rect = keep_button(gui.ui.view());
    let label = if offer.kept { "Replay gardé" } else { "Garder le replay" };
    let enabled = !offer.kept && offer.slot.is_none();

    if widgets::button(ui, "keep replay", rect, label, enabled) {
        offer.status = Status::Empty;
        offer.slot = Some(set_favorite(&offer.id, true, auth.token.clone()));
    }
    if let Some((msg, color)) = offer.status.shown(&gui.ui.palette()) {
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, msg)
            .right(rect.x() + rect.w(), rect.y() + rect.h() + 16.0)
            .size(theme::size::SMALL)
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
}

impl ReplayViewer {
    pub fn new(replay: Replay, focus: u8) -> Self {
        let start = replay.play();
        let mut viewer = Self {
            session: GameSession::new(0, Vec::new()),
            playback: start.clone(),
            start,
            paused: false,
            speed: 0,
            behind: 0.0,
        };
        viewer.watch(focus);

        viewer
    }

    /// Shows `focus`'s board large and the others beside it.
    fn watch(&mut self, focus: u8) {
        let players = &self.playback.replay().players;
        let focus = focus.clamp(1, u8::try_from(players.len()).unwrap_or(u8::MAX).max(1));
        let mut session = GameSession::new(0, players.iter().map(|p| p.name.clone()).collect());
        session.large.slot = focus;
        session.large.team = players.get(usize::from(focus) - 1).map_or(0, |p| p.team);

        for (i, player) in players.iter().enumerate() {
            let slot = u8::try_from(i + 1).unwrap_or(u8::MAX);
            if slot != focus {
                session.add_rival(Rival::new(Player::new(slot, player.team, Board::new(0, &shared::Rules::default()))));
            }
        }

        self.session = session;
        self.show();
    }

    fn show(&mut self) {
        let boards = self.playback.boards();
        let focus = usize::from(self.session.large.slot) - 1;

        if let Some(board) = boards.get(focus) {
            self.session.predicted_board = board.clone();
            self.session.large.board = board.clone();
        }
        for rival in &mut self.session.rivals {
            if let Some(board) = boards.get(usize::from(rival.player.slot) - 1) {
                rival.player.board = board.clone();
            }
        }
    }

    fn advance(&mut self) {
        let events = self.playback.step();
        self.show();
        if events.get(usize::from(self.session.large.slot) - 1).is_some_and(|e| e.locked) {
            crate::audio::play_lock();
        }
        logic::announce_events(&mut self.session);
    }

    pub fn restart(&mut self) {
        self.playback = self.start.clone();
        let focus = self.session.large.slot;
        self.watch(focus);
        self.paused = false;
    }

    /// Shows the next player's board large.
    fn next_player(&mut self) {
        let players = u8::try_from(self.playback.replay().players.len()).unwrap_or(u8::MAX);
        let next = self.session.large.slot % players.max(1) + 1;
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
pub fn open(state: &mut State, match_id: &str, focus: u8) {
    let slot = http::new_slot();
    let url = http::api_url(&format!("matches/{match_id}/replay"));
    http::get(url, None, Arc::clone(&slot));
    state.replay_loading = Some(Loading { slot, focus });
}

/// Opens the replay once it has come, over the history it was asked from.
/// An error stays there. A player who left it no longer wants the replay.
pub fn poll(state: &mut State, nav: &mut Nav) {
    let Some(loading) = state.replay_loading.as_mut() else {
        return;
    };
    let Some(result) = http::poll(&loading.slot) else {
        return;
    };
    let Some(Loading { focus, .. }) = state.replay_loading.take() else {
        return;
    };
    let replay = match result {
        Ok(resp) if resp.ok => shared::decode_bounded::<Replay>(&resp.bytes, MAX_REPLAY_SIZE),
        _ => None,
    };
    match (replay, nav.current_mut().history_mut()) {
        (Some(replay), Some(_)) => {
            let viewer = Box::new(ReplayViewer::new(replay, focus));
            nav.apply(Transition::Push(Screen::Replay(viewer)));
        }
        (None, Some(core)) => core.replay_status = Status::error("Impossible de charger ce replay."),
        (_, None) => {}
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

/// The keys, and the replay's time; the clicks are in [`show`].
pub fn update(tick: &Tick, viewer: &mut ReplayViewer) -> Transition {
    if tick.pressed(KeyCode::KeyR) {
        viewer.restart();
    } else if tick.pressed(KeyCode::Space) {
        viewer.paused = !viewer.paused;
    } else if tick.pressed(KeyCode::ArrowRight) {
        viewer.speed = (viewer.speed + 1) % SPEEDS.len();
    } else if tick.pressed(KeyCode::Tab) {
        viewer.next_player();
    }
    if tick.back() {
        return Transition::Pop;
    }
    viewer.step(tick.dt);
    Transition::Stay
}

pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, viewer: &mut ReplayViewer) -> Transition {
    let clicks = draw::draw_game(ui, &viewer.session, gui, Hud::Replay);
    if viewer.over() {
        if clicks.leave {
            return Transition::Pop;
        }
        if clicks.back {
            viewer.restart();
        }
        return Transition::Stay;
    }

    let [play, speed, player, quit] = controls(gui.ui.view());
    let icon = if viewer.paused { Icon::Play } else { Icon::Pause };
    let label = format!("Vitesse x{}", SPEEDS[viewer.speed]);
    let multi = viewer.playback.replay().players.len() > 1;

    let toggled = widgets::button(ui, "play", play, icon, true);
    let faster = widgets::button(ui, "speed", speed, label.as_str(), true);
    let next = widgets::button(ui, "player", player, "Joueur suivant", multi);
    let quit = widgets::button(ui, "quit", quit, "Quitter", true);
    draw_clock(ui.draw, &gui.ui, &mut ui.cx.fonts, &viewer.clock(), play);

    if toggled {
        viewer.paused = !viewer.paused;
    } else if faster {
        viewer.speed = (viewer.speed + 1) % SPEEDS.len();
    } else if next {
        viewer.next_player();
    }
    if quit {
        return Transition::Pop;
    }
    Transition::Stay
}

fn draw_clock(draw: &mut Draw, ui: &Ui, fonts: &mut Fonts, clock: &str, play: Rect) {
    fonts
        .write(draw, Face::Display, clock)
        .right(play.x() - 20.0, play.y() + play.h() / 2.0)
        .size(theme::size::LABEL)
        .color(ui.palette().text_dim);
}
