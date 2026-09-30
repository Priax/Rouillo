use notan::prelude::*;
use shared::{config, Board, GameState, IncomingGarbage, RoomSettings};

use crate::cpu::{Cpu, Difficulty};
use crate::draw::Hud;
use crate::state::{GameSession, Screen, State};
use crate::ui::{self, Pill, Rect, SettingsPanel, SharpText, View};
use crate::{logic, theme};

#[derive(Clone, Copy, Default)]
pub struct SoloSettings {
    opponent: Option<Difficulty>,
    room: RoomSettings,
}

impl SoloSettings {
    const COUNT: usize = 3;

    fn label(i: usize) -> &'static str {
        match i {
            0 => "Adversaire",
            _ => RoomSettings::label(i - 1),
        }
    }

    fn value(&self, i: usize) -> String {
        match i {
            0 => self.opponent.map_or("Aucun", Difficulty::label).into(),
            _ => self.room.value(i - 1),
        }
    }

    fn adjust(&mut self, i: usize, dir: i32) {
        if i > 0 {
            self.room.adjust(i - 1, dir);
            return;
        }
        let options: Vec<_> = std::iter::once(None).chain(Difficulty::ALL.map(Some)).collect();
        let at = options.iter().position(|&o| o == self.opponent).unwrap_or(0) as i32;
        self.opponent = options[(at + dir.signum()).rem_euclid(options.len() as i32) as usize];
    }
}

pub struct SoloGame {
    pub session: GameSession,
    settings: SoloSettings,
    cpu: Option<Cpu>,
    new_best: bool,
}

impl SoloGame {
    pub fn new(settings: SoloSettings) -> Self {
        Self::with_seed(settings, rand::random())
    }

    fn with_seed(settings: SoloSettings, seed: u64) -> Self {
        let board = || {
            let mut board = Board::new(
                config::GRID_WIDTH,
                config::GRID_HEIGHT,
                seed,
                settings.room.starting_level,
                settings.room.colors,
            );
            board.spawn_piece();
            board
        };
        let mut session = GameSession::new(1);
        session.predicted_board = board();
        session.other_board = board();
        Self {
            session,
            settings,
            cpu: settings.opponent.map(|difficulty| Cpu::new(difficulty, seed)),
            new_best: false,
        }
    }

    pub fn hud(&self, best: i32) -> Hud {
        Hud::Solo {
            versus: self.cpu.is_some(),
            best,
            new_best: self.new_best,
        }
    }

    fn over(&self) -> bool {
        self.session.predicted_board.state == GameState::GameOver
            || (self.cpu.is_some() && self.session.other_board.state == GameState::GameOver)
    }

    fn paused(&self) -> bool {
        self.session.predicted_board.state == GameState::Paused
    }

    fn set_paused(&mut self, paused: bool) {
        self.session.predicted_board.set_paused(paused);
        self.session.other_board.set_paused(paused);
    }

    fn step(&mut self, dt: f32) {
        for _ in 0..logic::take_steps(&mut self.session, dt) {
            self.advance();
            if self.over() {
                break;
            }
        }
    }

    fn advance(&mut self) {
        let Self { session, cpu, .. } = self;
        let sent = logic::advance_one(session);
        let now = session.local_tick;
        session.incoming.retain(|g| g.at > now);
        let Some(cpu) = cpu else {
            return;
        };

        let landed: u32 = session
            .opp_incoming
            .iter()
            .filter(|g| g.at <= now)
            .map(|g| g.amount)
            .sum();
        session.opp_incoming.retain(|g| g.at > now);
        let input = cpu.input(&session.other_board, session.opp_nuisance() + landed);
        let sent_back = session.other_board.step(input, landed);

        let at = now + config::GARBAGE_TRAVEL_TICKS;
        if sent > 0 {
            session.opp_incoming.push(IncomingGarbage { at, amount: sent });
        }
        if sent_back > 0 {
            session.incoming.push(IncomingGarbage { at, amount: sent_back });
            crate::audio::play_garbage();
        }
    }
}

