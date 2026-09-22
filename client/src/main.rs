use notan::app::Event;
use notan::draw::*;
use notan::prelude::*;
use shared::{config, ClientMessage};

mod audio;
mod connection;
mod draw;
mod friends;
mod http;
mod logic;
mod login;
mod menu;
mod network;
mod other_profile;
mod profile;
mod rooms;
mod state;

use menu::Btn;
use state::{Screen, State};

pub fn server_url() -> String {
    #[cfg(all(target_arch = "wasm32", not(debug_assertions)))]
    {
        let loc = web_sys::window().expect("window").location();
        let is_https = loc.protocol().map(|p| p == "https:").unwrap_or(false);
        let proto = if is_https { "wss" } else { "ws" };
        return format!("{}://{}/ws", proto, loc.host().expect("host"));
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
    let font = gfx.create_font(include_bytes!("../../assets/arcadeFont.ttf")).unwrap();
    let ui_font = gfx.create_font(include_bytes!("../../assets/uiFont.ttf")).unwrap();
    let mut state = State::new(font, ui_font);

    if let Some(token) = state::load_stored_token() {
        let slot = http::new_slot();
        http::get(http::api_url("me"), Some(token), slot.clone());
        state.startup_check = Some(slot);
    }

    state
}

fn event(state: &mut State, evt: Event) {
    if let Event::ReceivedCharacter(c) = evt {
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
                if let Some(p) = state.profile.as_mut() {
                    if p.editing {
                        match p.edit_focused {
                            state::ProfileEditField::Bio if p.edit_bio.chars().count() < 500 => {
                                p.edit_bio.push(c);
                            }
                            state::ProfileEditField::Music if p.edit_music.chars().count() < 200 => {
                                p.edit_music.push(c);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn update_invitation(app: &mut App, state: &mut State) {
    if state.pending_invitation.is_none() {
        return;
    }
    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let accept_btn = Btn {
        x: ww - 270.0,
        y: wh - 70.0,
        w: 120.0,
        h: 50.0,
    };
    let decline_btn = Btn {
        x: ww - 140.0,
        y: wh - 70.0,
        w: 110.0,
        h: 50.0,
    };
    if accept_btn.clicked(app) {
        if let Some((_, room_id, _)) = state.pending_invitation.take() {
            if state.conn.is_live() {
                state.conn.send(&ClientMessage::JoinRoom { id: room_id });
                state.screen = Screen::RoomBrowser;
            } else {
                state.conn.connect(connection::now_secs());
                state.rooms.clear();
                state.pending_join = Some(room_id);
                state.screen = Screen::RoomBrowser;
            }
        }
    } else if decline_btn.clicked(app) {
        state.pending_invitation = None;
    }
}

fn update(app: &mut App, state: &mut State) {
    let State { session, conn, .. } = state;
    if let Some(session) = session.as_mut() {
        session.clock += app.timer.delta_f32() as f64;
        session.ping_rtt_ms = conn.rtt_ms();
    }

    if state.startup_check.is_some() {
        login::poll_startup_check(state);
    }

    update_invitation(app, state);
    network::handle_server_messages(state);

    match state.screen {
        Screen::Auth => login::update_auth(app, state),
        Screen::Menu => menu::update_menu(app, state),
        Screen::Settings => menu::update_settings(app, state),
        Screen::RoomBrowser => rooms::update_browser(app, state),
        Screen::CreateRoom => rooms::update_create_room(app, state),
        Screen::JoinById => rooms::update_join_by_id(app, state),
        Screen::RoomLobby => rooms::update_lobby(app, state),
        Screen::Profile => profile::update_profile(app, state),
        Screen::Friends => friends::update_friends(app, state),
        Screen::OtherProfile => other_profile::update_other_profile(app, state),
        Screen::Game => {
            let is_host = state.lobby.as_ref().map(|l| l.is_host).unwrap_or(false);
            let State {
                session,
                settings,
                conn,
                ..
            } = &mut *state;
            if let Some(session) = session {
                logic::update_game(app, session, settings, conn, is_host);
            }
        }
    }
}

fn draw_invitation_banner(app: &mut App, gfx: &mut Graphics, state: &State) {
    let Some((ref from, _, ref room_name)) = state.pending_invitation else {
        return;
    };
    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let banner_y = wh - 80.0;
    let mut d = gfx.create_draw();
    d.rect((0.0, banner_y), (ww, 80.0))
        .color(Color::from_rgba(0.08, 0.10, 0.20, 0.96));
    d.rect((0.0, banner_y), (ww, 2.0)).color(Color::from_rgb(0.4, 0.4, 0.7));
    let msg = format!("{from} t'invite dans \"{room_name}\"");
    d.text(&state.font, &msg)
        .position(20.0, banner_y + 40.0)
        .size(20.0)
        .v_align_middle()
        .color(Color::WHITE);
    let accept_btn = Btn {
        x: ww - 270.0,
        y: banner_y + 15.0,
        w: 120.0,
        h: 50.0,
    };
    let decline_btn = Btn {
        x: ww - 140.0,
        y: banner_y + 15.0,
        w: 110.0,
        h: 50.0,
    };
    accept_btn.draw_styled(&mut d, app, &state.font, "Rejoindre", true);
    decline_btn.draw(&mut d, app, &state.font, "Ignorer");
    gfx.render(&d);
}

fn draw(app: &mut App, gfx: &mut Graphics, state: &mut State) {
    match state.screen {
        Screen::Auth => login::draw_auth(app, gfx, state),
        Screen::Menu => menu::draw_menu(app, gfx, state),
        Screen::Settings => menu::draw_settings(app, gfx, state),
        Screen::RoomBrowser => rooms::draw_browser(app, gfx, state),
        Screen::CreateRoom => rooms::draw_create_room(app, gfx, state),
        Screen::JoinById => rooms::draw_join_by_id(app, gfx, state),
        Screen::RoomLobby => rooms::draw_lobby(app, gfx, state),
        Screen::Profile => profile::draw_profile(app, gfx, state),
        Screen::Friends => friends::draw_friends(app, gfx, state),
        Screen::OtherProfile => other_profile::draw_other_profile(app, gfx, state),
        Screen::Game => {
            let is_host = state.lobby.as_ref().map(|l| l.is_host).unwrap_or(false);
            let can_pause = state.lobby.as_ref().is_some_and(|l| l.settings.pause.allows(l.is_host));
            if let Some(session) = state.session.as_ref() {
                draw::draw_game(app, gfx, session, &state.font, is_host, can_pause);
            }
        }
    }
    draw_invitation_banner(app, gfx, state);
    draw_reconnect_banner(app, gfx, state);
}

fn draw_reconnect_banner(app: &mut App, gfx: &mut Graphics, state: &State) {
    let Some((attempts, secs_left)) = state.conn.recovering(connection::now_secs()) else {
        return;
    };
    let ww = app.window().width() as f32;
    let mut d = gfx.create_draw();
    d.rect((0.0, 0.0), (ww, 44.0))
        .color(Color::from_rgba(0.35, 0.18, 0.05, 0.96));
    d.rect((0.0, 44.0), (ww, 2.0)).color(Color::from_rgb(0.9, 0.6, 0.2));
    let dots = ".".repeat(1 + (app.timer.elapsed_f32() * 2.0) as usize % 3);
    let msg = format!("Reconnexion{dots} (tentative {attempts}, {secs_left:.0}s restantes)");
    d.text(&state.font, &msg)
        .position(ww / 2.0, 22.0)
        .size(20.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(1.0, 0.85, 0.6));
    gfx.render(&d);
}

#[notan_main]
fn main() -> Result<(), String> {
    let icon = Some(include_bytes!("../../assets/puyo_puyo_icon.ico").as_ref());
    let win_config = WindowConfig::new()
        .set_title("Puyorust")
        .set_size(1280, 800)
        .set_resizable(true)
        .set_window_icon_data(icon)
        .set_taskbar_icon_data(icon);

    notan::init_with(setup)
        .add_config(DrawConfig)
        .add_config(win_config)
        .event(event)
        .update(update)
        .draw(draw)
        .build()
}
