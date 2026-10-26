use std::ops::{Index, IndexMut};

use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::{config, Board, GameState, InputKind, PuyoType, Rotation, BOARD_COLUMNS, BOARD_ROWS};

const TOP: usize = config::VISIBLE_ROW_OFFSET;
const MAX_W: usize = *BOARD_COLUMNS.end() as usize;
const MAX_H: usize = *BOARD_ROWS.end() as usize + TOP;
const GARBAGE: u8 = PuyoType::Garbage as u8 + 1;

/// A board's cells, in room for the largest board a room may pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Grid {
    cells: [[u8; MAX_W]; MAX_H],
    w: usize,
    h: usize,
}

type Marks = [[bool; MAX_W]; MAX_H];
const NO_MARKS: Marks = [[false; MAX_W]; MAX_H];

impl Grid {
    const fn new(w: usize, h: usize) -> Self {
        Self {
            cells: [[0; MAX_W]; MAX_H],
            w,
            h,
        }
    }

    const fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    /// The column pairs come in at, as `Board::spawn_col`.
    const fn spawn(&self) -> usize {
        self.w.saturating_sub(1) / 2
    }

    /// The highest a column may be: so many rows under the top.
    const fn under_top(&self, rows: usize) -> usize {
        self.h.saturating_sub(rows)
    }

    fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.cells[..self.h].iter().map(|row| &row[..self.w])
    }
}

impl Index<usize> for Grid {
    type Output = [u8; MAX_W];

    fn index(&self, row: usize) -> &Self::Output {
        &self.cells[row]
    }
}

impl IndexMut<usize> for Grid {
    fn index_mut(&mut self, row: usize) -> &mut Self::Output {
        &mut self.cells[row]
    }
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    Pro,
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Style {
    #[default]
    Balanced,
    /// Fires small chains as soon as it has them.
    Harasser,
    /// Keeps a low, clean board for one very long chain.
    Builder,
    /// Keeps a chain ready and fires it back when attacked.
    Counter,
    /// Chases empty boards for their bonus.
    AllClear,
    /// Builds known shapes: stairs, sandwich, GTR, or a U.
    Architect,
}

/// What a CPU is called on its board and in a lobby.
pub fn name(difficulty: Difficulty, style: Style) -> String {
    format!("{} ({})", style.character(), difficulty.label())
}

impl Style {
    pub const ALL: [Self; 6] = [
        Self::Balanced,
        Self::Harasser,
        Self::Builder,
        Self::Counter,
        Self::AllClear,
        Self::Architect,
    ];

    pub const fn character(self) -> &'static str {
        match self {
            Self::Balanced => "Personnage 1",
            Self::Harasser => "Personnage 2",
            Self::Builder => "Personnage 3",
            Self::Counter => "Personnage 4",
            Self::AllClear => "Personnage 5",
            Self::Architect => "Personnage 6",
        }
    }
}

/// Shapes the architect builds, as their lowest rows, top first, against
/// the left wall or anywhere else, either way round. A letter stands for a
/// colour: the same letter for the same colour, neighbours with different
/// letters for different ones.
const TEMPLATES: [&[&str]; 3] = [
    &[".ABC..", "ABCD..", "ABCD..", "ABCD.."],
    &["B.....", "A.....", "A.....", "AD....", "BD....", "BBDD.."],
    &["BAC...", "BBAC..", "AACC.."],
];

/// A shape's cells: the row counted up from the floor, the column, the letter.
type Template = Vec<(usize, usize, u8)>;

fn template(rows: &[&str]) -> Template {
    rows.iter()
        .rev()
        .enumerate()
        .flat_map(|(up, row)| {
            row.bytes()
                .enumerate()
                .filter(|&(_, b)| b != b'.')
                .map(move |(c, b)| (up, c, b))
        })
        .collect()
}

struct Profile {
    think_ticks: u32,
    input_ticks: u32,
    hard_drop: bool,
    lookahead: bool,
    /// The shortest chain worth firing when under attack.
    min_chain: u32,
    /// The shortest chain worth firing otherwise.
    calm_chain: u32,
    noise: f32,
    potential_weight: f32,
    height_cost: f32,
    all_clear: bool,
    template: Template,
    /// Stacks the sides and keeps the middle low, in a U.
    valley: bool,
}

impl Difficulty {
    pub const ALL: [Self; 4] = [Self::Easy, Self::Normal, Self::Hard, Self::Pro];

    pub fn label(self) -> &'static str {
        match self {
            Self::Easy => "Facile",
            Self::Normal => "Normal",
            Self::Hard => "Difficile",
            Self::Pro => "Pro",
        }
    }

    const fn level(self) -> usize {
        self as usize
    }
}

