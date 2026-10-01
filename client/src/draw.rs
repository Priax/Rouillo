use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::{Board, GameState, PuyoType, Settle};

use crate::config;
use crate::state::GameSession;
use crate::theme::{self, game, Palette};
use crate::ui::{Fonts, Rect, SharpText, Ui, View};

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
        let gap = 250.0;
        let boards = if two_boards { 2.0 } else { 1.0 };
        let start_x = (win_w - (board_w * boards + gap)) / 2.0;
        let offset_y = (win_h - board_h) / 2.0;
        Self {
            win_w,
            win_h,
            mine: Rect::at(start_x, offset_y, board_w, board_h),
            theirs: Rect::at(start_x + board_w + gap, offset_y, board_w, board_h),
            sidebar_x: start_x + board_w + 30.0,
        }
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
    let mut draw = ui.canvas(gfx);
    draw.clear(game::BACKGROUND);

    draw_boards(&mut draw, &pal, fonts, session, &layout, hud);
    draw_sidebar(
        &mut draw,
        &pal,
        fonts,
        &session.predicted_board,
        layout.sidebar_x,
        layout.mine.y,
    );
    if let Some(series) = hud.series() {
        draw.sharp_text(&fonts.text, &format!("Rounds: {} - {}", series.wins[0], series.wins[1]))
            .position(layout.sidebar_x, layout.mine.y + 390.0)
            .size(theme::size::LABEL)
            .color(pal.text_muted);
    }
    if let Hud::Solo {
        versus: false, best, ..
    } = hud
    {
        draw.sharp_text(
            &fonts.text,
            &format!("Best: {}", best.max(session.predicted_board.score)),
        )
        .position(layout.sidebar_x, layout.mine.y + 390.0)
        .size(theme::size::LABEL)
        .color(pal.text_muted);
    }
    draw_chain_anim(&mut draw, fonts, session.chain_display, layout.mine);
    if session.all_clear_timer > 0.0 {
        let alpha = (session.all_clear_timer / 3.0).min(1.0);
        draw.sharp_text(&fonts.display, "ALL CLEAR!")
            .position(layout.mine.x + layout.mine.w / 2.0, layout.mine.y + layout.mine.h / 2.0)
            .size(theme::size::TITLE)
            .h_align_center()
            .v_align_middle()
            .color(game::ALL_CLEAR.with_alpha(alpha));
    }
    if let Some(overlay) = Overlay::of(session, hud) {
        let game_over = matches!(overlay, Overlay::GameOver { .. });
        let leaving_forfeits = !game_over && !session.opponent_disconnected;
        draw_overlay(&mut draw, &pal, fonts, &overlay, &layout, hud, app.timer.elapsed_f32());
        draw_exit_buttons(&mut draw, ui, fonts, hud, leaving_forfeits);
    }
    #[cfg(debug_assertions)]
    if matches!(hud, Hud::Online(_)) {
        draw_debug(&mut draw, fonts, session, layout.win_h);
    }

    ui.render(gfx, &draw);
}

fn draw_boards(draw: &mut Draw, pal: &Palette, fonts: &Fonts, session: &GameSession, layout: &GameLayout, hud: Hud) {
    let me = &session.predicted_board;
    let (row_off, col_off) = session.piece_visual_offset;
    draw_board(
        draw,
        me,
        layout.mine,
        (row_off + fall_step(me), col_off),
        session.my_turn.satellite(),
    );
    draw.sharp_text(&fonts.text, "YOU")
        .position(layout.mine.x, layout.mine.y - 50.0)
        .size(theme::size::LABEL)
        .color(pal.text);
    draw_nuisance_bar(draw, pal, fonts, session.my_nuisance(), layout.mine);

    let Some(opponent) = hud.opponent() else {
        return;
    };
    let (opp_board, opp_offset) = session
        .opponent_view
        .frame()
        .unwrap_or((&session.other_board, (fall_step(&session.other_board), 0.0)));
    draw_board(draw, opp_board, layout.theirs, opp_offset, session.opp_turn.satellite());
    draw.sharp_text(&fonts.text, opponent)
        .position(layout.theirs.x, layout.theirs.y - 50.0)
        .size(theme::size::LABEL)
        .color(pal.text_muted);
    draw_nuisance_bar(draw, pal, fonts, session.opp_nuisance(), layout.theirs);
}

