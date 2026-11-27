#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aero::draw::Draw;
use aero::input::{Event, KeyCode};
use aero::math::{Affine2, Vec2};
use aero::{Cx, WindowConfig, WindowIcon};
use shared::{config, ClientMessage};

use crate::ui::Face;

mod account;
mod audio;
mod bindings_panel;
mod board_size;
mod chat;
mod connection;
mod controls;
mod cpu_seat;

mod demo;
mod draw;
mod friends;
mod help;
mod history;
mod http;
mod images;
mod interp;
mod leaderboard;
mod logic;
mod login;
mod menu;
mod nav;
mod network;
mod pads;
mod picker;
mod profile;
mod profile_about;
mod profile_edit;
mod ranked;
mod replay;
mod rooms;
mod solo;
mod sprites;
mod state;
mod storage;
mod theme;
mod tick;
mod title;
mod touch;
mod ui;
mod update;
#[cfg(not(target_arch = "wasm32"))]
mod updater;
mod wake;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(windows)]
mod windows;
mod wt;

use nav::{Nav, Root, Transition};
use pads::{Pad, Pads};
use state::{Network, Screen, State};
use tick::Tick;
use ui::Rect;

use crate::ui::{px, Gui, Typeset};

const BANNER_H: f32 = 80.0;
const MAINTENANCE_H: f32 = 30.0;
/// How often the game updates without drawing while nothing moves, for the network's timers.
const IDLE_BEAT: std::time::Duration = std::time::Duration::from_millis(250);

pub fn server_url() -> String {
    #[cfg(all(target_arch = "wasm32", not(debug_assertions)))]
    {
        let loc = web_sys::window().expect("window").location();
        let is_https = loc.protocol().map(|p| p == "https:").unwrap_or(false);
        let proto = if is_https { "wss" } else { "ws" };
        format!("{}://{}/ws", proto, loc.host().expect("host"))
    }
    #[cfg(not(all(target_arch = "wasm32", not(debug_assertions))))]
    {
        std::env::var("PUYO_SERVER").unwrap_or_else(|_| {
            if cfg!(debug_assertions) {
                config::SERVER_URL.to_string()
            } else {
                config::SERVER_URL_RELEASE.to_string()
            }
        })
    }
}

fn setup() -> Root {
    let gui = ui::Gui::new();
    let mut state = State::new();
    #[cfg(target_arch = "wasm32")]
    {
        web::start_back_button();
        audio::unlock_on_gesture();
    }

    #[cfg(not(target_arch = "wasm32"))]
    updater::check();

    let mut current = Screen::Auth;
    if state::load_stored_token().is_some() {
        login::check_stored_session(&mut state.startup);
        current = Screen::Title;
    }

    Root { nav: Nav::new(current), state, gui }
}

/// Gives the bindings panel the character a key typed, to name the key it waits for.
fn bind_typed(root: &mut Root, event: &Event) {
    let Event::Key { pressed: true, text: Some(text), .. } = event else { return };
    if matches!(root.nav.current(), Screen::Settings) {
        for c in text.chars() {
            root.state.bindings_panel.type_char(c);
        }
    }
}

fn game_chat_area(view: ui::View, touch: bool) -> Rect {
    let (w, h) = (380.0, 230.0);
    let y = if touch { 110.0 } else { view.h - h - 20.0 };
    Rect::at(20.0, y, w, h)
}

/// Whether the player may type in a game: a spectator always, a player only
/// when the board does not need the keys.
fn can_chat(state: &State) -> bool {
    state.session().is_some_and(|s| {
        s.spectating()
            || s.decided()
            || s.eliminated()
            || s.opponent_disconnected
            || s.large.board.state() == shared::GameState::Paused
    })
}

/// Opens and closes the in-game chat and returns the game's input for this frame, without the keys the chat took;
/// the field is in [`chat::Chat::show_overlay`].
fn game_chat(tick: &Tick, state: &mut State) -> controls::Frame {
    let mut input = state.input.controls.frame;
    let allowed = can_chat(state);
    let Some(chat) = state.room.as_mut().map(|r| &mut r.chat) else {
        return input;
    };
    if !allowed {
        chat.open = false;
        return input;
    }
    if chat.open {
        input.pause = false;
        input.restart = false;
        if tick.back() {
            chat.open = false;
        }
    } else if tick.pressed(KeyCode::Enter) {
        chat.open = true;
        chat.take_keyboard();
    }
    input
}

