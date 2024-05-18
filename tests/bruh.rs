#![allow(dead_code)]
use std::fmt;

#[derive(Debug, Clone, Copy)]
enum PuyoColor {
    Red,
    Blue,
    Green,
    Yellow,
}

#[derive(Debug, Clone, Copy)]
struct Puyo {
    color: PuyoColor,
}

impl fmt::Display for Puyo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = match self.color {
            PuyoColor::Red => 'R',
            PuyoColor::Blue => 'B',
            PuyoColor::Green => 'G',
            PuyoColor::Yellow => 'Y',
        };
        write!(f, "{}", symbol)
    }
}

impl fmt::Display for PuyoPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.puyo1, self.puyo2)
    }
}

#[derive(Debug, Clone, Copy)]
struct PuyoPair {
    puyo1: Puyo,
    puyo2: Puyo,
}

struct Board {
    width: usize,
    height: usize,
    cells: Vec<Vec<Option<PuyoPair>>>, // Vecteur 2D de cellules contenant des paires de Puyo
}

impl Board {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![vec![None; width]; height],
        }
    }

    fn add_puyo_pair(&mut self, row: usize, col: usize, puyo_pair: PuyoPair) {
        if row < self.height && col < self.width {
            self.cells[row][col] = Some(puyo_pair);
        }
    }

    fn display(&self) {
        for row in 0..self.height {
            print!("|");
            for col in 0..self.width {
                if let Some(puyo_pair) = self.cells[row][col] {
                    print!("{}", puyo_pair);
                } else {
                    print!("  ");
                }
                print!("|");
            }
            println!();
        }
    }
}

fn main() {
    let mut board = Board::new(6, 12);

    let puyo_pair = PuyoPair {
        puyo1: Puyo { color: PuyoColor::Red },
        puyo2: Puyo { color: PuyoColor::Yellow },
    };
    board.add_puyo_pair(0, 1, PuyoPair { puyo1: Puyo { color: (PuyoColor::Yellow) }, puyo2: Puyo { color: (PuyoColor::Red) }});
    board.add_puyo_pair(0, 2, puyo_pair);
    board.display();
}
