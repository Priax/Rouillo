use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use rand::Rng;

pub mod config;
use crate::config::*;

pub fn encode<T: serde::Serialize>(msg: &T) -> Vec<u8> {
    use bincode::Options;
    bincode::options()
        .with_varint_encoding()
        .serialize(msg)
        .expect("bincode serialize")
}

pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    use bincode::Options;
    bincode::options()
        .with_varint_encoding()
        .deserialize(bytes)
        .ok()
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

fn default_rng() -> rand::rngs::StdRng {
    use rand::SeedableRng;
    rand::rngs::StdRng::from_os_rng()
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientMessage {
    Input { kind: InputKind },
    TogglePause,
    RequestRestart,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerMessage {
    RoomFull,
    Welcome { player_id: u8 },
    GameStart,
    StateUpdate { p1_board: Board, p2_board: Board },
    Restart,
    OpponentDisconnected,
}

impl PuyoType {
    pub fn random_with_seed<R: Rng>(rng: &mut R) -> PuyoType {
        match rng.random_range(0..5) {
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
    pub cells: Vec<Vec<Option<PuyoType>>>,
    pub active_piece: Option<ActivePuyo>,
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
    #[serde(skip, default = "default_rng")] rng: rand::rngs::StdRng,
}

impl Board {
    pub fn new(width: usize, height: usize, seed: u64) -> Board {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let n1 = (PuyoType::random_with_seed(&mut rng), PuyoType::random_with_seed(&mut rng));
        let n2 = (PuyoType::random_with_seed(&mut rng), PuyoType::random_with_seed(&mut rng));

        Board {
            width,
            height,
            cells: vec![vec![None; width]; height],
            active_piece: None,
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
        self.next_next_types = (PuyoType::random_with_seed(&mut self.rng), PuyoType::random_with_seed(&mut self.rng));
        let new_piece = ActivePuyo { row: 1, col: 2, rotation: 0, axis_type: c1, sat_type: c2 };
        if self.check_collision(&new_piece) {
            self.state = GameState::GameOver;
        } else {
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
        if let Some(mut piece) = self.active_piece.take() {
            let (old_rot, old_col, old_row) = (piece.rotation, piece.col, piece.row);
            piece.rotation = (piece.rotation + direction) % 4;
            if self.check_collision(&piece) {
                piece.col -= 1;
                if self.check_collision(&piece) {
                    piece.col = old_col + 1;
                    if self.check_collision(&piece) {
                        piece.col = old_col;
                        piece.row -= 1;
                        if self.check_collision(&piece) {
                            piece.row = old_row;
                            piece.col = old_col;
                            piece.rotation = (old_rot + 2) % 4;
                            if self.check_collision(&piece) {
                                piece.rotation = old_rot;
                            }
                        }
                    }
                }
            }
            if piece.rotation != old_rot || piece.col != old_col || piece.row != old_row {
                self.reset_lock_if_needed();
            }
            self.active_piece = Some(piece);
        }
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
        let cb = COLOR_BONUS[color_count_len.min(5) as usize];
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
            for i in 0..leftover as usize {
                self.drop_one_garbage(cols[i]);
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
        // Colonne totalement pleine : le garbage ne peut pas être placé, game over.
        self.state = GameState::GameOver;
    }

    fn flood_fill(&self, r: usize, c: usize, target_type: PuyoType, group: &mut Vec<(usize, usize)>, visited: &mut HashSet<(usize, usize)>) {
        if visited.contains(&(r, c)) { return; }
        visited.insert((r, c));
        group.push((r, c));
        for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                if let Some(cell_type) = self.cells[nr as usize][nc as usize] {
                    if cell_type == target_type {
                        self.flood_fill(nr as usize, nc as usize, target_type, group, visited);
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
        1 + (self.played_time / LEVEL_DURATION) as u32
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
