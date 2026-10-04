use std::collections::VecDeque;

use shared::{config, Board, GameState, InputKind, PuyoType};

use crate::state::TurnAnim;

const STEP: f32 = 1.0 / config::SERVER_TICK_HZ as f32;
/// Ticks between two scripted keys: slow enough to follow each move.
const INPUT_GAP: u32 = 14;
const INTRO_SECS: f32 = 4.0;
const PAUSE_SECS: f32 = 2.8;
const OUTRO_SECS: f32 = 4.0;

/// A pair to play: its colours (the one turned around first), where its
/// first Puyo lands, its rotation as in `ActivePuyo`, and the nuisance the
/// opponent sends just before it.
struct Drop {
    pair: (char, char),
    col: i32,
    rot: usize,
    incoming: u32,
}

const fn drop(pair: (char, char), col: i32, rot: usize) -> Drop {
    Drop {
        pair,
        col,
        rot,
        incoming: 0,
    }
}

struct Lesson {
    title: &'static str,
    intro: &'static str,
    /// The board's lowest rows, top first: R B Y G P for colours, X for
    /// nuisance, `.` for nothing.
    board: &'static [&'static str],
    drops: &'static [Drop],
    /// What each pop shows, in order, while the game stops on it.
    pops: &'static [&'static str],
    garbage: &'static str,
    outro: &'static str,
}

const LESSONS: &[Lesson] = &[
    Lesson {
        title: "Les bases",
        intro: "Déplacez et tournez la paire qui tombe, puis posez-la.",
        board: &[],
        drops: &[drop(('B', 'Y'), 4, 1), drop(('R', 'R'), 0, 0), drop(('R', 'R'), 1, 0)],
        pops: &["Quatre Puyos de la même couleur se touchent: ils éclatent."],
        garbage: "",
        outro: "Plus un groupe est grand, plus il rapporte de points.",
    },
    Lesson {
        title: "La chaîne",
        intro: "Un groupe qui éclate fait tomber ce qu'il y a au-dessus de lui.",
        board: &[".B....", "BR....", "BR....", "BR...."],
        drops: &[drop(('R', 'R'), 2, 0)],
        pops: &[
            "Les rouges éclatent: le bleu posé dessus va tomber...",
            "...et compléter les bleus. Chaîne de 2: la 2e étape rapporte plus.",
        ],
        garbage: "",
        outro: "Préparer ces chutes à l'avance, c'est tout l'art du jeu.",
    },
    Lesson {
        title: "Stratégie: l'escalier",
        intro: "Chaque colonne prépare la suivante. Une seule paire va tout déclencher.",
        board: &[".GYB..", "GYBR..", "GYBR..", "GYBR.."],
        drops: &[drop(('R', 'R'), 4, 0)],
        pops: &[
            "1: la paire complète les rouges.",
            "2: le bleu tombe et complète les bleus.",
            "3: puis le jaune rejoint les jaunes.",
            "4: et le vert les verts. Chaîne de 4 !",
        ],
        garbage: "",
        outro: "Construisez sans déclencher trop tôt: une longue chaîne envoie une grosse attaque.",
    },
    Lesson {
        title: "Les nuisances",
        intro: "Les Puyos gris viennent de l'adversaire et ne forment pas de groupe.",
        board: &["RR....", "XXXXXX"],
        drops: &[
            drop(('R', 'R'), 2, 1),
            Drop {
                incoming: 6,
                ..drop(('B', 'Y'), 1, 0)
            },
        ],
        pops: &["Un gris disparaît quand un groupe éclate juste à côté."],
        garbage: "L'adversaire a fait une chaîne: ses nuisances tombent après votre paire.",
        outro: "Votre propre chaîne annule d'abord les nuisances qui vous attendent.",
    },
    Lesson {
        title: "Stratégie: l'All Clear",
        intro: "Vider tout le plateau donne un bonus.",
        board: &["BB....", "RR...."],
        drops: &[drop(('R', 'R'), 2, 1), drop(('B', 'B'), 2, 1)],
        pops: &[
            "Les rouges éclatent, les bleus tombent.",
            "Les derniers bleus éclatent...",
        ],
        garbage: "",
        outro: "...plus rien: All Clear ! Votre prochaine chaîne enverra une grosse attaque en plus.",
    },
];

fn puyo(c: char) -> Option<PuyoType> {
    Some(match c {
        'R' => PuyoType::Red,
        'B' => PuyoType::Blue,
        'Y' => PuyoType::Yellow,
        'G' => PuyoType::Green,
        'P' => PuyoType::Purple,
        'X' => PuyoType::Garbage,
        _ => return None,
    })
}

