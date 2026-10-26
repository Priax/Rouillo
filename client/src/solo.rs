use std::sync::Arc;

use notan::prelude::*;
use shared::cpu::{Cpu, Difficulty, Style};
use shared::{config, Board, GameState, IncomingGarbage, RoomSetting, RoomSettings, Targeting};

use crate::draw::Hud;
use crate::state::{GameSession, Rival, Screen, State};
use crate::ui::{self, Frame, Icon, Pill, Rect, SharpText, Size, Stepper, View, FILL};
use crate::{board_size, http, logic, rooms, theme};

/// The most CPUs a solo game takes on: each one thinks on this device.
pub const MAX_OPPONENTS: usize = 3;

#[derive(Clone, Default)]
pub struct SoloSettings {
    /// A CPU playing the player's board, for a game between CPUs.
    pilot: Option<Difficulty>,
    pilot_style: Style,
    opponents: Vec<(Difficulty, Style)>,
    room: RoomSettings,
}

/// The settings of the game itself, before the opponents.
const GAME_ROWS: usize = 5;
const PILOT_ROW: usize = 0;
const PILOT_STYLE_ROW: usize = 1;
const SIZE_ROW: usize = 2;
const LEVEL_ROW: usize = 3;

fn pilot_name(pilot: Option<Difficulty>) -> String {
    pilot.map_or_else(|| "Joueur".to_owned(), |d| format!("IA {}", d.label()))
}

impl SoloSettings {
    fn label(i: usize) -> &'static str {
        match i {
            PILOT_ROW => "Vous",
            PILOT_STYLE_ROW => "Personnage",
            SIZE_ROW => "Taille",
            LEVEL_ROW => rooms::setting_label(RoomSetting::StartingLevel),
            _ => rooms::setting_label(RoomSetting::Colors),
        }
    }

    fn value(&self, i: usize) -> String {
        match i {
            PILOT_ROW => pilot_name(self.pilot),
            PILOT_STYLE_ROW if self.pilot.is_none() => "-".to_owned(),
            PILOT_STYLE_ROW => self.pilot_style.character().to_owned(),
            SIZE_ROW => board_size::label(&self.room.rules),
            LEVEL_ROW => rooms::setting_value(&self.room, RoomSetting::StartingLevel),
            _ => rooms::setting_value(&self.room, RoomSetting::Colors),
        }
    }

    fn adjust(&mut self, i: usize, dir: i32) {
        match i {
            PILOT_ROW => {
                let options: Vec<_> = std::iter::once(None).chain(Difficulty::ALL.map(Some)).collect();
                self.pilot = ui::cycle(&options, self.pilot, dir);
            }
            PILOT_STYLE_ROW if self.pilot.is_some() => self.pilot_style = ui::cycle(&Style::ALL, self.pilot_style, dir),
            PILOT_STYLE_ROW | SIZE_ROW => {}
            LEVEL_ROW => self.room.adjust(RoomSetting::StartingLevel, dir),
            _ => self.room.adjust(RoomSetting::Colors, dir),
        }
    }

    /// Steps the difficulty of opponent `i` (`part` 0) or its character.
    fn adjust_opponent(&mut self, i: usize, part: usize, dir: i32) {
        let Some((difficulty, style)) = self.opponents.get_mut(i) else {
            return;
        };
        if part == 0 {
            *difficulty = ui::cycle(&Difficulty::ALL, *difficulty, dir);
        } else {
            *style = ui::cycle(&Style::ALL, *style, dir);
        }
    }

    fn add_opponent(&mut self) {
        if self.opponents.len() < MAX_OPPONENTS {
            let style = Style::ALL[self.opponents.len() % Style::ALL.len()];
            self.opponents.push((Difficulty::Normal, style));
        }
    }

    /// Whether a score here may be a record: the player alone on the
    /// standard board.
    fn counts_for_best(&self) -> bool {
        self.pilot.is_none() && self.opponents.is_empty() && board_size::is_standard(&self.room.rules)
    }

    /// Whether row `i` may be changed now: the pilot's character only with a pilot.
    fn editable(&self, i: usize) -> bool {
        i != PILOT_STYLE_ROW || self.pilot.is_some()
    }
}

pub struct SoloGame {
    pub session: GameSession,
    settings: SoloSettings,
    pilot: Option<Cpu>,
    /// One per rival, in the same order.
    cpus: Vec<Cpu>,
    /// Where each board's nuisance goes: 0 is the player's, `i + 1` rival `i`'s.
    targets: Vec<Option<usize>>,
    random_until: Vec<u32>,
    new_best: bool,
}

