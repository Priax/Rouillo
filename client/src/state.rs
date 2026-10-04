use notan::prelude::*;
use shared::{config, Board, GameState, IncomingGarbage, LobbyInfo, RoomId, RoomInfo, StampedInput};

use crate::connection::Connection;
use crate::interp::OpponentView;
use crate::storage;
use crate::ui::{Status, TextInput};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Title,
    Auth,
    Menu,
    PlayMenu,
    Settings,
    RoomBrowser,
    CreateRoom,
    JoinById,
    RoomLobby,
    Game,
    Profile,
    Friends,
    OtherProfile,
    SoloSetup,
    Solo,
    Ranked,
    Leaderboard,
    Help,
}

impl Screen {
    pub fn hue(self) -> f32 {
        use crate::theme::hue;
        match self {
            Self::Title | Self::Auth | Self::Menu | Self::PlayMenu | Self::Help => hue::PURPLE,
            Self::RoomBrowser
            | Self::CreateRoom
            | Self::JoinById
            | Self::RoomLobby
            | Self::Game
            | Self::SoloSetup
            | Self::Solo
            | Self::Ranked => hue::BLUE,
            Self::Friends => hue::GREEN,
            Self::Profile | Self::OtherProfile => hue::PINK,
            Self::Settings | Self::Leaderboard => hue::ORANGE,
        }
    }

