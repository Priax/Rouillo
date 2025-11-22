use std::collections::HashSet;
use rand::Rng;

pub const CELL_SIZE: f32 = 40.0; 
pub const GRID_WIDTH: usize = 6;
pub const GRID_HEIGHT: usize = 13;
pub const VISIBLE_ROW_OFFSET: usize = 1;

pub const MAX_LOCK_TIME: f32 = 0.5;
pub const MAX_LOCK_DELAY_MOVES: u32 = 15;
pub const MAX_TOTAL_GROUND_TIME: f32 = 2.0;
pub const DAS_DELAY: f32 = 0.2;
pub const DAS_SPEED: f32 = 0.05;
pub const SOFT_DROP_SPEED: f32 = 0.05;

const CHAIN_POWERS: [u32; 20] = [0, 0, 8, 16, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512];
const COLOR_BONUS: [u32; 6] = [0, 0, 3, 6, 12, 24];
const GROUP_BONUS: [u32; 8] = [0, 2, 3, 4, 5, 6, 7, 10];

#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash)]
pub enum PuyoType {
    Red,
    Blue,
    Yellow,
    Green,
    Purple,
}

impl PuyoType {
    pub fn random() -> PuyoType {
        let mut rng = rand::rng();
        let val = rng.random_range(0..5);
        match val {
            0 => PuyoType::Red,
            1 => PuyoType::Blue,
            2 => PuyoType::Yellow,
            3 => PuyoType::Green,
            _ => PuyoType::Purple,
        }
    }
}

#[derive(Clone)]
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
            0 => (-1, 0), 1 => (0, 1), 2 => (1, 0), 3 => (0, -1), _ => (-1, 0),
        };
        [(self.row, self.col), (self.row + dr, self.col + dc)]
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum GameState {
    Playing,
    ResolvingMatches,
    GameOver,
    Paused,
}

pub struct Board {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Vec<Option<PuyoType>>>,
    pub active_piece: Option<ActivePuyo>,
    pub next_types: (PuyoType, PuyoType),
    pub next_next_types: (PuyoType, PuyoType),
    pub score: i32,
    pub state: GameState,
    pub previous_state: Option<Box<GameState>>,
    pub lock_timer: f32,
    pub total_ground_timer: f32,
    pub is_touching_ground: bool,
    pub ground_move_count: u32,
    pub lowest_row_reached: i32,
    pub chain_count: u32, 
}

impl Board {
    pub fn new(width: usize, height: usize) -> Board {
        Board {
            width, height,
            cells: vec![vec![None; width]; height],
            active_piece: None,
            next_types: (PuyoType::random(), PuyoType::random()),
            next_next_types: (PuyoType::random(), PuyoType::random()),
            score: 0,
            state: GameState::Playing,
            previous_state: None,
            lock_timer: 0.0,
            total_ground_timer: 0.0,
            is_touching_ground: false,
            ground_move_count: 0,
            lowest_row_reached: -100,
            chain_count: 0,
        }
    }

    pub fn spawn_piece(&mut self) {
        if self.cells[VISIBLE_ROW_OFFSET][2].is_some() { self.state = GameState::GameOver; return; }
        let (c1, c2) = self.next_types;
        self.next_types = self.next_next_types;
        self.next_next_types = (PuyoType::random(), PuyoType::random());
        let new_piece = ActivePuyo { row: 1, col: 2, rotation: 0, axis_type: c1, sat_type: c2 };
        if self.check_collision(&new_piece) { self.state = GameState::GameOver; } else {
            self.lowest_row_reached = new_piece.row; self.active_piece = Some(new_piece);
            self.lock_timer = 0.0; self.total_ground_timer = 0.0; self.is_touching_ground = false;
            self.ground_move_count = 0; self.chain_count = 0;
        }
    }
    
