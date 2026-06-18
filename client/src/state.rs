use ewebsock::{WsMessage, WsReceiver, WsSender};
use notan::prelude::*;
use shared::{config, Board, ClientMessage, InputKind, LobbyInfo, RoomId, RoomInfo};

use crate::Font;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Auth,
    Menu,
    Settings,
    RoomBrowser,
    CreateRoom,
    JoinById,
    RoomLobby,
    Game,
    Profile,
    Friends,
    OtherProfile,
}

pub use crate::http::HttpSlot;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Login,
    Register,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthField {
    Username,
    Password,
}

pub struct AuthForm {
    pub username: String,
    pub password: String,
    pub focused: AuthField,
    pub mode: AuthMode,
    pub error: String,
    pub pending: Option<HttpSlot>,
}

impl Default for AuthForm {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            focused: AuthField::Username,
            mode: AuthMode::Login,
            error: String::new(),
            pending: None,
        }
    }
}

pub struct AuthInfo {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub elo: i32,
}

#[derive(serde::Deserialize)]
pub struct ApiAuthResponse {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub elo: i32,
}

#[derive(serde::Deserialize)]
pub struct ApiMeResponse {
    pub id: String,
    pub username: String,
    pub elo: i32,
}

#[derive(serde::Deserialize)]
pub struct ApiUserProfile {
    pub username: String,
    pub elo: i32,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub total_matches: i64,
    pub wins: i64,
    pub all_time_max_chain: i32,
    pub total_nuisance_sent: i64,
}

#[derive(serde::Deserialize, Clone)]
pub struct FriendEntry {
    pub user_id: String,
    pub username: String,
    pub elo: i32,
}

#[derive(serde::Deserialize)]
pub struct ApiFriendsResponse {
    pub friends: Vec<FriendEntry>,
    pub sent: Vec<FriendEntry>,
    pub received: Vec<FriendEntry>,
}

#[derive(serde::Deserialize, Clone)]
pub struct UserSearchEntry {
    pub user_id: String,
    pub username: String,
    pub elo: i32,
}

pub struct FriendsData {
    pub friends: Vec<FriendEntry>,
    pub sent: Vec<FriendEntry>,
    pub received: Vec<FriendEntry>,
    pub list_slot: Option<HttpSlot>,
    pub search_input: String,
    pub search_results: Vec<UserSearchEntry>,
    pub search_slot: Option<HttpSlot>,
    pub search_error: String,
    pub add_pending: Option<HttpSlot>,
    pub add_error: String,
    pub add_success: bool,
    pub confirm_remove: Option<String>,
    pub action_pending: Option<HttpSlot>,
    pub action_error: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FriendshipStatus {
    Unknown,
    NotFriends,
    Friends,
    RequestSent,
    RequestReceived,
}

pub struct OtherProfileData {
    pub user_id: String,
    pub username: String,
    pub elo: i32,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub total_matches: i64,
    pub wins: i64,
    pub all_time_max_chain: i32,
    pub total_nuisance_sent: i64,
    pub match_history: Vec<ApiMatchEntry>,
    pub profile_slot: Option<HttpSlot>,
    pub history_slot: Option<HttpSlot>,
    pub friendship_check_slot: Option<HttpSlot>,
    pub load_failed: bool,
    pub friendship: FriendshipStatus,
    pub friend_slot: Option<HttpSlot>,
    pub friend_msg: String,
    pub friend_success: bool,
    pub prev_screen: Screen,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProfileEditField {
    Bio,
    Music,
}

#[derive(serde::Deserialize)]
pub struct ApiMatchPlayer {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub max_chain: i16,
    pub nuisance_sent: i32,
}

#[derive(serde::Deserialize)]
pub struct ApiMatchEntry {
    pub winner_slot: i16,
    pub duration_secs: f64,
    pub player1: ApiMatchPlayer,
    pub player2: ApiMatchPlayer,
}

pub struct ProfileData {
    pub user_id: String,
    pub username: String,
    pub elo: i32,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub total_matches: i64,
    pub wins: i64,
    pub all_time_max_chain: i32,
    pub total_nuisance_sent: i64,
    pub match_history: Vec<ApiMatchEntry>,
    pub profile_slot: Option<HttpSlot>,
    pub history_slot: Option<HttpSlot>,
    pub editing: bool,
    pub edit_bio: String,
    pub edit_music: String,
    pub edit_focused: ProfileEditField,
    pub edit_pending: Option<HttpSlot>,
    pub edit_error: String,
}

pub fn load_stored_token() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("puyorust_token").ok().flatten())
            .filter(|t| !t.is_empty())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

pub fn save_token(token: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item("puyorust_token", token);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = token;
    }
}

pub fn clear_stored_token() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.remove_item("puyorust_token");
        }
    }
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
            1 => 0.005,
            _ => 0.01,
        }
    }

    fn range(i: usize) -> (f32, f32) {
        match i {
            0 => (0.05, 0.50),
            1 => (0.005, 0.20),
            _ => (0.05, 0.50),
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
    pub ui_font: Font,
    pub auth: Option<AuthInfo>,
    pub auth_form: AuthForm,
    pub profile: Option<ProfileData>,
    pub friends: Option<FriendsData>,
    pub other_profile: Option<OtherProfileData>,
    pub startup_check: Option<HttpSlot>,
    pub pending_invitation: Option<(String, RoomId, String)>,
    pub pending_join: Option<RoomId>,
    pub invite_overlay: bool,
    pub invite_slot: Option<HttpSlot>,
    pub invite_friends: Vec<FriendEntry>,
}

impl State {
    pub fn new(font: Font, ui_font: Font) -> Self {
        Self {
            screen: Screen::Auth,
            settings: Settings::default(),
            player_id: load_or_create_player_id(),
            net: None,
            rooms: Vec::new(),
            lobby: None,
            text_input: String::new(),
            notice: String::new(),
            session: None,
            font,
            ui_font,
            auth: None,
            auth_form: AuthForm::default(),
            profile: None,
            friends: None,
            other_profile: None,
            startup_check: None,
            pending_invitation: None,
            pending_join: None,
            invite_overlay: false,
            invite_slot: None,
            invite_friends: Vec::new(),
        }
    }
}
