use notan::prelude::*;
use notan::draw::*;
use shared::config;

mod state;
mod network;
mod logic;
mod draw;

use state::State;

fn server_url() -> String {
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
    let (ws_sender, ws_receiver) = ewebsock::connect(&server_url(), ewebsock::Options::default()).unwrap();
    let font = gfx.create_font(include_bytes!("../../assets/arcadeFont.ttf")).unwrap();

    State::new(ws_sender, ws_receiver, font)
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
        .update(logic::update)
        .draw(draw::draw)
        .build()
}