    pub fn needs_connection(self) -> bool {
        matches!(
            self,
            Self::RoomBrowser | Self::CreateRoom | Self::JoinById | Self::RoomLobby | Self::Game | Self::Ranked
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
    pub username: TextInput,
    pub password: TextInput,
    pub focused: AuthField,
    pub mode: AuthMode,
    pub status: Status,
    pub pending: Option<HttpSlot>,
}

pub const MAX_PASSWORD_CHARS: usize = 64;

impl Default for AuthForm {
    fn default() -> Self {
        Self {
            username: TextInput::default().max_chars(shared::MAX_USERNAME_CHARS),
            password: TextInput::masked().max_chars(MAX_PASSWORD_CHARS),
            focused: AuthField::Username,
            mode: AuthMode::Login,
            status: Status::Empty,
            pending: None,
        }
    }
}

impl AuthForm {
    pub const fn focused_input(&mut self) -> &mut TextInput {
        match self.focused {
            AuthField::Username => &mut self.username,
            AuthField::Password => &mut self.password,
        }
    }
}

pub struct AuthInfo {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub elo: i32,
    pub avatar_url: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct ApiAuthResponse {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub elo: i32,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct ApiMeResponse {
    pub id: String,
    pub username: String,
    pub elo: i32,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(serde::Deserialize, Default)]
pub struct ApiUserProfile {
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub banner_url: Option<String>,
    pub elo: i32,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub total_matches: i64,
    pub casual_matches: i64,
    pub casual_wins: i64,
    pub ranked_series: i64,
    pub ranked_series_won: i64,
    pub all_time_max_chain: i32,
    pub total_nuisance_sent: i64,
}

#[derive(serde::Deserialize, Clone)]
pub struct FriendEntry {
    pub user_id: String,
    pub username: String,
    pub elo: i32,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub playing: bool,
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
    #[serde(default)]
    pub avatar_url: Option<String>,
}

pub struct FriendsData {
    pub friends: Vec<FriendEntry>,
    pub sent: Vec<FriendEntry>,
    pub received: Vec<FriendEntry>,
    pub list_slot: Option<HttpSlot>,
    pub search_input: TextInput,
    pub search_results: Vec<UserSearchEntry>,
    pub search_slot: Option<HttpSlot>,
    pub search_status: Status,
    pub add_pending: Option<HttpSlot>,
    pub add_status: Status,
    pub confirm_remove: Option<String>,
    pub action_pending: Option<HttpSlot>,
    pub action_status: Status,
    pub pages: [crate::ui::Pager; 3],
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

#[derive(serde::Deserialize)]
pub struct ApiMatchPlayer {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub max_chain: i16,
    pub nuisance_sent: i32,
}

#[derive(serde::Deserialize)]
pub struct ApiMatchEntry {
    pub winner_slot: Option<i16>,
    #[serde(default)]
    pub ranked: bool,
    pub duration_secs: f64,
    pub player1: ApiMatchPlayer,
    pub player2: ApiMatchPlayer,
}

pub struct ProfileCore {
    pub user_id: String,
    pub info: ApiUserProfile,
    pub history: crate::history::History,
    pub about_open: bool,
    pub profile_slot: Option<HttpSlot>,
}

pub struct ProfileData {
    pub core: ProfileCore,
    pub prev_screen: Screen,
    pub edit: Option<crate::profile_edit::EditForm>,
    pub account: Option<crate::account::AccountForm>,
}

const TOKEN_KEY: &str = "puyorust_token";
const BEST_SCORE_KEY: &str = "rouillo_solo_best";
const PLAYER_ID_KEY: &str = "puyorust_player_id";

pub fn load_stored_token() -> Option<String> {
    storage::get(TOKEN_KEY)
}

pub fn save_token(token: &str) {
    storage::set(TOKEN_KEY, token);
}

pub fn clear_stored_token() {
    storage::remove(TOKEN_KEY);
}

pub fn load_best_score() -> i32 {
    storage::get(BEST_SCORE_KEY).and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub fn save_best_score(score: i32) {
    storage::set(BEST_SCORE_KEY, &score.to_string());
}

pub fn clear_best_score() {
    storage::remove(BEST_SCORE_KEY);
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
            0 => "Délai DAS",
            _ => "Vitesse DAS",
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

    const KEYS: [&'static str; Self::COUNT] = ["rouillo_das_delay", "rouillo_das_speed"];

    fn set(&mut self, i: usize, value: f32) {
        let (min, max) = Self::range(i);
        let value = value.clamp(min, max);
        match i {
            0 => self.das_delay = value,
            _ => self.das_speed = value,
        }
    }

    pub fn adjust(&mut self, i: usize, dir: i32) {
        self.set(i, self.value(i) + dir as f32 * Self::step(i));
    }

    pub fn load() -> Self {
        let mut settings = Self::default();
        for (i, key) in Self::KEYS.iter().enumerate() {
            if let Some(value) = storage::get(key)
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite())
            {
                settings.set(i, value);
            }
        }
        settings
    }

    pub fn save(self) {
        for (i, key) in Self::KEYS.iter().enumerate() {
            storage::set(key, &self.value(i).to_string());
        }
    }
}

impl State {
    /// The online game being played or watched.
    pub fn session(&self) -> Option<&GameSession> {
        self.room.as_ref()?.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Option<&mut GameSession> {
        self.room.as_mut()?.session.as_mut()
    }
}

/// Everything that lasts as long as a stay in one room.
pub struct Room {
    pub info: LobbyInfo,
    pub session: Option<GameSession>,
    pub chat: crate::chat::Chat,
    pub series_over: Option<(Option<u8>, i32)>,
    pub invite: Invite,
}

impl Room {
    pub fn new(info: LobbyInfo) -> Self {
        Self {
            info,
            session: None,
            chat: crate::chat::Chat::default(),
            series_over: None,
            invite: Invite::default(),
        }
    }
}

/// The lobby's list of friends to invite.
#[derive(Default)]
pub struct Invite {
    pub open: bool,
    pub slot: Option<HttpSlot>,
    pub friends: Vec<FriendEntry>,
    pub pager: crate::ui::Pager,
}

pub struct GameSession {
    pub board: Board,
    pub predicted_board: Board,
    pub other_board: Board,
    pub my_slot: u8,

    pub opponent_disconnected: bool,
    pub quit_menu: bool,
    /// The names above the boards, own first; a spectator's are both players'.
    pub labels: Option<[String; 2]>,

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
            quit_menu: false,
            labels: None,
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
    pub const fn spectating(&self) -> bool {
        self.my_slot == 0
    }

    pub fn decided(&self) -> bool {
        self.board.state == GameState::GameOver || self.other_board.state == GameState::GameOver
    }

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
    storage::get(PLAYER_ID_KEY).unwrap_or_else(|| {
        let id = gen_player_id();
        storage::set(PLAYER_ID_KEY, &id);
        id
    })
}

#[derive(AppState)]
pub struct State {
    pub screen: Screen,
    pub settings: Settings,
    pub player_id: String,
    pub conn: Connection,
    pub rooms: Vec<RoomInfo>,
    /// The room this player is in: dropped as a whole on leaving it.
    pub room: Option<Room>,
    pub room_pager: crate::ui::Pager,
    pub text_input: TextInput,
    pub notice: Status,
    pub fonts: crate::ui::Fonts,
    pub images: crate::images::Images,
    pub auth: Option<AuthInfo>,
    pub auth_form: AuthForm,
    pub profile: Option<ProfileData>,
    pub friends: Option<FriendsData>,
    pub other_profile: Option<OtherProfileData>,
    pub startup_check: Option<HttpSlot>,
    pub startup_retry_at: Option<f64>,
    pub title_seen: bool,
    pub pending_invitation: Option<(String, RoomId, String)>,
    pub pending_join: Option<RoomId>,
    pub pending_watch: Option<String>,
    pub ranked: crate::ranked::RankedView,
    pub help: Option<crate::help::HelpView>,
    pub leaderboard: Option<crate::leaderboard::Leaderboard>,
    pub outdated: bool,
    /// The server refused this version: online play waits for an update.
    pub too_old: bool,
    pub maintenance: bool,
    pub ui: crate::ui::Ui,
    pub keys: crate::ui::EditKeys,
    pub controls: crate::controls::Controls,
    pub bindings_panel: crate::bindings_panel::BindingsPanel,
    pub pads: crate::pads::Pads,
    pub touch: crate::touch::TouchPad,
    pub solo_settings: crate::solo::SoloSettings,
    pub solo: Option<crate::solo::SoloGame>,
    /// A guest's best lives on the device, an account's on the server.
    pub solo_best: i32,
    pub solo_best_slot: Option<HttpSlot>,
}

impl State {
    pub fn new(fonts: crate::ui::Fonts) -> Self {
        let player_id = load_or_create_player_id();
        Self {
            screen: Screen::Auth,
            settings: Settings::load(),
            player_id: player_id.clone(),
            conn: Connection::new(&player_id),
            rooms: Vec::new(),
            room: None,
            room_pager: crate::ui::Pager::default(),
            text_input: TextInput::default(),
            notice: Status::Empty,
            fonts,
            images: crate::images::Images::default(),
            auth: None,
            auth_form: AuthForm::default(),
            profile: None,
            friends: None,
            other_profile: None,
            startup_check: None,
            startup_retry_at: None,
            title_seen: false,
            pending_invitation: None,
            pending_join: None,
            pending_watch: None,
            ranked: crate::ranked::RankedView::default(),
            help: None,
            leaderboard: None,
            outdated: false,
            too_old: false,
            maintenance: false,
            ui: crate::ui::Ui::default(),
            keys: crate::ui::EditKeys::default(),
            controls: crate::controls::Controls::load(),
            bindings_panel: crate::bindings_panel::BindingsPanel::default(),
            pads: crate::pads::Pads::new(),
            touch: crate::touch::TouchPad::default(),
            solo_settings: crate::solo::SoloSettings::default(),
            solo: None,
            solo_best: load_best_score(),
            solo_best_slot: None,
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn saved_settings_come_back_and_bad_ones_are_kept_in_range() {
        let mut settings = Settings::default();
        settings.adjust(0, 3);
        settings.adjust(1, -1);
        settings.save();
        let loaded = Settings::load();
        assert_eq!(
            (loaded.das_delay, loaded.das_speed),
            (settings.das_delay, settings.das_speed)
        );

        storage::set(Settings::KEYS[0], "99");
        storage::set(Settings::KEYS[1], "NaN");
        let loaded = Settings::load();
        assert_eq!(loaded.das_delay, Settings::range(0).1, "clamped to the slowest allowed");
        assert_eq!(loaded.das_speed, Settings::default().das_speed, "garbage is ignored");
    }
}