pub fn update_game(app: &mut App, state: &mut State) {
    let State {
        solo,
        settings,
        ui,
        screen,
        solo_best,
        ..
    } = state;
    let Some(game) = solo.as_mut() else {
        *screen = Screen::SoloSetup;
        return;
    };

    if !game.over() && app.keyboard.was_pressed(KeyCode::Escape) {
        game.set_paused(!game.paused());
    }
    let (over, paused) = (game.over(), game.paused());
    if over || paused {
        let (quit, again) = crate::draw::exit_rows(ui.view());
        if ui.bar_clicked(quit) {
            *solo = None;
            *screen = Screen::SoloSetup;
            return;
        }
        if ui.bar_clicked(again) || (over && app.keyboard.was_pressed(KeyCode::KeyR)) {
            *game = SoloGame::new(game.settings);
            return;
        }
    }

    let dt = app.timer.delta_f32();
    if over || paused {
        game.session.sim_accumulator = 0.0;
        logic::release_keys(&mut game.session);
    } else {
        let session = &mut game.session;
        logic::handle_soft_drop_key(app, session, None);
        if session.predicted_board.state == GameState::Playing {
            logic::handle_game_input(app, session, *settings, None, dt);
        } else {
            logic::release_keys(session);
        }
        game.step(dt);
        let score = game.session.predicted_board.score;
        if game.over() && game.cpu.is_none() && score > *solo_best {
            *solo_best = score;
            game.new_best = true;
            crate::state::save_best_score(score);
        }
    }
    logic::animate(&mut game.session, dt);
}

fn setup_panel(view: View) -> SettingsPanel {
    SettingsPanel::new(view, SoloSettings::COUNT)
}

pub fn update_setup(app: &mut App, state: &mut State) {
    let panel = setup_panel(state.ui.view());
    for i in 0..SoloSettings::COUNT {
        if state.ui.clicked(panel.stepper(i).minus) {
            state.solo_settings.adjust(i, -1);
        }
        if state.ui.clicked(panel.stepper(i).plus) {
            state.solo_settings.adjust(i, 1);
        }
    }
    if state.ui.bar_clicked(panel.action_row(0)) || app.keyboard.was_pressed(KeyCode::Enter) {
        state.solo = Some(SoloGame::new(state.solo_settings));
        state.screen = Screen::Solo;
    } else if state.ui.bar_clicked(panel.action_row(1)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::Menu;
    }
}