impl SoloGame {
    pub fn new(settings: SoloSettings) -> Self {
        Self::with_seed(settings, rand::random())
    }

    fn with_seed(settings: SoloSettings, seed: u64) -> Self {
        let board = Board::for_match(seed, &settings.room.rules);
        let mine = settings
            .pilot
            .map_or_else(String::new, |d| shared::cpu::name(d, settings.pilot_style));
        let names = std::iter::once(mine).chain(settings.opponents.iter().map(|&(d, s)| shared::cpu::name(d, s)));
        let mut session = GameSession::new(1, names.collect());
        session.predicted_board = board.clone();
        let mut cpus = Vec::new();
        for (i, &(difficulty, style)) in settings.opponents.iter().enumerate() {
            cpus.push(Cpu::new(difficulty, style, seed.wrapping_add(i as u64)));
            let seat = i as u8 + 1;
            let (slot, team) = (seat + 1, shared::own_team(seat));
            session.add_rival(Rival::new(slot, team, board.clone()));
        }
        let pilot = settings.pilot.map(|difficulty| {
            Cpu::new(
                difficulty,
                settings.pilot_style,
                seed.wrapping_add(MAX_OPPONENTS as u64),
            )
        });
        let boards = cpus.len() + 1;
        Self {
            session,
            settings,
            pilot,
            cpus,
            targets: vec![None; boards],
            random_until: vec![0; boards],
            new_best: false,
        }
    }

    pub fn hud(&self, best: u32) -> Hud {
        Hud::Solo {
            records: self.settings.counts_for_best(),
            best,
            new_best: self.new_best,
            watching: self.pilot.is_some(),
        }
    }

    fn me_out(&self) -> bool {
        self.session.predicted_board.state() == GameState::GameOver
    }

    /// Whether the game is over: one side is left, or the player alone is out.
    fn over(&self) -> bool {
        self.session.outcome(self.me_out()).is_some()
    }

    /// Out of a game the CPUs play on.
    fn knocked_out(&self) -> bool {
        self.me_out() && !self.over()
    }

    /// Whether nothing is being played: the game is over or paused.
    pub fn idle(&self) -> bool {
        self.over() || self.paused()
    }

    fn paused(&self) -> bool {
        self.session.predicted_board.state() == GameState::Paused
    }

    fn set_paused(&mut self, paused: bool) {
        self.session.predicted_board.set_paused(paused);
        for rival in &mut self.session.rivals {
            rival.board.set_paused(paused);
        }
    }

    fn step(&mut self, dt: f32) {
        for _ in 0..logic::take_steps(&mut self.session, dt) {
            self.advance();
            if self.over() {
                break;
            }
        }
    }

    fn board(&self, i: usize) -> &Board {
        i.checked_sub(1)
            .map_or(&self.session.predicted_board, |r| &self.session.rivals[r].board)
    }

    /// Picks where each board's nuisance goes: the player by the way they
    /// chose, every CPU by chance.
    fn aim(&mut self) {
        let alive: Vec<usize> = (0..self.targets.len())
            .filter(|&i| self.board(i).state() != GameState::GameOver)
            .collect();
        let now = self.session.local_tick;
        for i in 0..self.targets.len() {
            let opponents: Vec<usize> = alive.iter().copied().filter(|&o| o != i).collect();
            if !alive.contains(&i) || opponents.is_empty() {
                self.targets[i] = None;
                continue;
            }
            let targeting = match self.session.targeting {
                Targeting::Manual(slot) => Targeting::Manual(slot.saturating_sub(1)),
                t => t,
            };
            let targeting = if i == 0 && self.pilot.is_none() {
                targeting
            } else {
                Targeting::Random
            };
            let aimed = targeting.aim(i, &opponents, &self.targets, |o| self.board(o).danger());
            let kept = self.targets[i].filter(|t| opponents.contains(t) && now < self.random_until[i]);
            self.targets[i] = Some(aimed.or(kept).unwrap_or_else(|| {
                self.random_until[i] = now + config::RANDOM_TARGET_TICKS;
                opponents[rand::random_range(0..opponents.len())]
            }));
        }
        let slot = |t: Option<usize>| t.map(|t| t as u8 + 1);
        self.session.target = slot(self.targets[0]);
        for (i, rival) in self.session.rivals.iter_mut().enumerate() {
            rival.target = slot(self.targets[i + 1]);
        }
    }

