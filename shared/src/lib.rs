use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

pub mod config;
use crate::config::{
    BOUNCE_FRAMES, CELL_PX, CELL_UNITS, CHAIN_POWERS, COLOR_BONUS, FALL_FRAMES_PER_CELL, FREE_FALL_ACCEL,
    FREE_FALL_MAX, FREE_FALL_START, GRACE_FRAMES, GROUP_BONUS, HALF_CELL_UNITS, LEVELS_PER_SPEEDUP, LEVEL_FRAMES,
    MARGIN_LEVELS, MARGIN_STEPS, MAX_PUSH_BACKS, MIN_FALL_FRAMES_PER_CELL, OJAMA_ACCEL, POP_FRAMES, PX_UNITS,
    SOFT_DROP_UNITS, SPAWN_COL, SPLIT_DELAY_AXIS, SPLIT_DELAY_SATELLITE, TARGET_POINTS, VISIBLE_ROW_OFFSET,
};

pub fn encode<T: serde::Serialize>(msg: &T) -> Result<Vec<u8>, bitcode::Error> {
    let raw = bitcode::serialize(msg)?;
    Ok(lz4_flex::block::compress_prepend_size(&raw))
}

pub const MAX_DECODED_SIZE: usize = 1 << 20;

pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    if size > MAX_DECODED_SIZE {
        return None;
    }
    let raw = lz4_flex::block::decompress_size_prepended(bytes).ok()?;
    bitcode::deserialize(&raw).ok()
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn finish(self) -> u64 {
        self.0
    }

    fn byte(&mut self, b: u8) {
        self.0 ^= b as u64;
        self.0 = self.0.wrapping_mul(0x100_0000_01b3);
    }

    fn bytes(&mut self, bs: &[u8]) {
        for b in bs {
            self.byte(*b);
        }
    }

    fn bool(&mut self, v: bool) {
        self.byte(v as u8);
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash, Serialize, Deserialize)]
pub enum PuyoType {
    Red,
    Blue,
    Yellow,
    Green,
    Purple,
    Garbage,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    MoveLeft,
    MoveRight,
    RotateCW,
    RotateCCW,
    SoftDropPress,
    SoftDropRelease,
    HardDrop,
}

