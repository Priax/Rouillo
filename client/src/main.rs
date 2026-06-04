use notan::prelude::*;
use notan::draw::*;
use shared::config;

mod state;
mod network;
mod logic;
mod draw;
mod menu;

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

fn update(app: &mut App, state: &mut State) {
    match state.screen {
        Screen::Menu => menu::update_menu(app, state),
        Screen::Settings => menu::update_settings(app, state),
        Screen::Game => {
            let back_to_menu = {
                let State { session, settings, .. } = &mut *state;
                match session {
                    Some(session) => logic::update_game(app, session, settings),
                    None => false,
                }
            };
            if back_to_menu {
                state.session = None;
                state.screen = Screen::Menu;
            }
        }
    }
}

fn draw(app: &mut App, gfx: &mut Graphics, state: &mut State) {
    match state.screen {
        Screen::Menu => menu::draw_menu(app, gfx, state),
        Screen::Settings => menu::draw_settings(app, gfx, state),
        Screen::Game => {
            if let Some(session) = state.session.as_ref() {
                draw::draw_game(app, gfx, session, &state.font);
            }
        }
    }
}

#[notan_main]
fn main() -> Result<(), String> {
    let win_config = WindowConfig::new()
        .set_title("Rouillo")
        .set_size(1280, 800)
        .set_resizable(true);

    notan::init_with(setup)
        .add_config(DrawConfig)
        .add_config(win_config)
        .update(update)
        .draw(draw)
        .build()
}
