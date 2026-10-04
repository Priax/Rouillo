use std::collections::VecDeque;

use shared::{config, Board, GameState, InputKind, PuyoType, RoomSettings};

use crate::cpu::{Cpu, Difficulty, Style};
use crate::state::TurnAnim;

const STEP: f32 = 1.0 / config::SERVER_TICK_HZ as f32;
/// Ticks between two scripted keys: slow enough to follow each move.
const INPUT_GAP: u32 = 14;
const INTRO_SECS: f32 = 4.0;
const PAUSE_SECS: f32 = 2.8;
const OUTRO_SECS: f32 = 4.0;

/// A game the hard CPU plays alone, chosen because its first pop is a 5-chain.
const CPU_SEED: u64 = 86;
/// The pairs of the match between two CPUs, chosen because the GTR wins it
/// with a 6-chain after a long fight.
const VERSUS_SEED: u64 = 38;
/// Makes the architect build a GTR, the third of its shapes.
const ARCHITECT_SEED: u64 = 2;

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

enum Play {
    /// Pairs placed one by one on a board given by its lowest rows, top
    /// first: R B Y G P for colours, X for nuisance, `.` for nothing.
    Script {
        board: &'static [&'static str],
        drops: &'static [Drop],
    },
    /// The hard CPU playing a real game, up to its 5-chain.
    Cpu,
    /// Two hard CPUs playing each other, until one loses.
    Versus,
}

struct Lesson {
    title: &'static str,
    intro: &'static str,
    play: Play,
    /// What each pop shows, in order, while the game stops on it.
    pops: &'static [&'static str],
    garbage: &'static str,
    outro: &'static str,
}

