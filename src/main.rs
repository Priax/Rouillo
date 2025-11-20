extern crate sdl2;
extern crate rand;

use std::time::{Duration, Instant};
use std::collections::HashSet;

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::{Point, Rect};
use sdl2::render::Canvas;
use sdl2::video::Window;
use sdl2::ttf::Font;
use rand::Rng;

const CELL_SIZE: u32 = 40;
const GRID_WIDTH: usize = 6;
const GRID_HEIGHT: usize = 12;
const FONT_SIZE: u16 = 24;
const MAX_LOCK_TIME: u64 = 500; 

#[derive(Clone, Copy, PartialEq, Debug)]
enum Puyo {
    Red,
    Blue,
    Yellow,
    Green,
}

impl Puyo {
    fn to_sdl_color(&self) -> Color {
        match self {
            Puyo::Red => Color::RGB(230, 50, 50),
            Puyo::Blue => Color::RGB(50, 50, 230),
            Puyo::Yellow => Color::RGB(230, 230, 50),
            Puyo::Green => Color::RGB(50, 200, 50),
        }
    }
    
    fn random() -> Puyo {
        let mut rng = rand::rng();
        let colors = [Puyo::Red, Puyo::Blue, Puyo::Yellow, Puyo::Green];
        colors[rng.random_range(0..colors.len())]
    }
}

struct ActivePuyo {
    row: i32,
    col: i32,
    rotation: usize,
    axis_color: Puyo,
    sat_color: Puyo,
}