pub fn draw_setup(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let fonts = &state.fonts;
    let mut draw = state.ui.screen_canvas(gfx);

    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    let mid = theme::HEADER_H / 2.0;
    draw.sharp_text(&fonts.display, "Solo")
        .position(60.0, mid)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);
    if state.solo_best > 0 {
        let best = format!("Meilleur score: {}", state.solo_best);
        let pill = Pill {
            text: &best,
            color: theme::GOLD,
            size: theme::size::SMALL,
        };
        ui::pills_ending_at(&mut draw, fonts, &[pill], view.w - 60.0, mid);
    }

    let panel = setup_panel(view);
    panel.draw(&mut draw, &pal, fonts, "Réglages de la partie");
    for i in 0..SoloSettings::COUNT {
        let value = state.solo_settings.value(i);
        state
            .ui
            .stepper(&mut draw, fonts, panel.stepper(i), SoloSettings::label(i), &value, true);
    }

    state.ui.menu_bar(
        &mut draw,
        fonts,
        panel.action_row(0),
        "Lancer la partie",
        theme::bar::GREEN,
    );
    state
        .ui
        .menu_bar(&mut draw, fonts, panel.action_row(1), "Retour", theme::bar::RED);

    state.ui.render(gfx, &draw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(opponent: Option<Difficulty>, seed: u64) -> SoloGame {
        let settings = SoloSettings {
            opponent,
            room: RoomSettings::default(),
        };
        SoloGame::with_seed(settings, seed)
    }

    #[test]
    fn both_players_are_dealt_the_same_pairs() {
        let game = game(Some(Difficulty::Easy), 9);
        let (me, cpu) = (&game.session.predicted_board, &game.session.other_board);
        assert_eq!(me.active_piece, cpu.active_piece);
        assert_eq!(me.next_types, cpu.next_types);
        assert_eq!(me.next_next_types, cpu.next_next_types);
    }

    #[test]
    fn the_cpu_attacks_and_its_nuisance_lands_after_the_trip() {
        let mut game = game(Some(Difficulty::Hard), 2);
        let mut expected = None;
        for _ in 0..3 * 60 * 60 {
            game.advance();
            let now = game.session.local_tick;
            if let Some(g) = game.session.incoming.first().copied() {
                if expected.is_none() {
                    assert_eq!(g.at, now + config::GARBAGE_TRAVEL_TICKS);
                    let before = game.session.my_nuisance();
                    expected = Some((g.at, before));
                }
            }
            if let Some((at, shown)) = expected {
                if now == at {
                    assert!(game.session.incoming.iter().all(|g| g.at > now), "landed twice");
                    assert!(game.session.predicted_board.pending_garbage > 0, "never landed");
                    assert!(shown > 0, "the attack in flight was not shown");
                    return;
                }
            }
        }
        panic!("the CPU never sent anything");
    }

    #[test]
    fn nuisance_sent_by_the_player_reaches_the_cpu() {
        let mut game = game(Some(Difficulty::Easy), 4);
        let now = game.session.local_tick;
        game.session
            .opp_incoming
            .push(IncomingGarbage { at: now + 3, amount: 7 });
        assert_eq!(game.session.opp_nuisance(), 7);
        for _ in 0..3 {
            game.advance();
        }
        assert!(game.session.opp_incoming.is_empty());
        assert_eq!(game.session.other_board.pending_garbage, 7);
        assert_eq!(game.session.opp_nuisance(), 7, "counted twice or lost on landing");
    }

    #[test]
    fn a_game_without_opponent_only_moves_the_player() {
        let mut game = game(None, 4);
        let idle = game.session.other_board.state_hash();
        for _ in 0..600 {
            game.advance();
        }
        assert_eq!(game.session.local_tick, 600);
        assert_eq!(game.session.other_board.state_hash(), idle);
        assert!(game.session.incoming.is_empty() && game.session.opp_incoming.is_empty());
    }

    #[test]
    fn a_topped_out_cpu_ends_the_game_only_when_there_is_one() {
        let mut versus = game(Some(Difficulty::Easy), 1);
        versus.session.other_board.state = GameState::GameOver;
        assert!(versus.over());

        let mut alone = game(None, 1);
        alone.session.other_board.state = GameState::GameOver;
        assert!(!alone.over());
    }

    #[test]
    fn pausing_stops_both_boards() {
        let mut game = game(Some(Difficulty::Normal), 6);
        game.set_paused(true);
        let (me, cpu) = (
            game.session.predicted_board.state_hash(),
            game.session.other_board.state_hash(),
        );
        for _ in 0..120 {
            game.advance();
        }
        assert_eq!(game.session.predicted_board.state_hash(), me);
        assert_eq!(game.session.other_board.state_hash(), cpu);
        game.set_paused(false);
        game.advance();
        assert_ne!(game.session.predicted_board.state_hash(), me);
    }

    #[test]
    fn the_opponent_setting_cycles_both_ways() {
        let mut settings = SoloSettings::default();
        assert_eq!(settings.value(0), "Aucun");
        settings.adjust(0, -1);
        assert_eq!(settings.opponent, Some(Difficulty::Hard));
        settings.adjust(0, 1);
        settings.adjust(0, 1);
        assert_eq!(settings.opponent, Some(Difficulty::Easy));
        settings.adjust(1, 1);
        assert_eq!(settings.room.starting_level, 2);
    }
}