/// Whether the gamepad drives the menus: everywhere but a board being played.
fn pad_menus(state: &State, screen: &Screen) -> bool {
    if state.bindings_panel.capturing() {
        return false;
    }

    match screen {
        Screen::Game => {
            let chatting = state.room.as_ref().is_some_and(|r| r.chat.open);
            can_chat(state) && !chatting || state.session().is_some_and(|s| s.quit_menu)
        }
        Screen::Solo(game) => game.idle(),
        _ => true,
    }
}

fn pad_direction(pads: &Pads) -> Option<(f32, f32)> {
    [
        (Pad::DPadUp, (0.0, -1.0)),
        (Pad::DPadDown, (0.0, 1.0)),
        (Pad::DPadLeft, (-1.0, 0.0)),
        (Pad::DPadRight, (1.0, 0.0)),
    ]
    .into_iter()
    .find(|(pad, _)| pads.pressed(*pad))
    .map(|(_, dir)| dir)
}

fn banner_buttons(ww: f32, wh: f32) -> (Rect, Rect) {
    let band = Rect::at(0.0, wh - BANNER_H, ww, BANNER_H).inset([15.0, 30.0, 15.0, 0.0]);
    let [accept, decline] = band.right_strip(240.0).columns(10.0, [px(120.0), px(110.0)]);

    (accept, decline)
}

fn accept_invitation(state: &mut State) -> Transition {
    let Some((_, room_id, _)) = state.pending_invitation.take() else {
        return Transition::Stay;
    };
    if state.net.conn.is_live() {
        state.net.conn.send(&ClientMessage::JoinRoom { id: room_id });
    } else {
        state.net.conn.connect(connection::now_secs());
        state.rooms.clear();
        state.net.pending_join = Some(room_id);
    }
    state.ranked.stop();
    Transition::To(Screen::RoomBrowser)
}

/// The fastest connected screen's frame time: Wayland does not say which
/// screen holds the window.
#[cfg(not(target_arch = "wasm32"))]
fn frame_time() -> std::time::Duration {
    static FRAME: std::sync::OnceLock<std::time::Duration> = std::sync::OnceLock::new();
    *FRAME.get_or_init(|| {
        let hz = display_info::DisplayInfo::all()
            .ok()
            .and_then(|screens| screens.iter().map(|s| s.frequency).reduce(f32::max))
            .filter(|hz| (30.0..=500.0).contains(hz))
            .unwrap_or(60.0);
        std::time::Duration::from_secs_f32(1.0 / hz)
    })
}

/// Caps the native loop at the screen's refresh rate. Vsync would do it, but on
/// Wayland it blocks the whole loop, network included, while the window is hidden.
#[cfg(not(target_arch = "wasm32"))]
fn pace_frame() {
    use std::cell::Cell;
    use std::time::Instant;
    thread_local!(static NEXT: Cell<Option<Instant>> = const { Cell::new(None) });
    NEXT.with(|next| {
        let now = Instant::now();
        let due = next.get().unwrap_or(now);
        if due > now {
            std::thread::sleep(due - now);
        }
        next.set(Some(due.max(now) + frame_time()));
    });
}

fn update_online_game(tick: &Tick, state: &mut State) -> Transition {
    let online = state.room.as_ref().map_or_else(logic::Online::default, |r| logic::Online {
        is_host: r.info.is_host,
        ranked: r.info.ranked.is_some(),
        ended: r.series_over.is_some(),
    });
    let input = game_chat(tick, state);
    let clicks = state.input.clicks;
    let State { room, settings, net: Network { conn, .. }, .. } = &mut *state;
    let left = room
        .as_mut()
        .and_then(|r| r.session.as_mut())
        .is_some_and(|s| logic::update_game(tick, (&input, clicks), s, *settings, conn, online));

    let transition = if left && online.ranked { ranked::enter(state) } else { Transition::Stay };
    replay::update_keep(state);

    transition
}