fn draw_sidebar(draw: &mut Draw, pal: &Palette, fonts: &Fonts, me: &Board, x: f32, top: f32) {
    draw.sharp_text(&fonts.display, &format!("Score: {}", me.score))
        .position(x, top + 20.0)
        .size(theme::size::HEADING)
        .color(pal.text);
    draw.sharp_text(&fonts.display, &format!("Level: {}", me.level()))
        .position(x, top + 60.0)
        .size(theme::size::HEADING)
        .color(theme::GOLD);

    let next_y = top + 140.0;
    draw.sharp_text(&fonts.display, "Next:")
        .position(x, next_y - 30.0)
        .size(theme::size::HEADING)
        .color(pal.text_muted);
    draw_preview(draw, (x, next_y), me.next_types, game::PREVIEW);

    let next_next_y = top + 170.0 + (config::CELL_SIZE * 2.5);
    draw.sharp_text(&fonts.text, "Next Next:")
        .position(x, next_next_y - 25.0)
        .size(theme::size::LABEL)
        .color(pal.text_muted);
    draw_preview(draw, (x, next_next_y), me.next_next_types, game::PREVIEW_NEXT);

    if me.state == GameState::Playing && me.active_piece.is_some() && !me.can_fall() {
        let used = me.ground_frames as f32 / config::GRACE_FRAMES as f32;
        let ratio = (1.0 - used).max(0.0);
        let col = if used > 0.75 { theme::DANGER } else { theme::WARNING };
        draw.rect((x, top + 350.0), (100.0 * ratio, 10.0)).color(col);
    }
}

