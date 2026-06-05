use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use rand::Rng;
use rand::RngExt;

pub mod config;
use crate::config::*;

pub fn encode<T: serde::Serialize>(msg: &T) -> Vec<u8> {
    let raw = bitcode::serialize(msg).expect("bitcode serialize");
    lz4_flex::compress_prepend_size(&raw)
}

pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    let raw = lz4_flex::decompress_size_prepended(bytes).ok()?;
    bitcode::deserialize(&raw).ok()
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
    SoftDrop,
    HardDrop,
}

pub type RoomId = u32;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct RoomSettings {
    pub starting_level: u32,
    pub colors: u32,
}

impl Default for RoomSettings {
    fn default() -> Self {
        Self { starting_level: 1, colors: 5 }
    }
}

impl RoomSettings {
    pub const COUNT: usize = 2;

    pub fn label(i: usize) -> &'static str {
        match i {
            0 => "Starting level",
            _ => "Colors",
        }
    }

    pub fn value(&self, i: usize) -> String {
        match i {
            0 => self.starting_level.to_string(),
            _ => self.colors.to_string(),
        }
    }

    pub fn adjust(&mut self, i: usize, dir: i32) {
        match i {
            0 => self.starting_level = (self.starting_level as i32 + dir).clamp(1, 15) as u32,
            _ => self.colors = (self.colors as i32 + dir).clamp(4, 5) as u32,
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LobbyInfo {
    pub id: RoomId,
    pub name: String,
    pub settings: RoomSettings,
    pub players: u8,
    pub your_slot: u8,
    pub is_host: bool,
    pub countdown: Option<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientMessage {
    Hello { player_id: String },
    Input { kind: InputKind, seq: u32 },
    TogglePause,
    RequestRestart,
    RequestRoomList,
    CreateRoom { name: String },
    JoinRoom { id: RoomId },
    LeaveRoom,
    SetRoomSetting { index: u8, dir: i32 },
    ToggleCountdown,
    ReturnToLobby,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerMessage {
    GameStart,
    StateUpdate { p1_board: Box<Board>, p2_board: Box<Board>, p1_ack: u32, p2_ack: u32 },
    Restart,
    OpponentDisconnected,
    RoomList { rooms: Vec<RoomInfo> },
    Lobby { info: LobbyInfo },
    JoinFailed { reason: String },
}

impl PuyoType {
    pub fn random_with_seed<R: Rng>(rng: &mut R, colors: u32) -> PuyoType {
        let n = colors.clamp(1, 5);
        match rng.random_range(0..n) {
            0 => PuyoType::Red,
            1 => PuyoType::Blue,
            2 => PuyoType::Yellow,
            3 => PuyoType::Green,
            _ => PuyoType::Purple,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
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

#[derive(PartialEq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum GameState {
    Playing,
    ResolvingMatches,
    DroppingGarbage,
    GameOver,
    Paused,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Board {
    pub width: usize,
    pub height: usize,
    pub start_level: u32,
    pub colors: u32,
    pub cells: Vec<Vec<Option<PuyoType>>>,
    pub active_piece: Option<ActivePuyo>,
    #[serde(default)] pub piece_id: u32,
    pub next_types: (PuyoType, PuyoType),
    pub next_next_types: (PuyoType, PuyoType),
    pub score: i32,
    pub state: GameState,
    #[serde(skip)] pub previous_state: Option<GameState>,
    pub pending_garbage: u32,
    pub nuisance_points: u32,
    pub lock_timer: f32,
    pub total_ground_timer: f32,
    pub is_touching_ground: bool,
    pub ground_move_count: u32,
    pub lowest_row_reached: i32,
    pub chain_count: u32,
    #[serde(default)] pub played_time: f32,
    #[serde(default)] pub fall_timer: f32,
    #[serde(default)] pub resolve_timer: f32,
    #[serde(default)] pub garbage_delay_timer: f32,
    rng: rand_chacha::ChaCha12Rng,
}

impl Board {
    pub fn new(width: usize, height: usize, seed: u64, start_level: u32, colors: u32) -> Board {
        use rand::SeedableRng;
        let mut rng = rand_chacha::ChaCha12Rng::seed_from_u64(seed);
        let n1 = (PuyoType::random_with_seed(&mut rng, colors), PuyoType::random_with_seed(&mut rng, colors));
        let n2 = (PuyoType::random_with_seed(&mut rng, colors), PuyoType::random_with_seed(&mut rng, colors));

        Board {
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
            lock_timer: 0.0,
            total_ground_timer: 0.0,
            is_touching_ground: false,
            ground_move_count: 0,
            lowest_row_reached: -100,
            chain_count: 0,
            played_time: 0.0,
            fall_timer: 0.0,
            resolve_timer: 0.0,
            garbage_delay_timer: 0.0,
            rng,
        }
    }

    pub fn spawn_piece(&mut self) {
        if self.cells[VISIBLE_ROW_OFFSET][2].is_some() {
            self.state = GameState::GameOver;
            return;
        }
        let (c1, c2) = self.next_types;
        self.next_types = self.next_next_types;
        self.next_next_types = (PuyoType::random_with_seed(&mut self.rng, self.colors), PuyoType::random_with_seed(&mut self.rng, self.colors));
        let new_piece = ActivePuyo { row: 1, col: 2, rotation: 0, axis_type: c1, sat_type: c2 };
        if self.check_collision(&new_piece) {
            self.state = GameState::GameOver;
        } else {
            self.piece_id = self.piece_id.wrapping_add(1);
            self.lowest_row_reached = new_piece.row;
            self.active_piece = Some(new_piece);
            self.lock_timer = 0.0;
            self.total_ground_timer = 0.0;
            self.is_touching_ground = false;
            self.ground_move_count = 0;
            self.chain_count = 0;
        }
    }

    pub fn get_ghost_piece(&self) -> Option<ActivePuyo> {
        let mut ghost = self.active_piece.clone()?;
        while !self.check_collision(&ghost) { ghost.row += 1; }
        ghost.row -= 1;
        Some(ghost)
    }

    pub fn check_collision(&self, piece: &ActivePuyo) -> bool {
        for (r, c) in piece.get_positions().iter() {
            if *c < 0 || *c >= self.width as i32 || *r >= self.height as i32 { return true; }
            if *r >= 0 && self.cells[*r as usize][*c as usize].is_some() { return true; }
        }
        false
    }

    fn reset_lock_if_needed(&mut self) {
        if self.is_touching_ground && self.ground_move_count < MAX_LOCK_DELAY_MOVES {
            self.lock_timer = 0.0;
            self.ground_move_count += 1;
        }
    }

    pub fn move_piece(&mut self, dx: i32) {
        if let Some(mut piece) = self.active_piece.take() {
            piece.col += dx;
            if self.check_collision(&piece) {
                piece.col -= dx;
            } else {
                self.reset_lock_if_needed();
                self.active_piece = Some(piece);
                return;
            }
            self.active_piece = Some(piece);
        }
    }

    pub fn rotate_piece(&mut self, direction: usize) {
        let Some(mut piece) = self.active_piece.take() else { return };
        let (old_rot, old_col, old_row) = (piece.rotation, piece.col, piece.row);
        let new_rot = (old_rot + direction) % 4;
        let sat = piece.get_positions()[1];
        let grounded = {
            let mut below = piece.clone();
            below.row += 1;
            self.check_collision(&below)
        };

        let candidates = [
            (old_row,     old_col,     new_rot,           true),                     // in place
            (old_row,     old_col - 1, new_rot,           true),                     // kick left
            (old_row,     old_col + 1, new_rot,           true),                     // kick right
            (old_row - 1, old_col,     new_rot,           grounded && new_rot == 2), // floor kick
            (sat.0,       sat.1,       (old_rot + 2) % 4, true),                     // pivot on satellite
        ];

        for (row, col, rotation, enabled) in candidates {
            if !enabled {
                continue;
            }
            piece.row = row;
            piece.col = col;
            piece.rotation = rotation;
            if !self.check_collision(&piece) {
                if piece.rotation != old_rot || piece.col != old_col || piece.row != old_row {
                    self.reset_lock_if_needed();
                }
                self.active_piece = Some(piece);
                return;
            }
        }

        piece.row = old_row;
        piece.col = old_col;
        piece.rotation = old_rot;
        self.active_piece = Some(piece);
    }

    pub fn hard_drop(&mut self) {
        if let Some(mut piece) = self.active_piece.take() {
            loop {
                piece.row += 1;
                if self.check_collision(&piece) { piece.row -= 1; break; }
            }
            self.active_piece = Some(piece);
            self.lock_piece();
        }
    }

    pub fn force_drop(&mut self) {
        if let Some(mut piece) = self.active_piece.take() {
            piece.row += 1;
            if self.check_collision(&piece) {
                piece.row -= 1;
                self.is_touching_ground = true;
            } else {
                self.is_touching_ground = false;
                self.lock_timer = 0.0;
            }
            self.active_piece = Some(piece);
        }
    }

    pub fn update_logic(&mut self, delta_time: f32) -> bool {
        let mut locked = false;
        if let Some(mut piece) = self.active_piece.take() {
            if piece.row > self.lowest_row_reached {
                self.lowest_row_reached = piece.row;
                self.total_ground_timer = 0.0;
                self.ground_move_count = 0;
            }
            piece.row += 1;
            let collision = self.check_collision(&piece);
            piece.row -= 1;
            if collision {
                self.is_touching_ground = true;
                self.lock_timer += delta_time;
                self.total_ground_timer += delta_time;
                if self.lock_timer > MAX_LOCK_TIME || self.total_ground_timer > MAX_TOTAL_GROUND_TIME {
                    self.active_piece = Some(piece);
                    self.lock_piece();
                    locked = true;
                } else {
                    self.active_piece = Some(piece);
                }
            } else {
                self.is_touching_ground = false;
                self.lock_timer = 0.0;
                self.active_piece = Some(piece);
            }
        }
        locked
    }

    fn lock_piece(&mut self) {
        if let Some(piece) = self.active_piece.take() {
            for (r, c) in piece.get_positions().iter() {
                if *r >= 0 && *r < self.height as i32 && *c >= 0 && *c < self.width as i32 {
                    let puyo_type = if *r == piece.row && *c == piece.col { piece.axis_type } else { piece.sat_type };
                    self.cells[*r as usize][*c as usize] = Some(puyo_type);
                }
            }
            self.apply_board_gravity();
            self.state = GameState::ResolvingMatches;
        }
    }

    pub fn apply_board_gravity(&mut self) -> bool {
        let mut moved = false;
        for col in 0..self.width {
            for row in (0..self.height - 1).rev() {
                if self.cells[row][col].is_some() && self.cells[row + 1][col].is_none() {
                    let mut drop_row = row;
                    while drop_row + 1 < self.height && self.cells[drop_row + 1][col].is_none() {
                        drop_row += 1;
                    }
                    self.cells[drop_row][col] = self.cells[row][col].take();
                    moved = true;
                }
            }
        }
        moved
    }

    pub fn check_matches(&mut self) -> Option<u32> {
        let mut to_remove = HashSet::new();
        let mut visited = HashSet::new();
        let mut group_sizes = Vec::new();
        let mut unique_colors = HashSet::new();
        let mut total_puyos_cleared = 0;

        for r in 0..self.height {
            for c in 0..self.width {
                if let Some(p_type) = self.cells[r][c] {

                    if p_type == PuyoType::Garbage {
                        continue;
                    }

                    if !visited.contains(&(r, c)) {
                        let mut group = Vec::new();
                        self.flood_fill(r, c, p_type, &mut group, &mut visited);
                        if group.len() >= 4 && group.iter().any(|(r, _)| *r >= VISIBLE_ROW_OFFSET) {
                            unique_colors.insert(p_type);
                            group_sizes.push(group.len() as u32);
                            total_puyos_cleared += group.len() as u32;
                            for pos in group {
                                to_remove.insert(pos);
                                self.mark_adjacent_garbage(pos.0, pos.1, &mut to_remove);
                            }
                        }
                    }
                }
            }
        }
        if to_remove.is_empty() { return None; }
        self.chain_count += 1;
        let score_gained = self.calculate_score(unique_colors.len(), total_puyos_cleared, &group_sizes);
        self.score += score_gained;

        for (r, c) in to_remove { self.cells[r][c] = None; }
        Some(score_gained as u32)
    }

    fn mark_adjacent_garbage(&self, r: usize, c: usize, to_remove: &mut HashSet<(usize, usize)>) {
        let neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        for (dr, dc) in neighbors.iter() {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;

            if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                let nr = nr as usize;
                let nc = nc as usize;
                if let Some(PuyoType::Garbage) = self.cells[nr][nc] {
                    to_remove.insert((nr, nc));
                }
            }
        }
    }

    fn calculate_score(&self, color_count_len: usize, total_cleared: u32, group_sizes: &[u32]) -> i32 {
        let chain_idx = (self.chain_count).min(19) as usize;
        let cp = CHAIN_POWERS[chain_idx];
        let cb = COLOR_BONUS[color_count_len.min(5)];
        let mut gb = 0;
        for &size in group_sizes { gb += GROUP_BONUS[(size.saturating_sub(4)).min(7) as usize]; }
        let mut multiplier = cp + cb + gb;
        if multiplier == 0 { multiplier = 1; }
        if multiplier > 999 { multiplier = 999; }

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

        'full: for _ in 0..full_lines {
            for c in 0..self.width {
                self.drop_one_garbage(c);
                if self.state == GameState::GameOver {
                    break 'full;
                }
            }
        }

        if self.state != GameState::GameOver && leftover > 0 {
            let mut cols: Vec<usize> = (0..self.width).collect();
            for i in 0..leftover as usize {
                let j = self.rng.random_range(i..self.width);
                cols.swap(i, j);
            }
            for &col in cols.iter().take(leftover as usize) {
                self.drop_one_garbage(col);
                if self.state == GameState::GameOver {
                    break;
                }
            }
        }
    }

    fn drop_one_garbage(&mut self, col: usize) {
        for r in (0..self.height).rev() {
            if self.cells[r][col].is_none() {
                self.cells[r][col] = Some(PuyoType::Garbage);
                return;
            }
        }
        self.state = GameState::GameOver;
    }

    fn flood_fill(&self, r: usize, c: usize, target_type: PuyoType, group: &mut Vec<(usize, usize)>, visited: &mut HashSet<(usize, usize)>) {
        let mut stack = vec![(r, c)];
        while let Some((r, c)) = stack.pop() {
            if !visited.insert((r, c)) { continue; }
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
    }

    pub fn resolve_step(&mut self) -> u32 {
        if self.apply_board_gravity() {
            return 0;
        }
        if let Some(score) = self.check_matches() {
            let total_nuisance = score + self.nuisance_points;
            self.nuisance_points = total_nuisance % 70;
            return total_nuisance / 70;
        }
        if self.state != GameState::GameOver {
            if self.pending_garbage > 0 {
                self.state = GameState::DroppingGarbage;
            } else if self.cells[VISIBLE_ROW_OFFSET][2].is_some() {
                self.state = GameState::GameOver;
            } else {
                self.state = GameState::Playing;
                self.spawn_piece();
            }
        }
        0
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

    pub fn level(&self) -> u32 {
        self.start_level + (self.played_time / LEVEL_DURATION) as u32
    }

    pub fn apply_input(&mut self, input: InputKind) {
        if self.state != GameState::Playing {
            return;
        }
        match input {
            InputKind::MoveLeft => self.move_piece(-1),
            InputKind::MoveRight => self.move_piece(1),
            InputKind::RotateCW => self.rotate_piece(1),
            InputKind::RotateCCW => self.rotate_piece(3),
            InputKind::SoftDrop => { self.force_drop(); self.fall_timer = 0.0; }
            InputKind::HardDrop => self.hard_drop(),
        }
    }

    pub fn predict_fall(&mut self, dt: f32) {
        if self.state != GameState::Playing {
            return;
        }
        let can_fall = match &self.active_piece {
            Some(p) => {
                let mut below = p.clone();
                below.row += 1;
                !self.check_collision(&below)
            }
            None => false,
        };
        if !can_fall {
            return;
        }
        let speed_decrease = (self.level() as f64 - 1.0) * FALL_SPEEDUP_PER_LEVEL;
        let fall_interval = (BASE_FALL_INTERVAL - speed_decrease).max(MIN_FALL_INTERVAL) as f32;
        self.fall_timer += dt;
        if self.fall_timer > fall_interval {
            self.force_drop();
            self.fall_timer = 0.0;
        }
    }

    pub fn tick(&mut self, dt: f32) -> u32 {
        if self.state == GameState::Playing || self.state == GameState::ResolvingMatches {
            self.played_time += dt;
        }

        let speed_decrease = (self.level() as f64 - 1.0) * FALL_SPEEDUP_PER_LEVEL;
        let fall_interval = (BASE_FALL_INTERVAL - speed_decrease).max(MIN_FALL_INTERVAL) as f32;

        let mut garbage_produced = 0;
        match self.state {
            GameState::Playing => {
                let locked = self.update_logic(dt);
                if locked {
                    self.fall_timer = 0.0;
                } else if !self.is_touching_ground {
                    self.fall_timer += dt;
                    if self.fall_timer > fall_interval {
                        self.force_drop();
                        self.fall_timer = 0.0;
                    }
                }
            }
            GameState::ResolvingMatches => {
                self.resolve_timer += dt;
                if self.resolve_timer > RESOLVE_STEP_INTERVAL {
                    garbage_produced = self.resolve_step();
                    self.resolve_timer = 0.0;
                }
            }
            GameState::DroppingGarbage => {
                self.garbage_delay_timer += dt;
                if self.garbage_delay_timer > GARBAGE_DROP_DELAY {
                    self.drop_garbage();
                    self.apply_board_gravity();
                    if self.state != GameState::GameOver {
                        self.state = GameState::Playing;
                        self.spawn_piece();
                        self.lock_timer = 0.0;
                        self.total_ground_timer = 0.0;
                        self.fall_timer = 0.0;
                    }
                    self.garbage_delay_timer = 0.0;
                }
            }
            _ => {}
        }
        garbage_produced
    }
}

#[cfg(test)]
mod tests;