fn update(cx: &mut Cx, root: &mut Root) {
    #[cfg(not(target_arch = "wasm32"))]
    pace_frame();
    let Root { nav, state, gui } = root;
    #[cfg(not(target_arch = "wasm32"))]
    match updater::status() {
        updater::Status::Available(_) => state.outdated = true,
        updater::Status::Done => {
            cx.exit();
            return;
        }
        _ => {}
    }
    let dt = cx.dt();
    let Some(window) = cx.window(aero::WindowId::MAIN) else { return };
    let view = ui::View::of(window);

    state.input.pads.update(&cx.input);
    let pad_nav = pad_menus(state, nav.current());
    let finger = cx.input.fingers().first().map(|f| f.at);
    gui.ui.note_pointer(finger.or_else(|| cx.input.pointer()));
    if pad_nav && state.input.pads.pressed(Pad::South) {
        cx.press_focus(aero::WindowId::MAIN);
    }
    gui.ui.set_clock(connection::now_secs());
    gui.ui.begin_frame(dt, view);
    if pad_nav {
        if let Some(dir) = pad_direction(&state.input.pads) {
            gui.ui.use_pad();
            cx.move_focus(aero::WindowId::MAIN, Vec2::from(dir));
        }
    }
    let back = pad_nav && state.input.pads.pressed(Pad::East);

    #[cfg(target_arch = "wasm32")]
    web::set_at_root(matches!(nav.current(), Screen::Title | Screen::Auth | Screen::Menu));
    #[cfg(target_arch = "wasm32")]
    let back = web::take_back() || back;
    let tick = &Tick::new(&cx.input, dt, (back, cx.wants_text(aero::WindowId::MAIN)));

    let in_game = matches!(nav.current(), Screen::Game | Screen::Solo(_));
    let (touch_held, touch_pause) = state.input.touch.read(tick.input, view, in_game);
    state.input.controls.update(tick, &state.input.pads, touch_held, touch_pause);

    let was_title = matches!(nav.current(), Screen::Title);
    let (rtt, via) = (state.net.conn.rtt_ms(), state.net.conn.via());
    if let Some(session) = state.session_mut() {
        session.net.clock += f64::from(dt);
        session.net.ping_rtt_ms = rtt;
        session.debug.via = via;
    }

    login::poll_startup_check(state, nav);
    solo::poll_best(&mut state.solo, state.auth.is_some());
    replay::poll(state, nav);

    network::keep_linked(&mut state.net, state.auth.is_some(), nav.current());
    network::handle_server_messages(state, nav);

    let transition = match nav.current_mut() {
        Screen::Title => title::update_title(tick, state),
        Screen::Auth => login::update_auth(state),
        Screen::Menu => menu::update_menu(tick, state, false),
        Screen::PlayMenu => menu::update_menu(tick, state, true),
        Screen::Settings => menu::update_settings(tick, state),
        Screen::RoomBrowser => rooms::update_browser(tick, gui, state),
        Screen::CreateRoom | Screen::JoinById => rooms::update_entry(tick),
        Screen::RoomLobby => {
            rooms::update_lobby(tick, gui, state);
            Transition::Stay
        }
        Screen::Profile(p) => profile::update_profile(tick, gui, state, p),
        Screen::Friends(f) => friends::update_friends(tick, gui, state, f),
        Screen::OtherProfile(p) => profile::update_other_profile(tick, gui, p),
        Screen::SoloSetup => solo::update_setup(tick, state),
        Screen::Ranked => ranked::update(tick, state),
        Screen::Leaderboard(board) => leaderboard::update(tick, gui, state, board),
        Screen::Help(view) => help::update(tick, view),
        Screen::Solo(game) => solo::update_game(tick, state, game),
        Screen::Replay(viewer) => replay::update(tick, viewer),
        Screen::Game => update_online_game(tick, state),
    };

    nav.apply(transition);
    // A game's clicks are for the update right after its frame: past it, they are stale.
    state.input.clicks = draw::GameClicks::default();
    if matches!(nav.current(), Screen::Menu) && !state.startup.title_seen {
        nav.apply(Transition::To(Screen::Title));
    }
    let fade = if was_title { title::FADE } else { ui::TRANSITION };
    gui.ui.set_screen(nav.current().id(), nav.current().hue(), fade);
}

/// The invitation along the bottom, over the screen.
fn show_invitation(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let Some((ref from, _, ref room_name)) = state.pending_invitation else {
        return Transition::Stay;
    };

    let pal = gui.ui.palette();
    let (ww, wh) = gui.ui.view().size();
    let band = Rect::at(0.0, wh - BANNER_H, ww, BANNER_H);
    let msg = format!("{from} vous invite dans \"{room_name}\"");
    let (accept_btn, decline_btn) = banner_buttons(ww, wh);

    let (joined, ignored) = ui.layer(aero::draw::Layer::Popup, |ui| {
        ui.interact(ui.id("invitation"), band, aero::ui::Sense::CLICK);
        ui::banner(ui.draw, band, pal.banner, pal.accent, ui::Edge::Top);
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, &msg)
            .left(20.0, band.y() + 40.0)
            .size(theme::size::LABEL)
            .color(pal.text);
        let joined = ui::widgets::button(ui, "join invitation", accept_btn, "Rejoindre", true);
        (joined, ui::widgets::button(ui, "ignore invitation", decline_btn, "Ignorer", true))
    });
    if joined {
        return accept_invitation(state);
    }
    if ignored {
        state.pending_invitation = None;
    }
    Transition::Stay
}

