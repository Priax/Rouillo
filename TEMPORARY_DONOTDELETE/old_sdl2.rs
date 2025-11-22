use std::time::{Duration, Instant};
use std::collections::HashSet;

use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Scancode};
use sdl2::pixels::Color;
use sdl2::rect::{Point, Rect};
use sdl2::render::{Canvas, BlendMode, Texture, TextureCreator};
use sdl2::video::{Window, WindowContext};
use sdl2::ttf::Font;
use rand::Rng;

const CELL_SIZE: u32 = 40;
const GRID_WIDTH: usize = 6;
const GRID_HEIGHT: usize = 13;
const VISIBLE_ROW_OFFSET: usize = 1;
const FONT_SIZE: u16 = 24;

const MAX_LOCK_TIME: u64 = 500;
const MAX_LOCK_DELAY_MOVES: u32 = 15;
const MAX_TOTAL_GROUND_TIME: u64 = 2000;
const DAS_DELAY: u128 = 200;
const DAS_SPEED: u128 = 50;
const SOFT_DROP_SPEED: u128 = 50;

const CHAIN_POWERS: [u32; 20] = [0, 0, 8, 16, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512];
const COLOR_BONUS: [u32; 6] = [0, 0, 3, 6, 12, 24];
const GROUP_BONUS: [u32; 8] = [0, 2, 3, 4, 5, 6, 7, 10];

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
        let mut rng = rand::rng();
        let colors = [PuyoType::Red, PuyoType::Blue, PuyoType::Yellow, PuyoType::Green, PuyoType::Purple];
        colors[rng.random_range(0..colors.len())]
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
            0 => (-1, 0), // Haut
            1 => (0, 1),  // Droite
            2 => (1, 0),  // Bas
            3 => (0, -1), // Gauche
            _ => (-1, 0),
        };
        [
            (self.row, self.col),
            (self.row + dr, self.col + dc)
        ]
    }
}

#[derive(PartialEq)]
enum GameState {
    Playing,
    ResolvingMatches,
    GameOver,
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
    lock_timer: Duration,
    total_ground_timer: Duration,
    is_touching_ground: bool,
    ground_move_count: u32,
    lowest_row_reached: i32,
    chain_count: u32, 
}

impl Board {
    fn new(width: usize, height: usize) -> Board {
        Board {
            width,
            height,
            cells: vec![vec![None; width]; height],
            active_piece: None,
            next_types: (PuyoType::random(), PuyoType::random()),
            next_next_types: (PuyoType::random(), PuyoType::random()),
            score: 0,
            state: GameState::Playing,
            lock_timer: Duration::from_millis(0),
            total_ground_timer: Duration::from_millis(0),
            is_touching_ground: false,
            ground_move_count: 0,
            lowest_row_reached: -100,
            chain_count: 0,
        }
    }

    fn spawn_piece(&mut self) {
        if self.cells[VISIBLE_ROW_OFFSET][2].is_some() {
            self.state = GameState::GameOver;
            return;
        }

        let (c1, c2) = self.next_types;
        self.next_types = self.next_next_types;
        self.next_next_types = (PuyoType::random(), PuyoType::random());

        let new_piece = ActivePuyo {
            row: 1, 
            col: 2, 
            rotation: 0,
            axis_type: c1,
            sat_type: c2,
        };

        if self.check_collision(&new_piece) {
             self.state = GameState::GameOver;
        } else {
            self.lowest_row_reached = new_piece.row;
            self.active_piece = Some(new_piece);
            self.lock_timer = Duration::from_millis(0);
            self.total_ground_timer = Duration::from_millis(0);
            self.is_touching_ground = false;
            self.ground_move_count = 0;
            self.chain_count = 0;
        }
    }

    fn get_ghost_piece(&self) -> Option<ActivePuyo> {
        let mut ghost = self.active_piece.clone()?;
        while !self.check_collision(&ghost) {
            ghost.row += 1;
        }
        ghost.row -= 1;
        Some(ghost)
    }

