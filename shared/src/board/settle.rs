use rand::RngExt;

use super::{Board, CellFall, Events, Fall, Phase, PuyoType, Stream};
use crate::config::{self, CHAIN_POWERS, COLOR_BONUS, GROUP_BONUS, MAX_GARBAGE_DROP, POP_FRAMES, VISIBLE_ROW_OFFSET};
use crate::small;

pub fn group_bonus(size: u32) -> u32 {
    GROUP_BONUS[size.saturating_sub(4).min(7) as usize]
}

pub fn link_score(link: u32, colours: usize, cleared: u32, group_bonuses: u32) -> u32 {
    let multiplier = (CHAIN_POWERS[link.min(19) as usize] + COLOR_BONUS[colours.min(5)] + group_bonuses).clamp(1, 999);
    10 * cleared * multiplier
}

impl Board {
    /// Finds the groups that pop now, counts them into the chain and the
    /// score: the score they make and their cells, nuisance next to them
    /// included. The cells stay on the grid.
    fn find_pops(&mut self) -> Option<(u32, Vec<(u8, u8)>)> {
        let (width, height) = (self.width(), self.height());
        let mut popped = vec![false; width * height];
        let mut visited = vec![false; width * height];
        let mut group_bonuses = 0;
        let mut colors_seen = [false; 6];
        let mut cleared = 0;

        for r in 0..height {
            for c in 0..width {
                let Some(colour) = self.cells[(r, c)] else {
                    continue;
                };
                if colour == PuyoType::Garbage || visited[self.cells.flat(r, c)] {
                    continue;
                }
                let group = self.cells.group(r, c, &mut visited);
                if group.len() < 4 || group.iter().all(|&(r, _)| r < VISIBLE_ROW_OFFSET) {
                    continue;
                }
                colors_seen[colour as usize] = true;
                group_bonuses += group_bonus(small(group.len()));
                cleared += small::<u32>(group.len());
                for (gr, gc) in group {
                    popped[self.cells.flat(gr, gc)] = true;
                    for (nr, nc) in self.cells.neighbours(gr, gc) {
                        if self.cells[(nr, nc)] == Some(PuyoType::Garbage) {
                            popped[self.cells.flat(nr, nc)] = true;
                        }
                    }
                }
            }
        }
        let cells: Vec<(u8, u8)> = (0..height)
            .flat_map(|r| (0..width).map(move |c| (r, c)))
            .filter(|&(r, c)| popped[self.cells.flat(r, c)])
            .map(|(r, c)| (small(r), small(c)))
            .collect();
        if cells.is_empty() {
            return None;
        }
        self.chain_count += 1;
        let colours = colors_seen.iter().filter(|&&seen| seen).count();
        let score = link_score(self.chain_count, colours, cleared, group_bonuses);
        self.score = self.score.saturating_add(score);
        Some((score, cells))
    }

    fn clear(&mut self, cells: &[(u8, u8)]) {
        for &(r, c) in cells {
            self.cells[(usize::from(r), usize::from(c))] = None;
        }
    }

    /// Pops what would pop now, at once: the score it makes.
    #[cfg(any(test, feature = "test-support"))]
    pub fn check_matches(&mut self) -> Option<u32> {
        let (score, cells) = self.find_pops()?;
        self.clear(&cells);
        Some(score)
    }

    /// Lets everything fall at once: whether anything moved.
    #[cfg(any(test, feature = "test-support"))]
    pub fn apply_board_gravity(&mut self) -> bool {
        !self.cells.collapse().is_empty()
    }

    /// Drops the nuisance waiting, `MAX_GARBAGE_DROP` at most: whole rows,
    /// then the rest in columns drawn at random.
    pub(super) fn drop_garbage(&mut self, events: &mut Events) {
        if self.pending_garbage == 0 {
            return;
        }
        let dropped = self.pending_garbage.min(MAX_GARBAGE_DROP);
        self.pending_garbage -= dropped;

        let width = self.width();
        let rows = dropped as usize / width;
        let leftover = dropped as usize % width;
        let mut columns: Vec<usize> = (0..rows).flat_map(|_| 0..width).collect();
        if leftover > 0 {
            let mut cols: Vec<usize> = (0..width).collect();
            self.rng.draw(Stream::Garbage, |r| {
                for i in 0..leftover {
                    cols.swap(i, r.random_range(i..width));
                }
            });
            columns.extend_from_slice(&cols[..leftover]);
        }

        let landed: Vec<(usize, usize)> = columns
            .into_iter()
            .filter_map(|col| Some((self.cells.drop_into(col, PuyoType::Garbage)?, col)))
            .collect();
        let mut lowest = vec![0; width];
        for &(r, c) in &landed {
            lowest[c] = lowest[c].max(r);
        }
        let cells = landed
            .into_iter()
            .map(|(r, c)| CellFall::new(r, c, lowest[c]))
            .collect();
        self.phase = Phase::Falling(Fall::new(cells, true));
        events.ojama_dropped = true;
    }

    /// Once everything has landed: pops, else nuisance, else the next pair.
    fn after_landing(&mut self, events: &mut Events) {
        self.phase = Phase::Settled;
        if let Some((score, cells)) = self.find_pops() {
            self.phase = Phase::Popping { frame: 0, cells };
            events.link = Some(self.chain_count);
            let target = self.target_points();
            let total_nuisance = score + self.nuisance_points;
            self.nuisance_points = total_nuisance % target;
            let mut attack = total_nuisance / target;
            if self.all_clear_bonus {
                attack += config::ALL_CLEAR_BONUS;
                self.all_clear_bonus = false;
            }
            let offset = attack.min(self.pending_garbage);
            self.pending_garbage -= offset;
            events.sent += attack - offset;
        } else if self.pending_garbage > 0 {
            self.drop_garbage(events);
        } else {
            self.next_pair(events);
        }
    }

    fn next_pair(&mut self, events: &mut Events) {
        let all_clear = self.cells.is_empty();
        self.last_was_all_clear = all_clear;
        if all_clear {
            self.all_clear_bonus = true;
            events.all_clear = true;
        }
        self.spawn(events);
    }

    pub(super) fn tick_settle(&mut self, events: &mut Events) {
        match &mut self.phase {
            Phase::Falling(fall) => {
                fall.frame += 1;
                if fall.frame < fall.frames {
                    return;
                }
                if fall.ojama {
                    self.next_pair(events);
                } else {
                    self.after_landing(events);
                }
            }
            Phase::Popping { frame, cells } => {
                *frame += 1;
                if *frame < POP_FRAMES {
                    return;
                }
                let cells = std::mem::take(cells);
                self.clear(&cells);
                let falls = self.cells.collapse().into_iter().map(CellFall::from).collect();
                self.phase = Phase::Falling(Fall::new(falls, false));
            }
            Phase::Settled => self.after_landing(events),
            Phase::Waiting | Phase::Pair(_) | Phase::GameOver => {}
        }
    }
}