    fn advance(&mut self) {
        if let Some(pilot) = &mut self.pilot {
            let threat = self.session.my_nuisance();
            if let Some(kind) = pilot.input(&self.session.predicted_board, threat) {
                self.session.predicted_board.apply_input(kind);
            }
        }
        let mut sent = vec![logic::advance_one(&mut self.session)];
        let now = self.session.local_tick;
        self.session.incoming.retain(|g| g.at > now);
        for (cpu, rival) in self.cpus.iter_mut().zip(&mut self.session.rivals) {
            let landed: u32 = rival.incoming.iter().filter(|g| g.at <= now).map(|g| g.amount).sum();
            rival.incoming.retain(|g| g.at > now);
            let input = cpu.input(&rival.board, rival.nuisance() + landed);
            sent.push(rival.board.step(input, landed).sent);
        }
        self.aim();
        let at = now + config::GARBAGE_TRAVEL_TICKS;
        for (from, amount) in sent.into_iter().enumerate().filter(|&(_, amount)| amount > 0) {
            let garbage = IncomingGarbage { at, amount };
            match self.targets[from] {
                Some(0) => {
                    self.session.incoming.push(garbage);
                    crate::audio::play_garbage();
                }
                Some(to) => self.session.rivals[to - 1].incoming.push(garbage),
                None => {}
            }
        }
    }
}

