use std::sync::Arc;

use notan::app::Event;
use notan::draw::DrawConfig;
use notan::prelude::*;
use shared::{config, ClientMessage};

mod audio;
mod connection;
mod cpu;
mod draw;
mod friends;
mod http;
mod interp;
mod logic;
mod login;
mod menu;
mod network;
mod profile;
mod profile_about;
mod profile_edit;
mod rooms;
mod solo;
mod state;
mod theme;
mod ui;
mod update;
#[cfg(target_arch = "wasm32")]
mod web;

use state::{Screen, State};
use ui::{Rect, SharpText};

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

fn setup(gfx: &mut Graphics) -> State {
    let fonts = ui::Fonts::load(gfx).expect("the bundled fonts are valid");
    let mut state = State::new(fonts);
    #[cfg(target_arch = "wasm32")]
    web::start_text_input();

    if let Some(token) = state::load_stored_token() {
        let slot = http::new_slot();
        http::get(http::api_url("me"), Some(token), Arc::clone(&slot));
        state.startup_check = Some(slot);
    }

    state
}

#[allow(clippy::needless_pass_by_value)]
fn event(state: &mut State, evt: Event) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Event::ReceivedCharacter(c) = evt {
        type_char(state, c);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (state, evt);
}

fn type_char(state: &mut State, c: char) {
    if c.is_control() {
        return;
    }
    match state.screen {
        Screen::Auth => match state.auth_form.focused {
            state::AuthField::Username if state.auth_form.username.chars().count() < 24 => {
                state.auth_form.username.push(c);
            }
            state::AuthField::Password if state.auth_form.password.len() < 64 => {
                state.auth_form.password.push(c);
            }
            _ => {}
        },
        Screen::CreateRoom if state.text_input.chars().count() < 24 => {
            state.text_input.push(c);
        }
        Screen::JoinById if c.is_ascii_digit() && state.text_input.len() < 9 => {
            state.text_input.push(c);
        }
        Screen::Friends => {
            if let Some(f) = state.friends.as_mut() {
                let allowed = c.is_alphanumeric() || c == '_' || c == '-' || c == ' ';
                if allowed && f.search_input.len() < 36 {
                    f.search_input.push(c);
                }
            }
        }
        Screen::Profile => {
            if let Some(form) = state.profile.as_mut().and_then(|p| p.edit.as_mut()) {
                form.type_char(c);
            }
        }
        _ => {}
    }
}

const BANNER_H: f32 = 80.0;

fn banner_buttons(ww: f32, wh: f32) -> (Rect, Rect) {
    let y = wh - BANNER_H + 15.0;
    (
        Rect::at(ww - 270.0, y, 120.0, 50.0),
        Rect::at(ww - 140.0, y, 110.0, 50.0),
    )
}

fn update_invitation(state: &mut State) {
    if state.pending_invitation.is_none() {
        return;
    }
    let (ww, wh) = state.ui.view().size();
    let (accept_btn, decline_btn) = banner_buttons(ww, wh);
    if state.ui.clicked(accept_btn) {
        if let Some((_, room_id, _)) = state.pending_invitation.take() {
            if state.conn.is_live() {
                state.conn.send(&ClientMessage::JoinRoom { id: room_id });
            } else {
                state.conn.connect(connection::now_secs());
                state.rooms.clear();
                state.pending_join = Some(room_id);
            }
            state.solo = None;
            state.screen = Screen::RoomBrowser;
        }
    } else if state.ui.clicked(decline_btn) {
        state.pending_invitation = None;
    }
}