fn profile(difficulty: Difficulty, style: Style, seed: u64) -> Profile {
    let (think_ticks, input_ticks, lookahead, hard_drop, noise) = match difficulty {
        Difficulty::Easy => (30, 18, false, false, 45.0),
        Difficulty::Normal => (28, 16, true, false, 30.0),
        Difficulty::Hard => (14, 7, true, false, 20.0),
        Difficulty::Pro => (9, 7, true, true, 0.0),
    };
    let pick = |chains: [u32; 4]| chains[difficulty.level()];
    let base = pick([1, 2, 5, 6]);
    let mut profile = Profile {
        think_ticks,
        input_ticks,
        hard_drop,
        lookahead,
        min_chain: base,
        calm_chain: base,
        noise,
        potential_weight: POTENTIAL_WEIGHT,
        height_cost: HEIGHT_COST,
        all_clear: false,
        template: Vec::new(),
        valley: false,
    };
    match style {
        Style::Balanced => {}
        Style::Harasser => {
            profile.calm_chain = pick([1, 2, 2, 3]);
            profile.min_chain = profile.calm_chain;
            profile.potential_weight *= 0.5;
        }
        Style::Builder => {
            profile.calm_chain = pick([2, 4, 7, 8]);
            profile.min_chain = profile.calm_chain;
            profile.potential_weight *= 1.5;
            profile.height_cost *= 3.0;
        }
        Style::Counter => {
            profile.min_chain = pick([1, 2, 4, 4]);
            profile.calm_chain = pick([3, 4, 7, 8]);
            profile.potential_weight *= 1.6;
            profile.height_cost *= 3.0;
        }
        Style::AllClear => {
            profile.calm_chain = pick([1, 2, 3, 4]);
            profile.min_chain = profile.calm_chain;
            profile.all_clear = true;
        }
        Style::Architect => {
            profile.calm_chain = pick([2, 3, 4, 5]);
            profile.min_chain = profile.calm_chain;
            match TEMPLATES.get(usize::try_from(seed % (TEMPLATES.len() as u64 + 1)).unwrap_or(0)) {
                Some(rows) => profile.template = template(rows),
                None => profile.valley = true,
            }
        }
    }
    profile
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Placement {
    col: usize,
    rotation: Rotation,
}

impl Placement {
    fn satellite_col(self) -> usize {
        match self.rotation {
            Rotation::Right => self.col + 1,
            Rotation::Left => self.col - 1,
            Rotation::Up | Rotation::Down => self.col,
        }
    }
}

fn placements(w: usize) -> impl Iterator<Item = Placement> {
    Rotation::ALL.into_iter().flat_map(move |rotation| {
        let cols = match rotation {
            Rotation::Right => 0..w - 1,
            Rotation::Left => 1..w,
            Rotation::Up | Rotation::Down => 0..w,
        };
        cols.map(move |col| Placement { col, rotation })
    })
}

const MAX_INPUTS: u32 = 10;

struct Exec {
    target: Placement,
    wait: u32,
    inputs_left: u32,
    dropped: bool,
}

impl Exec {
    fn new(target: Placement, wait: u32) -> Self {
        Self {
            target,
            wait,
            inputs_left: MAX_INPUTS,
            dropped: false,
        }
    }

    fn next(&mut self, board: &Board, profile: &Profile) -> Option<InputKind> {
        let piece = board.piece()?;
        if self.dropped {
            return None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return None;
        }
        let col: i32 = crate::small(self.target.col);
        let step = if self.inputs_left == 0 {
            None
        } else if piece.rotation != self.target.rotation {
            Some(if piece.rotation.ccw() == self.target.rotation {
                InputKind::RotateCCW
            } else {
                InputKind::RotateCW
            })
        } else if piece.col < col {
            Some(InputKind::MoveRight)
        } else if piece.col > col {
            Some(InputKind::MoveLeft)
        } else {
            None
        };
        if let Some(kind) = step {
            self.inputs_left -= 1;
            self.wait = profile.input_ticks;
            return Some(kind);
        }
        self.dropped = true;
        Some(if profile.hard_drop {
            InputKind::HardDrop
        } else {
            InputKind::SoftDropPress
        })
    }
}

const MAX_PAIR_TICKS: u32 = 900;

fn simulate(board: &Board, mut exec: Exec, profile: &Profile) -> Option<Board> {
    let mut board = board.clone();
    let id = board.piece_id;
    for _ in 0..MAX_PAIR_TICKS {
        if board.state() != GameState::Playing || board.piece_id != id {
            return Some(board);
        }
        let input = exec.next(&board, profile);
        board.step(input, 0);
    }
    None
}

fn grid_of(board: &Board) -> Grid {
    let mut grid = Grid::new(board.width(), board.height());
    for (r, row) in board.cells().rows().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            grid[r][c] = cell.map_or(0, code);
        }
    }
    grid
}

const fn code(t: PuyoType) -> u8 {
    t as u8 + 1
}

