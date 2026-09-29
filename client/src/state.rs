use notan::prelude::*;
use shared::{config, Board, IncomingGarbage, LobbyInfo, RoomId, RoomInfo, StampedInput};

use crate::connection::Connection;
use crate::interp::OpponentView;
use crate::ui::Status;

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

impl Screen {
    pub fn needs_connection(self) -> bool {
        matches!(
            self,
            Self::RoomBrowser | Self::CreateRoom | Self::JoinById | Self::RoomLobby | Self::Game
        )
    }
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
    pub status: Status,
    pub pending: Option<HttpSlot>,
}

impl Default for AuthForm {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            focused: AuthField::Username,
            mode: AuthMode::Login,
            status: Status::Empty,
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
    pub search_status: Status,
    pub add_pending: Option<HttpSlot>,
    pub add_status: Status,
    pub confirm_remove: Option<String>,
    pub action_pending: Option<HttpSlot>,
    pub action_status: Status,
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
    pub core: ProfileCore,
    pub friendship_check_slot: Option<HttpSlot>,
    pub load_failed: bool,
    pub friendship: FriendshipStatus,
    pub friend_slot: Option<HttpSlot>,
    pub friend_status: Status,
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

pub struct ProfileCore {
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
}

impl ProfileCore {
    pub fn loading(
        user_id: String,
        username: String,
        elo: i32,
        profile_slot: HttpSlot,
        history_slot: HttpSlot,
    ) -> Self {
        Self {
            user_id,
            username,
            elo,
            bio: None,
            favorite_music: None,
            total_matches: 0,
            wins: 0,
            all_time_max_chain: 0,
            total_nuisance_sent: 0,
            match_history: Vec::new(),
            profile_slot: Some(profile_slot),
            history_slot: Some(history_slot),
        }
    }
}

pub struct ProfileData {
    pub core: ProfileCore,
    pub editing: bool,
    pub edit_bio: String,
    pub edit_music: String,
    pub edit_focused: ProfileEditField,
    pub edit_pending: Option<HttpSlot>,
    pub edit_status: Status,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            das_delay: config::DAS_DELAY,
            das_speed: config::DAS_SPEED,
        }
    }
}

impl Settings {
    pub const COUNT: usize = 2;

    pub fn label(i: usize) -> &'static str {
        match i {
            0 => "DAS delay",
            _ => "DAS speed",
        }
    }

    pub fn value(self, i: usize) -> f32 {
        match i {
            0 => self.das_delay,
            _ => self.das_speed,
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
            _ => self.das_speed = new,
        }
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
    pub pending_inputs: Vec<(u32, StampedInput)>,

    pub key_timer_left: f32,
    pub key_timer_right: f32,
    pub soft_drop_held: bool,

    pub my_turn: TurnAnim,
    pub opp_turn: TurnAnim,

    pub piece_visual_offset: (f32, f32),
    pub opponent_view: OpponentView,

    pub chain_display: Option<(u32, f32)>,
    pub all_clear_timer: f32,

    pub clock: f64,
    pub sent_at: Vec<(u32, f64)>,

    pub sim_accumulator: f32,

    pub server_tick: u32,

    pub local_tick: u32,
    pub synced: bool,
    pub clock_correction: i32,
    pub drift_min: Option<i32>,
    pub drift_samples: u32,

    pub incoming: Vec<IncomingGarbage>,
    pub opp_incoming: Vec<IncomingGarbage>,

    pub announced_chain: (u32, u32),
    pub announced_all_clear: u32,

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub last_server_msg: String,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub last_rtt_ms: f32,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub ping_rtt_ms: Option<f32>,
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
            soft_drop_held: false,
            my_turn: TurnAnim::default(),
            opp_turn: TurnAnim::default(),
            piece_visual_offset: (0.0, 0.0),
            opponent_view: OpponentView::default(),
            chain_display: None,
            all_clear_timer: 0.0,
            clock: 0.0,
            sent_at: Vec::new(),
            sim_accumulator: 0.0,
            server_tick: 0,
            local_tick: 0,
            synced: false,
            clock_correction: 0,
            drift_min: None,
            drift_samples: 0,
            incoming: Vec::new(),
            opp_incoming: Vec::new(),
            announced_chain: (0, 0),
            announced_all_clear: 0,
            last_server_msg: String::new(),
            last_rtt_ms: 0.0,
            ping_rtt_ms: None,
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct TurnAnim {
    piece_id: u32,
    shown: f32,
}

const TURN_FRAMES: f32 = 7.0;

impl TurnAnim {
    pub fn update(&mut self, piece_id: u32, rotation: usize, dt: f32) {
        let target = rotation as f32;
        if piece_id != self.piece_id {
            *self = Self {
                piece_id,
                shown: target,
            };
            return;
        }
        let diff = (target - self.shown + 2.0).rem_euclid(4.0) - 2.0;
        let step = dt * 60.0 / TURN_FRAMES;
        self.shown = if diff.abs() <= step {
            target
        } else {
            (self.shown + step * diff.signum()).rem_euclid(4.0)
        };
    }

    pub fn satellite(self) -> (f32, f32) {
        let angle = self.shown * std::f32::consts::FRAC_PI_2;
        (-angle.cos(), angle.sin())
    }
}

impl GameSession {
    pub fn my_nuisance(&self) -> u32 {
        let travelling: u32 = self
            .incoming
            .iter()
            .filter(|g| g.at > self.local_tick)
            .map(|g| g.amount)
            .sum();
        self.predicted_board.pending_garbage + travelling
    }

    pub fn opp_nuisance(&self) -> u32 {
        let travelling: u32 = self.opp_incoming.iter().map(|g| g.amount).sum();
        self.other_board.pending_garbage + travelling
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
    pub conn: Connection,
    pub rooms: Vec<RoomInfo>,
    pub lobby: Option<LobbyInfo>,
    pub text_input: String,
    pub notice: Status,
    pub session: Option<GameSession>,
    pub fonts: crate::ui::Fonts,
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
    pub outdated: bool,
    pub ui: crate::ui::Ui,
    pub backspace: crate::ui::KeyRepeat,
}

impl State {
    pub fn new(fonts: crate::ui::Fonts) -> Self {
        let player_id = load_or_create_player_id();
        Self {
            screen: Screen::Auth,
            settings: Settings::default(),
            player_id: player_id.clone(),
            conn: Connection::new(&player_id),
            rooms: Vec::new(),
            lobby: None,
            text_input: String::new(),
            notice: Status::Empty,
            session: None,
            fonts,
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
            outdated: false,
            ui: crate::ui::Ui::default(),
            backspace: crate::ui::KeyRepeat::default(),
        }
    }
}
