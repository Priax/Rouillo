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
    current_falling_puyo: Option<(usize, usize, MyColor)>,
    score: i32,
}

#[derive(Clone, PartialEq)]
#[allow(dead_code)]
enum MyColor {
    Red,
    Blue,
    Yellow,
    Green,
}

#[allow(dead_code)]
impl Board {
    fn new(width: usize, height: usize) -> Board {
        Board {
            width,
            height,
            cells: vec![vec![None; width]; height],
            current_falling_puyo: None,
            score: 0,
        }
    }

    //? Unused
    fn add_puyos(&mut self, column: usize, my_color1: MyColor, my_color2: MyColor) {
        let mut empty_row = self.height;

        for row in 0..self.height {
            if self.cells[row][column].is_none() {
                empty_row = row;
            }
        }
        self.cells[empty_row][column] = Some(my_color1);
        self.cells[empty_row - 1][column] = Some(my_color2);
    }

    fn add_puyos_from_top(&mut self, column: usize, my_color1: MyColor, my_color2: MyColor) {
        let mut row = 0;

        while row < self.height && self.cells[row][column].is_some() {
            row += 1;
        }

        if row < self.height {
            self.cells[row][column] = Some(my_color1);
            self.cells[row + 1][column] = Some(my_color2);
        }
    }

    //? Unused
    fn add_puyo(&mut self, column: usize, my_color: MyColor) {
        for row in (0..self.height).rev() {
            if self.cells[row][column].is_none() {
                self.cells[row][column] = Some(my_color);
                break;
            }
        }
    }

    //? Unused
    fn display(&self) {
        println!("Score: {}", self.score);
        for row in &self.cells {
            for cell in row {
                match cell {
                    Some(my_color) => print!("{} ", my_color_to_char(my_color.clone())),
                    None => print!("- "),
                }
            }
            println!();
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
    fn check_matches(&mut self) {
        let mut visited: HashSet<(usize, usize)> = HashSet::new();

        for row in 0..self.height {
            for col in 0..self.width {
                if let Some(my_color) = &self.cells[row][col] {
                    if !visited.contains(&(row, col)) {
                        let mut group: HashSet<(usize, usize)> = HashSet::new();
                        self.find_connected(row, col, my_color, &mut group, &mut visited);
                        if group.len() >= 4 {
                            for (r, c) in group {
                                self.cells[r][c] = None;
                                self.score += 100;
                            }
                        }
                    }
                }
            }
        }

        self.apply_gravity();
    }

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
            let mut empty_row_count = 0;
            for row in (0..self.height).rev() {
                if self.cells[row][col].is_none() {
                    empty_row_count += 1;
                } else if empty_row_count > 0 {
                    let my_color = self.cells[row][col].clone();
                    self.cells[row][col] = None;
                    self.cells[row + empty_row_count][col] = my_color;
                }
            }
        }
    }
}

impl Board {
    fn tick_gravity(&mut self) -> bool {
        let mut any_falling = false;

        for col in 0..self.width {
            for row in (0..self.height - 1).rev() {
                if let Some(my_color) = {
                    let cell = &self.cells[row][col];
                    cell.as_ref().cloned()
                } {
                    if self.cells[row + 1][col].is_none() {
                        self.cells[row][col] = None;
                        self.cells[row + 1][col] = Some(my_color);
                        any_falling = true;
                    }
                }
            }
        }
        any_falling
    }

    fn go_left(&mut self) -> bool {
        let mut border = false;

        for row in 0..self.height {
            for col in 1..self.width {
                if let Some(my_color) = {
                    let cell = &self.cells[row][col];
                    cell.as_ref().cloned()
                } {
                    if self.cells[row][col - 1].is_none() {
                        self.cells[row][col] = None;
                        self.cells[row][col - 1] = Some(my_color);
                        border = true;
                    }
                }
            }
        }
        border
    }

    fn go_right(&mut self) -> bool {
        let mut border = false;

        for row in 0..self.height {
            for col in (0..self.width - 1).rev() {
                if let Some(my_color) = {
                    let cell = &self.cells[row][col];
                    cell.as_ref().cloned()
                } {
                    if self.cells[row][col + 1].is_none() {
                        self.cells[row][col] = None;
                        self.cells[row][col + 1] = Some(my_color);
                        border = true;
                    }
                }
            }
        }
        border
    }

}

