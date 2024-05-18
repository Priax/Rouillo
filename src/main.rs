extern crate sdl2;

use std::time::{Duration, SystemTime};
use std::collections::HashSet;

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::ttf::Font;

use sdl2::rect::Point;

const CELL_SIZE: usize = 32;
const FONT_SIZE: u16 = 16;
const SCORE_HEIGHT: i32 = 16;
const SCORE_WIDTH: i32 = 16;

struct Board {
    width: usize,
    height: usize,
    cells: Vec<Vec<Option<MyColor>>>,
    score: i32,
    active_puyos: Option<ActivePuyo>,
}

#[derive(Clone, PartialEq)]
#[allow(dead_code)]
enum MyColor {
    Red,
    Blue,
    Yellow,
    Green,
}

#[derive(Clone)]
struct ActivePuyo {
    positions: [(usize, usize); 2],
    colors: [MyColor; 2],
}

impl Board {
    fn new(width: usize, height: usize) -> Board {
        Board {
            width,
            height,
            cells: vec![vec![None; width]; height],
            score: 0,
            active_puyos: None,
        }
    }

    fn add_puyos_from_top(&mut self, column: usize, my_color1: MyColor, my_color2: MyColor) -> bool {
        let mut row = 0;

        while row < self.height && self.cells[row][column].is_some() {
            row += 1;
        }

        if row < self.height - 1 {
            self.cells[row][column] = Some(my_color1.clone());
            self.cells[row + 1][column] = Some(my_color2.clone());
            self.active_puyos = Some(ActivePuyo {
                positions: [(row, column), (row + 1, column)],
                colors: [my_color1, my_color2],
            });
        } else {
            print!("Temporary gameover\n");
            return false;
        }
        return true;
    }

    fn move_active_puyos(&mut self, direction: i32) {
        if let Some(ref mut active) = self.active_puyos {
            let mut new_positions = [(0, 0); 2];
            for (i, &(row, col)) in active.positions.iter().enumerate() {
                let new_col = (col as i32 + direction) as usize;
                if new_col < self.width && self.cells[row][new_col].is_none() {
                    new_positions[i] = (row, new_col);
                } else {
                    return;
                }
            }
            for &(row, col) in &active.positions {
                self.cells[row][col] = None;
            }
            for (i, &(row, col)) in new_positions.iter().enumerate() {
                self.cells[row][col] = Some(active.colors[i].clone());
            }
            active.positions = new_positions;
        }
    }
    
    // La garder pour plus tard
    /*fn drop_active_puyos(&mut self) -> bool {
        if let Some(ref mut active) = self.active_puyos {
            let mut can_fall = true;
            let mut any_fallen = false;
            while can_fall {
                can_fall = false;
                let mut new_positions = [(0, 0); 2];
                for (i, &(row, col)) in active.positions.iter().enumerate() {
                    if row + 1 < self.height && self.cells[row + 1][col].is_none() {
                        new_positions[i] = (row + 1, col);
                        can_fall = true;
                    } else {
                        new_positions[i] = (row, col);
                    }
                }
                if can_fall {
                    for &(row, col) in &active.positions {
                        self.cells[row][col] = None;
                    }
                    for (i, &(row, col)) in new_positions.iter().enumerate() {
                        self.cells[row][col] = Some(active.colors[i].clone());
                    }
                    active.positions = new_positions;
                    any_fallen = true;
                }
            }
            return any_fallen;
        }
        false
    }*/

    fn find_connected(&self, row: usize, col: usize, my_color: &MyColor, group: &mut HashSet<(usize, usize)>, visited: &mut HashSet<(usize, usize)>) {
        if visited.contains(&(row, col)) {
            return;
        }

        if let Some(curr_my_color) = &self.cells[row][col] {
            if curr_my_color == my_color {
                visited.insert((row, col));
                group.insert((row, col));

                if row + 1 < self.height {
                    self.find_connected(row + 1, col, my_color, group, visited);
                }
                if row > 0 {
                    self.find_connected(row - 1, col, my_color, group, visited);
                }
                if col + 1 < self.width {
                    self.find_connected(row, col + 1, my_color, group, visited);
                }
                if col > 0 {
                    self.find_connected(row, col - 1, my_color, group, visited);
                }
            }
        }
    }

    fn apply_gravity(&mut self) {
        for col in 0..self.width {
            let mut empty_row = self.height;
            for row in (0..self.height).rev() {
                if self.cells[row][col].is_none() && empty_row == self.height {
                    empty_row = row;
                } else if self.cells[row][col].is_some() && empty_row != self.height {
                    self.cells[empty_row][col] = self.cells[row][col].take();
                    empty_row -= 1;
                }
            }
        }
    }

    fn check_matches(&mut self) {
        let mut found_match = true;

        while found_match {
            found_match = false;
            let mut visited: HashSet<(usize, usize)> = HashSet::new();

            for row in 0..self.height {
                for col in 0..self.width {
                    if let Some(my_color) = &self.cells[row][col] {
                        if !visited.contains(&(row, col)) {
                            let mut group: HashSet<(usize, usize)> = HashSet::new();
                            self.find_connected(row, col, my_color, &mut group, &mut visited);
                            if group.len() >= 4 {
                                found_match = true;
                                for (r, c) in group {
                                    self.cells[r][c] = None;
                                    self.score += 100;
                                }
                            }
                        }
                    }
                }
            }

            if found_match {
                self.apply_gravity();
            }
        }
    }

