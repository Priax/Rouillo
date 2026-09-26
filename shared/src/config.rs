pub const CELL_SIZE: f32 = 40.0;
pub const GRID_WIDTH: usize = 6;
pub const GRID_HEIGHT: usize = 13;
pub const VISIBLE_ROW_OFFSET: usize = 1;
pub const SPAWN_COL: usize = 2;

// Timings follow Puyo Puyo Tsu, as measured by Puyo Nexus
// (puyonexus.com/wiki/Puyo_Puyo_Tsu/Frame_Data_Tables). The simulation runs
// at 60 Hz, so one tick is one of Tsu's frames.

/// Sub-cell units in one cell: the pair's fall progress counts up to this.
pub const CELL_UNITS: u32 = 0x10000;
/// Past this, the pair overlaps the row below and must fit there too to move
/// sideways or turn.
pub const HALF_CELL_UNITS: u32 = CELL_UNITS / 2;

/// Frames per cell of the pair's natural fall at level 1: Tsu's 2P versus
/// speed. Each level takes a frame off, down to the next constant.
pub const FALL_FRAMES_PER_CELL: u32 = 16;
/// Tsu's fastest versus speed ("Hardest").
pub const MIN_FALL_FRAMES_PER_CELL: u32 = 8;
/// Units per frame while down is held: half a cell, so 2 frames per cell,
/// whatever the natural speed.
pub const SOFT_DROP_UNITS: u32 = 0x8000;
pub const LEVEL_DURATION: f32 = 15.0;

/// Frames a pair may spend resting on something before it locks, in total
/// over its life. Soft dropping onto something skips it.
pub const GRACE_FRAMES: u32 = 32;
/// Floor kicks a pair gets; the one after locks it instead.
pub const MAX_PUSH_BACKS: u32 = 8;

/// Free fall of a split pair or of what is left after a pop, in 1/65536 px
/// (a cell is 16 px): start speed, gravity, top speed. These reproduce Tsu's
/// table exactly: 10 frames for one cell, 15 for two, ... 43 for thirteen.
pub const PX_UNITS: u32 = 0x10000;
pub const CELL_PX: u32 = 16;
pub const FREE_FALL_START: u32 = 0x10000;
pub const FREE_FALL_ACCEL: u32 = 0x3000;
pub const FREE_FALL_MAX: u32 = 0x80000;
/// Nuisance falls from standstill with a gravity of its own per column,
/// again matching Tsu's table for every column and height.
pub const OJAMA_ACCEL: [u32; 6] = [0x2400, 0x2600, 0x2000, 0x2A00, 0x2200, 0x2800];
/// Frames before each puyo of a locked pair starts to fall: axis, then
/// satellite.
pub const SPLIT_DELAY_AXIS: u8 = 1;
pub const SPLIT_DELAY_SATELLITE: u8 = 2;
/// A puyo squashes and springs back for this long on landing.
pub const BOUNCE_FRAMES: u32 = 16;
/// How long a group flashes before vanishing. Not in Puyo Nexus's tables:
/// an estimate, to be tuned by eye.
pub const POP_FRAMES: u32 = 40;

/// Horizontal autorepeat, as in Tsu: 8 frames before the first repeat, then
/// one move every 2 frames.
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

pub const SERVER_BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

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