    pub fn get_ghost_piece(&self) -> Option<ActivePuyo> {
        let mut ghost = self.active_piece.clone()?;
        while !self.check_collision(&ghost) { ghost.row += 1; }
        ghost.row -= 1; Some(ghost)
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
            self.lock_timer = 0.0; self.ground_move_count += 1;
        }
    }

    pub fn move_piece(&mut self, dx: i32) {
        if let Some(mut piece) = self.active_piece.take() {
            piece.col += dx;
            if self.check_collision(&piece) { piece.col -= dx; } else {
                self.active_piece = Some(piece); self.reset_lock_if_needed(); return;
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
                        piece.col = old_col; piece.row -= 1;
                        if self.check_collision(&piece) {
                            piece.row = old_row; piece.col = old_col; piece.rotation = old_rot;
                            let quick_rot = (old_rot + 2) % 4; piece.rotation = quick_rot;
                            if self.check_collision(&piece) { piece.rotation = old_rot; }
                        }
                    }
                }
            }
            if piece.rotation != old_rot || piece.col != old_col || piece.row != old_row { self.reset_lock_if_needed(); }
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
                piece.row -= 1; self.is_touching_ground = true;
            } else {
                self.is_touching_ground = false; self.lock_timer = 0.0;
            }
            self.active_piece = Some(piece);
        }
    }

    pub fn update_logic(&mut self, delta_time: f32) -> bool {
        let mut locked = false;
        if let Some(mut piece) = self.active_piece.take() {
            if piece.row > self.lowest_row_reached {
                self.lowest_row_reached = piece.row; self.total_ground_timer = 0.0; self.ground_move_count = 0;
            }
            piece.row += 1;
            let collision = self.check_collision(&piece);
            piece.row -= 1;

            if collision {
                self.is_touching_ground = true;
                self.lock_timer += delta_time;
                self.total_ground_timer += delta_time;
                if self.lock_timer > MAX_LOCK_TIME || self.total_ground_timer > MAX_TOTAL_GROUND_TIME {
                    self.active_piece = Some(piece); self.lock_piece(); locked = true;
                } else { self.active_piece = Some(piece); }
            } else {
                self.is_touching_ground = false; self.lock_timer = 0.0; self.active_piece = Some(piece);
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

    fn apply_board_gravity(&mut self) -> bool {
        let mut moved = false;
        for col in 0..self.width {
            for row in (0..self.height - 1).rev() {
                if self.cells[row][col].is_some() && self.cells[row + 1][col].is_none() {
                    let mut drop_row = row;
                    while drop_row + 1 < self.height && self.cells[drop_row + 1][col].is_none() { drop_row += 1; }
                    self.cells[drop_row][col] = self.cells[row][col].take();
                    moved = true;
                }
            }
        }
        moved
    }

    fn check_matches(&mut self) -> bool {
        let mut to_remove = HashSet::new();
        let mut visited = HashSet::new();
        let mut group_sizes = Vec::new();
        let mut unique_colors = HashSet::new();
        let mut total_puyos_cleared = 0;

        for r in 0..self.height {
            for c in 0..self.width {
                if let Some(p_type) = self.cells[r][c] {
                    if !visited.contains(&(r, c)) {
                        let mut group = Vec::new();
                        self.flood_fill(r, c, p_type, &mut group, &mut visited);
                        if group.len() >= 4 {
                            if group.iter().any(|(r, _)| *r >= VISIBLE_ROW_OFFSET) {
                                unique_colors.insert(p_type);
                                group_sizes.push(group.len() as u32);
                                total_puyos_cleared += group.len() as u32;
                                for pos in group { to_remove.insert(pos); }
                            }
                        }
                    }
                }
            }
        }
        if to_remove.is_empty() { return false; }
        self.chain_count += 1;
        self.calculate_score(unique_colors.len(), total_puyos_cleared, &group_sizes);
        for (r, c) in to_remove { self.cells[r][c] = None; }
        true
    }

    fn calculate_score(&mut self, color_count_len: usize, total_cleared: u32, group_sizes: &[u32]) {
        let chain_idx = (self.chain_count).min(19) as usize;
        let cp = CHAIN_POWERS[chain_idx];
        let cb = COLOR_BONUS[color_count_len.min(5) as usize];
        let mut gb = 0;
        for &size in group_sizes { gb += GROUP_BONUS[(size.saturating_sub(4)).min(7) as usize]; }
        let mut multiplier = cp + cb + gb;
        if multiplier == 0 { multiplier = 1; }
        if multiplier > 999 { multiplier = 999; }
        self.score += (10 * total_cleared) as i32 * multiplier as i32;
    }

    fn flood_fill(&self, r: usize, c: usize, target_type: PuyoType, group: &mut Vec<(usize, usize)>, visited: &mut HashSet<(usize, usize)>) {
        if visited.contains(&(r, c)) { return; }
        visited.insert((r, c)); group.push((r, c));
        for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)].iter() {
            let nr = r as i32 + dr; let nc = c as i32 + dc;
            if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                if let Some(cell_type) = self.cells[nr as usize][nc as usize] {
                    if cell_type == target_type { self.flood_fill(nr as usize, nc as usize, target_type, group, visited); }
                }
            }
        }
    }

    pub fn resolve_step(&mut self) {
        let fell = self.apply_board_gravity();
        if !fell {
            let matched = self.check_matches();
            if !matched && self.state != GameState::GameOver { self.state = GameState::Playing; self.spawn_piece(); }
        }
    }

    pub fn toggle_pause(&mut self) {
        match self.state {
            GameState::Paused => {
                if let Some(prev) = self.previous_state.take() {
                    self.state = *prev;
                } else {
                    self.state = GameState::Playing;
                }
            }
            GameState::GameOver => {},
            _ => {
                self.previous_state = Some(Box::new(self.state));
                self.state = GameState::Paused;
            }
        }
    }
}