        fn display_sdl2(&self, canvas: &mut sdl2::render::Canvas<sdl2::video::Window>, font: &Font, window_width: u32, window_height: u32) {
            canvas.set_draw_color(Color::RGB(0, 0, 0));
            canvas.clear();

            let board_width = self.width * CELL_SIZE as usize;
            let board_height = self.height * CELL_SIZE as usize;
            let board_x = ((window_width as usize - board_width) / 2) as i32;
            let board_y = ((window_height as usize - board_height) / 2) as i32;

            for (y, row) in self.cells.iter().enumerate() {
                for (x, cell) in row.iter().enumerate() {
                    let cell_x = board_x + (x * CELL_SIZE) as i32;
                    let cell_y = board_y + (y * CELL_SIZE) as i32;
                    match cell {
                        Some(my_color) => {
                            let color = match my_color {
                                MyColor::Red => Color::RGB(255, 0, 0),
                                MyColor::Blue => Color::RGB(0, 0, 255),
                                MyColor::Yellow => Color::RGB(255, 255, 0),
                                MyColor::Green => Color::RGB(0, 255, 0),
                            };
                            canvas.set_draw_color(color);
                            let _ = canvas.fill_rect(sdl2::rect::Rect::new(cell_x, cell_y, CELL_SIZE as u32, CELL_SIZE as u32));
                        }
                        None => {
                            canvas.set_draw_color(Color::RGB(255, 255, 255));
                            let _ = canvas.fill_rect(sdl2::rect::Rect::new(cell_x, cell_y, CELL_SIZE as u32, CELL_SIZE as u32));
                        }
                    }
                }
            }
        let score_text = format!("Score: {}", self.score);
        let score_position = Point::new(SCORE_WIDTH, SCORE_HEIGHT);
        render_text(canvas, &font, &score_text, score_position, Color::RGB(255, 255, 255));
    }
}

impl Board {
    fn step_fall_active_puyos(&mut self) -> bool {
        if let Some(ref mut active) = self.active_puyos {
            let mut can_fall = true;
            let mut new_positions = [(0, 0); 2];
            for (i, &(row, col)) in active.positions.iter().enumerate() {
                if row + 1 < self.height && self.cells[row + 1][col].is_none() {
                    new_positions[i] = (row + 1, col);
                    can_fall = true;
                } else {
                    new_positions[i] = (row + 1, col);
                    can_fall = false;
                }
            }
            if can_fall {
                for &(row, col) in &active.positions {
                    self.cells[row][col] = None;
                }
                for (i, &(row, col)) in new_positions.iter().enumerate() {
                    self.cells[row][col] = Some(active.colors[i].clone());
                }
                active.positions = new_positions;
                return true;
            }
        }
        false
    }
}

fn render_text(canvas: &mut sdl2::render::Canvas<sdl2::video::Window>, font: &sdl2::ttf::Font, text: &str, position: Point, color: sdl2::pixels::Color) {
    let surface = font.render(text)
        .blended(color)
        .map_err(|e| e.to_string())
        .unwrap();

    let texture_creator = canvas.texture_creator();
    let texture = texture_creator.create_texture_from_surface(&surface).unwrap();

    let (width, height) = surface.size();
    let dest_rect = sdl2::rect::Rect::new(position.x, position.y, width as u32, height as u32);

    canvas.copy(&texture, None, dest_rect).unwrap();
}

fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window("Puyo Puyo", 600, 600)
        .position_centered()
        .resizable()
        .opengl()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();
    let mut board = Board::new(6, 12);
    let mut rng = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_micros() as u64;

    let mut event_pump = sdl_context.event_pump().unwrap();

    let ttf_context = sdl2::ttf::init().unwrap();
    let font = ttf_context.load_font("./arcadeFont.ttf", FONT_SIZE).unwrap();

    let colors = [MyColor::Red, MyColor::Blue, MyColor::Yellow, MyColor::Green];

    let mut last_update = SystemTime::now();
    let update_interval = Duration::from_millis(250); // Le modifier selon le score

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} => break 'running,
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => break 'running,
                Event::KeyDown { keycode: Some(Keycode::Q), .. } => {
                    board.move_active_puyos(-1);
                },
                Event::KeyDown { keycode: Some(Keycode::D), .. } => {
                    board.move_active_puyos(1);
                },
                _ => {}
            }
        }

        if last_update.elapsed().unwrap() >= update_interval {
            last_update = SystemTime::now();
            let falling = board.step_fall_active_puyos();
            if !falling {
                board.check_matches();
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let color1 = &colors[(rng as usize) % colors.len()];
                let color2 = &colors[((rng >> 16) as usize) % colors.len()];
                if !board.add_puyos_from_top(3, color1.clone(), color2.clone()) {
                    break;
                }
            }

            canvas.set_draw_color(Color::RGB(0, 0, 0));
            canvas.clear();
            board.display_sdl2(&mut canvas, &font, 600, 600);
            canvas.present();
        }
    }
}
