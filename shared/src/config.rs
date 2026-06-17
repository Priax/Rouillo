pub const CELL_SIZE: f32 = 40.0;
pub const GRID_WIDTH: usize = 6;
pub const GRID_HEIGHT: usize = 13;
pub const VISIBLE_ROW_OFFSET: usize = 1;
pub const SPAWN_COL: usize = 2;

pub const MAX_LOCK_TIME: f32 = 0.5;
pub const MAX_LOCK_DELAY_MOVES: u32 = 15;
pub const MAX_TOTAL_GROUND_TIME: f32 = 2.0;

pub const DAS_DELAY: f32 = 0.11;
pub const DAS_SPEED: f32 = 0.05;
pub const SOFT_DROP_SPEED: f32 = 0.15;

pub const LEVEL_DURATION: f32 = 15.0;
pub const BASE_FALL_INTERVAL: f64 = 0.8;
pub const MIN_FALL_INTERVAL: f64 = 0.1;
pub const FALL_SPEEDUP_PER_LEVEL: f64 = 0.05;

pub const RESOLVE_STEP_INTERVAL: f32 = 0.10;
pub const GARBAGE_DROP_DELAY: f32 = 0.5;

pub const ALL_CLEAR_BONUS: u32 = 30;

pub const CHAIN_POWERS: [u32; 20] = [
    0, 0, 8, 16, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512,
];
pub const COLOR_BONUS: [u32; 6] = [0, 0, 3, 6, 12, 24];
pub const GROUP_BONUS: [u32; 8] = [0, 2, 3, 4, 5, 6, 7, 10];

pub const SERVER_PORT: u16 = 8080;
pub const CHANNEL_CAPACITY: usize = 256;

pub const SERVER_TICK_HZ: u64 = 60;
pub const STATE_BROADCAST_HZ: u64 = 60;

pub const SERVER_BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

pub const SERVER_URL: &str = "ws://127.0.0.1:8080/ws";

// à remplacer par mon vrai domaine
pub const SERVER_URL_RELEASE: &str = "wss://puyo.duckdns.org/ws";
