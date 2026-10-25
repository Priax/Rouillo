use super::{Board, Grid, Rules};
use crate::config::VISIBLE_ROW_OFFSET;
use crate::protocol::Thumb;
use crate::small;

impl Thumb {
    /// `board`, whose stack goes along only with `cells`.
    pub fn of(board: &Board, cells: bool) -> Self {
        Self {
            slot: 0,
            team: 0,
            target: None,
            incoming: Vec::new(),
            width: small(board.width()),
            height: small(board.height()),
            cells: cells.then(|| board.cells.as_slice().to_vec()),
            piece_id: board.piece_id,
            phase: board.phase.clone(),
            paused: board.paused,
            pending_garbage: board.pending_garbage,
        }
    }

    /// Brings `board`, the last one known for this player, up to date:
    /// whether the thumb held together and was applied.
    pub fn apply_to(self, board: &mut Board) -> bool {
        let (width, height) = (usize::from(self.width), usize::from(self.height));
        let mut next = if (board.width(), board.height()) == (width, height) {
            board.clone()
        } else {
            let rules = Rules {
                columns: self.width,
                rows: small(height.saturating_sub(VISIBLE_ROW_OFFSET)),
                ..Rules::default()
            };
            Board::new(0, &rules)
        };
        if let Some(cells) = self.cells {
            match Grid::from_cells(width, height, cells) {
                Some(grid) => next.cells = grid,
                None => return false,
            }
        }
        next.piece_id = self.piece_id;
        next.phase = self.phase;
        next.paused = self.paused;
        next.pending_garbage = self.pending_garbage;
        let applied = next.is_well_formed() && (next.width(), next.height()) == (width, height);
        if applied {
            *board = next;
        }
        applied
    }
}