fn draw_frame(cx: &mut Cx, draw: &mut Draw, root: &mut Root) {
    let Root { nav, state, gui } = root;
    if let Some(look) = cx.theme_mut::<ui::Look>() {
        look.palette = gui.ui.palette();
    }
    let scale = gui.ui.view().scale;
    draw.push_transform(Affine2::from_scale(Vec2::splat(scale)));
    let ui = &mut aero::ui::Ui::new(cx, draw);
    let mut transition = Transition::Stay;
    match nav.current_mut() {
        Screen::Title => title::draw_title(ui.draw, &mut ui.cx.fonts, gui),
        Screen::Auth => transition = login::show(ui, gui, state),
        Screen::Menu => transition = menu::show_menu(ui, gui, state, false),
        Screen::PlayMenu => transition = menu::show_menu(ui, gui, state, true),
        Screen::Settings => transition = menu::show_settings(ui, gui, state),
        Screen::RoomBrowser => transition = rooms::show_browser(ui, gui, state),
        Screen::CreateRoom => transition = rooms::show_create_room(ui, gui, state),
        Screen::JoinById => transition = rooms::show_join_by_id(ui, gui, state),
        Screen::RoomLobby => transition = rooms::show_lobby(ui, gui, state),
        Screen::Profile(p) => transition = profile::show_profile(ui, gui, state, p),
        Screen::Friends(f) => transition = friends::show_friends(ui, gui, state, f),
        Screen::OtherProfile(p) => transition = profile::show_other_profile(ui, gui, state, p),
        Screen::SoloSetup => transition = solo::show_setup(ui, gui, state),
        Screen::Ranked => transition = ranked::show(ui, gui, state),
        Screen::Leaderboard(board) => transition = leaderboard::show(ui, gui, state, board),
        Screen::Help(view) => transition = help::show(ui, gui, state, view),
        Screen::Replay(viewer) => transition = replay::show(ui, gui, viewer),
        Screen::Solo(game) => {
            let hud = game.hud(state.solo.best);
            state.input.clicks = draw::draw_game(ui, &game.session, gui, hud);
        }
        Screen::Game => {
            if let Some((room, session)) = state.room.as_ref().and_then(|r| Some((r, r.session.as_ref()?))) {
                state.input.clicks = draw::draw_game(ui, session, gui, draw::Hud::Online(online_role(room)));
            }
        }
    }

    let screen = nav.current();
    if matches!(screen, Screen::Game | Screen::Solo(_)) {
        let hint = can_chat(state).then_some("Entrée pour écrire");
        let area = game_chat_area(gui.ui.view(), state.input.touch.active);
        if let Some(room) = state.room.as_mut().filter(|_| matches!(screen, Screen::Game)) {
            if let Some(text) = room.chat.show_overlay(ui, gui, area, hint) {
                state.net.conn.send(&ClientMessage::Chat { text });
            }
        }
        if matches!(screen, Screen::Game) {
            replay::show_keep(ui, gui, state);
        }
        state.input.touch.draw(ui.draw, &mut ui.cx.fonts, gui.ui.view());
    }

    let invited = show_invitation(ui, gui, state);
    if !matches!(invited, Transition::Stay) {
        transition = invited;
    }
    draw_maintenance_banner(ui.draw, &mut ui.cx.fonts, gui, state, screen);
    draw_too_old_banner(ui.draw, &mut ui.cx.fonts, gui, state, screen);
    draw_reconnect_banner(ui.draw, &mut ui.cx.fonts, gui, state, screen);

    let view = gui.ui.view();
    // A game, and the screens that lead straight into one, need the phone
    // turned: the boards are laid out side by side.
    let needs_landscape = matches!(screen, Screen::RoomLobby | Screen::Game | Screen::Ranked | Screen::Solo(_));
    if state.input.touch.active && view.portrait() && needs_landscape {
        touch::draw_turn_hint(ui.draw, &mut ui.cx.fonts, view);
    }
    ui.draw.pop_transform();
    if !matches!(transition, Transition::Stay) {
        nav.apply(transition);
        ui.cx.request_redraw();
    }
}

/// The player's place in an online game, as its screen shows it.
fn online_role(room: &state::Room) -> draw::Role {
    let info = &room.info;
    draw::Role {
        is_host: info.is_host,
        watchers: info.spectators,
        can_pause: info.settings.pause.allows(info.is_host),
        series: info.ranked.as_ref().map(|ranked| {
            let me = usize::from(info.your_slot.clamp(1, 2) - 1);
            draw::SeriesView {
                wins: [ranked.wins[me], ranked.wins[1 - me]],
                over: room.series_over.map(|(winner, elo)| (winner.map(|w| w == info.your_slot), elo)),
            }
        }),
    }
}

