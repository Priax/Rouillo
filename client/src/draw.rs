use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::{Board, GameState, PuyoType, Settle};

use crate::config;
use crate::sprites::{self, Joint, Layer, Mood, Nuisance, Puyo};
use crate::state::GameSession;
use crate::theme::{self, game, Palette};
use crate::ui::{Fonts, Rect, SharpText, Ui, View};

const FRAME_PAD: f32 = 10.0;
const COLUMN_W: f32 = 170.0;
const VERSUS_GAP: f32 = 250.0;
const SIDE_GAP: f32 = 40.0;
const TRAY_H: f32 = 34.0;
const PANEL_H: f32 = 50.0;
const LABEL_H: f32 = 42.0;
const STAT_GAP: f32 = 18.0;
const VALUE_SIZE: f32 = 40.0;
const RADIUS: f32 = config::CELL_SIZE * 0.5;

struct GameLayout {
    win_w: f32,
    win_h: f32,
    mine: Rect,
    theirs: Rect,
    sidebar_x: f32,
}

impl GameLayout {
    fn new(view: View, two_boards: bool) -> Self {
        let (win_w, win_h) = view.size();
        let board_w = config::GRID_WIDTH as f32 * config::CELL_SIZE;
        let board_h = (config::GRID_HEIGHT - config::VISIBLE_ROW_OFFSET) as f32 * config::CELL_SIZE;
        let column = COLUMN_W + SIDE_GAP;
        let boards = if two_boards {
            board_w * 2.0 + VERSUS_GAP
        } else {
            board_w
        };
        let start_x = (win_w - boards - 2.0 * column) / 2.0 + column;
        let offset_y = (win_h - board_h) / 2.0 - 10.0;
        let sidebar_x = if two_boards {
            start_x + board_w + (VERSUS_GAP - COLUMN_W) / 2.0
        } else {
            start_x + board_w + SIDE_GAP
        };
        Self {
            win_w,
            win_h,
            mine: Rect::at(start_x, offset_y, board_w, board_h),
            theirs: Rect::at(start_x + board_w + VERSUS_GAP, offset_y, board_w, board_h),
            sidebar_x,
        }
    }

    const fn top(&self) -> f32 {
        self.mine.y - FRAME_PAD
    }
}

#[derive(Clone, Copy)]
pub struct Role {
    pub is_host: bool,
    pub can_pause: bool,
    pub series: Option<SeriesView>,
}

#[derive(Clone, Copy)]
pub struct SeriesView {
    pub wins: [u8; 2],
    pub over: Option<(Option<bool>, i32)>,
}

