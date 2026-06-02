use notan::prelude::*;
use shared::{Board, config};
use ewebsock::{WsReceiver, WsSender};
use crate::Font;

#[derive(AppState)]
pub struct State {
    pub board: Board,
    pub other_board: Board,
    pub my_player_id: Option<u8>,
    pub initial_seed: u64,
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,

    pub waiting_for_opponent: bool,
    pub opponent_disconnected: bool,
    pub room_full: bool,

    pub game_over_sent: bool,
    pub did_i_win: bool,

    pub my_piece_seq: u32,
    pub expected_opponent_seq: u32,
    pub resync_pending: bool,

    pub last_fall_time: f32,
    pub last_resolve_time: f32,
    pub played_time: f32,
    pub garbage_delay_timer: f32,

    pub key_timer_left: f32,
    pub key_timer_right: f32,
    pub key_timer_down: f32,

    pub last_server_msg: String,
    pub font: Font,
}

impl State {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, font: Font, now: f32) -> Self {
        let mut board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 12345);
        board.spawn_piece();
        let other_board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 12345);

        Self {
            board,
            other_board,
            my_player_id: None,
            initial_seed: 12345,
            ws_sender,
            ws_receiver,
            waiting_for_opponent: true,
            opponent_disconnected: false,
            room_full: false,
            game_over_sent: false,
            did_i_win: false,
            my_piece_seq: 0,
            expected_opponent_seq: 0,
            resync_pending: false,
            last_fall_time: now,
            last_resolve_time: now,
            played_time: 0.0,
            garbage_delay_timer: 0.0,
            key_timer_left: 0.0,
            key_timer_right: 0.0,
            key_timer_down: 0.0,
            last_server_msg: String::from("Connexion..."),
            font,
        }
    }
}