impl ActivePuyo {
    fn get_positions(&self) -> [(i32, i32); 2] {
        let (dr, dc) = match self.rotation {
            0 => (-1, 0),
            1 => (0, 1),
            2 => (1, 0),
            3 => (0, -1),
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
    cells: Vec<Vec<Option<Puyo>>>,
    active_piece: Option<ActivePuyo>,
    next_colors: (Puyo, Puyo), 
    score: i32,
    state: GameState,
    lock_timer: Duration,
    is_touching_ground: bool,
}

impl Board {
    fn new(width: usize, height: usize) -> Board {
        Board {
            width,
            height,
            cells: vec![vec![None; width]; height],
            active_piece: None,
            next_colors: (Puyo::random(), Puyo::random()), 
            score: 0,
            state: GameState::Playing,
            lock_timer: Duration::from_millis(0),
            is_touching_ground: false,
        }
    }

    fn spawn_piece(&mut self) {
        let (c1, c2) = self.next_colors;
        
        self.next_colors = (Puyo::random(), Puyo::random());

        let new_piece = ActivePuyo {
            row: 1, 
            col: 2, 
            rotation: 0, 
            axis_color: c1,
            sat_color: c2,
        };

        if self.check_collision(&new_piece) {
            self.state = GameState::GameOver;
        } else {
            self.active_piece = Some(new_piece);
            self.lock_timer = Duration::from_millis(0);
            self.is_touching_ground = false;
        }
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
            self.lock_timer = Duration::from_millis(0);
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

    fn rotate_piece(&mut self) {
        if let Some(mut piece) = self.active_piece.take() {
            let old_rot = piece.rotation;
            piece.rotation = (piece.rotation + 1) % 4;
            
            if self.check_collision(&piece) {
                let old_col = piece.col;
                piece.col -= 1;
                if !self.check_collision(&piece) { 
                    self.active_piece = Some(piece); 
                    self.reset_lock_if_needed();
                    return; 
                }
                piece.col = old_col + 1;
                if !self.check_collision(&piece) { 
                    self.active_piece = Some(piece); 
                    self.reset_lock_if_needed();
                    return; 
                }
                piece.col = old_col;
                piece.rotation = old_rot;
            } else {
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
            piece.row += 1;
            let collision = self.check_collision(&piece);
            piece.row -= 1;

            if collision {
                self.is_touching_ground = true;
                self.lock_timer += delta_time;
                if self.lock_timer.as_millis() as u64 > MAX_LOCK_TIME {
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
            for (r, _) in positions.iter() {
                if *r < 0 {
                    self.state = GameState::GameOver;
                    return;
                }
            }
            for (r, c) in positions.iter() {
                if *r >= 0 && *r < self.height as i32 && *c >= 0 && *c < self.width as i32 {
                    let color = if *r == piece.row && *c == piece.col { piece.axis_color } else { piece.sat_color };
                    self.cells[*r as usize][*c as usize] = Some(color);
                }
            }
        }
        self.state = GameState::ResolvingMatches;
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

        for r in 0..self.height {
            for c in 0..self.width {
                if let Some(color) = self.cells[r][c] {
                    if !visited.contains(&(r, c)) {
                        let mut group = Vec::new();
                        self.flood_fill(r, c, color, &mut group, &mut visited);
                        if group.len() >= 4 {
                            for pos in group {
                                to_remove.insert(pos);
                            }
                        }
                    }
                }
            }
        }

        if to_remove.is_empty() { return false; }

        self.score += (to_remove.len() * 100) as i32;
        for (r, c) in to_remove {
            self.cells[r][c] = None;
        }
        true
    }

    fn flood_fill(&self, r: usize, c: usize, color: Puyo, group: &mut Vec<(usize, usize)>, visited: &mut HashSet<(usize, usize)>) {
        if visited.contains(&(r, c)) { return; }
        if let Some(cell_color) = self.cells[r][c] {
            if cell_color == color {
                visited.insert((r, c));
                group.push((r, c));
                if r > 0 { self.flood_fill(r - 1, c, color, group, visited); }
                if r < self.height - 1 { self.flood_fill(r + 1, c, color, group, visited); }
                if c > 0 { self.flood_fill(r, c - 1, color, group, visited); }
                if c < self.width - 1 { self.flood_fill(r, c + 1, color, group, visited); }
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

    fn draw(&self, canvas: &mut Canvas<Window>, font: &Font) {
        canvas.set_draw_color(Color::RGB(20, 20, 20));
        canvas.clear();

        let (win_w, win_h) = canvas.output_size().unwrap();
        let board_w = (self.width as u32 * CELL_SIZE) as i32;
        let board_h = (self.height as u32 * CELL_SIZE) as i32;
        let offset_x = (win_w as i32 - board_w) / 2;
        let offset_y = (win_h as i32 - board_h) / 2;

        canvas.set_draw_color(Color::RGB(30, 30, 30));
        canvas.fill_rect(Rect::new(offset_x, offset_y, board_w as u32, board_h as u32)).unwrap();

        for r in 0..self.height {
            for c in 0..self.width {
                self.draw_cell(canvas, r as i32, c as i32, self.cells[r][c], offset_x, offset_y);
            }
        }

        if let Some(ref piece) = self.active_piece {
            let positions = piece.get_positions();
            self.draw_cell(canvas, positions[0].0, positions[0].1, Some(piece.axis_color), offset_x, offset_y);
            self.draw_cell(canvas, positions[1].0, positions[1].1, Some(piece.sat_color), offset_x, offset_y);
        }

        canvas.set_draw_color(Color::RGB(60, 60, 60));
        for i in 0..=self.width {
            let x = offset_x + (i as u32 * CELL_SIZE) as i32;
            canvas.draw_line(Point::new(x, offset_y), Point::new(x, offset_y + board_h)).unwrap();
        }
        for i in 0..=self.height {
            let y = offset_y + (i as u32 * CELL_SIZE) as i32;
            canvas.draw_line(Point::new(offset_x, y), Point::new(offset_x + board_w, y)).unwrap();
        }
        
        let ui_x = offset_x + board_w + 20;
        let ui_y = offset_y;

        let score_text = format!("Score: {}", self.score);
        let surface = font.render(&score_text).blended(Color::WHITE).unwrap();
        let texture_creator = canvas.texture_creator();
        let texture = texture_creator.create_texture_from_surface(&surface).unwrap();
        let (w, h) = surface.size();
        canvas.copy(&texture, None, Rect::new(ui_x, ui_y, w, h)).unwrap();

        let next_text_surface = font.render("Next:").blended(Color::RGB(200, 200, 200)).unwrap();
        let next_tex = texture_creator.create_texture_from_surface(&next_text_surface).unwrap();
        let (nw, nh) = next_text_surface.size();
        canvas.copy(&next_tex, None, Rect::new(ui_x, ui_y + 40, nw, nh)).unwrap();

        canvas.set_draw_color(Color::RGB(40, 40, 40));
        canvas.fill_rect(Rect::new(ui_x, ui_y + 75, CELL_SIZE, CELL_SIZE * 2 + 5)).unwrap();

        self.draw_cell(canvas, 0, 0, Some(self.next_colors.1), ui_x, ui_y + 75);
        self.draw_cell(canvas, 1, 0, Some(self.next_colors.0), ui_x, ui_y + 75);

        if self.is_touching_ground && self.state == GameState::Playing {
            let ratio = 1.0 - (self.lock_timer.as_millis() as f32 / MAX_LOCK_TIME as f32);
            let bar_width = (100.0 * ratio) as u32;
            canvas.set_draw_color(Color::RGB(255, 165, 0));
            canvas.fill_rect(Rect::new(ui_x, ui_y + 180, bar_width, 10)).unwrap();
        }

        if self.state == GameState::GameOver {
             let go_surface = font.render("GAME OVER").blended(Color::RGB(255, 0, 0)).unwrap();
             let go_tex = texture_creator.create_texture_from_surface(&go_surface).unwrap();
             let (gw, gh) = go_surface.size();
             canvas.copy(&go_tex, None, Rect::new((win_w as i32 - gw as i32)/2, (win_h as i32 - gh as i32)/2, gw, gh)).unwrap();
        }
    }

    fn draw_cell(&self, canvas: &mut Canvas<Window>, row: i32, col: i32, color: Option<Puyo>, dx: i32, dy: i32) {
        if let Some(c) = color {
            if row >= -2 {
                canvas.set_draw_color(c.to_sdl_color());
                canvas.fill_rect(Rect::new(
                    dx + col * CELL_SIZE as i32 + 1,
                    dy + row * CELL_SIZE as i32 + 1,
                    CELL_SIZE - 2,
                    CELL_SIZE - 2
                )).unwrap();
            }
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
            eprintln!("Erreur: Police introuvable.");
            return;
        }
    };

    let window = video_subsystem.window("Puyo Rust", 800, 600)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    let mut event_pump = sdl_context.event_pump().unwrap();

    let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT);
    board.spawn_piece();

    let mut last_time = Instant::now();
    let mut last_fall_time = Instant::now();
    let mut last_resolve_time = Instant::now();
    
    let fall_interval = Duration::from_millis(600);
    let resolve_interval = Duration::from_millis(200);

    'running: loop {
        let now = Instant::now();
        let delta = now.duration_since(last_time);
        last_time = now;

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => break 'running,
                Event::KeyDown { keycode: Some(key), .. } => {
                    if board.state == GameState::Playing {
                        match key {
                            Keycode::Left | Keycode::Q => board.move_piece(-1),
                            Keycode::Right | Keycode::D => board.move_piece(1),
                            Keycode::Down | Keycode::S => { 
                                board.force_drop(); 
                                last_fall_time = Instant::now(); 
                            },
                            Keycode::Space | Keycode::Return => {
                                board.hard_drop();
                                last_fall_time = Instant::now();
                            },
                            Keycode::Up | Keycode::Z => board.rotate_piece(),
                            _ => {}
                        }
                    } else if board.state == GameState::GameOver {
                        if key == Keycode::R {
                            board = Board::new(GRID_WIDTH, GRID_HEIGHT);
                            board.spawn_piece();
                        }
                    }
                }
                _ => {}
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

        board.draw(&mut canvas, &font);
        canvas.present();
    }
}