    fn check_collision(&self, piece: &ActivePuyo) -> bool {
        for (r, c) in piece.get_positions().iter() {
            if *c < 0 || *c >= self.width as i32 || *r >= self.height as i32 {
                return true;
            }
            if *r >= 0 {
                if self.cells[*r as usize][*c as usize].is_some() {
                    return true;
                }
            }
        }
        false
    }

    fn reset_lock_if_needed(&mut self) {
        if self.is_touching_ground {
            if self.ground_move_count < MAX_LOCK_DELAY_MOVES {
                self.lock_timer = Duration::from_millis(0);
                self.ground_move_count += 1;
            }
        }
    }

    fn move_piece(&mut self, dx: i32) {
        if let Some(mut piece) = self.active_piece.take() {
            piece.col += dx;
            if self.check_collision(&piece) {
                piece.col -= dx;
            } else {
                self.active_piece = Some(piece);
                self.reset_lock_if_needed();
                return;
            }
            self.active_piece = Some(piece);
        }
    }

    fn rotate_piece(&mut self, direction: usize) {
        if let Some(mut piece) = self.active_piece.take() {
            let old_rot = piece.rotation;
            let old_col = piece.col;
            let old_row = piece.row;

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
                            piece.rotation = old_rot;
                            
                            let quick_rot = (old_rot + 2) % 4;
                            piece.rotation = quick_rot;
                            
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

    fn hard_drop(&mut self) {
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

    fn update_logic(&mut self, delta_time: Duration) -> bool {
        let mut locked = false;
        if let Some(mut piece) = self.active_piece.take() {
            
            if piece.row > self.lowest_row_reached {
                self.lowest_row_reached = piece.row;
                self.total_ground_timer = Duration::from_millis(0);
                self.ground_move_count = 0;
            }

            piece.row += 1;
            let collision = self.check_collision(&piece);
            piece.row -= 1;

            if collision {
                self.is_touching_ground = true;
                self.lock_timer += delta_time;
                self.total_ground_timer += delta_time;

                let standard_timeout = self.lock_timer.as_millis() as u64 > MAX_LOCK_TIME;
                let hard_timeout = self.total_ground_timer.as_millis() as u64 > MAX_TOTAL_GROUND_TIME;

                if standard_timeout || hard_timeout {
                    self.active_piece = Some(piece);
                    self.lock_piece();
                    locked = true;
                } else {
                    self.active_piece = Some(piece);
                }
            } else {
                self.is_touching_ground = false;
                self.lock_timer = Duration::from_millis(0);
                self.active_piece = Some(piece);
            }
        }
        locked
    }

    fn force_drop(&mut self) {
        if let Some(mut piece) = self.active_piece.take() {
            piece.row += 1;
            if self.check_collision(&piece) {
                piece.row -= 1;
                self.is_touching_ground = true;
            } else {
                self.is_touching_ground = false;
                self.lock_timer = Duration::from_millis(0);
            }
            self.active_piece = Some(piece);
        }
    }

    fn lock_piece(&mut self) {
        if let Some(piece) = self.active_piece.take() {
            let positions = piece.get_positions();
            
            for (r, c) in positions.iter() {
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
                            let is_visible = group.iter().any(|(r, _)| *r >= VISIBLE_ROW_OFFSET);

                            if is_visible {
                                unique_colors.insert(p_type);
                                group_sizes.push(group.len() as u32);
                                total_puyos_cleared += group.len() as u32;

                                for pos in group {
                                    to_remove.insert(pos);
                                }
                            }
                        }
                    }
                }
            }
        }

        if to_remove.is_empty() { 
            return false; 
        }

        self.chain_count += 1;
        self.calculate_score(unique_colors.len(), total_puyos_cleared, &group_sizes);

        for (r, c) in to_remove {
            self.cells[r][c] = None;
        }
        true
    }