fn pair(colours: (char, char)) -> (PuyoType, PuyoType) {
    let colour = |c| puyo(c).unwrap_or(PuyoType::Red);
    (colour(colours.0), colour(colours.1))
}

fn board_for(lesson: &Lesson) -> Board {
    let mut board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 1, 1, 5);
    let top = config::GRID_HEIGHT - lesson.board.len();
    for (r, row) in lesson.board.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            board.cells[top + r][c] = puyo(ch);
        }
    }
    board.spawn_piece();
    board
}

/// The keys that take a freshly spawned pair to `d`. It then falls at its
/// normal speed, slow enough to follow.
fn keys_for(d: &Drop) -> VecDeque<InputKind> {
    let turns = match d.rot {
        1 => vec![InputKind::RotateCW],
        2 => vec![InputKind::RotateCW, InputKind::RotateCW],
        3 => vec![InputKind::RotateCCW],
        _ => vec![],
    };
    let dx = d.col - config::SPAWN_COL as i32;
    let side = if dx < 0 {
        InputKind::MoveLeft
    } else {
        InputKind::MoveRight
    };
    let moves = std::iter::repeat_n(side, dx.unsigned_abs() as usize);
    turns.into_iter().chain(moves).collect()
}

const fn key_label(input: InputKind) -> Option<&'static str> {
    match input {
        InputKind::MoveLeft => Some("Gauche"),
        InputKind::MoveRight => Some("Droite"),
        InputKind::RotateCW => Some("Tourner (horaire)"),
        InputKind::RotateCCW => Some("Tourner (anti-horaire)"),
        InputKind::SoftDropPress => Some("Descendre"),
        InputKind::HardDrop => Some("Poser"),
        InputKind::SoftDropRelease => None,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Intro(f32),
    Play,
    Paused(f32),
    Outro(f32),
}

/// The help screen's demo: short lessons played on a real board, which stop
/// to explain what just happened.
pub struct Demo {
    lesson: usize,
    pub board: Board,
    pub turn: TurnAnim,
    phase: Phase,
    next_drop: usize,
    piece_id: u32,
    keys: VecDeque<InputKind>,
    wait: u32,
    clock: f32,
    last_chain: u32,
    pops: usize,
    pub key: Option<(&'static str, f32)>,
    pub chain: Option<(u32, f32)>,
    text: &'static str,
}

impl Demo {
    pub fn new() -> Self {
        Self::at(0)
    }

    fn at(lesson: usize) -> Self {
        let l = &LESSONS[lesson];
        let mut demo = Self {
            lesson,
            board: board_for(l),
            turn: TurnAnim::default(),
            phase: Phase::Intro(INTRO_SECS),
            next_drop: 0,
            piece_id: 0,
            keys: VecDeque::new(),
            wait: INPUT_GAP,
            clock: 0.0,
            last_chain: 0,
            pops: 0,
            key: None,
            chain: None,
            text: l.intro,
        };
        demo.take_next_drop();
        demo
    }

    /// The lesson's name and number, shown above the board.
    pub fn title(&self) -> String {
        format!("{}/{}  {}", self.lesson + 1, LESSONS.len(), LESSONS[self.lesson].title)
    }

    /// The explanation on screen: while the game stands still only.
    pub fn text(&self) -> Option<&'static str> {
        (self.phase != Phase::Play).then_some(self.text)
    }

    /// Dresses the pair that just appeared as the next scripted one.
    fn take_next_drop(&mut self) {
        let l = &LESSONS[self.lesson];
        self.piece_id = self.board.piece_id;
        let Some(d) = l.drops.get(self.next_drop) else {
            self.board.active_piece = None;
            self.phase = Phase::Outro(OUTRO_SECS);
            self.text = l.outro;
            return;
        };
        if let Some(piece) = self.board.active_piece.as_mut() {
            (piece.axis_type, piece.sat_type) = pair(d.pair);
        }
        let upcoming = |i: usize| l.drops.get(i).map(|d| pair(d.pair));
        if let Some(next) = upcoming(self.next_drop + 1) {
            self.board.next_types = next;
        }
        if let Some(next) = upcoming(self.next_drop + 2) {
            self.board.next_next_types = next;
        }
        self.board.pending_garbage += d.incoming;
        self.keys = keys_for(d);
        self.wait = INPUT_GAP;
        self.next_drop += 1;
    }

