use notan::prelude::*;
use notan::draw::*;
use shared::config;

mod state;
mod network;
mod logic;
mod draw;

use state::State;

fn setup(app: &mut App, gfx: &mut Graphics) -> State {
    let (ws_sender, ws_receiver) = ewebsock::connect(config::SERVER_URL, ewebsock::Options::default()).unwrap();
    let font = gfx.create_font(include_bytes!("../../assets/arcadeFont.ttf")).unwrap();
    let now = app.timer.elapsed_f32();

    State::new(ws_sender, ws_receiver, font, now)
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