fn update(app: &mut App, state: &mut State) {
    let dt = app.timer.delta_f32();
    let view = ui::View::of(app);
    state.ui.begin_frame(dt, view, ui::Mouse::of(app, view));
    state.backspace.update(app.keyboard.is_down(KeyCode::Backspace), dt);
    #[cfg(target_arch = "wasm32")]
    for c in web::take_typed().chars() {
        type_char(state, c);
    }
    let State { session, conn, .. } = state;
    if let Some(session) = session.as_mut() {
        session.clock += app.timer.delta_f32() as f64;
        session.ping_rtt_ms = conn.rtt_ms();
    }

    if state.startup_check.is_some() {
        login::poll_startup_check(state);
    }

    update_invitation(state);
    network::handle_server_messages(state);

    match state.screen {
        Screen::Auth => login::update_auth(app, state),
        Screen::Menu => menu::update_menu(state),
        Screen::Settings => menu::update_settings(app, state),
        Screen::RoomBrowser => rooms::update_browser(state),
        Screen::CreateRoom => rooms::update_create_room(app, state),
        Screen::JoinById => rooms::update_join_by_id(app, state),
        Screen::RoomLobby => rooms::update_lobby(app, state),
        Screen::Profile => profile::update_profile(app, state),
        Screen::Friends => friends::update_friends(app, state),
        Screen::OtherProfile => profile::update_other_profile(app, state),
        Screen::SoloSetup => solo::update_setup(app, state),
        Screen::Solo => solo::update_game(app, state),
        Screen::Game => {
            let is_host = state.lobby.as_ref().is_some_and(|l| l.is_host);
            let State {
                session,
                settings,
                conn,
                ui,
                ..
            } = &mut *state;
            if let Some(session) = session {
                logic::update_game(app, ui, session, *settings, conn, is_host);
            }
        }
    }
    state.ui.set_screen(state.screen as usize, state.screen.hue());
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
    let msg = format!("{from} t'invite dans \"{room_name}\"");
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

fn draw(app: &mut App, gfx: &mut Graphics, state: &mut State) {
    match state.screen {
        Screen::Auth => login::draw_auth(gfx, state),
        Screen::Menu => menu::draw_menu(gfx, state),
        Screen::Settings => menu::draw_settings(gfx, state),
        Screen::RoomBrowser => rooms::draw_browser(gfx, state),
        Screen::CreateRoom => rooms::draw_create_room(gfx, state),
        Screen::JoinById => rooms::draw_join_by_id(gfx, state),
        Screen::RoomLobby => rooms::draw_lobby(gfx, state),
        Screen::Profile => profile::draw_profile(gfx, state),
        Screen::Friends => friends::draw_friends(gfx, state),
        Screen::OtherProfile => profile::draw_other_profile(gfx, state),
        Screen::SoloSetup => solo::draw_setup(gfx, state),
        Screen::Solo => {
            if let Some(game) = state.solo.as_ref() {
                let hud = game.hud(state.solo_best);
                draw::draw_game(app, gfx, &game.session, &state.ui, &state.fonts, hud);
            }
        }
        Screen::Game => {
            let role = draw::Role {
                is_host: state.lobby.as_ref().is_some_and(|l| l.is_host),
                can_pause: state.lobby.as_ref().is_some_and(|l| l.settings.pause.allows(l.is_host)),
            };
            if let Some(session) = state.session.as_ref() {
                draw::draw_game(app, gfx, session, &state.ui, &state.fonts, draw::Hud::Online(role));
            }
        }
    }
    draw_invitation_banner(gfx, state);
    draw_maintenance_banner(gfx, state);
    draw_reconnect_banner(app, gfx, state);
    state.ui.present(gfx);
}

const MAINTENANCE_H: f32 = 30.0;

fn draw_maintenance_banner(gfx: &mut Graphics, state: &State) {
    let playing = state.screen == Screen::Game && state.session.as_ref().is_some_and(|s| !s.decided());
    if !state.maintenance || !state.conn.is_live() || !state.screen.needs_connection() || playing {
        return;
    }
    let ww = state.ui.view().w;
    let mut d = state.ui.canvas(gfx);
    ui::banner(
        &mut d,
        Rect::at(0.0, 0.0, ww, MAINTENANCE_H),
        theme::WARNING_BANNER,
        theme::WARNING,
        ui::Edge::Bottom,
    );
    d.sharp_text(
        &state.fonts.text,
        "Le serveur redémarre: les parties en cours se terminent, aucune autre ne démarre.",
    )
    .position(ww / 2.0, MAINTENANCE_H / 2.0)
    .size(theme::size::SMALL)
    .h_align_center()
    .v_align_middle()
    .color(theme::WARNING_TEXT);
    state.ui.render(gfx, &d);
}

fn draw_reconnect_banner(app: &mut App, gfx: &mut Graphics, state: &State) {
    let Some((attempts, secs_left)) = state.conn.recovering(connection::now_secs()) else {
        return;
    };
    let ww = state.ui.view().w;
    let mut d = state.ui.canvas(gfx);
    ui::banner(
        &mut d,
        Rect::at(0.0, 0.0, ww, 46.0),
        theme::WARNING_BANNER,
        theme::WARNING,
        ui::Edge::Bottom,
    );
    let dots = ".".repeat(1 + (app.timer.elapsed_f32() * 2.0) as usize % 3);
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
    let icon = Some(include_bytes!("../../assets/puyo_puyo_icon.ico").as_ref());
    let win_config = WindowConfig::new()
        .set_title("Rouillo")
        .set_size(1280, 800)
        .set_resizable(true)
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
