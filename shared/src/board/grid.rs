use std::ops::{Index, IndexMut};

use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::{ActivePuyo, PuyoType};

/// The stack, row by row from the top, indexed by `(row, col)`.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Option<PuyoType>>,
}

/// A puyo that fell from `from` to `to` in column `col`.
#[derive(Clone, Copy)]
pub(super) struct Move {
    pub from: usize,
    pub col: usize,
    pub to: usize,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![None; width * height],
        }
    }

    /// A grid of these cells, row by row, if there are as many as its size
    /// says.
    pub fn from_cells(width: usize, height: usize, cells: Vec<Option<PuyoType>>) -> Option<Self> {
        let grid = Self { width, height, cells };
        grid.is_well_formed().then_some(grid)
    }

    /// Whether the cells fit the size: false only for one decoded from a
    /// malformed message.
    pub fn is_well_formed(&self) -> bool {
        self.width > 0 && self.width.checked_mul(self.height) == Some(self.cells.len())
    }

    pub fn as_slice(&self) -> &[Option<PuyoType>] {
        &self.cells
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    /// `(row, col)` as an index, if it is on the grid.
    pub fn checked(&self, r: i32, c: i32) -> Option<(usize, usize)> {
        let r = usize::try_from(r).ok().filter(|&r| r < self.height)?;
        let c = usize::try_from(c).ok().filter(|&c| c < self.width)?;
        Some((r, c))
    }

    pub fn rows(&self) -> std::slice::ChunksExact<'_, Option<PuyoType>> {
        self.cells.chunks_exact(self.width)
    }

    pub fn rows_mut(&mut self) -> std::slice::ChunksExactMut<'_, Option<PuyoType>> {
        self.cells.chunks_exact_mut(self.width)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Option<PuyoType>> {
        self.cells.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(Option::is_none)
    }

    /// How many cells of column `col` are taken, from the top of its stack
    /// down, gaps included.
    pub fn column_height(&self, col: usize) -> usize {
        self.height
            - (0..self.height)
                .find(|&r| self[(r, col)].is_some())
                .unwrap_or(self.height)
    }

    /// Whether `piece` leaves the grid or overlaps the stack.
    pub fn collides(&self, piece: &ActivePuyo) -> bool {
        piece
            .positions()
            .into_iter()
            .any(|(r, c)| self.checked(r, c).is_none_or(|at| self[at].is_some()))
    }

    /// Whether `piece` stands on the stack or the floor.
    pub fn rests(&self, piece: &ActivePuyo) -> bool {
        self.collides(&piece.shifted(1, 0))
    }

    pub(super) fn neighbours(&self, r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        [(-1, 0), (1, 0), (0, -1), (0, 1)]
            .into_iter()
            .filter_map(move |(dr, dc)| {
                let nr = r.checked_add_signed(dr)?;
                let nc = c.checked_add_signed(dc)?;
                (nr < self.height && nc < self.width).then_some((nr, nc))
            })
    }

    pub(super) const fn flat(&self, r: usize, c: usize) -> usize {
        r * self.width + c
    }

    /// The cells of the same colour connected to `(r, c)`, marked in `visited`.
    pub(super) fn group(&self, r: usize, c: usize, visited: &mut [bool]) -> Vec<(usize, usize)> {
        let colour = self[(r, c)];
        let mut group = Vec::new();
        let mut stack = vec![(r, c)];
        while let Some((r, c)) = stack.pop() {
            let at = self.flat(r, c);
            if visited[at] {
                continue;
            }
            visited[at] = true;
            group.push((r, c));
            stack.extend(self.neighbours(r, c).filter(|&n| self[n] == colour));
        }
        group
    }

    /// Lets every puyo fall to the floor or onto the stack.
    pub(super) fn collapse(&mut self) -> Vec<Move> {
        let mut moved = Vec::new();
        for col in 0..self.width {
            let mut floor = self.height;
            for row in (0..self.height).rev() {
                if let Some(t) = self[(row, col)] {
                    floor -= 1;
                    if floor != row {
                        self[(floor, col)] = Some(t);
                        self[(row, col)] = None;
                        moved.push(Move {
                            from: row,
                            col,
                            to: floor,
                        });
                    }
                }
            }
        }
        moved
    }

    /// Drops `puyo` into column `col`, onto its lowest empty cell: the row
    /// it lands on, or `None` with the column full.
    pub(super) fn drop_into(&mut self, col: usize, puyo: PuyoType) -> Option<usize> {
        let r = (0..self.height).rev().find(|&r| self[(r, col)].is_none())?;
        self[(r, col)] = Some(puyo);
        Some(r)
    }
}

impl<'a> IntoIterator for &'a Grid {
    type Item = &'a Option<PuyoType>;
    type IntoIter = std::slice::Iter<'a, Option<PuyoType>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Index<(usize, usize)> for Grid {
    type Output = Option<PuyoType>;

    fn index(&self, (r, c): (usize, usize)) -> &Self::Output {
        assert!(c < self.width, "column {c} is off the grid");
        &self.cells[self.flat(r, c)]
    }
}

impl IndexMut<(usize, usize)> for Grid {
    fn index_mut(&mut self, (r, c): (usize, usize)) -> &mut Self::Output {
        assert!(c < self.width, "column {c} is off the grid");
        let at = self.flat(r, c);
        &mut self.cells[at]
    }
}
