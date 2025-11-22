use macroquad::prelude::*;
use std::collections::HashSet;

const CELL_SIZE: f32 = 40.0;
const GRID_WIDTH: usize = 6;
const GRID_HEIGHT: usize = 13;
const VISIBLE_ROW_OFFSET: usize = 1;

const MAX_LOCK_TIME: f32 = 0.5;
const MAX_LOCK_DELAY_MOVES: u32 = 15;
const MAX_TOTAL_GROUND_TIME: f32 = 2.0;
const DAS_DELAY: f32 = 0.2;
const DAS_SPEED: f32 = 0.05;
const SOFT_DROP_SPEED: f32 = 0.05;

const CHAIN_POWERS: [u32; 20] = [0, 0, 8, 16, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512];
const COLOR_BONUS: [u32; 6] = [0, 0, 3, 6, 12, 24];
const GROUP_BONUS: [u32; 8] = [0, 2, 3, 4, 5, 6, 7, 10];

// Modèle
#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash)]
enum PuyoType {
    Red,
    Blue,
    Yellow,
    Green,
    Purple,
}

impl PuyoType {
    fn random() -> PuyoType {
        let val = rand::gen_range(0, 5);
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
struct ActivePuyo {
    row: i32,
    col: i32,
    rotation: usize,
    axis_type: PuyoType,
    sat_type: PuyoType,
}

impl ActivePuyo {
    fn get_positions(&self) -> [(i32, i32); 2] {
        let (dr, dc) = match self.rotation {
            0 => (-1, 0), 1 => (0, 1), 2 => (1, 0), 3 => (0, -1), _ => (-1, 0),
        };
        [(self.row, self.col), (self.row + dr, self.col + dc)]
    }
}

#[derive(PartialEq, Clone)]
enum GameState {
    Playing,
    ResolvingMatches,
    GameOver,
    Paused,
}

struct Board {
    width: usize,
    height: usize,
    cells: Vec<Vec<Option<PuyoType>>>,
    active_piece: Option<ActivePuyo>,
    next_types: (PuyoType, PuyoType),
    next_next_types: (PuyoType, PuyoType),
    score: i32,
    state: GameState,
    previous_state: Option<Box<GameState>>,
    lock_timer: f32,
    total_ground_timer: f32,
    is_touching_ground: bool,
    ground_move_count: u32,
    lowest_row_reached: i32,
    chain_count: u32, 
}

impl Board {
    fn new(width: usize, height: usize) -> Board {
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
 
    fn spawn_piece(&mut self) {
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
    
    fn get_ghost_piece(&self) -> Option<ActivePuyo> {
        let mut ghost = self.active_piece.clone()?;
        while !self.check_collision(&ghost) { ghost.row += 1; }
        ghost.row -= 1; Some(ghost)
    }

    fn check_collision(&self, piece: &ActivePuyo) -> bool {
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

    fn move_piece(&mut self, dx: i32) {
        if let Some(mut piece) = self.active_piece.take() {
            piece.col += dx;
            if self.check_collision(&piece) { piece.col -= dx; } else {
                self.active_piece = Some(piece); self.reset_lock_if_needed(); return;
            }
            self.active_piece = Some(piece);
        }
    }

    fn rotate_piece(&mut self, direction: usize) {
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

    fn hard_drop(&mut self) {
        if let Some(mut piece) = self.active_piece.take() {
            loop {
                piece.row += 1;
                if self.check_collision(&piece) { piece.row -= 1; break; }
            }
            self.active_piece = Some(piece);
            self.lock_piece();
        }
    }
    
    fn force_drop(&mut self) {
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

    fn update_logic(&mut self, delta_time: f32) -> bool {
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

    fn resolve_step(&mut self) {
        let fell = self.apply_board_gravity();
        if !fell {
            let matched = self.check_matches();
            if !matched && self.state != GameState::GameOver { self.state = GameState::Playing; self.spawn_piece(); }
        }
    }

    fn toggle_pause(&mut self) {
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
                self.previous_state = Some(Box::new(self.state.clone()));
                self.state = GameState::Paused;
            }
        }
    }
}

// Vue
fn get_puyo_color(puyo_type: PuyoType) -> Color {
    match puyo_type {
        PuyoType::Red => RED, PuyoType::Blue => BLUE, PuyoType::Yellow => GOLD, PuyoType::Green => GREEN, PuyoType::Purple => PURPLE,
    }
}

fn draw_board(board: &Board, offset_x: f32, offset_y: f32, board_w: f32, board_h: f32) {
    draw_rectangle(offset_x, offset_y, board_w, board_h, Color::new(0.12, 0.12, 0.12, 1.0));
    
    let x_cross = offset_x + (2.0 * CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw_line(x_cross, y_cross, x_cross + 20.0, y_cross + 20.0, 3.0, RED);
    draw_line(x_cross + 20.0, y_cross, x_cross, y_cross + 20.0, 3.0, RED);

    for r in VISIBLE_ROW_OFFSET..board.height {
        for c in 0..board.width {
            let draw_r = (r - VISIBLE_ROW_OFFSET) as f32;
            draw_cell(draw_r, c as f32, board.cells[r][c], offset_x, offset_y, 1.0);
        }
    }

    if board.state == GameState::Playing {
        if let Some(ghost) = board.get_ghost_piece() {
            let positions = ghost.get_positions();
            for pos in positions.iter() {
                let p_type = if pos.0 == ghost.row && pos.1 == ghost.col { ghost.axis_type } else { ghost.sat_type };
                draw_cell(pos.0 as f32 - VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 0.3);
            }
        }
    }

    if let Some(ref piece) = board.active_piece {
        let positions = piece.get_positions();
        for pos in positions.iter() {
            let p_type = if pos.0 == piece.row && pos.1 == piece.col { piece.axis_type } else { piece.sat_type };
            draw_cell(pos.0 as f32 - VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 1.0);
        }
    }

    let visible_height = (board.height - VISIBLE_ROW_OFFSET) as f32;
    for i in 0..=board.width {
        let x = offset_x + (i as f32 * CELL_SIZE);
        draw_line(x, offset_y, x, offset_y + board_h, 1.0, DARKGRAY);
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as f32 * CELL_SIZE);
        draw_line(offset_x, y, offset_x + board_w, y, 1.0, DARKGRAY);
    }
}

fn draw_cell(row: f32, col: f32, puyo_type: Option<PuyoType>, dx: f32, dy: f32, alpha: f32) {
    if let Some(pt) = puyo_type {
        if row >= 0.0 {
            let mut color = get_puyo_color(pt);
            color.a = alpha;
            draw_rectangle(dx + col * CELL_SIZE + 1.0, dy + row * CELL_SIZE + 1.0, CELL_SIZE - 2.0, CELL_SIZE - 2.0, color);
        }
    }
}

#[macroquad::main("Puyo Rust WASM")]
async fn main() {
    rand::srand(macroquad::miniquad::date::now() as u64);
    let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT);
    board.spawn_piece();

    let base_fall_interval = 0.8;
    let resolve_interval = 0.15;

    let mut last_fall_time = get_time();
    let mut last_resolve_time = get_time();
    let start_time = get_time();

    let mut key_timer_left: f32 = 0.0;
    let mut key_timer_right: f32 = 0.0;
    let mut key_timer_down: f32 = 0.0;

    loop {
        let delta_time = get_frame_time();
        let time_now = get_time();
        
        if is_key_pressed(KeyCode::Escape) {
            board.toggle_pause();
        }

        if is_key_pressed(KeyCode::R) && (board.state == GameState::GameOver || board.state == GameState::Paused) {
            board = Board::new(GRID_WIDTH, GRID_HEIGHT);
            board.spawn_piece();
            last_fall_time = get_time();
        }

        if board.state != GameState::Paused && board.state != GameState::GameOver {
            let seconds_played = time_now - start_time;
            let level = 1 + (seconds_played / 15.0) as u32;
            let speed_decrease = (level as f64 - 1.0) * 0.05;
            let current_interval = if speed_decrease >= (base_fall_interval - 0.1) { 0.1 } else { base_fall_interval - speed_decrease };

            if board.state == GameState::Playing {
                if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::Z) { board.rotate_piece(1); }
                if is_key_pressed(KeyCode::X) || is_key_pressed(KeyCode::W) { board.rotate_piece(3); }
                if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) {
                    board.hard_drop();
                    last_fall_time = get_time();
                }
                if is_key_down(KeyCode::Left) {
                    if key_timer_left == 0.0 {
                        board.move_piece(-1); key_timer_left = 0.0001;
                    } else {
                        key_timer_left += delta_time;
                        if key_timer_left > DAS_DELAY {
                            while key_timer_left > DAS_DELAY + DAS_SPEED { board.move_piece(-1); key_timer_left -= DAS_SPEED; }
                        }
                    }
                } else { key_timer_left = 0.0; }
                if is_key_down(KeyCode::Right) {
                    if key_timer_right == 0.0 {
                        board.move_piece(1); key_timer_right = 0.0001;
                    } else {
                        key_timer_right += delta_time;
                        if key_timer_right > DAS_DELAY {
                            while key_timer_right > DAS_DELAY + DAS_SPEED { board.move_piece(1); key_timer_right -= DAS_SPEED; }
                        }
                    }
                } else { key_timer_right = 0.0; }
                if is_key_down(KeyCode::Down) {
                    key_timer_down += delta_time;
                    if key_timer_down > SOFT_DROP_SPEED {
                        board.force_drop(); last_fall_time = get_time(); key_timer_down = 0.0;
                    }
                } else { key_timer_down = 0.0; }
            }

            match board.state {
                GameState::Playing => {
                    let locked = board.update_logic(delta_time);
                    if locked { last_fall_time = get_time(); } else {
                        if !board.is_touching_ground && (time_now - last_fall_time > current_interval) {
                            board.force_drop(); last_fall_time = get_time();
                        }
                    }
                },
                GameState::ResolvingMatches => {
                    if time_now - last_resolve_time > resolve_interval {
                        board.resolve_step(); last_resolve_time = get_time();
                    }
                },
                _ => {}
            }
        }

        clear_background(Color::new(0.05, 0.05, 0.05, 1.0));

        let board_w = GRID_WIDTH as f32 * CELL_SIZE;
        let board_h = (GRID_HEIGHT - VISIBLE_ROW_OFFSET) as f32 * CELL_SIZE;
        let offset_x = (screen_width() - board_w) / 2.0;
        let offset_y = (screen_height() - board_h) / 2.0;
        let ui_x = offset_x + board_w + 20.0;

        draw_board(&board, offset_x, offset_y, board_w, board_h);

        draw_text(&format!("Score: {}", board.score), ui_x, offset_y + 20.0, 30.0, WHITE);
        let seconds = time_now - start_time;
        draw_text(&format!("Level: {}", 1 + (seconds / 15.0) as u32), ui_x, offset_y + 50.0, 30.0, YELLOW);
        draw_text("Next:", ui_x, offset_y + 100.0, 30.0, GRAY);

        draw_rectangle(ui_x, offset_y + 110.0, CELL_SIZE, CELL_SIZE * 2.1, Color::new(0.2, 0.2, 0.2, 1.0));
        draw_cell(0.0, 0.0, Some(board.next_types.1), ui_x, offset_y + 110.0, 1.0);
        draw_cell(1.0, 0.0, Some(board.next_types.0), ui_x, offset_y + 110.0, 1.0);

        let nn_y = offset_y + 220.0;
        let nn_size = CELL_SIZE * 0.5;
        draw_rectangle(ui_x + 5.0, nn_y, nn_size, nn_size, get_puyo_color(board.next_next_types.1));
        draw_rectangle(ui_x + 5.0, nn_y + nn_size, nn_size, nn_size, get_puyo_color(board.next_next_types.0));

        if board.chain_count > 1 {
            draw_text(&format!("Chain: {}", board.chain_count), ui_x, nn_y + 80.0, 30.0, GREEN);
        }

        if board.is_touching_ground && board.state == GameState::Playing {
            let ratio = (1.0 - (board.lock_timer / MAX_LOCK_TIME)).min(1.0 - (board.total_ground_timer / MAX_TOTAL_GROUND_TIME));
            let bar_width = 100.0 * ratio;
            let col = if board.total_ground_timer > 1.5 { RED } else { ORANGE };
            draw_rectangle(ui_x, nn_y + 100.0, bar_width, 10.0, col);
        }

        if board.state == GameState::GameOver {
            draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.7));
            let text = "GAME OVER";
            let dims = measure_text(text, None, 60, 1.0);
            draw_text(text, screen_width()/2.0 - dims.width/2.0, screen_height()/2.0, 60.0, RED);
            let sub = "Press R to Restart";
            let sub_dims = measure_text(sub, None, 30, 1.0);
            draw_text(sub, screen_width()/2.0 - sub_dims.width/2.0, screen_height()/2.0 + 50.0, 30.0, WHITE);
        }

        if board.state == GameState::Paused {
            draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.5));
            let text = "PAUSED";
            let dims = measure_text(text, None, 60, 1.0);
            draw_text(text, screen_width()/2.0 - dims.width/2.0, screen_height()/2.0, 60.0, WHITE);
            let sub = "Press ESC to Resume";
            let sub_dims = measure_text(sub, None, 30, 1.0);
            draw_text(sub, screen_width()/2.0 - sub_dims.width/2.0, screen_height()/2.0 + 50.0, 30.0, LIGHTGRAY);
        }

        next_frame().await
    }
}