fn neighbours((w, h): (usize, usize), r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> {
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .filter_map(move |(dr, dc)| {
            let nr = r.checked_add_signed(dr)?;
            let nc = c.checked_add_signed(dc)?;
            (nr < h && nc < w).then_some((nr, nc))
        })
}

/// Cells as bytes: a group is flooded for every judged board.
struct Group {
    cells: [(u8, u8); MAX_W * MAX_H],
    len: usize,
}

impl Group {
    fn cells(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.cells[..self.len]
            .iter()
            .map(|&(r, c)| (usize::from(r), usize::from(c)))
    }
}

fn flood(grid: &Grid, r: usize, c: usize, seen: &mut Marks) -> Group {
    let colour = grid[r][c];
    let mut group = Group {
        cells: [(0, 0); MAX_W * MAX_H],
        len: 1,
    };
    group.cells[0] = (crate::small(r), crate::small(c));
    seen[r][c] = true;
    let mut next = 0;
    while next < group.len {
        let (r, c) = (usize::from(group.cells[next].0), usize::from(group.cells[next].1));
        next += 1;
        for (nr, nc) in neighbours(grid.size(), r, c) {
            if grid[nr][nc] == colour && !seen[nr][nc] {
                seen[nr][nc] = true;
                group.cells[group.len] = (crate::small(nr), crate::small(nc));
                group.len += 1;
            }
        }
    }
    group
}

fn pop_once(grid: &mut Grid, link: u32) -> Option<u32> {
    let mut seen = NO_MARKS;
    let mut remove = NO_MARKS;
    let mut colours = [false; 7];
    let (mut cleared, mut group_bonus) = (0, 0);
    for r in 0..grid.h {
        for c in 0..grid.w {
            let cell = grid[r][c];
            if cell == 0 || cell == GARBAGE || seen[r][c] {
                continue;
            }
            let group = flood(grid, r, c, &mut seen);
            if group.len < 4 || group.cells().all(|(r, _)| r < TOP) {
                continue;
            }
            colours[cell as usize] = true;
            let len: u32 = crate::small(group.len);
            cleared += len;
            group_bonus += crate::group_bonus(len);
            for (gr, gc) in group.cells() {
                remove[gr][gc] = true;
                for (nr, nc) in neighbours(grid.size(), gr, gc) {
                    if grid[nr][nc] == GARBAGE {
                        remove[nr][nc] = true;
                    }
                }
            }
        }
    }
    if cleared == 0 {
        return None;
    }
    for r in 0..grid.h {
        for c in 0..grid.w {
            if remove[r][c] {
                grid[r][c] = 0;
            }
        }
    }
    let colour_count = colours.iter().filter(|&&seen| seen).count();
    Some(crate::link_score(link, colour_count, cleared, group_bonus))
}

#[allow(clippy::needless_range_loop)]
fn settle(grid: &mut Grid) {
    for c in 0..grid.w {
        let mut floor = grid.h;
        for r in (0..grid.h).rev() {
            if grid[r][c] != 0 {
                floor -= 1;
                if floor != r {
                    grid[floor][c] = grid[r][c];
                    grid[r][c] = 0;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Chain {
    links: u32,
    score: u32,
}

fn resolve(grid: &mut Grid) -> Chain {
    let mut chain = Chain::default();
    while let Some(gained) = pop_once(grid, chain.links + 1) {
        chain.links += 1;
        chain.score += gained;
        settle(grid);
    }
    chain
}

fn height(grid: &Grid, col: usize) -> usize {
    (0..grid.h).find(|&r| grid[r][col] != 0).map_or(0, |r| grid.h - r)
}

fn drop_into(grid: &mut Grid, col: usize, cell: u8) -> bool {
    let h = height(grid, col);
    if h == grid.h {
        return false;
    }
    let row = grid.h - 1 - h;
    grid[row][col] = cell;
    true
}

/// How far under the top a column still lets pairs over it.
const PASSABLE_GAP: usize = 4;

fn place(grid: &Grid, (axis, satellite): (u8, u8), p: Placement) -> Option<Grid> {
    let sat_col = p.satellite_col();
    let spawn = grid.spawn();
    let lo = spawn.min(p.col).min(sat_col);
    let hi = spawn.max(p.col).max(sat_col);
    if (lo..=hi).any(|c| height(grid, c) > grid.under_top(PASSABLE_GAP)) {
        return None;
    }
    let mut grid = *grid;
    let order = if p.rotation == Rotation::Down {
        [(sat_col, satellite), (p.col, axis)]
    } else {
        [(p.col, axis), (sat_col, satellite)]
    };
    order
        .into_iter()
        .all(|(col, cell)| drop_into(&mut grid, col, cell))
        .then_some(grid)
}

fn potential(grid: &Grid) -> Chain {
    let mut best = Chain::default();
    for col in 0..grid.w {
        let h = height(grid, col);
        if h + TOP >= grid.h {
            continue;
        }
        let row = grid.h - 1 - h;
        let mut tried = [false; 7];
        for (nr, nc) in neighbours(grid.size(), row, col) {
            let colour = grid[nr][nc];
            if colour == 0 || colour == GARBAGE || tried[colour as usize] {
                continue;
            }
            tried[colour as usize] = true;
            let mut with = *grid;
            with[row][col] = colour;
            if flood(&with, row, col, &mut NO_MARKS.clone()).len < 4 {
                continue;
            }
            let chain = resolve(&mut with);
            if chain.score > best.score {
                best = chain;
            }
        }
    }
    best
}

const DEAD: f32 = -1.0e9;
/// How far under the top columns are safe, the column pairs come in at,
/// and a column that calls for any pop at all.
const SAFE_GAP: usize = 5;
const SAFE_SPAWN_GAP: usize = 8;
const URGENT_GAP: usize = 4;
const URGENT_THREAT: u32 = 6;
/// The attack the counter answers: smaller ones it lets land.
const ANSWERED_THREAT: u32 = 3;
const WASTED_POP: f32 = 0.05;
const POTENTIAL_WEIGHT: f32 = 0.5;
const LATER: f32 = 0.9;
const NO_SECOND_MOVE: f32 = -400.0;
const HEIGHT_COST: f32 = 2.0;
const ALL_CLEAR_VALUE: f32 = 1500.0;
/// Below this many Puyos, the all clear chaser looks a pair further ahead
/// and keeps each colour in one piece.
const ALL_CLEAR_RANGE: usize = 16;
const SPLIT_COLOUR_COST: f32 = 70.0;
const TEMPLATE_PLACED: f32 = 8.0;
const TEMPLATE_MATCH: f32 = 25.0;
const TEMPLATE_CLASH: f32 = 60.0;
const UNSAFE_HEIGHT_COST: f32 = 400.0;
const UNSAFE_SPAWN_COST: f32 = 300.0;
const PAIR_VALUE: f32 = 12.0;
const TRIPLET_VALUE: f32 = 30.0;

struct Judge<'a> {
    profile: &'a Profile,
    urgent: bool,
    threatened: bool,
    then: (u8, u8),
    target: u32,
    carried: u32,
    /// The longest chain the board has room to build.
    longest: u32,
}

/// One link of chain for so many visible cells: a chain longer than the
/// board has room for fills it up before it fires.
const CELLS_PER_LINK: usize = 9;

/// What a cell of height is worth in column `c`, for the U: the sides
/// high, the middle low.
fn valley(c: usize, w: usize) -> f32 {
    match c.min(w - 1 - c) {
        0 => 6.0,
        1 => 3.0,
        _ => -4.0,
    }
}

/// Nuisance short of a whole row lands in the middle first: the worst the
/// random columns can do to the way in.
fn bury(grid: &mut Grid, nuisance: u32) {
    let (n, w) = (nuisance as usize, grid.w);
    if n == 0 {
        return;
    }
    let mut middle_first: [usize; MAX_W] = std::array::from_fn(|c| c);
    middle_first[..w].sort_by_key(|&c| (2 * c).abs_diff(w - 1));
    let rows = (0..n / w).flat_map(|_| 0..w);
    for col in rows.chain(middle_first.into_iter().take(n % w)) {
        drop_into(grid, col, GARBAGE);
    }
}

/// On a nearly empty board, how many groups there are beyond one per colour.
fn split_colours(grid: &Grid) -> usize {
    let cells = grid.iter().flatten().filter(|&&cell| cell != 0).count();
    if cells > ALL_CLEAR_RANGE {
        return 0;
    }
    let mut seen = NO_MARKS;
    let mut groups = [0; 8];
    for r in 0..grid.h {
        for c in 0..grid.w {
            if grid[r][c] != 0 && !seen[r][c] {
                flood(grid, r, c, &mut seen);
                groups[grid[r][c] as usize] += 1;
            }
        }
    }
    groups.iter().map(|&n: &usize| n.saturating_sub(1)).sum::<usize>() + groups[GARBAGE as usize]
}

fn empty(grid: &Grid) -> bool {
    grid.iter().flatten().all(|&cell| cell == 0)
}

/// How well the board follows `template`, wherever it fits best.
fn template_value(grid: &Grid, template: &Template) -> f32 {
    let width = template.iter().map(|&(_, c, _)| c + 1).max().unwrap_or(0);
    if width == 0 || width > grid.w {
        return 0.0;
    }
    (0..=grid.w - width)
        .flat_map(|x| [false, true].map(|mirrored| (x, mirrored)))
        .map(|(x, mirrored)| {
            let col = |c: usize| x + if mirrored { width - 1 - c } else { c };
            template_value_at(grid, template.iter().map(|&(up, c, l)| (grid.h - 1 - up, col(c), l)))
        })
        .fold(f32::NEG_INFINITY, f32::max)
}

fn template_value_at(grid: &Grid, cells: impl Iterator<Item = (usize, usize, u8)>) -> f32 {
    let placed: Vec<_> = cells
        .filter(|&(r, c, _)| grid[r][c] != 0 && grid[r][c] != GARBAGE)
        .collect();
    let mut value = placed.len() as f32 * TEMPLATE_PLACED;
    for (i, &(r1, c1, l1)) in placed.iter().enumerate() {
        for &(r2, c2, l2) in &placed[i + 1..] {
            let same = grid[r1][c1] == grid[r2][c2];
            if l1 == l2 {
                value += if same { TEMPLATE_MATCH } else { -TEMPLATE_CLASH };
            } else if same && r1.abs_diff(r2) + c1.abs_diff(c2) == 1 {
                value -= TEMPLATE_CLASH;
            }
        }
    }
    value
}

impl Judge<'_> {
    fn need(&self) -> u32 {
        let wanted = if self.threatened {
            self.profile.min_chain
        } else {
            self.profile.calm_chain
        };
        wanted.min(self.longest)
    }

    /// Lands on `grid` what `chain` leaves of the `coming` nuisance, as much
    /// as falls at once: what is left to fall after the next pair.
    fn take(&self, chain: Chain, grid: &mut Grid, coming: u32) -> u32 {
        let attack = (chain.score + self.carried) / self.target;
        let left = coming.saturating_sub(attack);
        let now = left.min(config::MAX_GARBAGE_DROP);
        bury(grid, now);
        left - now
    }

    /// What firing `chain` is worth, given what the board looks like after it.
    fn fire(&self, chain: Chain, after: &Grid) -> f32 {
        if chain.links == 0 {
            return 0.0;
        }
        let cleared = self.profile.all_clear && empty(after);
        let wanted = self.urgent || chain.links >= self.need() || cleared;
        let bonus = if cleared { ALL_CLEAR_VALUE } else { 0.0 };
        chain.score as f32 * if wanted { 1.0 } else { WASTED_POP } + bonus
    }

    fn shape(&self, grid: &Grid) -> f32 {
        let spawn = grid.spawn();
        if grid[TOP][spawn] != 0 {
            return DEAD;
        }
        let mut value = 0.0;
        let (safe_height, safe_spawn) = (grid.under_top(SAFE_GAP), grid.under_top(SAFE_SPAWN_GAP));
        let safe = height(grid, spawn) <= safe_spawn;
        for c in 0..grid.w {
            let h = height(grid, c);
            let over = h.saturating_sub(safe_height) as f32;
            value -= over * over * UNSAFE_HEIGHT_COST + h as f32 * self.profile.height_cost;
            if self.profile.valley {
                value += h.min(safe_height) as f32 * valley(c, grid.w);
            }
        }
        let over = height(grid, spawn).saturating_sub(safe_spawn) as f32;
        value -= over * over * UNSAFE_SPAWN_COST;

        let mut seen = NO_MARKS;
        for r in 0..grid.h {
            for c in 0..grid.w {
                if grid[r][c] == 0 || grid[r][c] == GARBAGE || seen[r][c] {
                    continue;
                }
                value += match flood(grid, r, c, &mut seen).len {
                    1 => 0.0,
                    2 => PAIR_VALUE,
                    _ => TRIPLET_VALUE,
                };
            }
        }
        let answering = self.threatened && self.profile.min_chain < self.profile.calm_chain;
        if self.need() > 1 && !answering && safe {
            value += potential(grid).score as f32 * self.profile.potential_weight;
        }
        if self.profile.all_clear {
            value -= split_colours(grid) as f32 * SPLIT_COLOUR_COST;
        }
        value + template_value(grid, &self.profile.template)
    }

    /// Whether the pair after next can empty the board, for the all clear
    /// chaser once only a few Puyos are left.
    fn clear_later(&self, grid: &Grid) -> f32 {
        let cells = grid.iter().flatten().filter(|&&cell| cell != 0).count();
        if !self.profile.all_clear || cells == 0 || cells > ALL_CLEAR_RANGE {
            return 0.0;
        }
        let clears = placements(grid.w)
            .filter_map(|p| place(grid, self.then, p))
            .any(|mut grid| resolve(&mut grid).links > 0 && empty(&grid));
        if clears {
            LATER * LATER * ALL_CLEAR_VALUE
        } else {
            0.0
        }
    }

    fn after(&self, grid: &Grid, next: (u8, u8), coming: u32) -> f32 {
        if grid[TOP][grid.spawn()] != 0 {
            return DEAD;
        }
        if !self.profile.lookahead {
            return self.shape(grid);
        }
        placements(grid.w)
            .filter_map(|p| place(grid, next, p))
            .map(|mut grid| {
                let chain = resolve(&mut grid);
                let fired = self.fire(chain, &grid);
                self.take(chain, &mut grid, coming);
                LATER * fired + self.shape(&grid) + self.clear_later(&grid)
            })
            .fold(None, |best: Option<f32>, v| Some(best.map_or(v, |b| b.max(v))))
            .unwrap_or_else(|| self.shape(grid) + NO_SECOND_MOVE)
    }
}

struct Search {
    threat: u32,
    urgent: bool,
    placements: usize,
    tried: usize,
    elapsed: u32,
    best: (f32, Placement),
}

impl Search {
    fn new(board: &Board, threat: u32) -> Self {
        Self {
            threat,
            urgent: urgent(board, threat),
            placements: 4 * board.width() - 2,
            tried: 0,
            elapsed: 0,
            best: (
                f32::NEG_INFINITY,
                Placement {
                    col: board.spawn_col(),
                    rotation: Rotation::Up,
                },
            ),
        }
    }

    const fn done(&self) -> bool {
        self.tried >= self.placements
    }

    fn run(&mut self, board: &Board, profile: &Profile, wait: u32, count: usize, noise: &mut Noise) {
        let judge = Judge {
            profile,
            urgent: self.urgent,
            threatened: self.threat >= ANSWERED_THREAT,
            then: (code(board.next_next_types.0), code(board.next_next_types.1)),
            target: board.target_points().max(1),
            carried: board.nuisance_points,
            longest: crate::small::<u32>(board.width() * (board.height() - TOP) / CELLS_PER_LINK).max(1),
        };
        let next = (code(board.next_types.0), code(board.next_types.1));
        for target in placements(board.width()).skip(self.tried).take(count) {
            self.tried += 1;
            let Some(after) = simulate(board, Exec::new(target, wait), profile) else {
                continue;
            };
            let mut grid = grid_of(&after);
            let chain = resolve(&mut grid);
            let fired = judge.fire(chain, &grid);
            let later = judge.take(chain, &mut grid, self.threat);
            let value = fired + judge.after(&grid, next, later) + noise.next() * profile.noise;
            if value > self.best.0 {
                self.best = (value, target);
            }
        }
    }
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

enum Turn {
    Thinking(Search),
    /// With the nuisance coming when it was chosen.
    Playing(Exec, u32),
}

pub struct Cpu {
    profile: Profile,
    turn: Option<(u32, Turn)>,
    soft_drop_held: bool,
    noise: Noise,
}

impl Cpu {
    pub fn new(difficulty: Difficulty, style: Style, seed: u64) -> Self {
        Self {
            profile: profile(difficulty, style, seed),
            turn: None,
            soft_drop_held: false,
            noise: Noise(u32::try_from(seed & u64::from(u32::MAX)).unwrap_or(0) | 1),
        }
    }

    pub fn input(&mut self, board: &Board, threat: u32) -> Option<InputKind> {
        let mut unlimited = usize::MAX;
        self.input_within(board, threat, &mut unlimited)
    }

    /// As `input`, judging at most `budget` placements this tick and taking
    /// them from it: short of it, the CPU thinks on at the next tick.
    pub fn input_within(&mut self, board: &Board, threat: u32, budget: &mut usize) -> Option<InputKind> {
        let playing = board.state() == GameState::Playing && board.piece().is_some();
        let current = self.turn.as_ref().is_some_and(|(id, _)| *id == board.piece_id);
        if self.soft_drop_held && !(playing && current) {
            self.soft_drop_held = false;
            return Some(InputKind::SoftDropRelease);
        }
        if !playing {
            return None;
        }
        let rethink = match &self.turn {
            Some((_, Turn::Thinking(search))) => search.threat != threat,
            Some((_, Turn::Playing(exec, planned))) => *planned != threat && !exec.dropped,
            None => false,
        };
        if !current || rethink {
            self.turn = Some((board.piece_id, Turn::Thinking(Search::new(board, threat))));
        }
        let (_, turn) = self.turn.as_mut()?;
        if let Turn::Thinking(search) = turn {
            let think = self.profile.think_ticks;
            let wait = think.saturating_sub(search.elapsed);
            let count = search.placements.div_ceil(think.max(1) as usize).min(*budget);
            *budget -= count;
            search.run(board, &self.profile, wait, count, &mut self.noise);
            if !search.done() {
                search.elapsed += 1;
                return None;
            }
            *turn = Turn::Playing(Exec::new(search.best.1, wait), search.threat);
        }
        let Turn::Playing(exec, _) = turn else {
            return None;
        };
        let input = exec.next(board, &self.profile);
        if input == Some(InputKind::SoftDropPress) {
            self.soft_drop_held = true;
        }
        input
    }
}

fn urgent(board: &Board, threat: u32) -> bool {
    let grid = grid_of(board);
    threat >= URGENT_THREAT || (0..grid.w).any(|c| height(&grid, c) >= grid.under_top(URGENT_GAP))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rules;

    fn standard() -> Grid {
        Grid::new(config::GRID_WIDTH, config::GRID_HEIGHT)
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn random_grid(rng: &mut Noise, colours: u32, fill: f32) -> Grid {
        let mut grid = standard();
        for r in 0..grid.h {
            for c in 0..grid.w {
                if rng.next() < fill {
                    grid[r][c] = if rng.next() < 0.15 {
                        GARBAGE
                    } else {
                        1 + (rng.next() * colours as f32) as u8
                    };
                }
            }
        }
        settle(&mut grid);
        grid
    }

    fn board_with(grid: &Grid) -> Board {
        let mut board = Board::new(1, &Rules::default());
        for (r, row) in grid.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                board.cells_mut()[(r, c)] = match cell {
                    0 => None,
                    1 => Some(PuyoType::Red),
                    2 => Some(PuyoType::Blue),
                    3 => Some(PuyoType::Yellow),
                    4 => Some(PuyoType::Green),
                    5 => Some(PuyoType::Purple),
                    _ => Some(PuyoType::Garbage),
                };
            }
        }
        board
    }

    #[test]
    fn the_search_resolves_chains_like_the_board() {
        let mut rng = Noise(7);
        let mut chains = 0;
        for i in 0..2000 {
            let mut grid = random_grid(&mut rng, 3 + i % 3, 0.3 + (i % 5) as f32 * 0.1);
            let mut board = board_with(&grid);
            let mut score = 0;
            while let Some(gained) = board.check_matches() {
                score += gained;
                board.apply_board_gravity();
            }
            let chain = resolve(&mut grid);
            assert_eq!(
                (chain.links, chain.score),
                (board.chain_count, score),
                "grid {i} resolved differently"
            );
            assert_eq!(grid, grid_of(&board), "grid {i} ended differently");
            chains += u32::from(chain.links >= 2);
        }
        assert!(chains > 100, "only {chains} chains, the test proves little");
    }

    fn started(seed: u64) -> Board {
        started_on(seed, &Rules::default())
    }

    fn started_on(seed: u64, rules: &Rules) -> Board {
        let mut board = Board::new(seed, rules);
        board.spawn_piece();
        board
    }

    fn sized(columns: u8, rows: u8) -> Rules {
        Rules {
            columns,
            rows,
            ..Rules::default()
        }
    }

    #[test]
    fn a_placement_ends_the_same_whenever_it_is_judged() {
        let profile = profile(Difficulty::Normal, Style::Balanced, 0);
        let early = started(3);
        let mut late = early.clone();
        let waited = 7;
        for _ in 0..waited {
            late.step(None, 0);
        }
        for target in placements(early.width()) {
            let from_start = simulate(&early, Exec::new(target, profile.think_ticks), &profile).expect("it locks");
            let from_later =
                simulate(&late, Exec::new(target, profile.think_ticks - waited), &profile).expect("it locks");
            assert_eq!(from_start.state_hash(), from_later.state_hash(), "{target:?}");
        }
    }

    #[test]
    fn the_pair_lands_where_the_search_saw_it_land() {
        for (difficulty, rules) in Difficulty::ALL
            .into_iter()
            .flat_map(|d| [(d, Rules::default()), (d, sized(9, 14))])
        {
            let mut board = started_on(11, &rules);
            let mut cpu = Cpu::new(difficulty, Style::Balanced, 11);
            let mut expected: Option<(u32, Grid)> = None;
            let mut checked = 0;
            for _ in 0..30 * 60 {
                if board.state() == GameState::GameOver {
                    break;
                }
                let before = board.clone();
                let thinking = match &cpu.turn {
                    Some((id, Turn::Thinking(search))) if *id == board.piece_id => Some(search.elapsed),
                    Some((id, Turn::Playing(..))) if *id == board.piece_id => None,
                    _ => Some(0),
                };
                let input = cpu.input(&board, 0);
                if let (Some(elapsed), Some((id, Turn::Playing(exec, _)))) = (thinking, &cpu.turn) {
                    let wait = cpu.profile.think_ticks - elapsed;
                    let seen = simulate(&before, Exec::new(exec.target, wait), &cpu.profile).expect("it locks");
                    expected = Some((*id, grid_of(&seen)));
                }
                board.step(input, 0);
                if let Some((id, grid)) = expected {
                    if board.state() != GameState::Playing && board.piece_id == id {
                        assert_eq!(grid_of(&board), grid, "{difficulty:?}, pair {id}");
                        expected = None;
                        checked += 1;
                    }
                }
            }
            assert!(
                checked > 10,
                "{difficulty:?} on {rules:?}: only {checked} pairs checked"
            );
        }
    }

    #[test]
    fn a_cpu_short_of_budget_thinks_on_later() {
        let pairs = |budget: usize| {
            let mut board = started(4);
            let mut cpu = Cpu::new(Difficulty::Pro, Style::Balanced, 4);
            for _ in 0..30 * 60 {
                let mut left = budget;
                board.step(cpu.input_within(&board, 0, &mut left), 0);
                assert!(left <= budget);
            }
            board.piece_id
        };
        let (free, short, none) = (pairs(usize::MAX), pairs(1), pairs(0));
        assert!(
            short > 5 && short < free,
            "pairs: {free} free, {short} on one placement a tick"
        );
        assert!(none < short, "a CPU with no budget played as much: {none}");
    }

    #[test]
    fn soft_drop_is_released_before_the_next_pair_falls() {
        let mut board = started(5);
        let mut cpu = Cpu::new(Difficulty::Normal, Style::Balanced, 5);
        let (mut pressed, mut pairs) = (false, 0);
        let mut id = board.piece_id;
        for _ in 0..20 * 60 {
            let input = cpu.input(&board, 0);
            pressed |= input == Some(InputKind::SoftDropPress);
            board.step(input, 0);
            if board.piece_id != id {
                id = board.piece_id;
                pairs += 1;
                assert!(!board.soft_dropping, "pair {id} started out soft dropping");
            }
        }
        assert!(pressed && pairs > 3, "{pairs} pairs, soft drop used: {pressed}");
    }

    fn fires_two_chain(style: Style, threat: u32) -> bool {
        let mut grid = standard();
        for (r, row) in [".B....", "BR....", "BR....", "BR...."].iter().enumerate() {
            for (c, b) in row.bytes().enumerate() {
                let row = grid.h - 4 + r;
                grid[row][c] = match b {
                    b'R' => 1,
                    b'B' => 2,
                    _ => 0,
                };
            }
        }
        let mut board = board_with(&grid);
        board.spawn_piece();
        let piece = board.piece_mut().unwrap();
        (piece.axis_type, piece.sat_type) = (PuyoType::Red, PuyoType::Red);
        board.next_types = (PuyoType::Yellow, PuyoType::Green);
        board.next_next_types = (PuyoType::Purple, PuyoType::Yellow);
        let mut cpu = Cpu::new(Difficulty::Normal, style, 1);
        let id = board.piece_id;
        for _ in 0..600 {
            board.step(cpu.input(&board, threat), 0);
            if board.chain_count >= 2 {
                return true;
            }
            if board.piece_id != id && board.state() == GameState::Playing {
                return false;
            }
        }
        false
    }

    #[test]
    fn the_counter_keeps_its_chain_until_attacked() {
        assert!(!fires_two_chain(Style::Counter, 0), "fired with nothing coming");
        assert!(fires_two_chain(Style::Counter, 3), "kept its chain under attack");
        assert!(fires_two_chain(Style::Harasser, 0), "the harasser held back");
    }

    #[test]
    fn the_harasser_fires_shorter_chains_than_the_builder() {
        let average = |style| {
            let chains: Vec<u32> = (0..2)
                .flat_map(|seed| play_styled(Difficulty::Normal, style, seed, 90 * 60).chains)
                .filter(|&links| links > 1)
                .collect();
            chains.iter().sum::<u32>() as f32 / chains.len().max(1) as f32
        };
        let (harasser, builder) = (average(Style::Harasser), average(Style::Builder));
        assert!(harasser + 1.0 < builder, "harasser {harasser}, builder {builder}");
    }

    #[test]
    fn the_u_keeps_the_middle_lower_than_the_sides() {
        let dip = |style, seed| {
            let mut board = started(seed);
            let mut cpu = Cpu::new(Difficulty::Normal, style, seed);
            let mut dip = 0.0;
            for tick in 0..90 * 60 {
                board.step(cpu.input(&board, 0), 0);
                if tick % 60 == 0 {
                    let grid = grid_of(&board);
                    let h = |c| height(&grid, c) as f32;
                    dip += h(0) + h(5) - h(2) - h(3);
                }
            }
            dip / 90.0
        };
        let u_seed = TEMPLATES.len() as u64;
        let (u, balanced) = (dip(Style::Architect, u_seed), dip(Style::Balanced, u_seed));
        assert!(u > balanced + 1.0, "U {u}, balanced {balanced}");
    }

    #[test]
    fn every_template_chains_once_filled() {
        for (rows, links) in TEMPLATES.into_iter().zip([4, 3, 3]) {
            let mut grid = standard();
            for (up, c, label) in template(rows) {
                let row = grid.h - 1 - up;
                grid[row][c] = label - b'A' + 1;
            }
            assert_eq!(resolve(&mut grid.clone()).links, 0, "{rows:?} pops before its trigger");
            assert_eq!(potential(&grid).links, links, "{rows:?}");
        }
    }

    #[test]
    fn every_style_plays_on_every_board_size() {
        for (columns, rows) in [(4, 8), (5, 16), (8, 10), (10, 16)] {
            let rules = sized(columns, rows);
            for style in Style::ALL {
                let run = play_on(Difficulty::Hard, style, 1, 60 * 60, &rules);
                assert!(run.alive, "{style:?} topped out alone on {columns}x{rows}");
                assert!(run.best_chain >= 2, "{style:?} never chained on {columns}x{rows}");
            }
        }
    }

    #[test]
    fn the_architect_builds_its_shapes_anywhere() {
        let shape = template(TEMPLATES[2]);
        let mut grid = Grid::new(9, 13);
        for &(up, c, label) in &shape {
            let (row, col) = (grid.h - 1 - up, grid.w - 1 - c);
            grid[row][col] = label - b'A' + 1;
        }
        let full = shape.len() as f32 * TEMPLATE_PLACED;
        assert!(
            template_value(&grid, &shape) > full,
            "a GTR built against the right wall is not seen"
        );
    }

    #[test]
    fn harder_cpus_attack_more_and_none_dies_alone() {
        let mut sent = Vec::new();
        for difficulty in Difficulty::ALL {
            let mut total = 0;
            for seed in 0..2 {
                let run = play_alone(difficulty, seed, 75 * 60);
                assert!(run.alive, "{difficulty:?} topped out alone on seed {seed}");
                total += run.sent;
            }
            sent.push(total);
        }
        assert!(sent.is_sorted_by(|a, b| a < b), "nuisance sent: {sent:?}");
    }

    struct Run {
        ticks: u32,
        pairs: u32,
        best_chain: u32,
        chains: Vec<u32>,
        sent: u32,
        all_clears: u32,
        alive: bool,
    }

    fn play_alone(difficulty: Difficulty, seed: u64, ticks: u32) -> Run {
        play_styled(difficulty, Style::Balanced, seed, ticks)
    }

    fn play_styled(difficulty: Difficulty, style: Style, seed: u64, ticks: u32) -> Run {
        play_on(difficulty, style, seed, ticks, &Rules::default())
    }

    fn play_on(difficulty: Difficulty, style: Style, seed: u64, ticks: u32, rules: &Rules) -> Run {
        let mut board = started_on(seed, rules);
        let mut cpu = Cpu::new(difficulty, style, seed);
        let mut run = Run {
            ticks: 0,
            pairs: 0,
            best_chain: 0,
            chains: Vec::new(),
            sent: 0,
            all_clears: 0,
            alive: true,
        };
        let mut last = (board.piece_id, 0);
        for _ in 0..ticks {
            if board.state() == GameState::GameOver {
                run.alive = false;
                break;
            }
            let input = cpu.input(&board, 0);
            let events = board.step(input, 0);
            run.sent += events.sent;
            run.all_clears += u32::from(events.all_clear);
            run.ticks += 1;
            if board.piece_id != last.0 {
                if last.1 > 0 {
                    run.chains.push(last.1);
                }
                last = (board.piece_id, 0);
                run.pairs += 1;
            }
            last.1 = last.1.max(board.chain_count);
            run.best_chain = run.best_chain.max(board.chain_count);
        }
        run
    }

    fn duel(levels: [Difficulty; 2], seed: u64) -> (usize, u32) {
        duel_styled(levels.map(|level| (level, Style::Balanced)), seed)
    }

    /// Nuisance on its way, as (to, tick it lands, amount).
    type InFlight = Vec<(usize, u32, u32)>;

    /// Takes off what lands on `to` at `tick`: what lands, then what is
    /// still travelling.
    fn arrivals(in_flight: &mut InFlight, to: usize, tick: u32) -> (u32, u32) {
        let landed = in_flight.iter().filter(|g| g.0 == to && g.1 <= tick).map(|g| g.2).sum();
        in_flight.retain(|g| g.0 != to || g.1 > tick);
        let travelling = in_flight.iter().filter(|g| g.0 == to).map(|g| g.2).sum();
        (landed, travelling)
    }

    fn duel_styled(players: [(Difficulty, Style); 2], seed: u64) -> (usize, u32) {
        let mut boards = [started(seed), started(seed)];
        let mut cpus = [
            Cpu::new(players[0].0, players[0].1, seed),
            Cpu::new(players[1].0, players[1].1, seed + 1000),
        ];
        let mut in_flight = InFlight::new();
        for tick in 1..=30 * 60 * 60 {
            let mut sent = [0; 2];
            for i in 0..2 {
                let (landed, travelling) = arrivals(&mut in_flight, i, tick);
                let input = cpus[i].input(&boards[i], boards[i].pending_garbage + landed + travelling);
                sent[i] = boards[i].step(input, landed).sent;
            }
            for (i, amount) in sent.into_iter().enumerate() {
                if amount > 0 {
                    in_flight.push((1 - i, tick + config::GARBAGE_TRAVEL_TICKS, amount));
                }
            }
            let lost = [0, 1].map(|i| boards[i].state() == GameState::GameOver);
            if lost[0] || lost[1] {
                return (usize::from(lost[0] && !lost[1]), tick);
            }
        }
        panic!("{players:?} never finished on seed {seed}");
    }

    /// Plays alone under `amount` nuisance sent at `first`, then every
    /// `every` ticks if that is not 0: the tick it topped out on, if it did.
    fn play_attacked(difficulty: Difficulty, style: Style, seed: u64, (amount, first, every): Attack) -> Option<u32> {
        let mut board = started(seed);
        let mut cpu = Cpu::new(difficulty, style, seed);
        let mut in_flight = InFlight::new();
        for tick in 1..=DEFENCE_TICKS {
            if tick == first || (every > 0 && tick > first && (tick - first) % every == 0) {
                in_flight.push((0, tick + config::GARBAGE_TRAVEL_TICKS, amount));
            }
            let (landed, travelling) = arrivals(&mut in_flight, 0, tick);
            let input = cpu.input(&board, board.pending_garbage + landed + travelling);
            board.step(input, landed);
            if board.state() == GameState::GameOver {
                return Some(tick);
            }
        }
        None
    }

    type Attack = (u32, u32, u32);

    const DEFENCE_TICKS: u32 = 2 * 60 * 60;
    const ATTACKS: [Attack; 5] = [
        (18, 30 * 60, 0),
        (30, 30 * 60, 0),
        (48, 30 * 60, 0),
        (72, 30 * 60, 0),
        (6, 10 * 60, 10 * 60),
    ];

    #[test]
    #[ignore = "prints how long each CPU holds under attack, to tune its defence"]
    fn report_defence() {
        let seeds = std::env::var("SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
        let only = std::env::var("STYLES").unwrap_or_default();
        for difficulty in Difficulty::ALL {
            for style in Style::ALL
                .into_iter()
                .filter(|s| only.is_empty() || only.contains(&format!("{s:?}")))
            {
                let cells: Vec<String> = ATTACKS
                    .iter()
                    .map(|&attack| {
                        let deaths: Vec<u32> = (0..seeds)
                            .filter_map(|seed| play_attacked(difficulty, style, seed, attack))
                            .collect();
                        let mean = deaths.iter().sum::<u32>() as f32 / deaths.len().max(1) as f32 / 60.0;
                        let (amount, _, every) = attack;
                        let name = if every == 0 {
                            format!("{amount} once")
                        } else {
                            format!("{amount}/{}s", every / 60)
                        };
                        format!("{name}: {}/{seeds} dead ({mean:.0} s)", deaths.len())
                    })
                    .collect();
                println!("{difficulty:?} {style:?}: {}", cells.join(", "));
            }
        }
    }

    #[test]
    #[ignore = "prints how each style plays, to tune them"]
    fn report_styles() {
        let difficulty = match std::env::var("LEVEL").as_deref() {
            Ok("easy") => Difficulty::Easy,
            Ok("normal") => Difficulty::Normal,
            _ => Difficulty::Hard,
        };
        let only = std::env::var("STYLES").unwrap_or_default();
        for style in Style::ALL
            .into_iter()
            .filter(|s| only.is_empty() || only.contains(&format!("{s:?}")))
        {
            let (mut pairs, mut sent, mut deaths, mut ticks, mut clears) = (0, 0, 0, 0, 0);
            let mut hist = [0u32; 12];
            let seeds = 40;
            for seed in 0..seeds {
                let run = play_styled(difficulty, style, seed, 3 * 60 * 60);
                pairs += run.pairs;
                sent += run.sent;
                ticks += run.ticks;
                clears += run.all_clears;
                deaths += u32::from(!run.alive);
                for c in run.chains {
                    hist[(c as usize).min(11)] += 1;
                }
            }
            let minutes = ticks as f32 / 3600.0;
            let mut wins = 0;
            let games = 30;
            for seed in 0..games {
                let (winner, _) = duel_styled([(difficulty, style), (difficulty, Style::Balanced)], seed);
                wins += u32::from(winner == 0);
            }
            println!(
                "{style:?}: {:.1} pairs/min, {:.0} nuisance/min, {deaths}/{seeds} deaths, {clears} all clears, chains {:?}, beats balanced {wins}/{games}",
                pairs as f32 / minutes,
                sent as f32 / minutes,
                &hist[1..],
            );
        }
    }

    #[test]
    #[ignore = "prints how each difficulty plays, to tune the search"]
    fn report() {
        for difficulty in Difficulty::ALL {
            let t0 = std::time::Instant::now();
            let (mut pairs, mut sent, mut deaths, mut ticks) = (0, 0, 0, 0);
            let mut hist = [0u32; 12];
            let seeds = 100;
            for seed in 0..seeds {
                let run = play_alone(difficulty, seed, 3 * 60 * 60);
                pairs += run.pairs;
                sent += run.sent;
                ticks += run.ticks;
                deaths += u32::from(!run.alive);
                for c in run.chains {
                    hist[(c as usize).min(11)] += 1;
                }
            }
            let minutes = ticks as f32 / 3600.0;
            println!(
                "{difficulty:?}: {:.1} pairs/min, {:.0} nuisance/min, {deaths}/{seeds} deaths, chains {:?}, {:?}/pair",
                pairs as f32 / minutes,
                sent as f32 / minutes,
                &hist[1..],
                t0.elapsed() / pairs.max(1)
            );
        }
        for levels in [
            [Difficulty::Easy, Difficulty::Easy],
            [Difficulty::Easy, Difficulty::Normal],
            [Difficulty::Normal, Difficulty::Hard],
            [Difficulty::Hard, Difficulty::Pro],
        ] {
            let (mut wins, mut ticks) = ([0; 2], 0);
            let games = 60;
            for seed in 0..games {
                let (winner, length) = duel(levels, seed);
                wins[winner] += 1;
                ticks += length;
            }
            println!(
                "{levels:?}: wins {wins:?}, {:.0} s on average",
                ticks as f32 / games as f32 / 60.0
            );
        }
    }
}