pub type RoomId = u32;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingGarbage {
    pub at: u32,
    pub amount: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StampedInput {
    pub tick: u32,
    pub kind: InputKind,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PausePolicy {
    Everyone,
    HostOnly,
    Nobody,
}

impl PausePolicy {
    const ALL: [Self; 3] = [Self::Everyone, Self::HostOnly, Self::Nobody];

    pub fn label(self) -> &'static str {
        match self {
            Self::Everyone => "Tous",
            Self::HostOnly => "Hôte",
            Self::Nobody => "Personne",
        }
    }

    pub fn allows(self, is_host: bool) -> bool {
        match self {
            Self::Everyone => true,
            Self::HostOnly => is_host,
            Self::Nobody => false,
        }
    }

    fn step(self, dir: i32) -> Self {
        let n = Self::ALL.len() as i32;
        let i = Self::ALL.iter().position(|&p| p == self).unwrap_or(0) as i32;
        Self::ALL[(i + dir.signum()).rem_euclid(n) as usize]
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct RoomSettings {
    pub starting_level: u32,
    pub colors: u32,
    pub friends_only: bool,
    pub pause: PausePolicy,
}

impl Default for RoomSettings {
    fn default() -> Self {
        Self {
            starting_level: 1,
            colors: 5,
            friends_only: false,
            pause: PausePolicy::Everyone,
        }
    }
}

impl RoomSettings {
    pub const COUNT: usize = 4;

    pub fn label(i: usize) -> &'static str {
        match i {
            0 => "Starting level",
            1 => "Colors",
            2 => "Amis seulement",
            _ => "Pause",
        }
    }

    pub fn value(&self, i: usize) -> String {
        match i {
            0 => match config::starting_garbage_rows(self.starting_level) {
                0 => self.starting_level.to_string(),
                1 => format!("{} (+1 ligne)", self.starting_level),
                rows => format!("{} (+{rows} lignes)", self.starting_level),
            },
            1 => self.colors.to_string(),
            2 => {
                if self.friends_only {
                    "Oui".into()
                } else {
                    "Non".into()
                }
            }
            _ => self.pause.label().into(),
        }
    }

    pub fn adjust(&mut self, i: usize, dir: i32) {
        match i {
            0 => self.starting_level = (self.starting_level as i32 + dir).clamp(1, 15) as u32,
            1 => self.colors = (self.colors as i32 + dir).clamp(4, 5) as u32,
            2 => self.friends_only = !self.friends_only,
            3 => self.pause = self.pause.step(dir),
            _ => {}
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RoomInfo {
    pub id: RoomId,
    pub name: String,
    pub players: u8,
    pub max: u8,
    pub in_game: bool,
    pub friends_only: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LobbyInfo {
    pub id: RoomId,
    pub name: String,
    pub settings: RoomSettings,
    pub players: u8,
    pub connected: u8,
    pub your_slot: u8,
    pub is_host: bool,
    pub countdown: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientMessage {
    Hello {
        player_id: String,
        auth_token: Option<String>,
        username: Option<String>,
        last_disconnect_reason: Option<String>,
    },
    Input {
        kind: InputKind,
        seq: u32,
        tick: u32,
    },
    TogglePause,
    RequestRestart,
    RequestRoomList,
    CreateRoom {
        name: String,
    },
    JoinRoom {
        id: RoomId,
    },
    LeaveRoom,
    SetRoomSetting {
        index: u8,
        dir: i32,
    },
    ToggleCountdown,
    ReturnToLobby,
    InviteFriend {
        user_id: String,
    },
    Ping {
        id: u32,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerMessage {
    GameStart,
    StateUpdate {
        p1_board: Box<Board>,
        p2_board: Box<Board>,
        p1_rng: Option<Box<BoardRng>>,
        p2_rng: Option<Box<BoardRng>>,
        p1_ack: u32,
        p2_ack: u32,
        tick: u32,
        p1_incoming: Vec<IncomingGarbage>,
        p2_incoming: Vec<IncomingGarbage>,
    },
    Restart,
    OpponentDisconnected,
    RoomList {
        rooms: Vec<RoomInfo>,
    },
    Lobby {
        info: LobbyInfo,
    },
    JoinFailed {
        reason: String,
    },
    FriendInvitation {
        from_username: String,
        room_id: RoomId,
        room_name: String,
    },
    Pong {
        id: u32,
    },
}

impl PuyoType {
    pub fn random_with_seed<R: Rng>(rng: &mut R, colors: u32) -> Self {
        let n = colors.clamp(1, 5);
        match rng.random_range(0..n) {
            0 => Self::Red,
            1 => Self::Blue,
            2 => Self::Yellow,
            3 => Self::Green,
            _ => Self::Purple,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ActivePuyo {
    pub row: i32,
    pub col: i32,
    pub rotation: usize,
    pub axis_type: PuyoType,
    pub sat_type: PuyoType,
}

impl ActivePuyo {
    pub fn get_positions(&self) -> [(i32, i32); 2] {
        let (dr, dc) = match self.rotation {
            0 => (-1, 0),
            1 => (0, 1),
            2 => (1, 0),
            3 => (0, -1),
            _ => (-1, 0),
        };
        [(self.row, self.col), (self.row + dr, self.col + dc)]
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum GameState {
    Playing,
    ResolvingMatches,
    DroppingGarbage,
    GameOver,
    Paused,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Settle {
    Idle,
    Falling { frame: u32, frames: u32 },
    Popping { frame: u32 },
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CellFall {
    pub row: u8,
    pub col: u8,
    pub cells_fallen: u8,
    pub delay: u8,
    pub ojama: bool,
}

pub fn fallen_px(frames: u32, start: u32, accel: u32) -> u64 {
    let (mut y, mut v) = (0u64, start);
    for _ in 0..frames {
        y += v as u64;
        v = (v + accel).min(FREE_FALL_MAX);
    }
    y
}

pub fn frames_to_fall(cells: u32, start: u32, accel: u32) -> u32 {
    let target = (cells * CELL_PX) as u64 * PX_UNITS as u64;
    let (mut y, mut v, mut frames) = (0u64, start, 0);
    while y < target {
        y += v as u64;
        v = (v + accel).min(FREE_FALL_MAX);
        frames += 1;
    }
    frames
}

impl CellFall {
    fn start_and_accel(self) -> (u32, u32) {
        if self.ojama {
            (0, OJAMA_ACCEL[self.col as usize % OJAMA_ACCEL.len()])
        } else {
            (FREE_FALL_START, FREE_FALL_ACCEL)
        }
    }

    pub fn lands_at(&self) -> u32 {
        let (start, accel) = self.start_and_accel();
        self.delay as u32 + frames_to_fall(self.cells_fallen as u32, start, accel)
    }

    pub fn height_at(&self, frame: u32) -> f32 {
        let (start, accel) = self.start_and_accel();
        let moving = frame.saturating_sub(self.delay as u32);
        let total = (self.cells_fallen as u32 * CELL_PX) as u64 * PX_UNITS as u64;
        let left = total.saturating_sub(fallen_px(moving, start, accel));
        left as f32 / (CELL_PX * PX_UNITS) as f32
    }

    pub fn bounce_at(&self, frame: u32) -> Option<f32> {
        let since = frame.checked_sub(self.lands_at())?;
        (since < BOUNCE_FRAMES).then(|| since as f32 / BOUNCE_FRAMES as f32)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Board {
    pub width: usize,
    pub height: usize,
    pub start_level: u32,
    pub colors: u32,
    pub cells: Vec<Vec<Option<PuyoType>>>,
    pub active_piece: Option<ActivePuyo>,
    pub piece_id: u32,
    pub next_types: (PuyoType, PuyoType),
    pub next_next_types: (PuyoType, PuyoType),
    pub score: i32,
    pub state: GameState,
    #[serde(skip)]
    pub previous_state: Option<GameState>,
    pub pending_garbage: u32,
    pub nuisance_points: u32,
    pub chain_count: u32,
    pub last_was_all_clear: bool,
    pub all_clear_bonus: bool,
    pub match_frames: u32,
    pub fall_offset: u32,
    pub soft_dropping: bool,
    pub ground_frames: u32,
    pub push_backs: u32,
    pub settle: Settle,
    pub falls: Vec<CellFall>,
    pub popping: Vec<(u8, u8)>,
    #[serde(skip)]
    rng: BoardRng,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoardRng {
    pieces: rand_chacha::ChaCha12Rng,
    garbage: rand_chacha::ChaCha12Rng,
}

impl BoardRng {
    fn from_seed(seed: u64) -> Self {
        use rand::SeedableRng;
        let pieces = rand_chacha::ChaCha12Rng::seed_from_u64(seed);
        let mut garbage = pieces.clone();
        garbage.set_stream(1);
        Self { pieces, garbage }
    }

    fn position(&self) -> RngPosition {
        RngPosition {
            pieces: self.pieces.get_word_pos(),
            garbage: self.garbage.get_word_pos(),
        }
    }
}

impl Default for BoardRng {
    fn default() -> Self {
        Self::from_seed(0)
    }
}

/// How far each of a board's random streams has advanced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RngPosition {
    pub pieces: u128,
    pub garbage: u128,
}

impl Board {
    pub fn new(width: usize, height: usize, seed: u64, start_level: u32, colors: u32) -> Self {
        let mut rng = BoardRng::from_seed(seed);
        let n1 = (
            PuyoType::random_with_seed(&mut rng.pieces, colors),
            PuyoType::random_with_seed(&mut rng.pieces, colors),
        );
        let n2 = (
            PuyoType::random_with_seed(&mut rng.pieces, colors),
            PuyoType::random_with_seed(&mut rng.pieces, colors),
        );

        let mut board = Self {
            width,
            height,
            start_level,
            colors,
            cells: vec![vec![None; width]; height],
            active_piece: None,
            piece_id: 0,
            next_types: n1,
            next_next_types: n2,
            score: 0,
            state: GameState::Playing,
            previous_state: None,
            pending_garbage: 0,
            nuisance_points: 0,
            chain_count: 0,
            last_was_all_clear: false,
            all_clear_bonus: false,
            match_frames: 0,
            fall_offset: 0,
            soft_dropping: false,
            ground_frames: 0,
            push_backs: 0,
            settle: Settle::Idle,
            falls: Vec::new(),
            popping: Vec::new(),
            rng,
        };
        let rows = config::starting_garbage_rows(start_level) as usize;
        for row in board.cells.iter_mut().rev().take(rows) {
            row.fill(Some(PuyoType::Garbage));
        }
        board
    }

    pub fn spawn_piece(&mut self) {
        if self.cells[VISIBLE_ROW_OFFSET][SPAWN_COL].is_some() {
            self.state = GameState::GameOver;
            return;
        }
        let (c1, c2) = self.next_types;
        self.next_types = self.next_next_types;
        self.next_next_types = (
            PuyoType::random_with_seed(&mut self.rng.pieces, self.colors),
            PuyoType::random_with_seed(&mut self.rng.pieces, self.colors),
        );
        let new_piece = ActivePuyo {
            row: 1,
            col: SPAWN_COL as i32,
            rotation: 0,
            axis_type: c1,
            sat_type: c2,
        };
        if self.check_collision(&new_piece) {
            self.state = GameState::GameOver;
        } else {
            self.piece_id = self.piece_id.wrapping_add(1);
            self.active_piece = Some(new_piece);
            self.fall_offset = 0;
            self.ground_frames = 0;
            self.push_backs = 0;
            self.chain_count = 0;
        }
    }

    pub fn get_ghost_piece(&self) -> Option<ActivePuyo> {
        let mut ghost = self.active_piece.clone()?;
        while !self.check_collision(&ghost) {
            ghost.row += 1;
        }
        ghost.row -= 1;
        Some(ghost)
    }

    pub fn check_collision(&self, piece: &ActivePuyo) -> bool {
        for (r, c) in &piece.get_positions() {
            if *c < 0 || *c >= self.width as i32 || *r >= self.height as i32 {
                return true;
            }
            if *r >= 0 && self.cells[*r as usize][*c as usize].is_some() {
                return true;
            }
        }
        false
    }

    fn fits(&self, piece: &ActivePuyo) -> bool {
        if self.check_collision(piece) {
            return false;
        }
        if self.fall_offset < HALF_CELL_UNITS {
            return true;
        }
        let mut lower = piece.clone();
        lower.row += 1;
        !self.check_collision(&lower)
    }

    pub fn move_piece(&mut self, dx: i32) {
        let Some(mut piece) = self.active_piece.clone() else {
            return;
        };
        piece.col += dx;
        if self.fits(&piece) {
            self.active_piece = Some(piece);
        }
    }

    pub fn rotate_piece(&mut self, direction: usize) {
        let Some(piece) = self.active_piece.clone() else {
            return;
        };
        let (old_rot, old_col, old_row) = (piece.rotation, piece.col, piece.row);
        let new_rot = (old_rot + direction) % 4;
        let sat = piece.get_positions()[1];

        let candidates = [
            (old_row, old_col, new_rot, true, false),             // in place
            (old_row, old_col - 1, new_rot, new_rot == 1, false), // off a wall on the right
            (old_row, old_col + 1, new_rot, new_rot == 3, false), // off a wall on the left
            (old_row - 1, old_col, new_rot, new_rot == 2, true),  // off the floor
            (sat.0, sat.1, (old_rot + 2) % 4, true, false),       // pivot on satellite
        ];

        for (row, col, rotation, applies, push_back) in candidates {
            if !applies {
                continue;
            }
            let turned = ActivePuyo {
                row,
                col,
                rotation,
                ..piece.clone()
            };
            if push_back {
                if !self.fits(&turned) {
                    continue;
                }
                if self.push_backs >= MAX_PUSH_BACKS {
                    self.lock_piece();
                    return;
                }
                self.push_backs += 1;
                self.fall_offset = 0;
                self.active_piece = Some(turned);
                return;
            }
            if self.fits(&turned) {
                self.active_piece = Some(turned);
                return;
            }
        }
    }

    pub fn hard_drop(&mut self) {
        if let Some(mut piece) = self.active_piece.take() {
            loop {
                piece.row += 1;
                if self.check_collision(&piece) {
                    piece.row -= 1;
                    break;
                }
            }
            self.active_piece = Some(piece);
            self.lock_piece();
        }
    }

    pub fn can_fall(&self) -> bool {
        let Some(piece) = &self.active_piece else {
            return false;
        };
        let mut below = piece.clone();
        below.row += 1;
        !self.check_collision(&below)
    }

    fn fall_frames_per_cell(&self) -> u32 {
        FALL_FRAMES_PER_CELL
            .saturating_sub(self.level().saturating_sub(1) / LEVELS_PER_SPEEDUP)
            .max(MIN_FALL_FRAMES_PER_CELL)
    }

    pub fn fall_progress(&self) -> f32 {
        if !self.can_fall() {
            return 0.0;
        }
        self.fall_offset as f32 / CELL_UNITS as f32
    }

    fn tick_pair(&mut self) {
        if self.active_piece.is_none() {
            return;
        }
        let speed = if self.soft_dropping {
            SOFT_DROP_UNITS
        } else {
            CELL_UNITS / self.fall_frames_per_cell()
        };
        if self.can_fall() {
            self.fall_offset += speed;
            while self.fall_offset >= CELL_UNITS && self.can_fall() {
                self.fall_offset -= CELL_UNITS;
                if let Some(piece) = self.active_piece.as_mut() {
                    piece.row += 1;
                }
            }
        }
        if self.can_fall() {
            return;
        }
        self.fall_offset = 0;
        self.ground_frames += 1;
        if self.soft_dropping || self.ground_frames > GRACE_FRAMES {
            self.lock_piece();
        }
    }

    fn lock_piece(&mut self) {
        self.last_was_all_clear = false;
        let Some(piece) = self.active_piece.take() else {
            return;
        };
        let mut placed = Vec::new();
        for (i, (r, c)) in piece.get_positions().iter().enumerate() {
            if *r >= 0 && *r < self.height as i32 && *c >= 0 && *c < self.width as i32 {
                let puyo_type = if i == 0 { piece.axis_type } else { piece.sat_type };
                self.cells[*r as usize][*c as usize] = Some(puyo_type);
                let delay = if i == 0 {
                    SPLIT_DELAY_AXIS
                } else {
                    SPLIT_DELAY_SATELLITE
                };
                placed.push((*r as usize, *c as usize, delay));
            }
        }
        self.fall_offset = 0;
        self.state = GameState::ResolvingMatches;

        let moved = self.collapse();
        let mut falls = Vec::new();
        for (r, c, delay) in placed {
            let fell = moved.iter().find(|&&(from, col, _)| from == r && col == c);
            let (row, cells_fallen) = match fell {
                Some(&(from, _, to)) => (to, to - from),
                None => (r, 0),
            };
            falls.push(CellFall {
                row: row as u8,
                col: c as u8,
                cells_fallen: cells_fallen as u8,
                delay,
                ojama: false,
            });
        }
        self.begin_fall(falls);
    }

    fn collapse(&mut self) -> Vec<(usize, usize, usize)> {
        let mut moved = Vec::new();
        for col in 0..self.width {
            let mut floor = self.height;
            for row in (0..self.height).rev() {
                if let Some(t) = self.cells[row][col] {
                    floor -= 1;
                    if floor != row {
                        self.cells[floor][col] = Some(t);
                        self.cells[row][col] = None;
                        moved.push((row, col, floor));
                    }
                }
            }
        }
        moved
    }

    pub fn apply_board_gravity(&mut self) -> bool {
        !self.collapse().is_empty()
    }

    fn begin_fall(&mut self, falls: Vec<CellFall>) {
        let frames = falls.iter().map(|f| f.lands_at() + BOUNCE_FRAMES).max().unwrap_or(0);
        self.falls = falls;
        self.settle = Settle::Falling { frame: 0, frames };
    }

    fn start_pop(&mut self) -> Option<u32> {
        let mut to_remove = vec![vec![false; self.width]; self.height];
        let mut visited = vec![vec![false; self.width]; self.height];
        let mut group_sizes = Vec::new();
        let mut colors_seen = [false; 6];
        let mut total_puyos_cleared = 0;

        for r in 0..self.height {
            for c in 0..self.width {
                let Some(p_type) = self.cells[r][c] else {
                    continue;
                };
                if p_type == PuyoType::Garbage || visited[r][c] {
                    continue;
                }
                let group = self.flood_fill(r, c, p_type, &mut visited);
                if group.len() >= 4 && group.iter().any(|(r, _)| *r >= VISIBLE_ROW_OFFSET) {
                    colors_seen[p_type as usize] = true;
                    group_sizes.push(group.len() as u32);
                    total_puyos_cleared += group.len() as u32;
                    for (gr, gc) in group {
                        to_remove[gr][gc] = true;
                        self.mark_adjacent_garbage(gr, gc, &mut to_remove);
                    }
                }
            }
        }
        let popping: Vec<(u8, u8)> = (0..self.height)
            .flat_map(|r| (0..self.width).map(move |c| (r, c)))
            .filter(|&(r, c)| to_remove[r][c])
            .map(|(r, c)| (r as u8, c as u8))
            .collect();
        if popping.is_empty() {
            return None;
        }
        self.chain_count += 1;
        let colors = colors_seen.iter().filter(|&&seen| seen).count();
        let score_gained = self.calculate_score(colors, total_puyos_cleared, &group_sizes);
        self.score += score_gained;
        self.popping = popping;
        Some(score_gained as u32)
    }

    fn clear_popping(&mut self) {
        for (r, c) in std::mem::take(&mut self.popping) {
            self.cells[r as usize][c as usize] = None;
        }
    }

    pub fn check_matches(&mut self) -> Option<u32> {
        let score = self.start_pop()?;
        self.clear_popping();
        Some(score)
    }

    fn mark_adjacent_garbage(&self, r: usize, c: usize, to_remove: &mut [Vec<bool>]) {
        let neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        for (dr, dc) in &neighbors {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;

            if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                let nr = nr as usize;
                let nc = nc as usize;
                if self.cells[nr][nc] == Some(PuyoType::Garbage) {
                    to_remove[nr][nc] = true;
                }
            }
        }
    }

    fn calculate_score(&self, color_count_len: usize, total_cleared: u32, group_sizes: &[u32]) -> i32 {
        let chain_idx = (self.chain_count).min(19) as usize;
        let cp = CHAIN_POWERS[chain_idx];
        let cb = COLOR_BONUS[color_count_len.min(5)];
        let mut gb = 0;
        for &size in group_sizes {
            gb += GROUP_BONUS[(size.saturating_sub(4)).min(7) as usize];
        }
        let multiplier = (cp + cb + gb).clamp(1, 999);

        (10 * total_cleared) as i32 * multiplier as i32
    }

    pub fn drop_garbage(&mut self) {
        if self.pending_garbage == 0 {
            return;
        }
        let garbage_to_drop = self.pending_garbage.min(30);
        self.pending_garbage -= garbage_to_drop;

        let full_lines = garbage_to_drop / self.width as u32;
        let leftover = garbage_to_drop % self.width as u32;
        let mut landed: Vec<(usize, usize)> = Vec::new();

        for _ in 0..full_lines {
            for c in 0..self.width {
                if let Some(r) = self.drop_one_garbage(c) {
                    landed.push((r, c));
                }
            }
        }

        if leftover > 0 {
            let mut cols: Vec<usize> = (0..self.width).collect();
            for i in 0..leftover as usize {
                let j = self.rng.garbage.random_range(i..self.width);
                cols.swap(i, j);
            }
            for &col in cols.iter().take(leftover as usize) {
                if let Some(r) = self.drop_one_garbage(col) {
                    landed.push((r, col));
                }
            }
        }

        let mut falls = Vec::new();
        for col in 0..self.width {
            let Some(bottom) = landed.iter().filter(|&&(_, c)| c == col).map(|&(r, _)| r).max() else {
                continue;
            };
            for &(r, _) in landed.iter().filter(|&&(_, c)| c == col) {
                falls.push(CellFall {
                    row: r as u8,
                    col: col as u8,
                    cells_fallen: bottom as u8,
                    delay: 0,
                    ojama: true,
                });
            }
        }
        self.begin_fall(falls);
    }

    fn drop_one_garbage(&mut self, col: usize) -> Option<usize> {
        let r = (0..self.height).rev().find(|&r| self.cells[r][col].is_none())?;
        self.cells[r][col] = Some(PuyoType::Garbage);
        Some(r)
    }

    fn flood_fill(&self, r: usize, c: usize, target_type: PuyoType, visited: &mut [Vec<bool>]) -> Vec<(usize, usize)> {
        let mut group = Vec::new();
        let mut stack = vec![(r, c)];
        while let Some((r, c)) = stack.pop() {
            if visited[r][c] {
                continue;
            }
            visited[r][c] = true;
            group.push((r, c));
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                    if let Some(cell_type) = self.cells[nr as usize][nc as usize] {
                        if cell_type == target_type {
                            stack.push((nr as usize, nc as usize));
                        }
                    }
                }
            }
        }
        group
    }

    fn after_landing(&mut self) -> u32 {
        self.falls.clear();
        self.settle = Settle::Idle;
        if let Some(score) = self.start_pop() {
            self.settle = Settle::Popping { frame: 0 };
            let target = self.target_points();
            let total_nuisance = score + self.nuisance_points;
            self.nuisance_points = total_nuisance % target;
            let mut attack = total_nuisance / target;
            if self.all_clear_bonus {
                attack += config::ALL_CLEAR_BONUS;
                self.all_clear_bonus = false;
            }
            let offset = attack.min(self.pending_garbage);
            self.pending_garbage -= offset;
            return attack - offset;
        }
        if self.pending_garbage > 0 {
            self.state = GameState::DroppingGarbage;
            self.drop_garbage();
            return 0;
        }
        self.next_pair();
        0
    }

    fn next_pair(&mut self) {
        if self.cells[VISIBLE_ROW_OFFSET][SPAWN_COL].is_some() {
            self.state = GameState::GameOver;
            return;
        }
        let ac = self.check_all_clear();
        self.last_was_all_clear = ac;
        if ac {
            self.all_clear_bonus = true;
        }
        self.state = GameState::Playing;
        self.spawn_piece();
    }

    fn tick_settle(&mut self) -> u32 {
        match self.settle {
            Settle::Falling { frame, frames } => {
                let frame = frame + 1;
                if frame < frames {
                    self.settle = Settle::Falling { frame, frames };
                    return 0;
                }
                if self.state == GameState::DroppingGarbage {
                    self.falls.clear();
                    self.settle = Settle::Idle;
                    self.next_pair();
                    return 0;
                }
                self.after_landing()
            }
            Settle::Popping { frame } => {
                let frame = frame + 1;
                if frame < POP_FRAMES {
                    self.settle = Settle::Popping { frame };
                    return 0;
                }
                self.clear_popping();
                let falls = self
                    .collapse()
                    .into_iter()
                    .map(|(from, col, to)| CellFall {
                        row: to as u8,
                        col: col as u8,
                        cells_fallen: (to - from) as u8,
                        delay: 0,
                        ojama: false,
                    })
                    .collect();
                self.begin_fall(falls);
                0
            }
            Settle::Idle => {
                if self.state == GameState::DroppingGarbage {
                    self.next_pair();
                    return 0;
                }
                self.after_landing()
            }
        }
    }

    fn check_all_clear(&self) -> bool {
        self.cells
            .iter()
            .all(|row| row.iter().all(std::option::Option::is_none))
    }

    pub fn toggle_pause(&mut self) {
        match self.state {
            GameState::Paused => {
                self.state = self.previous_state.take().unwrap_or(GameState::Playing);
            }
            GameState::GameOver => {}
            _ => {
                self.previous_state = Some(self.state);
                self.state = GameState::Paused;
            }
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused {
            if self.state != GameState::Paused && self.state != GameState::GameOver {
                self.previous_state = Some(self.state);
                self.state = GameState::Paused;
            }
        } else if self.state == GameState::Paused {
            self.state = self.previous_state.take().unwrap_or(GameState::Playing);
        }
    }

    pub fn target_points(&self) -> u32 {
        let steps = self.level().saturating_sub(MARGIN_LEVELS).min(MARGIN_STEPS);
        let target = TARGET_POINTS as u64 * 3u64.pow(steps) / 4u64.pow(steps);
        target.max(1) as u32
    }

    pub fn level(&self) -> u32 {
        self.start_level + self.match_frames / LEVEL_FRAMES
    }

    pub fn rng_position(&self) -> RngPosition {
        self.rng.position()
    }

    pub fn rng_state(&self) -> BoardRng {
        self.rng.clone()
    }

    pub fn set_rng(&mut self, rng: BoardRng) {
        self.rng = rng;
    }

    pub fn apply_input(&mut self, input: InputKind) {
        match input {
            InputKind::SoftDropPress => self.soft_dropping = true,
            InputKind::SoftDropRelease => self.soft_dropping = false,
            _ => {}
        }
        if self.state != GameState::Playing {
            return;
        }
        match input {
            InputKind::MoveLeft => self.move_piece(-1),
            InputKind::MoveRight => self.move_piece(1),
            InputKind::RotateCW => self.rotate_piece(1),
            InputKind::RotateCCW => self.rotate_piece(3),
            InputKind::HardDrop => self.hard_drop(),
            InputKind::SoftDropPress | InputKind::SoftDropRelease => {}
        }
    }

    pub fn state_hash(&self) -> u64 {
        let Self {
            width,
            height,
            start_level,
            colors,
            cells,
            active_piece,
            piece_id,
            next_types,
            next_next_types,
            score,
            state,
            previous_state,
            pending_garbage,
            nuisance_points,
            chain_count,
            last_was_all_clear,
            all_clear_bonus,
            match_frames,
            fall_offset,
            soft_dropping,
            ground_frames,
            push_backs,
            settle,
            falls,
            popping,
            rng,
        } = self;

        let mut h = Fnv::new();
        h.usize(*width);
        h.usize(*height);
        h.u32(*start_level);
        h.u32(*colors);
        for row in cells {
            for cell in row {
                h.byte(match cell {
                    None => 0,
                    Some(t) => 1 + *t as u8,
                });
            }
        }
        match active_piece {
            None => h.byte(0),
            Some(p) => {
                h.byte(1);
                h.i32(p.row);
                h.i32(p.col);
                h.usize(p.rotation);
                h.byte(p.axis_type as u8);
                h.byte(p.sat_type as u8);
            }
        }
        h.u32(*piece_id);
        h.byte(next_types.0 as u8);
        h.byte(next_types.1 as u8);
        h.byte(next_next_types.0 as u8);
        h.byte(next_next_types.1 as u8);
        h.i32(*score);
        h.byte(*state as u8);
        match previous_state {
            None => h.byte(0),
            Some(s) => {
                h.byte(1);
                h.byte(*s as u8);
            }
        }
        h.u32(*pending_garbage);
        h.u32(*nuisance_points);
        h.u32(*chain_count);
        h.bool(*last_was_all_clear);
        h.bool(*all_clear_bonus);
        h.u32(*match_frames);
        h.u32(*fall_offset);
        h.bool(*soft_dropping);
        h.u32(*ground_frames);
        h.u32(*push_backs);
        match settle {
            Settle::Idle => h.byte(0),
            Settle::Falling { frame, frames } => {
                h.byte(1);
                h.u32(*frame);
                h.u32(*frames);
            }
            Settle::Popping { frame } => {
                h.byte(2);
                h.u32(*frame);
            }
        }
        h.usize(falls.len());
        for f in falls {
            h.bytes(&[f.row, f.col, f.cells_fallen, f.delay, f.ojama as u8]);
        }
        h.usize(popping.len());
        for (r, c) in popping {
            h.bytes(&[*r, *c]);
        }
        for r in [&rng.pieces, &rng.garbage] {
            h.bytes(&r.get_seed());
            h.u64(r.get_stream());
            h.bytes(&r.get_word_pos().to_le_bytes());
        }

        h.finish()
    }

    pub fn step(&mut self, inputs: impl IntoIterator<Item = InputKind>, landed_garbage: u32) -> u32 {
        for kind in inputs {
            self.apply_input(kind);
        }
        let produced = self.tick();
        self.pending_garbage += landed_garbage;
        produced
    }

    #[must_use]
    pub fn replay(&self, from: u32, to: u32, inputs: &[StampedInput], incoming: &[IncomingGarbage]) -> Self {
        let mut board = self.clone();
        let mut next = 0;
        let mut t = from;
        while t < to {
            t += 1;
            let start = next;
            while next < inputs.len() && inputs[next].tick <= t {
                next += 1;
            }
            let landed = incoming.iter().filter(|g| g.at == t).map(|g| g.amount).sum();
            board.step(inputs[start..next].iter().map(|i| i.kind), landed);
        }
        for input in &inputs[next..] {
            board.apply_input(input.kind);
        }
        board
    }

    pub fn tick(&mut self) -> u32 {
        if matches!(
            self.state,
            GameState::Playing | GameState::ResolvingMatches | GameState::DroppingGarbage
        ) {
            self.match_frames += 1;
        }
        match self.state {
            GameState::Playing => {
                self.tick_pair();
                0
            }
            GameState::ResolvingMatches | GameState::DroppingGarbage => self.tick_settle(),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests;
