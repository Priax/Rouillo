#![allow(dead_code)]

use std::fmt;
use std::io::{Write, stdout};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq)]
enum PuyoColor {
    Red,
    Blue,
    Green,
    Yellow,
    // Ajoutez plus de couleurs au besoin
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Rotation {
    Clockwise,
    CounterClockwise,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct Puyo {
    color: PuyoColor,
}

#[derive(Debug)]
struct Board {
    width: usize,
    height: usize,
    cells: Vec<Vec<Option<Puyo>>>,
}

impl Board {
    fn new(width: usize, height: usize) -> Self {
        Board {
            width,
            height,
            cells: vec![vec![None; width]; height],
        }
    }

    fn add_puyo(&mut self, column: usize, puyo: Puyo) -> Result<(), &'static str> {
        if column >= self.width {
            return Err("Column out of bounds");
        }
        for row in 0..self.height {
            if self.cells[row][column].is_none() {
                self.cells[row][column] = Some(puyo);
                return Ok(());
            }
        }
        Err("Column is full")
    }

    fn add_random_puyos(&mut self, column: usize) -> Result<(), &'static str> {
        if column >= self.width {
            return Err("Column out of bounds");
        }

        let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let mut rng_seed = current_time.as_secs() as u32;

        let color1 = match rng_seed % 4 {
            0 => PuyoColor::Red,
            1 => PuyoColor::Blue,
            2 => PuyoColor::Green,
            _ => PuyoColor::Yellow,
        };
        rng_seed += 1; // Augmente la graine pour obtenir une couleur différente
        let color2 = match rng_seed % 4 {
            0 => PuyoColor::Red,
            1 => PuyoColor::Blue,
            2 => PuyoColor::Green,
            _ => PuyoColor::Yellow,
        };

        // Ajoute les puyos à la colonne spécifiée
        self.add_puyo(column, Puyo { color: color1 })?;
        self.add_puyo(column, Puyo { color: color2 })?;

        Ok(())
    }

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
    /*fn move_puyo(&mut self, column: usize, direction: Direction) -> Result<(), &'static str> {
        // Implementation du déplacement gauche/droite
    }

    fn rotate_puyo(&mut self, column: usize, rotation: Rotation) -> Result<(), &'static str> {
        // Implementation de la rotation
    }*/
    }

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in &self.cells {
            for cell in row {
                match cell {
                    Some(puyo) => write!(f, "{}", match puyo.color {
                        PuyoColor::Red => "R",
                        PuyoColor::Blue => "B",
                        PuyoColor::Green => "G",
                        PuyoColor::Yellow => "Y",
                    })?,
                    None => write!(f, "-")?,
                }
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

fn main() {
    let mut board = Board::new(6, 12);
    let _ = board.add_random_puyos(3);
    loop {
        print!("\x1B[2J\x1B[1;1H"); // Clear the terminal
        stdout().flush().unwrap(); // Flush the terminal
        println!("{}", board);
        std::thread::sleep(Duration::from_millis(500)); // Introduce a delay of 500 milliseconds
        let _ = board.tick_gravity();
    }
}
