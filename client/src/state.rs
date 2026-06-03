use notan::prelude::*;
use shared::{Board, config};
use ewebsock::{WsReceiver, WsSender};
use crate::Font;

#[derive(AppState)]
pub struct State {
    pub board: Board,
    pub other_board: Board,
    pub my_player_id: Option<u8>,
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,

    pub waiting_for_opponent: bool,
    pub opponent_disconnected: bool,
    pub room_full: bool,

    pub key_timer_left: f32,
    pub key_timer_right: f32,
    pub key_timer_down: f32,

    pub last_server_msg: String,
    pub font: Font,
}

impl State {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, font: Font) -> Self {
        let board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 0);
        let other_board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 0);

        Self {
            board,
            other_board,
            my_player_id: None,
            ws_sender,
            ws_receiver,
            waiting_for_opponent: true,
            opponent_disconnected: false,
            room_full: false,
            key_timer_left: 0.0,
            key_timer_right: 0.0,
            key_timer_down: 0.0,
            last_server_msg: String::from("Connexion..."),
            font,
        }
    }
}
