pub const CELL_SIZE: f32 = 40.0;
pub const GRID_WIDTH: usize = 6;
pub const GRID_HEIGHT: usize = 13;
pub const VISIBLE_ROW_OFFSET: usize = 1;
pub const SPAWN_COL: usize = 2;

// Timings follow Puyo Puyo Tsu, as measured by Puyo Nexus
// (puyonexus.com/wiki/Puyo_Puyo_Tsu/Frame_Data_Tables). The simulation runs
// at 60 Hz, so one tick is one of Tsu's frames.

pub const CELL_UNITS: u32 = 0x10000;
pub const HALF_CELL_UNITS: u32 = CELL_UNITS / 2;

pub const FALL_FRAMES_PER_CELL: u32 = 16;
pub const MIN_FALL_FRAMES_PER_CELL: u32 = 8;
pub const LEVELS_PER_SPEEDUP: u32 = 2;

pub const TARGET_POINTS: u32 = 70;
pub const MARGIN_LEVELS: u32 = 6;
pub const MARGIN_STEPS: u32 = 14;

pub const fn starting_garbage_rows(level: u32) -> u32 {
    let rows = level.saturating_sub(1) / 3;
    if rows > 4 {
        4
    } else {
        rows
    }
}
pub const SOFT_DROP_UNITS: u32 = 0x8000;
pub const LEVEL_FRAMES: u32 = 16 * 60;

pub const GRACE_FRAMES: u32 = 32;
pub const MAX_PUSH_BACKS: u32 = 8;

pub const PX_UNITS: u32 = 0x10000;
pub const CELL_PX: u32 = 16;
pub const FREE_FALL_START: u32 = 0x10000;
pub const FREE_FALL_ACCEL: u32 = 0x3000;
pub const FREE_FALL_MAX: u32 = 0x80000;
pub const OJAMA_ACCEL: [u32; 6] = [0x2400, 0x2600, 0x2000, 0x2A00, 0x2200, 0x2800];
pub const SPLIT_DELAY_AXIS: u8 = 1;
pub const SPLIT_DELAY_SATELLITE: u8 = 2;
pub const BOUNCE_FRAMES: u32 = 16;
pub const POP_FRAMES: u32 = 34;

pub const DAS_DELAY: f32 = 8.0 / 60.0;
pub const DAS_SPEED: f32 = 2.0 / 60.0;

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

pub const RECONNECT_GRACE_SECS: u64 = 120;

pub const RECONNECT_MARGIN_SECS: u64 = 5;

pub const SERVER_BIND_ADDRESS: [u8; 4] = [127, 0, 0, 1];

pub const SERVER_URL: &str = "ws://127.0.0.1:8080/ws";

pub const SERVER_URL_RELEASE: &str = "wss://puyo.priax.org/ws";
pub const API_URL_RELEASE: &str = "https://puyo.priax.org";

pub const CLIENT_SIM_DT: f32 = 1.0 / SERVER_TICK_HZ as f32;

pub const MAX_SIM_STEPS_PER_FRAME: u32 = 5;

pub const PING_INTERVAL_SECS: f64 = 5.0;

pub const PING_TIMEOUT_SECS: f64 = 15.0;

const _: () = assert!(PING_TIMEOUT_SECS > 2.0 * PING_INTERVAL_SECS);

pub const INPUT_LEAD_MARGIN_TICKS: u32 = 2;

pub const MAX_INPUT_LEAD_TICKS: u32 = 30;

const _: () = assert!(INPUT_LEAD_MARGIN_TICKS < MAX_INPUT_LEAD_TICKS);

pub const GARBAGE_TRAVEL_TICKS: u32 = 20;

const _: () = assert!(GARBAGE_TRAVEL_TICKS >= 1);