fn my_color_to_char(my_color: MyColor) -> char {
    match my_color {
        MyColor::Red => 'R',
        MyColor::Blue => 'B',
        MyColor::Yellow => 'Y',
        MyColor::Green => 'G',
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
        .window("PicTekChat", 600, 600)
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

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} => break 'running,
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => break 'running,
                Event::KeyDown { keycode: Some(Keycode::Q), .. } => {
                    let _ = board.go_left();
                },
                Event::KeyDown { keycode: Some(Keycode::D), .. } => {
                    let _ = board.go_right();
                },
                _ => {}
            }
        }

        let falling = board.tick_gravity();extern crate sdl2;

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
        }

        #[derive(Clone, PartialEq)]
        #[allow(dead_code)]
        enum MyColor {
            Red,
            Blue,
            Yellow,
            Green,
        }

        #[allow(dead_code)]
        impl Board {
            fn new(width: usize, height: usize) -> Board {
                Board {
                    width,
                    height,
                    cells: vec![vec![None; width]; height],
                    score: 0,
                }
            }

            //? Unused
            fn add_puyos(&mut self, column: usize, my_color1: MyColor, my_color2: MyColor) {
                let mut empty_row = self.height;

                for row in 0..self.height {
                    if self.cells[row][column].is_none() {
                        empty_row = row;
                    }
                }
                self.cells[empty_row][column] = Some(my_color1);
                self.cells[empty_row - 1][column] = Some(my_color2);
            }

            fn add_puyos_from_top(&mut self, column: usize, my_color1: MyColor, my_color2: MyColor) {
                let mut row = 0;

                while row < self.height && self.cells[row][column].is_some() {
                    row += 1;
                }

                if row < self.height {
                    self.cells[row][column] = Some(my_color1);
                    self.cells[row + 1][column] = Some(my_color2);
                }
            }

            //? Unused
            fn add_puyo(&mut self, column: usize, my_color: MyColor) {
                for row in (0..self.height).rev() {
                    if self.cells[row][column].is_none() {
                        self.cells[row][column] = Some(my_color);
                        break;
                    }
                }
            }

            //? Unused
            fn display(&self) {
                println!("Score: {}", self.score);
                for row in &self.cells {
                    for cell in row {
                        match cell {
                            Some(my_color) => print!("{} ", my_color_to_char(my_color.clone())),
                            None => print!("- "),
                        }
                    }
                    println!();
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
            fn check_matches(&mut self) {
                let mut visited: HashSet<(usize, usize)> = HashSet::new();

                for row in 0..self.height {
                    for col in 0..self.width {
                        if let Some(my_color) = &self.cells[row][col] {
                            if !visited.contains(&(row, col)) {
                                let mut group: HashSet<(usize, usize)> = HashSet::new();
                                self.find_connected(row, col, my_color, &mut group, &mut visited);
                                if group.len() >= 4 {
                                    for (r, c) in group {
                                        self.cells[r][c] = None;
                                        self.score += 100;
                                    }
                                }
                            }
                        }
                    }
                }

                self.apply_gravity();
            }

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
                    let mut empty_row_count = 0;
                    for row in (0..self.height).rev() {
                        if self.cells[row][col].is_none() {
                            empty_row_count += 1;
                        } else if empty_row_count > 0 {
                            let my_color = self.cells[row][col].clone();
                            self.cells[row][col] = None;
                            self.cells[row + empty_row_count][col] = my_color;
                        }
                    }
                }
            }
        }

        impl Board {
            fn tick_gravity(&mut self) -> bool {
                let mut any_falling = false;

                for col in 0..self.width {
                    for row in (0..self.height - 1).rev() {
                        if let Some(my_color) = {
                            let cell = &self.cells[row][col];
                            cell.as_ref().cloned()
                        } {
                            if self.cells[row + 1][col].is_none() {
                                self.cells[row][col] = None;
                                self.cells[row + 1][col] = Some(my_color);
                                any_falling = true;
                            }
                        }
                    }
                }
                any_falling
            }

            fn go_left(&mut self) -> bool {
                let mut border = false;

                for row in 0..self.height {
                    for col in 1..self.width {
                        if let Some(my_color) = {
                            let cell = &self.cells[row][col];
                            cell.as_ref().cloned()
                        } {
                            if self.cells[row][col - 1].is_none() {
                                self.cells[row][col] = None;
                                self.cells[row][col - 1] = Some(my_color);
                                border = true;
                            }
                        }
                    }
                }
                border
            }

            fn go_right(&mut self) -> bool {
                let mut border = false;

                for row in 0..self.height {
                    for col in (0..self.width - 1).rev() {
                        if let Some(my_color) = {
                            let cell = &self.cells[row][col];
                            cell.as_ref().cloned()
                        } {
                            if self.cells[row][col + 1].is_none() {
                                self.cells[row][col] = None;
                                self.cells[row][col + 1] = Some(my_color);
                                border = true;
                            }
                        }
                    }
                }
                border
            }

        }

        fn my_color_to_char(my_color: MyColor) -> char {
            match my_color {
                MyColor::Red => 'R',
                MyColor::Blue => 'B',
                MyColor::Yellow => 'Y',
                MyColor::Green => 'G',
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
                .window("PicTekChat", 600, 600)
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

            'running: loop {
                for event in event_pump.poll_iter() {
                    match event {
                        Event::Quit {..} => break 'running,
                        Event::KeyDown { keycode: Some(Keycode::Escape), .. } => break 'running,
                        Event::KeyDown { keycode: Some(Keycode::Q), .. } => board.go_left(),
                        Event::KeyDown { keycode: Some(Keycode::D), .. } => board.go_right(),
                        _ => {}
                    }
                }

                let falling = board.tick_gravity();
                if !falling {
                    board.check_matches();
                    rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    let color1 = &colors[(rng as usize) % colors.len()];
                    let color2 = &colors[((rng >> 16) as usize) % colors.len()];
                    board.add_puyos_from_top(3, color1.clone(), color2.clone());
                }

                canvas.set_draw_color(Color::RGB(0, 0, 0));
                canvas.clear();
                board.display_sdl2(&mut canvas, &font, 600, 600);
                canvas.present();
                std::thread::sleep(Duration::from_millis(500));
            }
        }

        if !falling {
            board.check_matches();
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let color1 = &colors[(rng as usize) % colors.len()];
            let color2 = &colors[((rng >> 16) as usize) % colors.len()];
            board.add_puyos_from_top(3, color1.clone(), color2.clone());
        }

        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        board.display_sdl2(&mut canvas, &font, 600, 600);
        canvas.present();
        std::thread::sleep(Duration::from_millis(500));
    }
}