    pub fn update(&mut self, dt: f32) {
        if let Some(key) = self.key.as_mut() {
            key.1 -= dt;
        }
        if let Some(chain) = self.chain.as_mut() {
            chain.1 -= dt;
        }
        self.key = self.key.filter(|k| k.1 > 0.0);
        self.chain = self.chain.filter(|c| c.1 > 0.0);
        match &mut self.phase {
            Phase::Intro(left) | Phase::Paused(left) => {
                *left -= dt;
                if *left <= 0.0 {
                    self.phase = Phase::Play;
                }
            }
            Phase::Outro(left) => {
                *left -= dt;
                if *left <= 0.0 {
                    *self = Self::at((self.lesson + 1) % LESSONS.len());
                }
                return;
            }
            Phase::Play => {}
        }
        if self.phase != Phase::Play {
            return;
        }
        self.clock += dt;
        while self.clock >= STEP && self.phase == Phase::Play {
            self.clock -= STEP;
            self.step();
        }
        if let Some(p) = &self.board.active_piece {
            self.turn.update(self.board.piece_id, p.rotation, dt);
        }
    }

    fn step(&mut self) {
        let mut inputs = Vec::new();
        if self.board.state == GameState::Playing {
            if self.wait > 0 {
                self.wait -= 1;
            } else if let Some(key) = self.keys.pop_front() {
                if let Some(label) = key_label(key) {
                    self.key = Some((label, 1.2));
                    self.wait = INPUT_GAP;
                }
                inputs.push(key);
            }
        }
        let was_dropping = self.board.state == GameState::DroppingGarbage;
        self.board.step(inputs, 0);
        let l = &LESSONS[self.lesson];
        let chain = self.board.chain_count;
        if chain > self.last_chain {
            self.chain = Some((chain, 2.0));
            if let Some(text) = l.pops.get(self.pops) {
                self.text = text;
                self.phase = Phase::Paused(PAUSE_SECS);
            }
            self.pops += 1;
        }
        self.last_chain = chain;
        if !was_dropping && self.board.state == GameState::DroppingGarbage && !l.garbage.is_empty() {
            self.text = l.garbage;
            self.phase = Phase::Paused(PAUSE_SECS);
        }
        if self.board.piece_id != self.piece_id {
            self.take_next_drop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Seen {
        best_chain: u32,
        pops: usize,
        garbage_left: usize,
        all_clear: bool,
        dropped_garbage: bool,
    }

    /// Plays a lesson through, skipping its pauses.
    fn play(lesson: usize) -> Seen {
        let mut demo = Demo::at(lesson);
        let mut seen = Seen {
            best_chain: 0,
            pops: 0,
            garbage_left: 0,
            all_clear: false,
            dropped_garbage: false,
        };
        for _ in 0..60 * 120 {
            if let Phase::Outro(_) = demo.phase {
                seen.garbage_left = demo
                    .board
                    .cells
                    .iter()
                    .flatten()
                    .filter(|c| **c == Some(PuyoType::Garbage))
                    .count();
                seen.all_clear = demo.board.last_was_all_clear;
                seen.pops = demo.pops;
                return seen;
            }
            demo.phase = Phase::Play;
            demo.step();
            seen.best_chain = seen.best_chain.max(demo.board.chain_count);
            seen.dropped_garbage |= demo.board.state == GameState::DroppingGarbage;
            assert_ne!(demo.board.state, GameState::GameOver, "lesson {lesson} lost");
        }
        panic!("lesson {lesson} never ended");
    }

    #[test]
    fn the_basics_pop_one_group() {
        assert_eq!(play(0).best_chain, 1);
    }

    #[test]
    fn the_chain_lesson_chains_twice() {
        assert_eq!(play(1).best_chain, 2);
    }

    #[test]
    fn the_stairs_fire_a_four_chain_from_one_pair() {
        assert_eq!(play(2).best_chain, 4);
    }

    #[test]
    fn the_nuisance_lesson_clears_grey_then_takes_an_attack() {
        let seen = play(3);
        assert_eq!(seen.best_chain, 1);
        assert!(seen.dropped_garbage, "the attack never fell");
        assert_eq!(
            seen.garbage_left,
            2 + 6,
            "two greys untouched by the pop, six from the attack"
        );
    }

    #[test]
    fn the_all_clear_lesson_empties_the_board() {
        assert!(play(4).all_clear);
    }

    #[test]
    fn every_lesson_explains_each_of_its_pops() {
        for (i, l) in LESSONS.iter().enumerate() {
            assert_eq!(play(i).pops, l.pops.len(), "lesson {i}: one explanation per pop");
        }
    }

    #[test]
    fn the_demo_loops_through_every_lesson() {
        let mut demo = Demo::new();
        let mut visited = vec![false; LESSONS.len()];
        for _ in 0..60 * 600 {
            visited[demo.lesson] = true;
            demo.update(STEP);
        }
        assert!(visited.iter().all(|v| *v), "{visited:?}");
    }
}