const LESSONS: &[Lesson] = &[
    Lesson {
        title: "Les bases",
        intro: "Déplacez et tournez la paire qui tombe, puis posez-la.",
        play: Play::Script {
            board: &[],
            drops: &[drop(('B', 'Y'), 4, 1), drop(('R', 'R'), 0, 0), drop(('R', 'R'), 1, 0)],
        },
        pops: &["Quatre Puyos de la même couleur se touchent: ils éclatent."],
        garbage: "",
        outro: "Plus un groupe est grand, plus il rapporte de points.",
    },
    Lesson {
        title: "La chaîne",
        intro: "Un groupe qui éclate fait tomber ce qu'il y a au-dessus de lui.",
        play: Play::Script {
            board: &[".B....", "BR....", "BR....", "BR...."],
            drops: &[drop(('R', 'R'), 2, 0)],
        },
        pops: &[
            "Le groupe du bas éclate: ce qui était posé dessus va tomber...",
            "...et compléter un autre groupe. Chaîne de 2: la 2e étape rapporte plus.",
        ],
        garbage: "",
        outro: "Préparer ces chutes à l'avance, c'est tout l'art du jeu.",
    },
    Lesson {
        title: "Stratégie: l'escalier",
        intro: "Chaque colonne prépare la suivante. Une seule paire va tout déclencher.",
        play: Play::Script {
            board: &[".GYB..", "GYBR..", "GYBR..", "GYBR.."],
            drops: &[drop(('R', 'R'), 4, 0)],
        },
        pops: &[
            "La paire complète le groupe de droite.",
            "Le Puyo posé dessus tombe dans la colonne voisine.",
            "Même chose, une colonne plus loin.",
            "Et encore une fois. Chaîne de 4 !",
        ],
        garbage: "",
        outro: "Construisez sans déclencher trop tôt: une longue chaîne envoie une grosse attaque.",
    },
    Lesson {
        title: "Technique: le sandwich",
        intro: "Un groupe coincé entre deux Puyos d'une autre couleur.",
        play: Play::Script {
            board: &["B.....", "R.....", "R.....", "R.....", "B.....", "BB...."],
            drops: &[drop(('R', 'R'), 1, 0)],
        },
        pops: &[
            "Le groupe du milieu éclate: il n'y a plus rien entre les deux bleus...",
            "...le bleu du haut retombe sur ceux du bas. Chaîne de 2 dans une seule colonne.",
        ],
        garbage: "",
        outro: "Le sandwich empile une chaîne en hauteur, sans prendre de place sur les côtés.",
    },
    Lesson {
        title: "Les nuisances",
        intro: "Les Puyos gris viennent de l'adversaire et ne forment pas de groupe.",
        play: Play::Script {
            board: &["RR....", "XXXXXX"],
            drops: &[
                drop(('R', 'R'), 2, 1),
                Drop {
                    incoming: 6,
                    ..drop(('B', 'Y'), 1, 0)
                },
            ],
        },
        pops: &["Un gris disparaît quand un groupe éclate juste à côté."],
        garbage: "L'adversaire a fait une chaîne: ses nuisances tombent après votre paire.",
        outro: "Votre propre chaîne annule d'abord les nuisances qui vous attendent.",
    },
    Lesson {
        title: "Un début de partie",
        intro: "On range trois couleurs en escalier sans rien faire éclater, puis une paire déclenche tout.",
        play: Play::Script {
            board: &[],
            drops: &[
                drop(('G', 'G'), 0, 0),
                drop(('Y', 'G'), 1, 3),
                drop(('Y', 'Y'), 1, 0),
                drop(('R', 'R'), 2, 0),
                drop(('G', 'R'), 1, 1),
                drop(('Y', 'R'), 2, 1),
            ],
        },
        pops: &[
            "Le dernier rouge, posé à droite, complète les rouges.",
            "Le jaune qui reposait sur les rouges tombe sur les jaunes.",
            "Et le vert, sur les verts.",
        ],
        garbage: "",
        outro: "Plus un seul Puyo: All Clear ! Votre prochaine chaîne enverra une grosse attaque en plus.",
    },
    Lesson {
        title: "La défaite",
        intro: "Le haut du plateau, sous la croix, doit rester libre.",
        play: Play::Script {
            board: &[
                "RBYGRB", "YGRBYG", "RBYGRB", "YGRBYG", "RBYGRB", "YGRBYG", "RBYGRB", "YGRBYG",
            ],
            drops: &[Drop {
                incoming: 30,
                ..drop(('P', 'P'), 0, 0)
            }],
        },
        pops: &[],
        garbage: "L'adversaire envoie une énorme attaque sur un plateau déjà haut...",
        outro:
            "...les gris recouvrent la croix: la partie est perdue. Gardez de la place, et répondez par vos chaînes.",
    },
    Lesson {
        title: "Une vraie partie",
        intro: "L'IA difficile joue normalement et prépare une chaîne de 5.",
        play: Play::Cpu,
        pops: &[],
        garbage: "",
        outro: "Avec de l'entraînement, c'est vous qui jouerez comme ça.",
    },
    Lesson {
        title: "Un match entre IA",
        intro: "Deux IA difficiles s'affrontent avec les mêmes paires. À gauche, celle qui monte un GTR, une forme célèbre.",
        play: Play::Versus,
        pops: &[],
        garbage: "",
        outro: "Le GTR a tenu: ses chaînes ont enterré l'adversaire sous les nuisances.",
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

fn board_for(art: &[&str]) -> Board {
    let mut board = Board::new(config::GRID_WIDTH, config::GRID_HEIGHT, 1, 1, 5);
    let top = config::GRID_HEIGHT - art.len();
    for (r, row) in art.iter().enumerate() {
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

/// The right-hand player of a match between two CPUs.
pub struct Rival {
    pub board: Board,
    cpu: Cpu,
    pub turn: TurnAnim,
    pub chain: Option<(u32, f32)>,
    last_chain: u32,
}

fn versus(seed: u64) -> (Board, Option<Cpu>, Option<Rival>) {
    let board = Board::for_match(seed, &RoomSettings::default());
    let rival = Rival {
        board: board.clone(),
        cpu: Cpu::new(Difficulty::Hard, Style::Balanced, seed),
        turn: TurnAnim::default(),
        chain: None,
        last_chain: 0,
    };
    let cpu = Cpu::new(Difficulty::Hard, Style::Architect, ARCHITECT_SEED);
    (board, Some(cpu), Some(rival))
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
    cpu: Option<Cpu>,
    pub rival: Option<Rival>,
    /// Nuisance on its way: to the rival or not, the tick it lands, how much.
    in_flight: Vec<(bool, u32, u32)>,
    tick: u32,
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
        let (board, cpu, rival) = match l.play {
            Play::Script { board, .. } => (board_for(board), None, None),
            Play::Cpu => (
                Board::for_match(CPU_SEED, &RoomSettings::default()),
                Some(Cpu::new(Difficulty::Hard, Style::Balanced, CPU_SEED)),
                None,
            ),
            Play::Versus => versus(VERSUS_SEED),
        };
        let mut demo = Self {
            lesson,
            board,
            turn: TurnAnim::default(),
            phase: Phase::Intro(INTRO_SECS),
            next_drop: 0,
            piece_id: 0,
            keys: VecDeque::new(),
            wait: INPUT_GAP,
            clock: 0.0,
            last_chain: 0,
            pops: 0,
            cpu,
            rival,
            in_flight: Vec::new(),
            tick: 0,
            key: None,
            chain: None,
            text: l.intro,
        };
        if demo.cpu.is_none() {
            demo.take_next_drop();
        }
        demo
    }

    /// The lesson's name and number, shown above the board.
    pub fn title(&self) -> String {
        format!("{}/{}  {}", self.lesson + 1, LESSONS.len(), LESSONS[self.lesson].title)
    }

    /// The players' names, left then right, in a match between two CPUs.
    pub fn players(&self) -> Option<[&'static str; 2]> {
        self.rival
            .as_ref()
            .map(|_| [Style::Architect.character(), Style::Balanced.character()])
    }

    /// The explanation on screen: while the game stands still only.
    pub fn text(&self) -> Option<&'static str> {
        (self.phase != Phase::Play).then_some(self.text)
    }

    /// Dresses the pair that just appeared as the next scripted one.
    fn take_next_drop(&mut self) {
        let l = &LESSONS[self.lesson];
        self.piece_id = self.board.piece_id;
        let Play::Script { drops, .. } = l.play else { return };
        let Some(d) = drops.get(self.next_drop) else {
            self.board.active_piece = None;
            self.end();
            return;
        };
        if let Some(piece) = self.board.active_piece.as_mut() {
            (piece.axis_type, piece.sat_type) = pair(d.pair);
        }
        let upcoming = |i: usize| drops.get(i).map(|d| pair(d.pair));
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
        if let Some(rival) = self.rival.as_mut() {
            rival.chain = rival.chain.map(|(n, t)| (n, t - dt)).filter(|c| c.1 > 0.0);
            if let Some(p) = &rival.board.active_piece {
                rival.turn.update(rival.board.piece_id, p.rotation, dt);
            }
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

    fn end(&mut self) {
        self.phase = Phase::Outro(OUTRO_SECS);
        self.text = LESSONS[self.lesson].outro;
    }

    fn step(&mut self) {
        if self.rival.is_some() {
            self.versus_step();
            return;
        }
        if let Some(cpu) = self.cpu.as_mut() {
            let input = cpu.input(&self.board, 0);
            if let Some(label) = input.and_then(key_label) {
                self.key = Some((label, 0.6));
            }
            self.board.step(input, 0);
            self.explain_pops();
            let chain_over = self.pops > 0 && self.board.chain_count == 0;
            if chain_over || self.board.state == GameState::GameOver {
                self.end();
            }
            return;
        }
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
        self.explain_pops();
        let l = &LESSONS[self.lesson];
        if !was_dropping && self.board.state == GameState::DroppingGarbage && !l.garbage.is_empty() {
            self.text = l.garbage;
            self.phase = Phase::Paused(PAUSE_SECS);
        }
        if self.board.state == GameState::GameOver {
            self.end();
        } else if self.board.piece_id != self.piece_id {
            self.take_next_drop();
        }
    }

    fn versus_step(&mut self) {
        let Self {
            board,
            cpu: Some(cpu),
            rival: Some(rival),
            in_flight,
            tick,
            ..
        } = self
        else {
            return;
        };
        *tick += 1;
        let now = *tick;
        let mut sent = [0; 2];
        for (i, (board, cpu)) in [(board, cpu), (&mut rival.board, &mut rival.cpu)]
            .into_iter()
            .enumerate()
        {
            let to_me = |g: &&(bool, u32, u32)| g.0 == (i == 1);
            let landed: u32 = in_flight.iter().filter(to_me).filter(|g| g.1 <= now).map(|g| g.2).sum();
            let travelling: u32 = in_flight.iter().filter(to_me).filter(|g| g.1 > now).map(|g| g.2).sum();
            let input = cpu.input(board, board.pending_garbage + landed + travelling);
            sent[i] = board.step(input, landed);
        }
        in_flight.retain(|g| g.1 > now);
        for (i, amount) in sent.into_iter().enumerate() {
            if amount > 0 {
                in_flight.push((i == 0, now + config::GARBAGE_TRAVEL_TICKS, amount));
            }
        }
        let links = rival.board.chain_count;
        if links > rival.last_chain {
            rival.chain = Some((links, 2.0));
        }
        rival.last_chain = links;
        let over = rival.board.state == GameState::GameOver;
        self.explain_pops();
        if over || self.board.state == GameState::GameOver {
            self.end();
        }
    }

    /// Stops on each new link of a chain to say what happened.
    fn explain_pops(&mut self) {
        let chain = self.board.chain_count;
        if chain > self.last_chain {
            self.chain = Some((chain, 2.0));
            if let Some(text) = LESSONS[self.lesson].pops.get(self.pops) {
                self.text = text;
                self.phase = Phase::Paused(PAUSE_SECS);
            }
            self.pops += 1;
        }
        self.last_chain = chain;
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
        lost: bool,
        rival_lost: bool,
        ticks: u32,
    }

    /// Plays a lesson through, skipping its pauses.
    fn play(lesson: usize) -> Seen {
        play_demo(Demo::at(lesson))
    }

    fn play_demo(mut demo: Demo) -> Seen {
        let lesson = demo.lesson;
        let mut seen = Seen {
            best_chain: 0,
            pops: 0,
            garbage_left: 0,
            all_clear: false,
            dropped_garbage: false,
            lost: false,
            rival_lost: false,
            ticks: 0,
        };
        for _ in 0..60 * 180 {
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
                seen.lost = demo.board.state == GameState::GameOver;
                seen.rival_lost = demo.rival.is_some_and(|r| r.board.state == GameState::GameOver);
                return seen;
            }
            demo.phase = Phase::Play;
            demo.step();
            seen.ticks += 1;
            seen.best_chain = seen.best_chain.max(demo.board.chain_count);
            seen.dropped_garbage |= demo.board.state == GameState::DroppingGarbage;
        }
        panic!("lesson {lesson} never ended");
    }

    fn lesson(title: &str) -> usize {
        LESSONS.iter().position(|l| l.title == title).expect("a lesson")
    }

    #[test]
    fn the_basics_pop_one_group() {
        assert_eq!(play(lesson("Les bases")).best_chain, 1);
    }

    #[test]
    fn the_chain_lesson_chains_twice() {
        assert_eq!(play(lesson("La chaîne")).best_chain, 2);
    }

    #[test]
    fn the_stairs_fire_a_four_chain_from_one_pair() {
        assert_eq!(play(lesson("Stratégie: l'escalier")).best_chain, 4);
    }

    #[test]
    fn the_sandwich_chains_twice_in_one_column() {
        assert_eq!(play(lesson("Technique: le sandwich")).best_chain, 2);
    }

    #[test]
    fn the_nuisance_lesson_clears_grey_then_takes_an_attack() {
        let seen = play(lesson("Les nuisances"));
        assert_eq!(seen.best_chain, 1);
        assert!(seen.dropped_garbage, "the attack never fell");
        assert_eq!(
            seen.garbage_left,
            2 + 6,
            "two greys untouched by the pop, six from the attack"
        );
    }

    #[test]
    fn the_opening_ends_in_an_all_clear_three_chain() {
        let seen = play(lesson("Un début de partie"));
        assert_eq!(seen.best_chain, 3);
        assert!(seen.all_clear);
    }

    #[test]
    fn the_defeat_lesson_is_lost_to_an_attack() {
        let seen = play(lesson("La défaite"));
        assert!(seen.lost);
        assert!(seen.dropped_garbage);
        assert_eq!(seen.pops, 0, "nothing pops on the way");
    }

    #[test]
    fn the_real_game_fires_its_five_chain() {
        let seen = play(lesson("Une vraie partie"));
        assert_eq!(seen.best_chain, 5);
        assert_eq!(seen.pops, 5, "the 5-chain is the first pop of the game");
        assert!(!seen.lost);
    }

    #[test]
    fn the_gtr_wins_the_match_with_a_long_chain() {
        let seen = play(lesson("Un match entre IA"));
        assert!(seen.rival_lost && !seen.lost);
        assert_eq!(seen.best_chain, 6);
        assert!(seen.ticks > 30 * 60, "over in {} ticks", seen.ticks);
    }

    #[test]
    fn every_lesson_explains_each_of_its_pops() {
        for (i, l) in LESSONS.iter().enumerate() {
            if matches!(l.play, Play::Script { .. }) {
                assert_eq!(play(i).pops, l.pops.len(), "lesson {i}: one explanation per pop");
            }
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
