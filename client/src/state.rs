use notan::prelude::*;
use shared::{Board, InputKind, config};
use ewebsock::{WsReceiver, WsSender};
use crate::Font;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    Settings,
    Game,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub das_delay: f32,
    pub das_speed: f32,
    pub soft_drop_speed: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            das_delay: config::DAS_DELAY,
            das_speed: config::DAS_SPEED,
            soft_drop_speed: config::SOFT_DROP_SPEED,
        }
    }
}

impl Settings {
    pub const COUNT: usize = 3;

    pub fn label(i: usize) -> &'static str {
        match i {
            0 => "DAS delay",
            1 => "DAS speed",
            _ => "Soft drop",
        }
    }

    pub fn value(&self, i: usize) -> f32 {
        match i {
            0 => self.das_delay,
            1 => self.das_speed,
            _ => self.soft_drop_speed,
        }
    }

    fn step(i: usize) -> f32 {
        match i {
            0 => 0.01,
            _ => 0.005,
        }
    }

    fn range(i: usize) -> (f32, f32) {
        match i {
            0 => (0.05, 0.50),
            _ => (0.005, 0.20),
        }
    }

    pub fn adjust(&mut self, i: usize, dir: i32) {
        let (min, max) = Self::range(i);
        let new = (self.value(i) + dir as f32 * Self::step(i)).clamp(min, max);
        match i {
            0 => self.das_delay = new,
            1 => self.das_speed = new,
            _ => self.soft_drop_speed = new,
        }
    }
}

pub struct GameSession {
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
}

impl GameSession {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
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
        }
    }
}

#[derive(AppState)]
pub struct State {
    pub screen: Screen,
    pub settings: Settings,
    pub session: Option<GameSession>,
    pub font: Font,
}

impl State {
    pub fn new(font: Font) -> Self {
        Self {
            screen: Screen::Menu,
            settings: Settings::default(),
            session: None,
            font,
        }
    }
}