fn draw_maintenance_banner(draw: &mut Draw, fonts: &mut ui::Fonts, gui: &mut Gui, state: &State, screen: &Screen) {
    let playing = matches!(screen, Screen::Game) && state.session().is_some_and(|s| !s.decided());
    if state.net.maintenance && state.net.conn.is_live() && screen.needs_connection() && !playing {
        draw_warning_banner(
            draw,
            &mut gui.ui,
            fonts,
            "Le serveur redémarre: les parties en cours se terminent, aucune autre ne démarre.",
        );
    }
}

/// Says why online play is closed while the server refuses this version.
fn draw_too_old_banner(draw: &mut Draw, fonts: &mut ui::Fonts, gui: &mut Gui, state: &State, screen: &Screen) {
    if state.net.too_old && matches!(screen, Screen::Menu | Screen::PlayMenu) {
        draw_warning_banner(draw, &mut gui.ui, fonts, crate::update::TOO_OLD);
    }
}

fn draw_warning_banner(draw: &mut Draw, ui: &mut ui::Ui, fonts: &mut ui::Fonts, text: &str) {
    let ww = ui.view().w;

    ui::banner(draw, Rect::at(0.0, 0.0, ww, MAINTENANCE_H), theme::WARNING_BANNER, theme::WARNING, ui::Edge::Bottom);
    fonts
        .write(draw, Face::Text, text)
        .center(ww / 2.0, MAINTENANCE_H / 2.0)
        .size(theme::size::SMALL)
        .color(theme::WARNING_TEXT);
}

fn draw_reconnect_banner(draw: &mut Draw, fonts: &mut ui::Fonts, gui: &mut Gui, state: &State, screen: &Screen) {
    let Some((attempts, secs_left)) = state.net.conn.recovering(connection::now_secs()) else {
        return;
    };
    if !screen.needs_connection() {
        return;
    }

    let ww = gui.ui.view().w;
    let dots = gui.ui.waiting_dots();
    gui.ui.next_second();
    let msg = format!("Reconnexion{dots} (tentative {attempts}, {secs_left:.0}s restantes)");

    ui::banner(draw, Rect::at(0.0, 0.0, ww, 46.0), theme::WARNING_BANNER, theme::WARNING, ui::Edge::Bottom);
    fonts.write(draw, Face::Text, &msg).center(ww / 2.0, 22.0).size(theme::size::LABEL).color(theme::WARNING_TEXT);
}

struct Rouillo(Root);

impl aero::App for Rouillo {
    fn event(&mut self, _: &mut Cx, event: &Event) {
        if matches!(event, Event::GpuReset) {
            self.0.gui.images.rebuild();
        }
        bind_typed(&mut self.0, event);
    }

    fn update(&mut self, cx: &mut Cx) {
        update(cx, &mut self.0);
    }

    /// A frame follows while something moves, or when something shown changes; otherwise a slow beat of updates
    /// without drawing keeps the network's timers going.
    fn draw(&mut self, cx: &mut Cx, draw: &mut Draw) {
        draw_frame(cx, draw, &mut self.0);
        let change_in = self.0.gui.ui.take_change_in();
        if self.0.gui.ui.take_moving() {
            cx.request_redraw();
        } else {
            if let Some(delay) = change_in {
                cx.request_redraw_in(std::time::Duration::from_secs_f64(delay));
            }
            cx.request_update_in(IDLE_BEAT);
        }
    }
}

fn main() -> Result<(), aero::Error> {
    #[cfg(windows)]
    windows::log_to_file();
    let window = WindowConfig::new().title("Rouillo").size(1280, 800).resizable(true).app_id("org.priax.Rouillo");
    let window = match images::decode(include_bytes!("../../assets/puyo_puyo_icon.ico")) {
        Ok((width, height, rgba)) => window.icon(WindowIcon { width, height, rgba }),
        Err(e) => {
            eprintln!("window icon: {e}");
            window
        }
    };
    #[cfg(target_arch = "wasm32")]
    let window = window.maximized(true);

    aero::run(aero::Config::new().window(window), |cx| {
        wake::init(cx.waker());
        let root = setup();
        ui::register_fonts(&mut cx.fonts);
        cx.set_theme(root.gui.ui.look());
        Rouillo(root)
    })
}
