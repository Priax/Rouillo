#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use notan::app::Event;
use notan::draw::DrawConfig;
use notan::prelude::*;
use shared::{config, ClientMessage};

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
mod title;
mod touch;
mod ui;
mod update;
#[cfg(not(target_arch = "wasm32"))]
mod updater;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(windows)]
mod windows;
mod wt;

use nav::{Nav, Root, Transition};
use pads::{Pad, Pads};
use state::{Input, Network, Screen, State};
use ui::{Rect, SharpText, Size};

const BANNER_H: f32 = 80.0;

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

fn setup(gfx: &mut Graphics) -> Root {
    let fonts = ui::Fonts::load(gfx).expect("the bundled fonts are valid");
    let mut state = State::new(fonts);
    #[cfg(target_arch = "wasm32")]
    {
        web::start_text_input();
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

    Root {
        nav: Nav::new(current),
        state,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn event(root: &mut Root, evt: Event) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Event::ReceivedCharacter(c) = evt {
        if let Some(c) = root.state.input.keys.key_char(c) {
            type_char(&mut root.state, &mut root.nav, c);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (root, evt);
}

fn type_char(state: &mut State, nav: &mut Nav, c: char) {
    if matches!(nav.current, Screen::Settings) {
        state.bindings_panel.type_char(c);
    } else if let Some(input) = typing(state, nav) {
        input.type_char(c);
    }
}

/// The field the keyboard types into on this screen, if any. Each field
/// carries its own limits.
fn typing<'a>(state: &'a mut State, nav: &'a mut Nav) -> Option<&'a mut ui::TextInput> {
    match &mut nav.current {
        Screen::Auth => Some(state.auth_form.focused_input()),
        Screen::CreateRoom | Screen::JoinById => Some(&mut state.text_input),
        Screen::RoomBrowser => state.room_pager.typing(),
        Screen::RoomLobby => {
            let room = state.room.as_mut()?;
            if room.invite.open {
                room.invite.pager.typing()
            } else {
                room.chat.typing()
            }
        }
        Screen::Game => {
            let chat = &mut state.room.as_mut()?.chat;
            chat.open.then_some(&mut chat.input)
        }
        Screen::Friends(f) => friends::typing(f),
        Screen::Profile(p) => profile::typing(p),
        Screen::OtherProfile(p) => p.core.history.typing(),
        _ => None,
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
            || s.board.state() == shared::GameState::Paused
    })
}

/// Runs the in-game chat and returns the game's input for this frame, without
/// the keys the chat took.
fn game_chat(app: &App, state: &mut State) -> controls::Frame {
    let mut input = state.input.controls.frame;
    let allowed = can_chat(state);
    let area = game_chat_area(state.ui.view(), state.input.touch.active);
    let State {
        room,
        ui,
        fonts,
        input: Input { keys, .. },
        net: Network { conn, .. },
        ..
    } = state;
    let Some(chat) = room.as_mut().map(|r| &mut r.chat) else {
        return input;
    };
    if !allowed {
        chat.open = false;
        return input;
    }
    if chat.open {
        input.pause = false;
        input.restart = false;
        if app.keyboard.was_pressed(KeyCode::Escape) {
            chat.open = false;
        } else if let Some(text) = chat.edit(&app.keyboard, keys) {
            conn.send(&ClientMessage::Chat { text });
            chat.open = false;
        }
    } else if app.keyboard.was_pressed(KeyCode::Enter) || chat.click(ui, fonts, area) {
        chat.open = true;
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
    let band = Rect::at(0.0, wh - BANNER_H, ww, BANNER_H).pad(15.0, 30.0, 15.0, 0.0);
    let [accept, decline] = band.right(240.0).columns(10.0, [Size::Px(120.0), Size::Px(110.0)]);
    (accept, decline)
}

fn update_invitation(state: &mut State, nav: &mut Nav) {
    if state.pending_invitation.is_none() {
        return;
    }
    let (ww, wh) = state.ui.view().size();
    let (accept_btn, decline_btn) = banner_buttons(ww, wh);
    let on_banner = state.ui.clicked(Rect::at(0.0, wh - BANNER_H, ww, BANNER_H));
    if state.ui.clicked(accept_btn) {
        if let Some((_, room_id, _)) = state.pending_invitation.take() {
            if state.net.conn.is_live() {
                state.net.conn.send(&ClientMessage::JoinRoom { id: room_id });
            } else {
                state.net.conn.connect(connection::now_secs());
                state.rooms.clear();
                state.net.pending_join = Some(room_id);
            }
            state.ranked.stop();
            nav.apply(Transition::To(Screen::RoomBrowser));
        }
    } else if state.ui.clicked(decline_btn) {
        state.pending_invitation = None;
    }
    if on_banner {
        state.ui.swallow_click();
    }
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

fn update_online_game(app: &mut App, state: &mut State) -> Transition {
    let online = state
        .room
        .as_ref()
        .map_or_else(logic::Online::default, |r| logic::Online {
            is_host: r.info.is_host,
            ranked: r.info.ranked.is_some(),
            ended: r.series_over.is_some(),
        });
    let input = game_chat(app, state);
    let State {
        room,
        settings,
        net: Network { conn, .. },
        ui,
        ..
    } = &mut *state;
    let left = room
        .as_mut()
        .and_then(|r| r.session.as_mut())
        .is_some_and(|s| logic::update_game(app, &input, ui, s, *settings, conn, online));
    let transition = if left && online.ranked {
        ranked::enter(state)
    } else {
        Transition::Stay
    };
    replay::update_keep(state);
    transition
}

fn update(app: &mut App, root: &mut Root) {
    #[cfg(not(target_arch = "wasm32"))]
    pace_frame();
    let Root { nav, state } = root;
    let dt = app.timer.delta_f32();
    let view = ui::View::of(app);
    state.input.pads.update();
    let pad_nav = pad_menus(state, &nav.current);
    let real = ui::Mouse::of(app, view);
    state.ui.note_pointer(real);
    let mouse = if pad_nav && state.input.pads.pressed(Pad::South) {
        state.ui.pad_click().unwrap_or(real)
    } else {
        real
    };
    state.ui.begin_frame(dt, view, mouse);
    if pad_nav {
        if let Some(dir) = pad_direction(&state.input.pads) {
            state.ui.move_focus(dir);
        }
        if state.input.pads.pressed(Pad::East) {
            app.keyboard.pressed.insert(KeyCode::Escape);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        web::set_at_root(matches!(nav.current, Screen::Title | Screen::Auth | Screen::Menu));
        if web::take_back() {
            app.keyboard.pressed.insert(KeyCode::Escape);
        }
    }
    state.input.keys.update(&app.keyboard, dt);
    if state.input.keys.paste {
        for c in ui::clipboard_text().unwrap_or_default().chars() {
            type_char(state, nav, c);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    match updater::status() {
        updater::Status::Available(_) => state.outdated = true,
        updater::Status::Done => std::process::exit(0),
        _ => {}
    }
    let in_game = matches!(nav.current, Screen::Game | Screen::Solo(_));
    let (touch_held, touch_pause) = state.input.touch.read(app, view, in_game);
    state
        .input
        .controls
        .update(&app.keyboard, &state.input.pads, touch_held, touch_pause);
    #[cfg(target_arch = "wasm32")]
    web::want_text(typing(state, nav).is_some());
    #[cfg(target_arch = "wasm32")]
    for c in web::take_typed().chars() {
        type_char(state, nav, c);
    }
    let was_title = matches!(nav.current, Screen::Title);
    let (rtt, via) = (state.net.conn.rtt_ms(), state.net.conn.via());
    if let Some(session) = state.session_mut() {
        session.clock += app.timer.delta_f32() as f64;
        session.ping_rtt_ms = rtt;
        session.via = via;
    }

    login::poll_startup_check(state, nav);
    solo::poll_best(&mut state.solo, state.auth.is_some());
    replay::poll(state, nav);

    network::keep_linked(&mut state.net, state.auth.is_some(), &nav.current);
    update_invitation(state, nav);
    network::handle_server_messages(state, nav);

    let transition = match &mut nav.current {
        Screen::Title => title::update_title(app, state),
        Screen::Auth => login::update_auth(app, state),
        Screen::Menu => menu::update_menu(app, state, false),
        Screen::PlayMenu => menu::update_menu(app, state, true),
        Screen::Settings => menu::update_settings(app, state),
        Screen::RoomBrowser => rooms::update_browser(app, state),
        Screen::CreateRoom => rooms::update_create_room(app, state),
        Screen::JoinById => rooms::update_join_by_id(app, state),
        Screen::RoomLobby => {
            rooms::update_lobby(app, state);
            Transition::Stay
        }
        Screen::Profile(p) => profile::update_profile(app, state, p),
        Screen::Friends(f) => friends::update_friends(app, state, f),
        Screen::OtherProfile(p) => profile::update_other_profile(app, state, p),
        Screen::SoloSetup => solo::update_setup(app, state),
        Screen::Ranked => ranked::update(app, state),
        Screen::Leaderboard(board) => leaderboard::update(app, state, board),
        Screen::Help(view) => help::update(app, state, view),
        Screen::Solo(game) => solo::update_game(app, state, game),
        Screen::Replay(viewer) => replay::update(app, state, viewer),
        Screen::Game => update_online_game(app, state),
    };
    nav.apply(transition);
    if matches!(nav.current, Screen::Menu) && !state.startup.title_seen {
        nav.apply(Transition::To(Screen::Title));
    }
    let fade = if was_title { title::FADE } else { ui::TRANSITION };
    state.ui.set_screen(nav.current.id(), nav.current.hue(), fade);
}

fn draw_invitation_banner(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let Some((ref from, _, ref room_name)) = state.pending_invitation else {
        return;
    };
    let (ww, wh) = state.ui.view().size();
    let banner_y = wh - BANNER_H;
    let mut d = state.ui.canvas(gfx);
    ui::banner(
        &mut d,
        Rect::at(0.0, banner_y, ww, BANNER_H),
        pal.banner,
        pal.accent,
        ui::Edge::Top,
    );
    let msg = format!("{from} vous invite dans \"{room_name}\"");
    d.sharp_text(&state.fonts.text, &msg)
        .position(20.0, banner_y + 40.0)
        .size(theme::size::LABEL)
        .v_align_middle()
        .color(pal.text);
    let (accept_btn, decline_btn) = banner_buttons(ww, wh);
    state.ui.button(&mut d, &state.fonts, accept_btn, "Rejoindre");
    state.ui.button(&mut d, &state.fonts, decline_btn, "Ignorer");
    state.ui.render(gfx, &d);
}

fn draw(app: &mut App, gfx: &mut Graphics, root: &mut Root) {
    let Root { nav, state } = root;
    let screen = &nav.current;
    match screen {
        Screen::Title => title::draw_title(gfx, state),
        Screen::Auth => login::draw_auth(gfx, state),
        Screen::Menu => menu::draw_menu(gfx, state, false),
        Screen::PlayMenu => menu::draw_menu(gfx, state, true),
        Screen::Settings => menu::draw_settings(gfx, state),
        Screen::RoomBrowser => rooms::draw_browser(gfx, state),
        Screen::CreateRoom => rooms::draw_create_room(gfx, state),
        Screen::JoinById => rooms::draw_join_by_id(gfx, state),
        Screen::RoomLobby => rooms::draw_lobby(gfx, state),
        Screen::Profile(p) => profile::draw_profile(gfx, state, p),
        Screen::Friends(f) => friends::draw_friends(gfx, state, f),
        Screen::OtherProfile(p) => profile::draw_other_profile(gfx, state, p),
        Screen::SoloSetup => solo::draw_setup(gfx, state),
        Screen::Ranked => ranked::draw(gfx, state),
        Screen::Leaderboard(board) => leaderboard::draw(gfx, state, board),
        Screen::Help(view) => help::draw(gfx, state, view),
        Screen::Replay(viewer) => replay::draw(app, gfx, state, viewer),
        Screen::Solo(game) => {
            let hud = game.hud(state.solo.best);
            draw::draw_game(app, gfx, &game.session, &state.ui, &state.fonts, hud);
        }
        Screen::Game => {
            if let Some(room) = &state.room {
                let l = &room.info;
                let role = draw::Role {
                    is_host: l.is_host,
                    watchers: l.spectators,
                    can_pause: l.settings.pause.allows(l.is_host),
                    series: l.ranked.as_ref().map(|ranked| {
                        let me = usize::from(l.your_slot.clamp(1, 2) - 1);
                        draw::SeriesView {
                            wins: [ranked.wins[me], ranked.wins[1 - me]],
                            over: room
                                .series_over
                                .map(|(winner, elo)| (winner.map(|w| w == l.your_slot), elo)),
                        }
                    }),
                };
                if let Some(session) = &room.session {
                    draw::draw_game(app, gfx, session, &state.ui, &state.fonts, draw::Hud::Online(role));
                }
            }
        }
    }
    if matches!(screen, Screen::Game | Screen::Solo(_)) {
        let mut d = state.ui.canvas(gfx);
        if let Some(room) = state.room.as_ref().filter(|_| matches!(screen, Screen::Game)) {
            let hint = can_chat(state).then_some("Entrée pour écrire");
            room.chat.draw_overlay(
                &mut d,
                &state.ui,
                &state.fonts,
                game_chat_area(state.ui.view(), state.input.touch.active),
                hint,
            );
            replay::draw_keep(&mut d, state);
        }
        state.input.touch.draw(&mut d, &state.fonts, state.ui.view());
        state.ui.render(gfx, &d);
    }
    draw_invitation_banner(gfx, state);
    draw_maintenance_banner(gfx, state, screen);
    draw_too_old_banner(gfx, state, screen);
    draw_reconnect_banner(gfx, state, screen);
    let view = state.ui.view();
    // A game, and the screens that lead straight into one, need the phone
    // turned: the boards are laid out side by side.
    let needs_landscape = matches!(
        screen,
        Screen::RoomLobby | Screen::Game | Screen::Ranked | Screen::Solo(_)
    );
    if state.input.touch.active && view.portrait() && needs_landscape {
        let mut d = state.ui.canvas(gfx);
        touch::draw_turn_hint(&mut d, &state.fonts, view);
        state.ui.render(gfx, &d);
    }
    state.ui.present(gfx);
}

const MAINTENANCE_H: f32 = 30.0;

fn draw_maintenance_banner(gfx: &mut Graphics, state: &State, screen: &Screen) {
    let playing = matches!(screen, Screen::Game) && state.session().is_some_and(|s| !s.decided());
    if state.net.maintenance && state.net.conn.is_live() && screen.needs_connection() && !playing {
        draw_warning_banner(
            gfx,
            &state.ui,
            &state.fonts,
            "Le serveur redémarre: les parties en cours se terminent, aucune autre ne démarre.",
        );
    }
}

/// Says why online play is closed while the server refuses this version.
fn draw_too_old_banner(gfx: &mut Graphics, state: &State, screen: &Screen) {
    if state.net.too_old && matches!(screen, Screen::Menu | Screen::PlayMenu) {
        draw_warning_banner(gfx, &state.ui, &state.fonts, crate::update::TOO_OLD);
    }
}

fn draw_warning_banner(gfx: &mut Graphics, ui: &ui::Ui, fonts: &ui::Fonts, text: &str) {
    let ww = ui.view().w;
    let mut d = ui.canvas(gfx);
    ui::banner(
        &mut d,
        Rect::at(0.0, 0.0, ww, MAINTENANCE_H),
        theme::WARNING_BANNER,
        theme::WARNING,
        ui::Edge::Bottom,
    );
    d.sharp_text(&fonts.text, text)
        .position(ww / 2.0, MAINTENANCE_H / 2.0)
        .size(theme::size::SMALL)
        .h_align_center()
        .v_align_middle()
        .color(theme::WARNING_TEXT);
    ui.render(gfx, &d);
}

fn draw_reconnect_banner(gfx: &mut Graphics, state: &State, screen: &Screen) {
    let Some((attempts, secs_left)) = state.net.conn.recovering(connection::now_secs()) else {
        return;
    };
    if !screen.needs_connection() {
        return;
    }
    let ww = state.ui.view().w;
    let mut d = state.ui.canvas(gfx);
    ui::banner(
        &mut d,
        Rect::at(0.0, 0.0, ww, 46.0),
        theme::WARNING_BANNER,
        theme::WARNING,
        ui::Edge::Bottom,
    );
    let dots = state.ui.waiting_dots();
    let msg = format!("Reconnexion{dots} (tentative {attempts}, {secs_left:.0}s restantes)");
    d.sharp_text(&state.fonts.text, &msg)
        .position(ww / 2.0, 22.0)
        .size(theme::size::LABEL)
        .h_align_center()
        .v_align_middle()
        .color(theme::WARNING_TEXT);
    state.ui.render(gfx, &d);
}

#[notan_main]
fn main() -> Result<(), String> {
    #[cfg(windows)]
    windows::log_to_file();
    let icon = Some(include_bytes!("../../assets/puyo_puyo_icon.ico").as_ref());
    let win_config = WindowConfig::new()
        .set_title("Rouillo")
        .set_size(1280, 800)
        .set_resizable(true)
        .set_high_dpi(true)
        .set_app_id("org.priax.Rouillo")
        .set_window_icon_data(icon)
        .set_taskbar_icon_data(icon);
    #[cfg(target_arch = "wasm32")]
    let win_config = win_config.set_maximized(true);

    notan::init_with(setup)
        .add_config(DrawConfig)
        .add_config(win_config)
        .event(event)
        .update(update)
        .draw(draw)
        .build()
}