impl Hud {
    const fn series(self) -> Option<SeriesView> {
        match self {
            Self::Online(role) => role.series,
            Self::Solo { .. } => None,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Hud {
    Online(Role),
    Solo { versus: bool, best: i32, new_best: bool },
}

impl Hud {
    const fn opponent(self) -> Option<&'static str> {
        match self {
            Self::Online(_) => Some("OPPONENT"),
            Self::Solo { versus: true, .. } => Some("CPU"),
            Self::Solo { versus: false, .. } => None,
        }
    }

    const fn deciding(self, session: &GameSession) -> &Board {
        match self {
            Self::Online(_) => &session.board,
            Self::Solo { .. } => &session.predicted_board,
        }
    }
}

enum Overlay {
    GameOver { i_lost: bool, draw: bool },
    OpponentGone,
    Paused,
    QuitMenu,
}

impl Overlay {
    fn of(session: &GameSession, hud: Hud) -> Option<Self> {
        let me = hud.deciding(session);
        if let Some(SeriesView {
            over: Some((won, _)), ..
        }) = hud.series()
        {
            return Some(Self::GameOver {
                i_lost: won != Some(true),
                draw: false,
            });
        }
        let i_lost = me.state == GameState::GameOver;
        let they_lost = hud.opponent().is_some() && session.other_board.state == GameState::GameOver;
        if i_lost || they_lost {
            Some(Self::GameOver {
                i_lost,
                draw: i_lost && they_lost,
            })
        } else if session.opponent_disconnected {
            Some(Self::OpponentGone)
        } else if me.state == GameState::Paused {
            Some(Self::Paused)
        } else if session.quit_menu {
            Some(Self::QuitMenu)
        } else {
            None
        }
    }
}

pub fn draw_game(app: &mut App, gfx: &mut Graphics, session: &GameSession, ui: &Ui, fonts: &Fonts, hud: Hud) {
    let layout = GameLayout::new(ui.view(), hud.opponent().is_some());
    let pal = ui.palette();
    let time = app.timer.elapsed_f32();
    let mut draw = ui.canvas(gfx);
    draw.clear(game::BACKGROUND);
    draw_backdrop(&mut draw, layout.win_w, layout.win_h, time);

    draw_boards(&mut draw, &pal, fonts, session, &layout, hud, time);
    draw_sidebar(&mut draw, &pal, fonts, session, &layout, hud, time);
    draw_chain_anim(&mut draw, fonts, session.chain_display, layout.mine);
    if session.all_clear_timer > 0.0 {
        let alpha = (session.all_clear_timer / 3.0).min(1.0);
        let (cx, cy) = (layout.mine.x + layout.mine.w / 2.0, layout.mine.y + layout.mine.h / 2.0);
        shadowed(
            &mut draw,
            fonts,
            "ALL CLEAR!",
            (cx, cy),
            theme::size::TITLE,
            game::ALL_CLEAR.with_alpha(alpha),
        );
    }
    if let Some(overlay) = Overlay::of(session, hud) {
        let game_over = matches!(overlay, Overlay::GameOver { .. });
        let leaving_forfeits = !game_over && !session.opponent_disconnected;
        draw_overlay(&mut draw, &pal, fonts, &overlay, &layout, hud, time);
        draw_exit_buttons(&mut draw, ui, fonts, hud, leaving_forfeits);
    }
    #[cfg(debug_assertions)]
    if matches!(hud, Hud::Online(_)) {
        draw_debug(&mut draw, fonts, session, layout.win_h);
    }

    ui.render(gfx, &draw);
}

fn draw_backdrop(draw: &mut Draw, w: f32, h: f32, time: f32) {
    const STEP: f32 = 72.0;
    let drift = (time * 9.0) % (STEP * 2.0);
    let rows = (h / STEP) as i32 + 3;
    let cols = (w / STEP) as i32 + 3;
    for j in -1..rows {
        for i in -1..cols {
            let stagger = if j % 2 == 0 { 0.0 } else { STEP / 2.0 };
            let x = i as f32 * STEP + stagger + drift / 2.0 - STEP;
            let y = j as f32 * STEP + drift / 2.0 - STEP;
            draw.circle(15.0).position(x, y).color(game::PATTERN);
        }
    }
}

fn draw_boards(
    draw: &mut Draw,
    pal: &Palette,
    fonts: &Fonts,
    session: &GameSession,
    layout: &GameLayout,
    hud: Hud,
    time: f32,
) {
    let me = &session.predicted_board;
    let (row_off, col_off) = session.piece_visual_offset;
    let piece = Piece {
        offset: (row_off + fall_step(me), col_off),
        satellite: session.my_turn.satellite(),
    };
    draw_board(draw, me, layout.mine, piece, time);
    draw_lock_meter(draw, me, layout.mine);
    draw_header(draw, fonts, "YOU", pal.text, session.my_nuisance(), layout.mine);
    let mut stats = vec![("SCORE", me.score.to_string())];
    if let Hud::Solo {
        versus: false, best, ..
    } = hud
    {
        stats.push(("BEST", best.max(me.score).to_string()));
    }
    let column_x = layout.mine.x - SIDE_GAP - COLUMN_W;
    draw_stats(draw, pal, fonts, (column_x, layout.top()), &stats);

    let Some(opponent) = hud.opponent() else {
        return;
    };
    let (opp_board, opp_offset) = session
        .opponent_view
        .frame()
        .unwrap_or((&session.other_board, (fall_step(&session.other_board), 0.0)));
    let piece = Piece {
        offset: opp_offset,
        satellite: session.opp_turn.satellite(),
    };
    draw_board(draw, opp_board, layout.theirs, piece, time);
    draw_header(
        draw,
        fonts,
        opponent,
        pal.text_muted,
        session.opp_nuisance(),
        layout.theirs,
    );
    let column_x = layout.theirs.x + layout.theirs.w + SIDE_GAP;
    draw_stats(
        draw,
        pal,
        fonts,
        (column_x, layout.top()),
        &[("SCORE", opp_board.score.to_string())],
    );
}

fn framed(area: Rect) -> Rect {
    Rect::at(
        area.x - FRAME_PAD,
        area.y - FRAME_PAD,
        area.w + 2.0 * FRAME_PAD,
        area.h + 2.0 * FRAME_PAD,
    )
}

fn panel(draw: &mut Draw, rect: Rect) {
    draw.rect((rect.x, rect.y + 3.0), (rect.w, rect.h))
        .corner_radius(10.0)
        .color(Color::BLACK.with_alpha(0.25));
    draw.rect((rect.x, rect.y), (rect.w, rect.h))
        .corner_radius(10.0)
        .color(game::PANEL);
    draw.rect((rect.x, rect.y), (rect.w, rect.h))
        .corner_radius(10.0)
        .stroke(2.0)
        .color(game::FRAME_INNER);
}

fn label(draw: &mut Draw, pal: &Palette, fonts: &Fonts, text: &str, (x, y): (f32, f32)) {
    draw.sharp_text(&fonts.display, text)
        .position(x + 4.0, y)
        .size(theme::size::HEADING)
        .color(pal.text_dim);
}

fn draw_stats(draw: &mut Draw, pal: &Palette, fonts: &Fonts, (x, mut y): (f32, f32), stats: &[(&str, String)]) {
    for (name, value) in stats {
        label(draw, pal, fonts, name, (x, y));
        let rect = Rect::at(x, y + LABEL_H, COLUMN_W, PANEL_H);
        panel(draw, rect);
        draw.sharp_text(&fonts.display, value)
            .position(rect.x + rect.w - 14.0, rect.y + rect.h / 2.0)
            .size(VALUE_SIZE)
            .h_align_right()
            .v_align_middle()
            .color(pal.text);
        y = rect.y + rect.h + STAT_GAP;
    }
}

fn draw_header(draw: &mut Draw, fonts: &Fonts, name: &str, color: Color, nuisance: u32, area: Rect) {
    let frame = framed(area);
    let tray = Rect::at(frame.x, frame.y - TRAY_H - 8.0, frame.w, TRAY_H);
    draw.sharp_text(&fonts.display, name)
        .position(frame.x + 4.0, tray.y - 40.0)
        .size(theme::size::HEADING)
        .color(color);
    draw.rect((tray.x, tray.y), (tray.w, tray.h))
        .corner_radius(tray.h / 2.0)
        .color(game::PANEL);
    draw.rect((tray.x, tray.y), (tray.w, tray.h))
        .corner_radius(tray.h / 2.0)
        .stroke(2.0)
        .color(game::FRAME_INNER);
    let icons = Nuisance::tray(nuisance, config::GRID_WIDTH);
    let cy = tray.y + tray.h / 2.0;
    for (i, icon) in icons.into_iter().enumerate() {
        let cx = area.x + (i as f32 + 0.5) * config::CELL_SIZE;
        sprites::nuisance_icon(draw, icon, (cx, cy), 12.0, game::PANEL);
    }
    if nuisance > 0 {
        draw.sharp_text(&fonts.display, &nuisance.to_string())
            .position(frame.x + frame.w - 4.0, tray.y - 40.0)
            .size(theme::size::HEADING)
            .h_align_right()
            .color(game::nuisance(nuisance));
    }
}

fn draw_sidebar(
    draw: &mut Draw,
    pal: &Palette,
    fonts: &Fonts,
    session: &GameSession,
    layout: &GameLayout,
    hud: Hud,
    time: f32,
) {
    let me = &session.predicted_board;
    let (x, top) = (layout.sidebar_x, layout.top());
    label(draw, pal, fonts, "NEXT", (x, top));
    let next = Rect::at(x, top + LABEL_H, COLUMN_W, 2.0 * config::CELL_SIZE + 50.0);
    panel(draw, next);
    let pair = |draw: &mut Draw, (cx, y): (f32, f32), (axis, sat): (PuyoType, PuyoType), scale: f32, id: usize| {
        let step = config::CELL_SIZE * scale;
        for (i, kind) in [sat, axis].into_iter().enumerate() {
            let mut p = Puyo::new(kind, (cx, y + step * (i as f32 + 0.5)), RADIUS * scale);
            p.mood = blink(time, i, id);
            sprites::single(draw, &p);
        }
    };
    let first_y = next.y + (next.h - 2.0 * config::CELL_SIZE) / 2.0;
    pair(draw, (next.x + next.w * 0.32, first_y), me.next_types, 1.0, 1);
    pair(
        draw,
        (next.x + next.w * 0.72, first_y + 30.0),
        me.next_next_types,
        0.75,
        2,
    );

    let mut stats = vec![("LEVEL", me.level().to_string())];
    if let Some(series) = hud.series() {
        stats.push(("ROUNDS", format!("{} - {}", series.wins[0], series.wins[1])));
    }
    draw_stats(draw, pal, fonts, (x, next.y + next.h + STAT_GAP), &stats);
}

fn draw_lock_meter(draw: &mut Draw, me: &Board, area: Rect) {
    if me.state != GameState::Playing || me.active_piece.is_none() || me.can_fall() {
        return;
    }
    let used = me.ground_frames as f32 / config::GRACE_FRAMES as f32;
    let ratio = (1.0 - used).max(0.0);
    let color = if used > 0.75 { theme::DANGER } else { theme::WARNING };
    draw.rect((area.x, area.y + area.h + 3.0), (area.w * ratio, 4.0))
        .corner_radius(2.0)
        .color(color);
}

fn shadowed(draw: &mut Draw, fonts: &Fonts, text: &str, (x, y): (f32, f32), size: f32, color: Color) {
    let shadow = game::TEXT_SHADOW.with_alpha(game::TEXT_SHADOW.a * color.a);
    for (dx, dy) in [(0.0, 4.0), (-2.0, 2.0), (2.0, 2.0)] {
        draw.sharp_text(&fonts.display, text)
            .position(x + dx, y + dy)
            .size(size)
            .h_align_center()
            .v_align_middle()
            .color(shadow);
    }
    draw.sharp_text(&fonts.display, text)
        .position(x, y)
        .size(size)
        .h_align_center()
        .v_align_middle()
        .color(color);
}

fn draw_overlay(
    draw: &mut Draw,
    pal: &Palette,
    fonts: &Fonts,
    overlay: &Overlay,
    layout: &GameLayout,
    hud: Hud,
    elapsed: f32,
) {
    let (cx, cy) = (layout.win_w / 2.0, layout.win_h / 2.0);
    let centered = |draw: &mut Draw, text: &str, y: f32, size: f32, color: Color| {
        draw.sharp_text(&fonts.display, text)
            .position(cx, y)
            .size(size)
            .h_align_center()
            .v_align_middle()
            .color(color);
    };
    let scrim = match overlay {
        Overlay::GameOver { .. } => theme::SCRIM_DARK,
        Overlay::OpponentGone => game::DISCONNECT_SCRIM,
        Overlay::Paused | Overlay::QuitMenu => theme::SCRIM_LIGHT,
    };
    draw.rect((0.0, 0.0), (layout.win_w, layout.win_h)).color(scrim);
    if let (Overlay::GameOver { i_lost, draw: tie }, Some(series)) = (overlay, hud.series()) {
        let (title, color) = match series.over {
            Some((Some(true), _)) => ("VICTORY", theme::GOLD),
            Some((Some(false), _)) => ("DEFEAT", theme::DANGER),
            Some((None, _)) => ("SERIES CANCELLED", pal.text),
            None if *tie => ("DRAW", pal.text),
            None if *i_lost => ("ROUND LOST", theme::DANGER),
            None => ("ROUND WON", theme::GOLD),
        };
        centered(draw, title, cy - 50.0, theme::size::HERO, color);
        let score = format!("{} - {}", series.wins[0], series.wins[1]);
        centered(draw, &score, cy + 5.0, theme::size::TITLE, pal.text);
        let forfeited = series.wins.iter().all(|&w| w < config::RANKED_WINS);
        let info = match series.over {
            Some((Some(true), elo)) if forfeited => format!("Opponent forfeited, ELO {elo:+}"),
            Some((Some(_), elo)) => format!("ELO {elo:+}"),
            Some((None, _)) => "Server restarting, no ELO change".to_string(),
            None => "Next round...".to_string(),
        };
        centered(draw, &info, cy + 50.0, theme::size::HEADING, pal.text_dim);
        return;
    }
    match overlay {
        Overlay::GameOver { i_lost, draw: tie } => {
            let (title, color) = if matches!(hud, Hud::Solo { new_best: true, .. }) {
                ("NEW RECORD !", theme::GOLD)
            } else if *tie {
                ("DRAW", pal.text)
            } else if *i_lost {
                ("GAME OVER", theme::DANGER)
            } else {
                ("YOU WIN !", theme::GOLD)
            };
            centered(draw, title, cy - 20.0, theme::size::HERO, color);
            centered(draw, "Press R to Restart", cy + 50.0, theme::size::HEADING, pal.text);
        }
        Overlay::QuitMenu => {
            centered(draw, "FORFEIT ?", cy - 40.0, theme::size::HERO, pal.text);
            centered(
                draw,
                "Press ESC to resume",
                cy + 30.0,
                theme::size::HEADING,
                pal.text_dim,
            );
        }
        Overlay::OpponentGone => centered(
            draw,
            "OPPONENT DISCONNECTED",
            cy - 20.0,
            theme::size::TITLE,
            theme::DANGER,
        ),
        Overlay::Paused => {
            let blink = 0.2 + (elapsed * 2.0).sin().abs() * 0.8;
            let can_resume = match hud {
                Hud::Online(role) => role.can_pause,
                Hud::Solo { .. } => true,
            };
            let hint = if can_resume {
                "Press ESC to continue"
            } else {
                "Seul l'hôte peut reprendre"
            };
            centered(draw, "PAUSED", cy - 40.0, theme::size::HERO, pal.text.with_alpha(blink));
            centered(draw, hint, cy + 30.0, theme::size::HEADING, pal.text.with_alpha(blink));
        }
    }
}

#[cfg(debug_assertions)]
fn draw_debug(draw: &mut Draw, fonts: &Fonts, session: &GameSession, win_h: f32) {
    let ping = session
        .ping_rtt_ms
        .map_or_else(|| "--".to_string(), |ms| format!("{ms:.0} ms"));
    let board = |label: &str, b: &Board| {
        format!(
            "{label} state={:?} pid={} pend_garb={} nuis={}",
            b.state, b.piece_id, b.pending_garbage, b.nuisance_points
        )
    };
    let lines = [
        (
            format!(
                "DEBUG NET: srv tick={} (+{}) {}",
                session.server_tick,
                i64::from(session.local_tick) - i64::from(session.server_tick),
                session.last_server_msg
            ),
            game::DEBUG_NET,
        ),
        (
            format!("RTT ping: {ping} | input->ack: {:.0} ms", session.last_rtt_ms),
            game::DEBUG_NET,
        ),
        (board("ME ", &session.predicted_board), game::DEBUG_BOARDS),
        (board("OPP", &session.other_board), game::DEBUG_BOARDS),
    ];
    for (i, (text, color)) in lines.iter().enumerate() {
        draw.sharp_text(&fonts.text, text)
            .position(10.0, win_h - 30.0 - i as f32 * 25.0)
            .size(theme::size::LABEL)
            .color(*color);
    }
}

const EXIT_ROW_H: f32 = 60.0;

pub fn exit_rows(view: View, with_back: bool) -> (Rect, Rect) {
    let top = view.h / 2.0 + 90.0;
    let first = Rect::at(0.0, top, view.w, EXIT_ROW_H);
    let second = Rect::at(0.0, top + EXIT_ROW_H, view.w, EXIT_ROW_H);
    if with_back {
        (second, first)
    } else {
        (first, second)
    }
}

fn draw_exit_buttons(draw: &mut Draw, ui: &Ui, fonts: &Fonts, hud: Hud, forfeits: bool) {
    let (leave, back, has_back) = match hud {
        Hud::Online(Role {
            series: Some(series), ..
        }) => {
            let leave = if series.over.is_some() {
                "Retour au menu"
            } else {
                "Abandonner (défaite)"
            };
            (leave, "", false)
        }
        Hud::Solo { .. } => ("Quitter", "Recommencer", true),
        Hud::Online(role) if forfeits => ("Abandonner (défaite)", "Lobby (défaite)", role.is_host),
        Hud::Online(role) => ("Quitter la room", "Retour au lobby", role.is_host),
    };
    let (leave_row, back_row) = exit_rows(ui.view(), has_back);
    ui.menu_bar(draw, fonts, leave_row, leave, theme::bar::RED);
    if has_back {
        ui.menu_bar(draw, fonts, back_row, back, theme::bar::YELLOW);
    }
}

const FALL_STEPS_PER_CELL: f32 = config::CELL_PX as f32;

pub fn fall_step(board: &Board) -> f32 {
    (board.fall_progress() * FALL_STEPS_PER_CELL).floor() / FALL_STEPS_PER_CELL
}

#[derive(Clone, Copy)]
struct Piece {
    offset: (f32, f32),
    satellite: (f32, f32),
}

fn blink(time: f32, r: usize, c: usize) -> Mood {
    let phase = ((r * 7 + c * 13) % 17) as f32 / 17.0 * 5.0;
    if (time + phase) % 5.0 < 0.14 {
        Mood::Blinking
    } else {
        Mood::Awake
    }
}

fn draw_well(draw: &mut Draw, area: Rect, danger: bool, time: f32) {
    let frame = framed(area);
    draw.rect((frame.x, frame.y + 5.0), (frame.w, frame.h))
        .corner_radius(18.0)
        .color(Color::BLACK.with_alpha(0.35));
    draw.rect((frame.x, frame.y), (frame.w, frame.h))
        .corner_radius(18.0)
        .color(if danger {
            theme::mix(game::FRAME, theme::DANGER, 0.5 + 0.5 * (time * 6.0).sin())
        } else {
            game::FRAME
        });
    draw.rect((frame.x + 4.0, frame.y + 4.0), (frame.w - 8.0, frame.h - 8.0))
        .corner_radius(14.0)
        .color(game::FRAME_INNER);
    draw.rect((area.x - 2.0, area.y - 2.0), (area.w + 4.0, area.h + 4.0))
        .corner_radius(10.0)
        .color(game::WELL);

    let cell = config::CELL_SIZE;
    let cols = config::GRID_WIDTH;
    let rows = config::GRID_HEIGHT - config::VISIBLE_ROW_OFFSET;
    for c in (1..cols).step_by(2) {
        draw.rect((area.x + c as f32 * cell, area.y), (cell, area.h))
            .color(game::WELL_LANE);
    }
    for r in 0..rows {
        for c in 0..cols {
            draw.circle(1.6)
                .position(area.x + (c as f32 + 0.5) * cell, area.y + (r as f32 + 0.5) * cell)
                .color(game::WELL_DOT);
        }
    }

    let (cx, cy) = (area.x + (config::SPAWN_COL as f32 + 0.5) * cell, area.y + cell / 2.0);
    let arm = cell * 0.22;
    let alpha = if danger { 0.75 + 0.25 * (time * 6.0).sin() } else { 0.55 };
    for (dx, dy) in [(arm, arm), (arm, -arm)] {
        draw.path()
            .move_to(cx - dx, cy - dy)
            .line_to(cx + dx, cy + dy)
            .stroke(5.0)
            .round_cap()
            .color(game::DEATH_CROSS.with_alpha(alpha));
    }
}

struct Cell {
    row: usize,
    col: usize,
    puyo: Puyo,
    settled: bool,
}

fn draw_board(draw: &mut Draw, board: &Board, area: Rect, piece: Piece, time: f32) {
    let hidden = config::VISIBLE_ROW_OFFSET;
    let danger = (hidden..hidden + 3).any(|r| board.cells[r][config::SPAWN_COL].is_some());
    draw_well(draw, area, danger, time);

    let cell = config::CELL_SIZE;
    let center = |row: f32, col: f32| (area.x + (col + 0.5) * cell, area.y + (row + 0.5) * cell);
    let fall_frame = match board.settle {
        Settle::Falling { frame, .. } => Some(frame),
        _ => None,
    };
    let pop_frame = match board.settle {
        Settle::Popping { frame } => Some(frame),
        _ => None,
    };

    let mut cells = Vec::new();
    for r in 0..board.height {
        for c in 0..board.width {
            let Some(kind) = board.cells[r][c] else {
                continue;
            };
            let mut row = r as f32 - hidden as f32;
            let mut squash = 0.0;
            let mut settled = true;
            if let Some(frame) = fall_frame {
                if let Some(f) = board.falls.iter().find(|f| f.row as usize == r && f.col as usize == c) {
                    let height = f.height_at(frame);
                    row -= height;
                    let bounce = f.bounce_at(frame);
                    settled = height <= 0.0 && bounce.is_none();
                    squash = (bounce.unwrap_or(0.0) * std::f32::consts::PI).sin() * 0.22;
                }
            }
            if row < -0.5 {
                continue;
            }
            let mut puyo = Puyo::new(kind, center(row, c as f32), RADIUS);
            puyo.squash = squash;
            puyo.mood = blink(time, r, c);
            if let Some(frame) = pop_frame {
                if board.popping.contains(&(r as u8, c as u8)) {
                    pop_look(&mut puyo, frame);
                }
            }
            cells.push(Cell {
                row: r,
                col: c,
                puyo,
                settled: settled && r >= hidden,
            });
        }
    }

    let mut at = vec![None; board.height * board.width];
    for (i, c) in cells.iter().enumerate() {
        at[c.row * board.width + c.col] = Some(i);
    }
    let mut bridges = Vec::new();
    for a in &cells {
        let neighbours = [
            (
                Joint::Right,
                (a.col + 1 < board.width).then(|| a.row * board.width + a.col + 1),
            ),
            (
                Joint::Down,
                (a.row + 1 < board.height).then(|| (a.row + 1) * board.width + a.col),
            ),
        ];
        for (joint, index) in neighbours {
            let Some(b) = index.and_then(|i| at[i]).map(|i| &cells[i]) else {
                continue;
            };
            if a.settled && b.settled && a.puyo.kind == b.puyo.kind && a.puyo.kind != PuyoType::Garbage {
                let mut puyo = a.puyo;
                puyo.alpha = a.puyo.alpha.min(b.puyo.alpha);
                puyo.flash = a.puyo.flash.max(b.puyo.flash);
                bridges.push((puyo, joint));
            }
        }
    }
    for layer in [Layer::Shadow, Layer::Body] {
        for (puyo, joint) in &bridges {
            sprites::bridge(draw, puyo, *joint, cell, layer);
        }
        for c in &cells {
            sprites::layer(draw, &c.puyo, layer);
        }
    }
    for c in &cells {
        sprites::face(draw, &c.puyo);
    }

    if board.state != GameState::Playing && board.state != GameState::Paused {
        return;
    }
    if let Some(ghost) = board.get_ghost_piece() {
        for (i, &(r, c)) in ghost.get_positions().iter().enumerate() {
            if r < hidden as i32 || board.cells[r as usize][c as usize].is_some() {
                continue;
            }
            let kind = if i == 0 { ghost.axis_type } else { ghost.sat_type };
            let at = center(r as f32 - hidden as f32, c as f32 + piece.offset.1);
            draw.circle(RADIUS * 0.32)
                .position(at.0, at.1)
                .color(game::puyo(kind).with_alpha(0.6));
        }
    }
    if let Some(ref active) = board.active_piece {
        let axis_r = active.row as f32 - hidden as f32 + piece.offset.0;
        let axis_c = active.col as f32 + piece.offset.1;
        let (sr, sc) = piece.satellite;
        let parts = [
            (axis_r + sr, axis_c + sc, active.sat_type, false),
            (axis_r, axis_c, active.axis_type, true),
        ];
        for (row, col, kind, axis) in parts {
            if row < -0.5 {
                continue;
            }
            let at = center(row, col);
            let mut puyo = Puyo::new(kind, at, RADIUS);
            puyo.mood = blink(time, 0, usize::from(axis));
            sprites::single(draw, &puyo);
            if axis {
                let pulse = 0.35 + 0.25 * (time * 5.0).sin();
                draw.circle(RADIUS + 1.0)
                    .position(at.0, at.1)
                    .stroke(2.5)
                    .color(Color::WHITE.with_alpha(pulse));
            }
        }
    }
}

fn pop_look(puyo: &mut Puyo, frame: u32) {
    const FADE: u32 = 8;
    let fade_from = config::POP_FRAMES.saturating_sub(FADE);
    puyo.mood = Mood::Popping;
    if frame >= fade_from {
        let t = (frame - fade_from) as f32 / FADE as f32;
        puyo.alpha = 1.0 - t;
        puyo.radius *= 1.0 + t * 0.3;
        puyo.flash = 0.6;
    } else if (frame / 4).is_multiple_of(2) {
        puyo.flash = 0.55;
    }
}

fn draw_chain_anim(draw: &mut Draw, fonts: &Fonts, chain_display: Option<(u32, f32)>, board: Rect) {
    let Some((count, t)) = chain_display else {
        return;
    };
    let alpha = (t / 0.5).min(1.0_f32);
    let pop = ((t - 1.75).max(0.0) / 0.25).min(1.0);
    let size = theme::size::TITLE * (1.0 + pop * 0.5);
    let cx = board.x + board.w / 2.0;
    let cy = board.y + board.h * 0.32 - (2.0 - t).max(0.0) * 14.0;
    shadowed(
        draw,
        fonts,
        &format!("{count} CHAIN!"),
        (cx, cy),
        size,
        game::chain(count).with_alpha(alpha),
    );
}
