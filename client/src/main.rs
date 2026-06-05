use notan::prelude::*;
use notan::draw::*;
use notan::app::Event;
use shared::config;

mod state;
mod network;
mod logic;
mod draw;
mod menu;
mod rooms;

use state::{State, Screen};

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
        config::SERVER_URL.to_string()
    }
}

fn setup(gfx: &mut Graphics) -> State {
    let font = gfx.create_font(include_bytes!("../../assets/arcadeFont.ttf")).unwrap();
    State::new(font)
}

fn event(state: &mut State, evt: Event) {
    if let Event::ReceivedCharacter(c) = evt {
        if c.is_control() {
            return;
        }
        match state.screen {
            Screen::CreateRoom if state.text_input.chars().count() < 24 => {
                state.text_input.push(c);
            }
            Screen::JoinById if c.is_ascii_digit() && state.text_input.len() < 9 => {
                state.text_input.push(c);
            }
            _ => {}
        }
    }
}

fn update(app: &mut App, state: &mut State) {
    network::handle_server_messages(state);

    match state.screen {
        Screen::Menu => menu::update_menu(app, state),
        Screen::Settings => menu::update_settings(app, state),
        Screen::RoomBrowser => rooms::update_browser(app, state),
        Screen::CreateRoom => rooms::update_create_room(app, state),
        Screen::JoinById => rooms::update_join_by_id(app, state),
        Screen::RoomLobby => rooms::update_lobby(app, state),
        Screen::Game => {
            let is_host = state.lobby.as_ref().map(|l| l.is_host).unwrap_or(false);
            let State { session, settings, net, .. } = &mut *state;
            if let (Some(session), Some(net)) = (session, net) {
                logic::update_game(app, session, settings, net, is_host);
            }
        }
    }
}

fn draw(app: &mut App, gfx: &mut Graphics, state: &mut State) {
    match state.screen {
        Screen::Menu => menu::draw_menu(app, gfx, state),
        Screen::Settings => menu::draw_settings(app, gfx, state),
        Screen::RoomBrowser => rooms::draw_browser(app, gfx, state),
        Screen::CreateRoom => rooms::draw_create_room(app, gfx, state),
        Screen::JoinById => rooms::draw_join_by_id(app, gfx, state),
        Screen::RoomLobby => rooms::draw_lobby(app, gfx, state),
        Screen::Game => {
            let is_host = state.lobby.as_ref().map(|l| l.is_host).unwrap_or(false);
            if let Some(session) = state.session.as_ref() {
                draw::draw_game(app, gfx, session, &state.font, is_host);
            }
        }
    }
}

#[notan_main]
fn main() -> Result<(), String> {
    let win_config = WindowConfig::new()
        .set_title("Puyorust")
        .set_size(1280, 800)
        .set_resizable(true);

    notan::init_with(setup)
        .add_config(DrawConfig)
        .add_config(win_config)
        .event(event)
        .update(update)
        .draw(draw)
        .build()
}
