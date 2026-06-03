use notan::prelude::*;
use shared::{Board, InputKind, config};
use ewebsock::{WsReceiver, WsSender};
use crate::Font;

#[derive(AppState)]
pub struct State {
    pub board: Board,
    pub predicted_board: Board,
    pub other_board: Board,
    pub my_player_id: Option<u8>,
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,

    pub waiting_for_opponent: bool,
    pub opponent_disconnected: bool,
    pub room_full: bool,

    pub input_seq: u32,                          // compteur d'inputs envoyés
    pub my_ack: u32,                             // dernier seq acquitté par le serveur
    pub pending_inputs: Vec<(u32, InputKind)>,   // inputs envoyés non encore acquittés

    pub key_timer_left: f32,
    pub key_timer_right: f32,
    pub key_timer_down: f32,

    pub last_server_msg: String,
    pub font: Font,
}

impl State {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, font: Font) -> Self {
        let board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 0);
        let predicted_board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 0);
        let other_board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 0);

        Self {
            board,
            predicted_board,
            other_board,
            my_player_id: None,
            ws_sender,
            ws_receiver,
            waiting_for_opponent: true,
            opponent_disconnected: false,
            room_full: false,
            input_seq: 0,
            my_ack: 0,
            pending_inputs: Vec::new(),
            key_timer_left: 0.0,
            key_timer_right: 0.0,
            key_timer_down: 0.0,
            last_server_msg: String::from("Connexion..."),
            font,
        }
    }
}