    fn calculate_score(&mut self, color_count_len: usize, total_cleared: u32, group_sizes: &[u32]) {
        let chain_idx = (self.chain_count).min(19) as usize;
        let cp = CHAIN_POWERS[chain_idx];

        let color_count_idx = color_count_len.min(5) as usize;
        let cb = COLOR_BONUS[color_count_idx];

        let mut gb = 0;
        for &size in group_sizes {
            let idx = (size.saturating_sub(4)).min(7) as usize;
            gb += GROUP_BONUS[idx];
        }

        let mut multiplier = cp + cb + gb;
        if multiplier == 0 { multiplier = 1; }
        if multiplier > 999 { multiplier = 999; }

        let step_score = (10 * total_cleared) as i32 * multiplier as i32;
        self.score += step_score;
    }

    fn flood_fill(&self, r: usize, c: usize, target_type: PuyoType, group: &mut Vec<(usize, usize)>, visited: &mut HashSet<(usize, usize)>) {
        if visited.contains(&(r, c)) { return; }
        visited.insert((r, c)); 
        group.push((r, c));

        let deltas = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        for (dr, dc) in deltas.iter() {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                let nr = nr as usize;
                let nc = nc as usize;
                if let Some(cell_type) = self.cells[nr][nc] {
                    if cell_type == target_type {
                        self.flood_fill(nr, nc, target_type, group, visited);
                    }
                }
            }
        }
    }

    fn resolve_step(&mut self) {
        let fell = self.apply_board_gravity();
        
        if !fell {
            let matched = self.check_matches();
            
            if !matched {
                if self.state != GameState::GameOver {
                    self.state = GameState::Playing;
                    self.spawn_piece();
                }
            }
        }
    }
}

fn get_puyo_color(puyo_type: PuyoType) -> Color {
    match puyo_type {
        PuyoType::Red => Color::RGB(220, 20, 60),
        PuyoType::Blue => Color::RGB(30, 144, 255),
        PuyoType::Yellow => Color::RGB(255, 215, 0),
        PuyoType::Green => Color::RGB(50, 205, 50),
        PuyoType::Purple => Color::RGB(153, 50, 204),
    }
}

struct TextCache<'a> {
    texture: Texture<'a>,
    rect: Rect,
    last_value: String,
}

impl<'a> TextCache<'a> {
    fn new(creator: &'a TextureCreator<WindowContext>, font: &Font, initial_text: &str, color: Color, x: i32, y: i32) -> Self {
        let surface = font.render(initial_text).blended(color).expect("Failed to render text surface");
        let texture = creator.create_texture_from_surface(&surface).expect("Failed to create text texture");
        let (w, h) = surface.size();
        
        TextCache {
            texture,
            rect: Rect::new(x, y, w, h),
            last_value: initial_text.to_string(),
        }
    }

    fn update(&mut self, creator: &'a TextureCreator<WindowContext>, font: &Font, new_text: &str, color: Color) {
        if self.last_value != new_text {
            let surface = font.render(new_text).blended(color).expect("Failed to render update text");
            self.texture = creator.create_texture_from_surface(&surface).expect("Failed to create update texture");
            let (w, h) = surface.size();
            self.rect.set_width(w);
            self.rect.set_height(h);
            self.last_value = new_text.to_string();
        }
    }

    fn draw(&self, canvas: &mut Canvas<Window>) {
        canvas.copy(&self.texture, None, self.rect).unwrap();
    }
}

struct GameTextures<'a> {
    label_next: Texture<'a>,
    label_game_over: Texture<'a>,
    label_restart: Texture<'a>,
}

