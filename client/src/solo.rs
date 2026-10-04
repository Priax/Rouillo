use std::sync::Arc;

use notan::prelude::*;
use shared::{config, Board, GameState, IncomingGarbage, RoomSettings};

use crate::cpu::{Cpu, Difficulty, Style};
use crate::draw::Hud;
use crate::state::{GameSession, Screen, State};
use crate::ui::{self, Pill, Rect, SettingsPanel, SharpText, View};
use crate::{http, logic, theme};

#[derive(Clone, Copy, Default)]
pub struct SoloSettings {
    opponent: Option<Difficulty>,
    character: Style,
    room: RoomSettings,
}

fn cycle<T: Copy + PartialEq>(options: &[T], at: T, dir: i32) -> T {
    let i = options.iter().position(|&o| o == at).unwrap_or(0) as i32;
    options[(i + dir.signum()).rem_euclid(options.len() as i32) as usize]
}

impl SoloSettings {
    const COUNT: usize = 4;

    fn label(i: usize) -> &'static str {
        match i {
            0 => "Adversaire",
            1 => "Personnage",
            _ => RoomSettings::label(i - 2),
        }
    }

    fn value(&self, i: usize) -> String {
        match i {
            0 => self.opponent.map_or("Aucun", Difficulty::label).into(),
            1 if self.opponent.is_none() => "-".into(),
            1 => self.character.character().into(),
            _ => self.room.value(i - 2),
        }
    }

    fn adjust(&mut self, i: usize, dir: i32) {
        match i {
            0 => {
                let options: Vec<_> = std::iter::once(None).chain(Difficulty::ALL.map(Some)).collect();
                self.opponent = cycle(&options, self.opponent, dir);
            }
            1 if self.opponent.is_some() => self.character = cycle(&Style::ALL, self.character, dir),
            1 => {}
            _ => self.room.adjust(i - 2, dir),
        }
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
        let board = Board::for_match(seed, &settings.room);
        let mut session = GameSession::new(1);
        session.predicted_board = board.clone();
        session.other_board = board;
        Self {
            session,
            settings,
            cpu: settings
                .opponent
                .map(|difficulty| Cpu::new(difficulty, settings.character, seed)),
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

    /// Whether nothing is being played: the game is over or paused.
    pub fn idle(&self) -> bool {
        self.over() || self.paused()
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

/// Sends a score to the account, which keeps its best. At sign-in this is the
/// device's best, so a guest's record follows them into their account.
pub fn send_best(state: &mut State, score: i32) {
    let Some(auth) = &state.auth else { return };
    let slot = http::new_slot();
    let body = serde_json::json!({ "score": score }).to_string();
    http::post_json(
        http::api_url("me/solo-best"),
        body,
        Some(auth.token.clone()),
        Arc::clone(&slot),
    );
    state.solo_best_slot = Some(slot);
}

pub fn signed_in(state: &mut State) {
    send_best(state, crate::state::load_best_score());
}

pub fn poll_best(state: &mut State) {
    #[derive(serde::Deserialize)]
    struct Best {
        solo_best: i32,
    }
    let Some(result) = http::take_json::<Best>(&mut state.solo_best_slot) else {
        return;
    };
    if state.auth.is_none() {
        return;
    }
    match result {
        Ok(best) => {
            state.solo_best = state.solo_best.max(best.solo_best);
            crate::state::clear_best_score();
        }
        // Kept on the device until the next sign-in sends it again.
        Err(_) => crate::state::save_best_score(state.solo_best),
    }
}

pub fn update_game(app: &mut App, state: &mut State) {
    let signed_in = state.auth.is_some();
    let mut record = None;
    let State {
        solo,
        settings,
        ui,
        screen,
        solo_best,
        controls,
        ..
    } = state;
    let input = controls.frame;
    let Some(game) = solo.as_mut() else {
        *screen = Screen::SoloSetup;
        return;
    };

    if !game.over() && input.pause {
        game.set_paused(!game.paused());
    }
    let (over, paused) = (game.over(), game.paused());
    if over || paused {
        let (quit, again) = crate::draw::exit_rows(ui.view(), true);
        if ui.bar_clicked(quit) {
            *solo = None;
            *screen = Screen::SoloSetup;
            return;
        }
        if ui.bar_clicked(again) || (over && input.restart) {
            *game = SoloGame::new(game.settings);
            return;
        }
    }

    let dt = app.timer.delta_f32();
    if over || paused {
        logic::hold(&mut game.session);
    } else {
        logic::read_controls(&input, &mut game.session, *settings, None, dt);
        game.step(dt);
        let score = game.session.predicted_board.score;
        if game.over() && game.cpu.is_none() && score > *solo_best {
            *solo_best = score;
            game.new_best = true;
            if signed_in {
                record = Some(score);
            } else {
                crate::state::save_best_score(score);
            }
        }
    }
    logic::animate(&mut game.session, dt);
    if let Some(score) = record {
        send_best(state, score);
    }
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
        state.screen = Screen::PlayMenu;
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
        let editable = i != 1 || state.solo_settings.opponent.is_some();
        state.ui.stepper(
            &mut draw,
            fonts,
            panel.stepper(i),
            SoloSettings::label(i),
            &value,
            editable,
        );
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
            ..SoloSettings::default()
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
        settings.adjust(1, -1);
        assert_eq!(settings.character, Style::Architect);
        settings.adjust(2, 1);
        assert_eq!(settings.room.starting_level, 2);
    }
}