fn draw_preview(draw: &mut Draw, origin: (f32, f32), (axis, satellite): (PuyoType, PuyoType), bg: Color) {
    draw.rect(origin, (config::CELL_SIZE, config::CELL_SIZE * 2.1))
        .color(bg);
    draw_puyo(draw, origin, Sprite::solid(0.0, 0.0, satellite));
    draw_puyo(draw, origin, Sprite::solid(1.0, 0.0, axis));
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

pub fn exit_rows(view: View) -> (Rect, Rect) {
    let top = view.h / 2.0 + 90.0;
    (
        Rect::at(0.0, top, view.w, EXIT_ROW_H),
        Rect::at(0.0, top + EXIT_ROW_H, view.w, EXIT_ROW_H),
    )
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
    let (leave_row, back_row) = exit_rows(ui.view());
    ui.menu_bar(draw, fonts, leave_row, leave, theme::bar::RED);
    if has_back {
        ui.menu_bar(draw, fonts, back_row, back, theme::bar::YELLOW);
    }
}

const FALL_STEPS_PER_CELL: f32 = config::CELL_PX as f32;

pub fn fall_step(board: &Board) -> f32 {
    (board.fall_progress() * FALL_STEPS_PER_CELL).floor() / FALL_STEPS_PER_CELL
}

fn draw_board(draw: &mut Draw, board: &Board, area: Rect, piece_offset: (f32, f32), satellite: (f32, f32)) {
    let Rect {
        x: offset_x,
        y: offset_y,
        w: board_w,
        h: board_h,
    } = area;
    let origin = (offset_x, offset_y);
    draw.rect((offset_x, offset_y), (board_w, board_h)).color(game::BOARD);

    let x_cross = offset_x + (config::SPAWN_COL as f32 * config::CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw.line((x_cross, y_cross), (x_cross + 20.0, y_cross + 20.0))
        .width(3.0)
        .color(game::DEATH_CROSS);
    draw.line((x_cross + 20.0, y_cross), (x_cross, y_cross + 20.0))
        .width(3.0)
        .color(game::DEATH_CROSS);

    let fall_frame = match board.settle {
        Settle::Falling { frame, .. } => Some(frame),
        _ => None,
    };
    let pop_frame = match board.settle {
        Settle::Popping { frame } => Some(frame),
        _ => None,
    };
    let hidden = config::VISIBLE_ROW_OFFSET as f32;
    for r in 0..board.height {
        for c in 0..board.width {
            let Some(pt) = board.cells[r][c] else {
                continue;
            };
            let mut row = r as f32 - hidden;
            let mut bounce = 0.0;
            let mut alpha = 1.0;
            if let Some(frame) = fall_frame {
                if let Some(f) = board.falls.iter().find(|f| f.row as usize == r && f.col as usize == c) {
                    row -= f.height_at(frame);
                    bounce = f.bounce_at(frame).unwrap_or(0.0);
                }
            }
            if let Some(frame) = pop_frame {
                if board.popping.contains(&(r as u8, c as u8)) {
                    alpha = pop_alpha(frame);
                }
            }
            let sprite = Sprite {
                row,
                col: c as f32,
                kind: pt,
                alpha,
                bounce,
            };
            draw_puyo(draw, origin, sprite);
        }
    }

    if board.state == GameState::Playing || board.state == GameState::Paused {
        if let Some(ghost) = board.get_ghost_piece() {
            for pos in &ghost.get_positions() {
                if pos.0 >= 0 && (board.cells[pos.0 as usize][pos.1 as usize]).is_some() {
                    continue;
                }
                let p_type = if pos.0 == ghost.row && pos.1 == ghost.col {
                    ghost.axis_type
                } else {
                    ghost.sat_type
                };
                let draw_r = pos.0 as f32 - hidden;
                let draw_c = pos.1 as f32 + piece_offset.1;
                let faded = Sprite {
                    alpha: GHOST_ALPHA,
                    ..Sprite::solid(draw_r, draw_c, p_type)
                };
                draw_puyo(draw, origin, faded);
            }
        }
        if let Some(ref piece) = board.active_piece {
            let axis_r = piece.row as f32 - hidden + piece_offset.0;
            let axis_c = piece.col as f32 + piece_offset.1;
            draw_puyo(draw, origin, Sprite::solid(axis_r, axis_c, piece.axis_type));
            let (sr, sc) = satellite;
            draw_puyo(draw, origin, Sprite::solid(axis_r + sr, axis_c + sc, piece.sat_type));
        }
    }

    let visible_height = (board.height - config::VISIBLE_ROW_OFFSET) as f32;
    for i in 0..=board.width {
        let x = offset_x + (i as f32 * config::CELL_SIZE);
        draw.line((x, offset_y), (x, offset_y + board_h))
            .width(1.0)
            .color(game::GRID);
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as f32 * config::CELL_SIZE);
        draw.line((offset_x, y), (offset_x + board_w, y))
            .width(1.0)
            .color(game::GRID);
    }
}

fn pop_alpha(frame: u32) -> f32 {
    const FADE: u32 = 8;
    let fade_from = config::POP_FRAMES.saturating_sub(FADE);
    if frame >= fade_from {
        1.0 - (frame - fade_from) as f32 / FADE as f32
    } else if (frame / 4).is_multiple_of(2) {
        1.0
    } else {
        0.35
    }
}

const GHOST_ALPHA: f32 = 0.3;

#[derive(Clone, Copy)]
struct Sprite {
    row: f32,
    col: f32,
    kind: PuyoType,
    alpha: f32,
    bounce: f32,
}

impl Sprite {
    const fn solid(row: f32, col: f32, kind: PuyoType) -> Self {
        Self {
            row,
            col,
            kind,
            alpha: 1.0,
            bounce: 0.0,
        }
    }
}

fn draw_puyo(draw: &mut Draw, (dx, dy): (f32, f32), sprite: Sprite) {
    let Sprite {
        row,
        col,
        kind: pt,
        alpha,
        bounce,
    } = sprite;
    if row < 0.0 || alpha <= 0.0 {
        return;
    }
    let color = game::puyo(pt).with_alpha(alpha);

    let squash = (bounce * std::f32::consts::PI).sin() * 0.22;
    let size = config::CELL_SIZE - 2.0;
    let (w, h) = (size * (1.0 + squash * 0.6), size * (1.0 - squash));
    let x = dx + col * config::CELL_SIZE + 1.0 + (size - w) / 2.0;
    let y = dy + row * config::CELL_SIZE + 1.0 + (size - h);

    draw.rect((x, y), (w, h)).color(color);

    if pt == PuyoType::Garbage {
        draw.rect((x + w * 0.25, y + h * 0.25), (w * 0.5, h * 0.5))
            .color(game::GARBAGE_CORE.with_alpha(alpha));
    }
}

fn draw_nuisance_bar(draw: &mut Draw, pal: &Palette, fonts: &Fonts, nuisance: u32, board: Rect) {
    if nuisance == 0 {
        return;
    }
    let (board_x, board_w, bar_y) = (board.x, board.w, board.y - 28.0);

    let block_w = board_w / config::GRID_WIDTH as f32;
    let block_h = 20.0;
    let rocks = (nuisance / 6).min(config::GRID_WIDTH as u32);
    let leftover = nuisance % 6;

    let color = game::nuisance(nuisance);

    for i in 0..rocks {
        let x = board_x + i as f32 * block_w + 1.0;
        draw.rect((x, bar_y), (block_w - 2.0, block_h)).color(color);
    }

    if leftover > 0 && rocks < config::GRID_WIDTH as u32 {
        let x = board_x + rocks as f32 * block_w + 1.0;
        let partial_w = (block_w - 2.0) * leftover as f32 / 6.0;
        draw.rect((x, bar_y), (partial_w, block_h)).color(color.with_alpha(0.5));
    }

    if nuisance > config::GRID_WIDTH as u32 * 6 {
        draw.sharp_text(&fonts.text, &format!("+{nuisance}"))
            .position(board_x + board_w - 2.0, bar_y - 1.0)
            .size(theme::size::SMALL)
            .h_align_right()
            .color(pal.text);
    }
}

fn draw_chain_anim(draw: &mut Draw, fonts: &Fonts, chain_display: Option<(u32, f32)>, board: Rect) {
    let Some((count, t)) = chain_display else {
        return;
    };
    let alpha = (t / 0.5).min(1.0_f32);
    let scale = 1.0 + ((t - 1.7).max(0.0) / 0.3 * 0.4).min(0.4_f32);
    let size = theme::size::TITLE * scale;

    let cx = board.x + board.w / 2.0;
    let cy = board.y + board.h * 0.35;

    let chain_color = game::chain(count).with_alpha(alpha);
    draw.sharp_text(&fonts.display, &format!("{count}  CHAIN!"))
        .position(cx, cy)
        .size(size)
        .h_align_center()
        .v_align_middle()
        .color(chain_color);
}