fn draw_board<'a>(
    board: &Board, 
    canvas: &mut Canvas<Window>, 
    static_textures: &GameTextures<'a>,
    offset_x: i32,
    offset_y: i32,
    board_w: i32,
    board_h: i32
) {
    canvas.set_draw_color(Color::RGB(30, 30, 30));
    canvas.fill_rect(Rect::new(offset_x, offset_y, board_w as u32, board_h as u32)).unwrap();

    let x_cross = offset_x + (2 * CELL_SIZE) as i32 + 10;
    let y_cross = offset_y + 10;
    canvas.set_draw_color(Color::RGB(150, 50, 50));
    canvas.draw_line(Point::new(x_cross, y_cross), Point::new(x_cross + 20, y_cross + 20)).unwrap();
    canvas.draw_line(Point::new(x_cross + 20, y_cross), Point::new(x_cross, y_cross + 20)).unwrap();

    for r in VISIBLE_ROW_OFFSET..board.height {
        for c in 0..board.width {
            let draw_r = (r - VISIBLE_ROW_OFFSET) as i32;
            draw_cell(canvas, draw_r, c as i32, board.cells[r][c], offset_x, offset_y, 255);
        }
    }

    if board.state == GameState::Playing {
        if let Some(ghost) = board.get_ghost_piece() {
            let positions = ghost.get_positions();
            for pos in positions.iter() {
                draw_cell(canvas, pos.0 - VISIBLE_ROW_OFFSET as i32, pos.1, Some(if pos.0 == ghost.row && pos.1 == ghost.col { ghost.axis_type } else { ghost.sat_type }), offset_x, offset_y, 80);
            }
        }
    }

    if let Some(ref piece) = board.active_piece {
        let positions = piece.get_positions();
        for pos in positions.iter() {
            draw_cell(canvas, pos.0 - VISIBLE_ROW_OFFSET as i32, pos.1, Some(if pos.0 == piece.row && pos.1 == piece.col { piece.axis_type } else { piece.sat_type }), offset_x, offset_y, 255);
        }
    }

    canvas.set_draw_color(Color::RGB(60, 60, 60));
    let visible_height = (board.height - VISIBLE_ROW_OFFSET) as u32;
    for i in 0..=board.width {
        let x = offset_x + (i as u32 * CELL_SIZE) as i32;
        canvas.draw_line(Point::new(x, offset_y), Point::new(x, offset_y + board_h)).unwrap();
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as u32 * CELL_SIZE) as i32;
        canvas.draw_line(Point::new(offset_x, y), Point::new(offset_x + board_w, y)).unwrap();
    }

    if board.state == GameState::GameOver {
        let (win_w, win_h) = canvas.output_size().unwrap();
        let q_go = static_textures.label_game_over.query();
        let x_go = (win_w as i32 - q_go.width as i32)/2;
        let y_go = (win_h as i32 - q_go.height as i32)/2;
        canvas.copy(&static_textures.label_game_over, None, Rect::new(x_go, y_go, q_go.width, q_go.height)).unwrap();

        let q_res = static_textures.label_restart.query();
        let x_res = (win_w as i32 - q_res.width as i32)/2;
        let y_res = y_go + 50;
        canvas.copy(&static_textures.label_restart, None, Rect::new(x_res, y_res, q_res.width, q_res.height)).unwrap();
    }
}

fn draw_cell(canvas: &mut Canvas<Window>, row: i32, col: i32, puyo_type: Option<PuyoType>, dx: i32, dy: i32, alpha: u8) {
    if let Some(pt) = puyo_type {
        if row >= 0 {
            let mut rgba = get_puyo_color(pt);
            rgba.a = alpha;
            canvas.set_draw_color(rgba);
            canvas.fill_rect(Rect::new(
                dx + col * CELL_SIZE as i32 + 1,
                dy + row * CELL_SIZE as i32 + 1,
                CELL_SIZE - 2,
                CELL_SIZE - 2
            )).unwrap();
        }
    }
}

fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let ttf_context = sdl2::ttf::init().unwrap();

    let font_path = "arcadeFont.ttf";
    let font = match ttf_context.load_font(font_path, FONT_SIZE) {
        Ok(f) => f,
        Err(_) => {
            println!("Warning: Font not found, trying system font or failing.");
             match ttf_context.load_font("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", FONT_SIZE) {
                Ok(f) => f,
                Err(_) => panic!("Failed to load any font"),
            }
        }
    };

    let window = video_subsystem.window("Puyo Rust - Pure MVC", 800, 640)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    canvas.set_blend_mode(BlendMode::Blend);

    let texture_creator = canvas.texture_creator();

    let surface_next = font.render("Next:").blended(Color::RGB(200, 200, 200)).unwrap();
    let tex_next = texture_creator.create_texture_from_surface(&surface_next).unwrap();

    let surface_go = font.render("GAME OVER").blended(Color::RGB(255, 0, 0)).unwrap();
    let tex_go = texture_creator.create_texture_from_surface(&surface_go).unwrap();

    let surface_res = font.render("Press R to Restart").blended(Color::RGB(255, 255, 255)).unwrap();
    let tex_res = texture_creator.create_texture_from_surface(&surface_res).unwrap();

    let game_textures = GameTextures {
        label_next: tex_next,
        label_game_over: tex_go,
        label_restart: tex_res,
    };

    let (win_w, win_h) = canvas.output_size().unwrap();
    let board_w = (GRID_WIDTH as u32 * CELL_SIZE) as i32;
    let board_h = ((GRID_HEIGHT - VISIBLE_ROW_OFFSET) as u32 * CELL_SIZE) as i32;
    let offset_x = (win_w as i32 - board_w) / 2;
    let offset_y = (win_h as i32 - board_h) / 2;
    let ui_x = offset_x + board_w + 20;
    let ui_y = offset_y;

    let mut score_display = TextCache::new(&texture_creator, &font, "Score: 0", Color::WHITE, ui_x, ui_y);
    let mut level_display = TextCache::new(&texture_creator, &font, "Level: 1", Color::YELLOW, ui_x, ui_y + 30);
    let mut chain_display = TextCache::new(&texture_creator, &font, "Chain: 0", Color::GREEN, ui_x, ui_y + 240);

    let mut event_pump = sdl_context.event_pump().unwrap();

    let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT);
    board.spawn_piece();

    let mut last_time = Instant::now();
    let mut last_fall_time = Instant::now();
    let mut last_resolve_time = Instant::now();
    let start_time = Instant::now(); 

    let base_fall_interval = 800;
    let resolve_interval = Duration::from_millis(150);

    let mut key_timer_left: u128 = 0;
    let mut key_timer_right: u128 = 0;
    let mut key_timer_down: u128 = 0;

    'running: loop {
        let now = Instant::now();
        let delta = now.duration_since(last_time);
        last_time = now;
        let delta_millis = delta.as_millis();

        let seconds_played = start_time.elapsed().as_secs();
        let level = 1 + (seconds_played / 15);
        
        let speed_decrease = (level - 1) * 50;
        let current_interval_ms = if speed_decrease >= (base_fall_interval - 100) {
            100
        } else {
            base_fall_interval - speed_decrease
        };
        let fall_interval = Duration::from_millis(current_interval_ms);

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => break 'running,
                Event::KeyDown { keycode: Some(key), .. } => {
                    if key == Keycode::R && board.state == GameState::GameOver {
                        board = Board::new(GRID_WIDTH, GRID_HEIGHT);
                        board.spawn_piece();
                        last_fall_time = Instant::now();
                        continue;
                    }

                    if board.state == GameState::Playing {
                        match key {
                            Keycode::Up | Keycode::Z => board.rotate_piece(1),
                            Keycode::X | Keycode::W => board.rotate_piece(3),
                            Keycode::Space | Keycode::Return => {
                                board.hard_drop();
                                last_fall_time = Instant::now();
                            },
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if board.state == GameState::Playing {
            let state = event_pump.keyboard_state();
            let mut handle_das = |pressed: bool, timer: &mut u128, dir: i32| {
                if pressed {
                    if *timer == 0 {
                        board.move_piece(dir);
                        *timer = 1;
                    } else {
                        *timer += delta_millis;
                        if *timer > DAS_DELAY {
                            while *timer > DAS_DELAY + DAS_SPEED {
                                board.move_piece(dir);
                                *timer -= DAS_SPEED;
                            }
                        }
                    }
                } else {
                    *timer = 0;
                }
            };

            handle_das(state.is_scancode_pressed(Scancode::Left), &mut key_timer_left, -1);
            handle_das(state.is_scancode_pressed(Scancode::Right), &mut key_timer_right, 1);

            if state.is_scancode_pressed(Scancode::Down) {
                key_timer_down += delta_millis;
                if key_timer_down > SOFT_DROP_SPEED {
                     board.force_drop();
                     last_fall_time = Instant::now();
                     key_timer_down = 0;
                }
            } else {
                key_timer_down = 0;
            }
        }

        match board.state {
            GameState::Playing => {
                let locked = board.update_logic(delta);
                if locked {
                    last_fall_time = Instant::now();
                } else {
                    if !board.is_touching_ground {
                        if last_fall_time.elapsed() > fall_interval {
                            board.force_drop();
                            last_fall_time = Instant::now();
                        }
                    }
                }
            },
            GameState::ResolvingMatches => {
                if last_resolve_time.elapsed() > resolve_interval {
                    board.resolve_step();
                    last_resolve_time = Instant::now();
                }
            },
            GameState::GameOver => {}
        }

        canvas.set_draw_color(Color::RGB(20, 20, 20));
        canvas.clear();

        draw_board(&board, &mut canvas, &game_textures, offset_x, offset_y, board_w, board_h);

        score_display.update(&texture_creator, &font, &format!("Score: {}", board.score), Color::WHITE);
        score_display.draw(&mut canvas);

        level_display.update(&texture_creator, &font, &format!("Level: {}", level), Color::YELLOW);
        level_display.draw(&mut canvas);
        
        let q_next = &game_textures.label_next.query();
        canvas.copy(&game_textures.label_next, None, Rect::new(ui_x, ui_y + 70, q_next.width, q_next.height)).unwrap();

        canvas.set_draw_color(Color::RGB(40, 40, 40));
        canvas.fill_rect(Rect::new(ui_x, ui_y + 105, CELL_SIZE, CELL_SIZE * 2 + 5)).unwrap();
        draw_cell(&mut canvas, 0, 0, Some(board.next_types.1), ui_x, ui_y + 105, 255);
        draw_cell(&mut canvas, 1, 0, Some(board.next_types.0), ui_x, ui_y + 105, 255);

        let nn_y_start = ui_y + 210;
        let nn_scale = 0.5;
        let nn_size = (CELL_SIZE as f32 * nn_scale) as u32;
        
        canvas.set_draw_color(get_puyo_color(board.next_next_types.1));
        canvas.fill_rect(Rect::new(ui_x + 5, nn_y_start, nn_size, nn_size)).unwrap();
        canvas.set_draw_color(get_puyo_color(board.next_next_types.0));
        canvas.fill_rect(Rect::new(ui_x + 5, nn_y_start + nn_size as i32, nn_size, nn_size)).unwrap();

        if board.chain_count > 1 {
            chain_display.update(&texture_creator, &font, &format!("Chain: {}", board.chain_count), Color::GREEN);
            chain_display.draw(&mut canvas);
        }

        if board.is_touching_ground && board.state == GameState::Playing {
            let ratio_std = 1.0 - (board.lock_timer.as_millis() as f32 / MAX_LOCK_TIME as f32);
            let ratio_hard = 1.0 - (board.total_ground_timer.as_millis() as f32 / MAX_TOTAL_GROUND_TIME as f32);
            
            let ratio = ratio_std.min(ratio_hard);
            let bar_width = (100.0 * ratio).max(0.0) as u32;

            if board.total_ground_timer.as_millis() > 1500 {
                canvas.set_draw_color(Color::RGB(255, 0, 0)); 
            } else if board.ground_move_count > 10 {
                canvas.set_draw_color(Color::RGB(255, 100, 0));
            } else {
                canvas.set_draw_color(Color::RGB(255, 165, 0));
            }
            canvas.fill_rect(Rect::new(ui_x, nn_y_start + 150, bar_width, 10)).unwrap();
        }

        canvas.present();
    }
}
