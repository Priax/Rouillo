use ewebsock::{WsMessage, WsReceiver, WsSender};
use notan::prelude::*;
use shared::{config, Board, ClientMessage, InputKind, LobbyInfo, RoomInfo};

use crate::Font;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    Settings,
    RoomBrowser,
    CreateRoom,
    JoinById,
    RoomLobby,
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

pub struct Net {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
}

impl Net {
    pub fn send(&mut self, msg: &ClientMessage) {
        self.ws_sender.send(WsMessage::Binary(shared::encode(msg)));
    }
}

pub struct GameSession {
    pub board: Board,
    pub predicted_board: Board,
    pub other_board: Board,
    pub my_slot: u8,

    pub opponent_disconnected: bool,

    pub input_seq: u32,
    pub my_ack: u32,
    pub pending_inputs: Vec<(u32, InputKind)>,

    pub key_timer_left: f32,
    pub key_timer_right: f32,
    pub key_timer_down: f32,

    pub piece_visual_offset: (f32, f32),
    pub opponent_piece_offset: (f32, f32),

    pub chain_display: Option<(u32, f32)>,
    pub all_clear_timer: f32,

    pub clock: f64,
    pub sent_at: Vec<(u32, f64)>,

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub last_server_msg: String,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub last_rtt_ms: f32,
}

impl GameSession {
    pub fn new(my_slot: u8) -> Self {
        let board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 0, 1, 5);
        Self {
            predicted_board: board.clone(),
            other_board: board.clone(),
            board,
            my_slot,
            opponent_disconnected: false,
            input_seq: 0,
            my_ack: 0,
            pending_inputs: Vec::new(),
            key_timer_left: 0.0,
            key_timer_right: 0.0,
            key_timer_down: 0.0,
            piece_visual_offset: (0.0, 0.0),
            opponent_piece_offset: (0.0, 0.0),
            chain_display: None,
            all_clear_timer: 0.0,
            clock: 0.0,
            sent_at: Vec::new(),
            last_server_msg: String::new(),
            last_rtt_ms: 0.0,
        }
    }
}

fn gen_player_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

pub fn load_or_create_player_id() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(Some(id)) = storage.get_item("puyorust_player_id") {
                if !id.is_empty() {
                    return id;
                }
            }
            let id = gen_player_id();
            let _ = storage.set_item("puyorust_player_id", &id);
            return id;
        }
        gen_player_id()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        gen_player_id()
    }
}

#[derive(AppState)]
pub struct State {
    pub screen: Screen,
    pub settings: Settings,
    pub player_id: String,
    pub net: Option<Net>,
    pub rooms: Vec<RoomInfo>,
    pub lobby: Option<LobbyInfo>,
    pub text_input: String,
    pub notice: String,
    pub session: Option<GameSession>,
    pub font: Font,
}

impl State {
    pub fn new(font: Font) -> Self {
        Self {
            screen: Screen::Menu,
            settings: Settings::default(),
            player_id: load_or_create_player_id(),
            net: None,
            rooms: Vec::new(),
            lobby: None,
            text_input: String::new(),
            notice: String::new(),
            session: None,
            font,
        }
    }
}