/// Sends a score to the account, which keeps its best. At sign-in this is the
/// device's best, so a guest's record follows them into their account.
pub fn send_best(state: &mut State, score: u32) {
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
        solo_best: u32,
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

    if game.knocked_out() && (input.pause || ui.bar_clicked(crate::draw::out_row(ui.view()))) {
        *solo = None;
        *screen = Screen::SoloSetup;
        return;
    }
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
            *game = SoloGame::new(game.settings.clone());
            return;
        }
    }

    let dt = app.timer.delta_f32();
    if over || paused {
        logic::hold(&mut game.session);
    } else {
        if game.pilot.is_none() && !game.knocked_out() {
            let click = crate::draw::game_click(ui, &game.session);
            logic::choose_target(&input, click, &mut game.session);
            logic::read_controls(&input, &mut game.session, *settings, None, dt);
        }
        game.step(dt);
        let score = game.session.predicted_board.score;
        if game.over() && game.settings.counts_for_best() && score > *solo_best {
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

const ROW_H: f32 = 44.0;
const ROW_GAP: f32 = 12.0;
/// The opponents' rows stand a little lower than tiles would.
const FOES_TOP: f32 = 8.0;
const CARD_GAP: f32 = 24.0;
const GAME_W: f32 = 420.0;
const FOES_W: f32 = 600.0;
const ADD_W: f32 = 260.0;

/// The settings screen: the game's settings in one card, the opponents in
/// another, side by side or one above the other on a narrow screen.
struct Setup {
    game: Rect,
    foes: Rect,
    view_w: f32,
}

/// An opponent's row: its number, its difficulty, its character, the
/// button that takes it away.
struct FoeRow {
    number: Rect,
    difficulty: Rect,
    style: Rect,
    remove: Rect,
}

impl Setup {
    fn new(view: View) -> Self {
        let body = Frame::of(view).body;
        let game_h = ui::card_height(GAME_ROWS as f32 * ui::TILE_H);
        let foes_h = ui::card_height(FOES_TOP + (MAX_OPPONENTS + 1) as f32 * (ROW_H + ROW_GAP));
        let (game, foes) = if view.portrait() {
            let game = body.top(game_h).max_width(FOES_W);
            let below = body.below(game_h + CARD_GAP);
            (game, below.top(foes_h).max_width(FOES_W))
        } else {
            let both = body.top(game_h.max(foes_h)).max_width(GAME_W + CARD_GAP + FOES_W);
            (both.left(GAME_W), both.right(FOES_W))
        };
        Self {
            game,
            foes,
            view_w: view.w,
        }
    }

    fn tile(&self, i: usize) -> Rect {
        ui::card_body(self.game).nth_row(i, ui::TILE_H, 0.0)
    }

    fn row(&self, i: usize) -> Rect {
        ui::card_body(self.foes)
            .pad(FOES_TOP, 0.0, 0.0, 0.0)
            .nth_row(i, ROW_H, ROW_GAP)
    }

    fn foe(&self, i: usize) -> FoeRow {
        let [number, difficulty, style, remove] = self
            .row(i)
            .columns(20.0, [Size::Px(24.0), FILL, Size::Fill(1.3), Size::Px(ROW_H)]);
        FoeRow {
            number,
            difficulty,
            style,
            remove,
        }
    }

    fn add(&self, opponents: usize) -> Rect {
        self.row(opponents).centered(ADD_W, ROW_H)
    }

    fn action_row(&self, k: usize) -> Rect {
        let lower = if self.foes.bottom() > self.game.bottom() {
            self.foes
        } else {
            self.game
        };
        ui::action_row(lower, self.view_w, k)
    }
}

pub fn update_setup(app: &mut App, state: &mut State) {
    let setup = Setup::new(state.ui.view());
    let settings = &mut state.solo_settings;
    if state.solo_sizing {
        match board_size::update(app, &state.ui, &settings.room.rules, true) {
            Some(board_size::Action::Close) => state.solo_sizing = false,
            Some(board_size::Action::Adjust(steps)) => {
                for (setting, dir) in steps {
                    settings.room.adjust(setting, dir);
                }
            }
            None => {}
        }
        return;
    }
    let ui = &state.ui;
    for i in 0..GAME_ROWS {
        let tile = setup.tile(i);
        if i == SIZE_ROW {
            if ui.clicked(Stepper::tile_band(tile)) {
                state.solo_sizing = true;
                return;
            }
            continue;
        }
        let stepper = Stepper::in_tile(tile);
        if ui.clicked(stepper.minus) {
            settings.adjust(i, -1);
        } else if ui.clicked(stepper.plus) {
            settings.adjust(i, 1);
        }
    }
    for i in 0..settings.opponents.len() {
        let row = setup.foe(i);
        if ui.clicked(row.remove) {
            settings.opponents.remove(i);
            return;
        }
        for (part, band) in [row.difficulty, row.style].into_iter().enumerate() {
            let stepper = Stepper::across(band);
            if ui.clicked(stepper.minus) {
                settings.adjust_opponent(i, part, -1);
            } else if ui.clicked(stepper.plus) {
                settings.adjust_opponent(i, part, 1);
            }
        }
    }
    if settings.opponents.len() < MAX_OPPONENTS && ui.clicked(setup.add(settings.opponents.len())) {
        settings.add_opponent();
    }

    if ui.bar_clicked(setup.action_row(0)) || app.keyboard.was_pressed(KeyCode::Enter) {
        state.solo = Some(SoloGame::new(state.solo_settings.clone()));
        state.screen = Screen::Solo;
    } else if ui.bar_clicked(setup.action_row(1)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::PlayMenu;
    }
}

pub fn draw_setup(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let fonts = &state.fonts;
    let settings = &state.solo_settings;
    let mut draw = state.ui.screen_canvas(gfx);
    state.ui.set_input(!state.solo_sizing);

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

    let setup = Setup::new(view);
    ui::panel_card(&mut draw, &pal, fonts, setup.game, "Partie");
    for i in 0..GAME_ROWS {
        let (tile, label, value) = (setup.tile(i), SoloSettings::label(i), settings.value(i));
        if i == SIZE_ROW {
            state.ui.button_tile(&mut draw, fonts, tile, label, &value);
        } else {
            state
                .ui
                .stepper_tile(&mut draw, fonts, tile, label, &value, settings.editable(i));
        }
    }

    ui::panel_card(&mut draw, &pal, fonts, setup.foes, "Adversaires");
    for (i, &(difficulty, style)) in settings.opponents.iter().enumerate() {
        let row = setup.foe(i);
        let (x, y) = row.number.center();
        draw.sharp_text(&fonts.display, &(i + 1).to_string())
            .position(x, y)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_dim);
        state
            .ui
            .stepper_band(&mut draw, fonts, row.difficulty, difficulty.label(), true);
        state
            .ui
            .stepper_band(&mut draw, fonts, row.style, style.character(), true);
        state.ui.button(&mut draw, fonts, row.remove, Icon::Close);
    }
    let count = settings.opponents.len();
    if count < MAX_OPPONENTS {
        state.ui.button(&mut draw, fonts, setup.add(count), "Ajouter un CPU");
    }

    state.ui.menu_bar(
        &mut draw,
        fonts,
        setup.action_row(0),
        "Lancer la partie",
        theme::bar::GREEN,
    );
    state
        .ui
        .menu_bar(&mut draw, fonts, setup.action_row(1), "Retour", theme::bar::RED);

    if state.solo_sizing {
        board_size::draw(&mut draw, &state.ui, fonts, &settings.room.rules, true);
    }
    state.ui.render(gfx, &draw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(opponent: Option<Difficulty>, seed: u64) -> SoloGame {
        let settings = SoloSettings {
            opponents: opponent.map(|d| (d, Style::Balanced)).into_iter().collect(),
            ..SoloSettings::default()
        };
        SoloGame::with_seed(settings, seed)
    }

    fn against(opponents: &[(Difficulty, Style)], pilot: Option<Difficulty>, seed: u64) -> SoloGame {
        let settings = SoloSettings {
            pilot,
            opponents: opponents.to_vec(),
            ..SoloSettings::default()
        };
        SoloGame::with_seed(settings, seed)
    }

    #[test]
    fn both_players_are_dealt_the_same_pairs() {
        let game = game(Some(Difficulty::Easy), 9);
        let (me, cpu) = (&game.session.predicted_board, &game.session.rivals[0].board);
        assert_eq!(me.piece(), cpu.piece());
        assert_eq!(me.next_types, cpu.next_types);
        assert_eq!(me.next_next_types, cpu.next_next_types);
    }

    #[test]
    fn the_cpu_attacks_and_its_nuisance_lands_after_the_trip() {
        let mut game = against(&[(Difficulty::Hard, Style::Balanced)], Some(Difficulty::Easy), 2);
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
        game.session.rivals[0]
            .incoming
            .push(IncomingGarbage { at: now + 3, amount: 7 });
        assert_eq!(game.session.rivals[0].nuisance(), 7);
        for _ in 0..3 {
            game.advance();
        }
        let cpu = &game.session.rivals[0];
        assert!(cpu.incoming.is_empty());
        assert_eq!(cpu.board.pending_garbage, 7);
        assert_eq!(cpu.nuisance(), 7, "counted twice or lost on landing");
    }

    #[test]
    fn a_game_without_opponent_only_moves_the_player() {
        let mut game = game(None, 4);
        assert!(game.session.rivals.is_empty());
        for _ in 0..600 {
            game.advance();
        }
        assert_eq!(game.session.local_tick, 600);
        assert!(game.session.incoming.is_empty());
    }

    #[test]
    fn a_topped_out_cpu_ends_the_game() {
        let mut versus = game(Some(Difficulty::Easy), 1);
        assert!(!versus.over());
        versus.session.rivals[0].board.lose();
        assert!(versus.over());
    }

    #[test]
    fn pausing_stops_both_boards() {
        let mut game = game(Some(Difficulty::Normal), 6);
        game.set_paused(true);
        let (me, cpu) = (
            game.session.predicted_board.state_hash(),
            game.session.rivals[0].board.state_hash(),
        );
        for _ in 0..120 {
            game.advance();
        }
        assert_eq!(game.session.predicted_board.state_hash(), me);
        assert_eq!(game.session.rivals[0].board.state_hash(), cpu);
        game.set_paused(false);
        game.advance();
        assert_ne!(game.session.predicted_board.state_hash(), me);
    }

    #[test]
    fn opponents_are_added_up_to_the_most_and_set_one_by_one() {
        let mut settings = SoloSettings::default();
        for _ in 0..MAX_OPPONENTS + 2 {
            settings.add_opponent();
        }
        assert_eq!(settings.opponents.len(), MAX_OPPONENTS);
        assert_eq!(
            settings.opponents[1],
            (Difficulty::Normal, Style::Harasser),
            "each one a new character"
        );
        settings.adjust_opponent(1, 0, 1);
        settings.adjust_opponent(1, 1, -1);
        assert_eq!(settings.opponents[1], (Difficulty::Hard, Style::Balanced));
        assert_eq!(settings.value(PILOT_ROW), "Joueur");
        settings.adjust(PILOT_STYLE_ROW, 1);
        assert_eq!(settings.value(PILOT_STYLE_ROW), "-", "a character for no CPU");
        assert!(!settings.editable(PILOT_STYLE_ROW));
        settings.adjust(PILOT_ROW, -1);
        assert_eq!(settings.value(PILOT_ROW), "IA Pro");
        settings.adjust(PILOT_STYLE_ROW, -1);
        assert_eq!(settings.pilot_style, Style::Architect);
        let game = SoloGame::with_seed(settings.clone(), 2);
        assert_eq!(
            game.session.name_of(1),
            "Personnage 6 (Pro)",
            "the pilot plays as picked"
        );
        settings.adjust(LEVEL_ROW, 1);
        assert_eq!(settings.room.rules.starting_level, 2);
    }

    #[test]
    fn the_settings_and_the_opponents_stay_in_their_cards() {
        for view in [View::default(), View::fit(393.0, 852.0)] {
            let setup = Setup::new(view);
            let inside = |r: Rect, card: Rect| {
                let body = ui::card_body(card);
                r.x >= body.x && r.x + r.w <= body.x + body.w && r.y >= body.y && r.bottom() <= card.bottom()
            };
            for i in 0..GAME_ROWS {
                assert!(
                    inside(setup.tile(i), setup.game),
                    "tile {i} out of its card in {view:?}"
                );
            }
            for i in 0..=MAX_OPPONENTS {
                assert!(inside(setup.row(i), setup.foes), "row {i} out of its card in {view:?}");
            }
            let (first, last) = (setup.action_row(0), setup.action_row(1));
            assert!(first.y > setup.game.bottom().max(setup.foes.bottom()));
            assert!(last.bottom() <= view.h, "the actions run off {view:?}");
        }
    }

    #[test]
    fn a_record_counts_alone_on_the_standard_board_only() {
        let shows_best = |settings: &SoloSettings| {
            matches!(
                SoloGame::with_seed(settings.clone(), 1).hud(0),
                Hud::Solo { records: true, .. }
            )
        };
        let mut settings = SoloSettings::default();
        assert!(settings.counts_for_best() && shows_best(&settings));
        settings.room.adjust(RoomSetting::Columns, 1);
        assert!(!settings.counts_for_best(), "on a wider board");
        assert!(!shows_best(&settings), "the best shown where no record counts");
        settings.room.adjust(RoomSetting::Columns, -1);
        settings.pilot = Some(Difficulty::Easy);
        assert!(!settings.counts_for_best(), "played by a CPU");
        settings.pilot = None;
        settings.add_opponent();
        assert!(!settings.counts_for_best(), "against a CPU");
    }

    #[test]
    fn cpus_attack_each_other_and_the_last_one_standing_wins() {
        let foes = [
            (Difficulty::Hard, Style::Harasser),
            (Difficulty::Easy, Style::Balanced),
            (Difficulty::Easy, Style::Balanced),
        ];
        let mut game = against(&foes, Some(Difficulty::Easy), 3);
        let mut hit = [false; 3];
        for _ in 0..10 * 60 * 60 {
            game.advance();
            for (i, rival) in game.session.rivals.iter().enumerate() {
                hit[i] |= rival.board.pending_garbage > 0;
            }
            if game.over() {
                break;
            }
        }
        assert!(game.over(), "nobody won in ten minutes");
        assert!(
            hit.iter().filter(|&&h| h).count() >= 2,
            "nuisance only went one way: {hit:?}"
        );
        let alive = game.session.teams_alive(game.me_out());
        assert_eq!(alive.len(), 1, "one side left: {alive:?}");
    }

    #[test]
    fn the_cpus_play_on_once_the_player_is_out() {
        let foes = [
            (Difficulty::Normal, Style::Balanced),
            (Difficulty::Normal, Style::Builder),
        ];
        let mut game = against(&foes, None, 5);
        game.session.predicted_board.lose();
        assert!(game.knocked_out() && !game.over());
        let before: Vec<u64> = game.session.rivals.iter().map(|r| r.board.state_hash()).collect();
        for _ in 0..120 {
            game.advance();
        }
        let after: Vec<u64> = game.session.rivals.iter().map(|r| r.board.state_hash()).collect();
        assert!(
            before.iter().zip(&after).all(|(b, a)| b != a),
            "a CPU stopped with the player"
        );
        assert!(
            game.targets[1..].iter().all(|&t| t != Some(0)),
            "a CPU still aims at the player"
        );
    }

    #[test]
    fn a_cpu_plays_the_board_of_a_player_who_watches() {
        let mut game = against(&[], Some(Difficulty::Hard), 8);
        let first = game.session.predicted_board.piece_id;
        for _ in 0..30 * 60 {
            game.advance();
        }
        assert!(
            game.session.predicted_board.piece_id > first + 5,
            "the CPU placed no pairs"
        );
        assert!(matches!(game.hud(0), Hud::Solo { watching: true, .. }));
    }
}
