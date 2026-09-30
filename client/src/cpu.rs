use shared::{config, Board, GameState, InputKind, PuyoType};

const W: usize = config::GRID_WIDTH;
const H: usize = config::GRID_HEIGHT;
const TOP: usize = config::VISIBLE_ROW_OFFSET;
const SPAWN: usize = config::SPAWN_COL;

type Grid = [[u8; W]; H];
const GARBAGE: u8 = PuyoType::Garbage as u8 + 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

struct Profile {
    think_ticks: u32,
    input_ticks: u32,
    hard_drop: bool,
    lookahead: bool,
    min_chain: u32,
    noise: f32,
}

impl Difficulty {
    pub const ALL: [Self; 3] = [Self::Easy, Self::Normal, Self::Hard];

    pub fn label(self) -> &'static str {
        match self {
            Self::Easy => "Facile",
            Self::Normal => "Normal",
            Self::Hard => "Difficile",
        }
    }

    const fn profile(self) -> Profile {
        match self {
            Self::Easy => Profile {
                think_ticks: 30,
                input_ticks: 18,
                hard_drop: false,
                lookahead: false,
                min_chain: 1,
                noise: 40.0,
            },
            Self::Normal => Profile {
                think_ticks: 26,
                input_ticks: 15,
                hard_drop: false,
                lookahead: true,
                min_chain: 2,
                noise: 20.0,
            },
            Self::Hard => Profile {
                think_ticks: 10,
                input_ticks: 5,
                hard_drop: false,
                lookahead: true,
                min_chain: 5,
                noise: 10.0,
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Placement {
    col: usize,
    rotation: usize,
}

impl Placement {
    fn satellite_col(self) -> usize {
        match self.rotation {
            1 => self.col + 1,
            3 => self.col - 1,
            _ => self.col,
        }
    }
}

fn placements() -> impl Iterator<Item = Placement> {
    (0..4).flat_map(|rotation| {
        let cols = match rotation {
            1 => 0..W - 1,
            3 => 1..W,
            _ => 0..W,
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
        let piece = board.active_piece.as_ref()?;
        if self.dropped {
            return None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return None;
        }
        let col = self.target.col as i32;
        let step = if self.inputs_left == 0 {
            None
        } else if piece.rotation != self.target.rotation {
            Some(match (self.target.rotation + 4 - piece.rotation) % 4 {
                3 => InputKind::RotateCCW,
                _ => InputKind::RotateCW,
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
        if board.state != GameState::Playing || board.piece_id != id {
            return Some(board);
        }
        let input = exec.next(&board, profile);
        board.step(input, 0);
    }
    None
}

fn grid_of(board: &Board) -> Grid {
    let mut grid = [[0; W]; H];
    for (r, row) in board.cells.iter().take(H).enumerate() {
        for (c, cell) in row.iter().take(W).enumerate() {
            grid[r][c] = cell.map_or(0, code);
        }
    }
    grid
}

const fn code(t: PuyoType) -> u8 {
    t as u8 + 1
}

fn neighbours(r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> {
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .filter_map(move |(dr, dc)| {
            let nr = r.checked_add_signed(dr)?;
            let nc = c.checked_add_signed(dc)?;
            (nr < H && nc < W).then_some((nr, nc))
        })
}

struct Group {
    cells: [(usize, usize); W * H],
    len: usize,
}

impl Group {
    fn cells(&self) -> &[(usize, usize)] {
        &self.cells[..self.len]
    }
}

fn flood(grid: &Grid, r: usize, c: usize, seen: &mut [[bool; W]; H]) -> Group {
    let colour = grid[r][c];
    let mut group = Group {
        cells: [(0, 0); W * H],
        len: 1,
    };
    group.cells[0] = (r, c);
    seen[r][c] = true;
    let mut next = 0;
    while next < group.len {
        let (r, c) = group.cells[next];
        next += 1;
        for (nr, nc) in neighbours(r, c) {
            if grid[nr][nc] == colour && !seen[nr][nc] {
                seen[nr][nc] = true;
                group.cells[group.len] = (nr, nc);
                group.len += 1;
            }
        }
    }
    group
}

fn pop_once(grid: &mut Grid, link: u32) -> Option<u32> {
    let mut seen = [[false; W]; H];
    let mut remove = [[false; W]; H];
    let mut colours = [false; 7];
    let (mut cleared, mut group_bonus) = (0, 0);
    for r in 0..H {
        for c in 0..W {
            let cell = grid[r][c];
            if cell == 0 || cell == GARBAGE || seen[r][c] {
                continue;
            }
            let group = flood(grid, r, c, &mut seen);
            if group.len < 4 || group.cells().iter().all(|&(r, _)| r < TOP) {
                continue;
            }
            colours[cell as usize] = true;
            cleared += group.len as u32;
            group_bonus += shared::group_bonus(group.len as u32);
            for &(gr, gc) in group.cells() {
                remove[gr][gc] = true;
                for (nr, nc) in neighbours(gr, gc) {
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
    for r in 0..H {
        for c in 0..W {
            if remove[r][c] {
                grid[r][c] = 0;
            }
        }
    }
    let colour_count = colours.iter().filter(|&&seen| seen).count();
    Some(shared::link_score(link, colour_count, cleared, group_bonus))
}

#[allow(clippy::needless_range_loop)]
fn settle(grid: &mut Grid) {
    for c in 0..W {
        let mut floor = H;
        for r in (0..H).rev() {
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
    (0..H).find(|&r| grid[r][col] != 0).map_or(0, |r| H - r)
}

fn drop_into(grid: &mut Grid, col: usize, cell: u8) -> bool {
    let h = height(grid, col);
    if h == H {
        return false;
    }
    grid[H - 1 - h][col] = cell;
    true
}

const PASSABLE_HEIGHT: usize = 9;

fn place(grid: &Grid, (axis, satellite): (u8, u8), p: Placement) -> Option<Grid> {
    let sat_col = p.satellite_col();
    let lo = SPAWN.min(p.col).min(sat_col);
    let hi = SPAWN.max(p.col).max(sat_col);
    if (lo..=hi).any(|c| height(grid, c) > PASSABLE_HEIGHT) {
        return None;
    }
    let mut grid = *grid;
    let order = if p.rotation == 2 {
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
    for col in 0..W {
        let h = height(grid, col);
        if h + TOP >= H {
            continue;
        }
        let row = H - 1 - h;
        let mut tried = [false; 7];
        for (nr, nc) in neighbours(row, col) {
            let colour = grid[nr][nc];
            if colour == 0 || colour == GARBAGE || tried[colour as usize] {
                continue;
            }
            tried[colour as usize] = true;
            let mut with = *grid;
            with[row][col] = colour;
            if flood(&with, row, col, &mut [[false; W]; H]).len < 4 {
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
const SAFE_HEIGHT: usize = 8;
const SAFE_SPAWN_HEIGHT: usize = 5;
const URGENT_HEIGHT: usize = 9;
const URGENT_THREAT: u32 = 6;
const WASTED_POP: f32 = 0.05;
const POTENTIAL_WEIGHT: f32 = 0.5;
const LATER: f32 = 0.9;
const NO_SECOND_MOVE: f32 = -400.0;
const HEIGHT_COST: f32 = 2.0;
const UNSAFE_HEIGHT_COST: f32 = 400.0;
const UNSAFE_SPAWN_COST: f32 = 300.0;
const PAIR_VALUE: f32 = 12.0;
const TRIPLET_VALUE: f32 = 30.0;

fn shape(grid: &Grid, builds: bool) -> f32 {
    if grid[TOP][SPAWN] != 0 {
        return DEAD;
    }
    let mut value = 0.0;
    for c in 0..W {
        let h = height(grid, c);
        let over = h.saturating_sub(SAFE_HEIGHT) as f32;
        value -= over * over * UNSAFE_HEIGHT_COST + h as f32 * HEIGHT_COST;
    }
    let over = height(grid, SPAWN).saturating_sub(SAFE_SPAWN_HEIGHT) as f32;
    value -= over * over * UNSAFE_SPAWN_COST;

    let mut seen = [[false; W]; H];
    for r in 0..H {
        for c in 0..W {
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
    if builds {
        value += potential(grid).score as f32 * POTENTIAL_WEIGHT;
    }
    value
}

struct Judge<'a> {
    profile: &'a Profile,
    urgent: bool,
}

impl Judge<'_> {
    fn fire(&self, chain: Chain) -> f32 {
        let wanted = self.urgent || chain.links >= self.profile.min_chain;
        chain.score as f32 * if wanted { 1.0 } else { WASTED_POP }
    }

    fn builds(&self) -> bool {
        self.profile.min_chain > 1
    }

    fn after(&self, grid: &Grid, next: (u8, u8)) -> f32 {
        if grid[TOP][SPAWN] != 0 {
            return DEAD;
        }
        if !self.profile.lookahead {
            return shape(grid, self.builds());
        }
        placements()
            .filter_map(|p| place(grid, next, p))
            .map(|mut grid| {
                let chain = resolve(&mut grid);
                LATER * self.fire(chain) + shape(&grid, self.builds())
            })
            .fold(None, |best: Option<f32>, v| Some(best.map_or(v, |b| b.max(v))))
            .unwrap_or_else(|| shape(grid, self.builds()) + NO_SECOND_MOVE)
    }
}

const PLACEMENTS: usize = 4 * W - 2;

struct Search {
    urgent: bool,
    tried: usize,
    elapsed: u32,
    best: (f32, Placement),
}

impl Search {
    const fn new(urgent: bool) -> Self {
        Self {
            urgent,
            tried: 0,
            elapsed: 0,
            best: (
                f32::NEG_INFINITY,
                Placement {
                    col: SPAWN,
                    rotation: 0,
                },
            ),
        }
    }

    const fn done(&self) -> bool {
        self.tried >= PLACEMENTS
    }

    fn run(&mut self, board: &Board, profile: &Profile, wait: u32, count: usize, noise: &mut Noise) {
        let judge = Judge {
            profile,
            urgent: self.urgent,
        };
        let next = (code(board.next_types.0), code(board.next_types.1));
        for target in placements().skip(self.tried).take(count) {
            self.tried += 1;
            let Some(after) = simulate(board, Exec::new(target, wait), profile) else {
                continue;
            };
            let mut grid = grid_of(&after);
            let chain = resolve(&mut grid);
            let value = judge.fire(chain) + judge.after(&grid, next) + noise.next() * profile.noise;
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
    Playing(Exec),
}

pub struct Cpu {
    profile: Profile,
    turn: Option<(u32, Turn)>,
    soft_drop_held: bool,
    noise: Noise,
}

impl Cpu {
    pub fn new(difficulty: Difficulty, seed: u64) -> Self {
        Self {
            profile: difficulty.profile(),
            turn: None,
            soft_drop_held: false,
            noise: Noise((seed as u32) | 1),
        }
    }

    pub fn input(&mut self, board: &Board, threat: u32) -> Option<InputKind> {
        let playing = board.state == GameState::Playing && board.active_piece.is_some();
        let current = self.turn.as_ref().is_some_and(|(id, _)| *id == board.piece_id);
        if self.soft_drop_held && !(playing && current) {
            self.soft_drop_held = false;
            return Some(InputKind::SoftDropRelease);
        }
        if !playing {
            return None;
        }
        if !current {
            let search = Search::new(urgent(board, threat));
            self.turn = Some((board.piece_id, Turn::Thinking(search)));
        }
        let (_, turn) = self.turn.as_mut()?;
        if let Turn::Thinking(search) = turn {
            let think = self.profile.think_ticks;
            let wait = think.saturating_sub(search.elapsed);
            let count = PLACEMENTS.div_ceil(think.max(1) as usize);
            search.run(board, &self.profile, wait, count, &mut self.noise);
            if !search.done() {
                search.elapsed += 1;
                return None;
            }
            *turn = Turn::Playing(Exec::new(search.best.1, wait));
        }
        let Turn::Playing(exec) = turn else {
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
    threat >= URGENT_THREAT || (0..W).any(|c| height(&grid, c) >= URGENT_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_grid(rng: &mut Noise, colours: u32, fill: f32) -> Grid {
        let mut grid = [[0; W]; H];
        for row in &mut grid {
            for cell in row.iter_mut() {
                if rng.next() < fill {
                    *cell = if rng.next() < 0.15 {
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
        let mut board = Board::new(W, H, 1, 1, 5);
        for (r, row) in grid.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                board.cells[r][c] = match cell {
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
        let mut board = Board::new(W, H, seed, 1, 5);
        board.spawn_piece();
        board
    }

    #[test]
    fn a_placement_ends_the_same_whenever_it_is_judged() {
        let profile = Difficulty::Normal.profile();
        let early = started(3);
        let mut late = early.clone();
        let waited = 7;
        for _ in 0..waited {
            late.step(None, 0);
        }
        for target in placements() {
            let from_start = simulate(&early, Exec::new(target, profile.think_ticks), &profile).expect("it locks");
            let from_later =
                simulate(&late, Exec::new(target, profile.think_ticks - waited), &profile).expect("it locks");
            assert_eq!(from_start.state_hash(), from_later.state_hash(), "{target:?}");
        }
    }

    #[test]
    fn the_pair_lands_where_the_search_saw_it_land() {
        for difficulty in Difficulty::ALL {
            let mut board = started(11);
            let mut cpu = Cpu::new(difficulty, 11);
            let mut expected: Option<(u32, Grid)> = None;
            let mut checked = 0;
            for _ in 0..30 * 60 {
                if board.state == GameState::GameOver {
                    break;
                }
                let before = board.clone();
                let thinking = match &cpu.turn {
                    Some((id, Turn::Thinking(search))) if *id == board.piece_id => Some(search.elapsed),
                    Some((id, Turn::Playing(_))) if *id == board.piece_id => None,
                    _ => Some(0),
                };
                let input = cpu.input(&board, 0);
                if let (Some(elapsed), Some((id, Turn::Playing(exec)))) = (thinking, &cpu.turn) {
                    let wait = cpu.profile.think_ticks - elapsed;
                    let seen = simulate(&before, Exec::new(exec.target, wait), &cpu.profile).expect("it locks");
                    expected = Some((*id, grid_of(&seen)));
                }
                board.step(input, 0);
                if let Some((id, grid)) = expected {
                    if board.state != GameState::Playing && board.piece_id == id {
                        assert_eq!(grid_of(&board), grid, "{difficulty:?}, pair {id}");
                        expected = None;
                        checked += 1;
                    }
                }
            }
            assert!(checked > 10, "{difficulty:?}: only {checked} pairs checked");
        }
    }

    #[test]
    fn soft_drop_is_released_before_the_next_pair_falls() {
        let mut board = started(5);
        let mut cpu = Cpu::new(Difficulty::Normal, 5);
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
        assert!(sent[0] < sent[1] && sent[1] < sent[2], "nuisance sent: {sent:?}");
    }

    struct Run {
        ticks: u32,
        pairs: u32,
        best_chain: u32,
        chains: Vec<u32>,
        sent: u32,
        alive: bool,
    }

    fn play_alone(difficulty: Difficulty, seed: u64, ticks: u32) -> Run {
        let mut board = started(seed);
        let mut cpu = Cpu::new(difficulty, seed);
        let mut run = Run {
            ticks: 0,
            pairs: 0,
            best_chain: 0,
            chains: Vec::new(),
            sent: 0,
            alive: true,
        };
        let mut last = (board.piece_id, 0);
        for _ in 0..ticks {
            if board.state == GameState::GameOver {
                run.alive = false;
                break;
            }
            let input = cpu.input(&board, 0);
            run.sent += board.step(input, 0);
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
        let mut boards = [started(seed), started(seed)];
        let mut cpus = [Cpu::new(levels[0], seed), Cpu::new(levels[1], seed + 1000)];
        let mut in_flight: Vec<(usize, u32, u32)> = Vec::new();
        for tick in 1..=30 * 60 * 60 {
            let mut sent = [0; 2];
            for i in 0..2 {
                let landed: u32 = in_flight
                    .iter()
                    .filter(|&&(to, at, _)| to == i && at <= tick)
                    .map(|g| g.2)
                    .sum();
                in_flight.retain(|&(to, at, _)| to != i || at > tick);
                let travelling: u32 = in_flight.iter().filter(|g| g.0 == i).map(|g| g.2).sum();
                let input = cpus[i].input(&boards[i], boards[i].pending_garbage + landed + travelling);
                sent[i] = boards[i].step(input, landed);
            }
            for (i, amount) in sent.into_iter().enumerate() {
                if amount > 0 {
                    in_flight.push((1 - i, tick + config::GARBAGE_TRAVEL_TICKS, amount));
                }
            }
            let lost = [0, 1].map(|i| boards[i].state == GameState::GameOver);
            if lost[0] || lost[1] {
                return (usize::from(lost[0] && !lost[1]), tick);
            }
        }
        panic!("{levels:?} never finished on seed {seed}");
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
