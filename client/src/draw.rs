use notan::draw::*;
use notan::prelude::*;
use shared::*;

use crate::state::GameSession;
use crate::{config, Font};

pub fn draw_game(
    app: &mut App,
    gfx: &mut Graphics,
    session: &GameSession,
    font: &Font,
    is_host: bool,
    can_pause: bool,
) {
    let game_over = session.board.state == GameState::GameOver || session.other_board.state == GameState::GameOver;
    let leaving_forfeits = !game_over && !session.opponent_disconnected;

    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.05));

    let win_w = app.window().width() as f32;
    let win_h = app.window().height() as f32;

    let board_w = config::GRID_WIDTH as f32 * config::CELL_SIZE;
    let board_h = (config::GRID_HEIGHT - config::VISIBLE_ROW_OFFSET) as f32 * config::CELL_SIZE;
    let gap = 250.0;
    let total_w = board_w * 2.0 + gap;

    let start_x = (win_w - total_w) / 2.0;
    let offset_y = (win_h - board_h) / 2.0;

    let ui_x = start_x + board_w + 30.0;

    let me = &session.predicted_board;
    let (row_off, col_off) = session.piece_visual_offset;
    draw_board(
        &mut draw,
        me,
        start_x,
        offset_y,
        board_w,
        board_h,
        (row_off + fall_step(me), col_off),
        session.my_turn.satellite(),
    );
    draw.text(font, "YOU")
        .position(start_x, offset_y - 50.0)
        .size(20.0)
        .color(Color::WHITE);
    draw_nuisance_bar(
        &mut draw,
        font,
        session.my_nuisance(),
        start_x,
        offset_y - 28.0,
        board_w,
    );

    let opponent_x = start_x + board_w + gap;
    let (opp_board, opp_offset) = session
        .opponent_view
        .frame()
        .unwrap_or((&session.other_board, (0.0, 0.0)));
    draw_board(
        &mut draw,
        opp_board,
        opponent_x,
        offset_y,
        board_w,
        board_h,
        opp_offset,
        session.opp_turn.satellite(),
    );
    draw.text(font, "OPPONENT")
        .position(opponent_x, offset_y - 50.0)
        .size(20.0)
        .color(Color::GRAY);
    draw_nuisance_bar(
        &mut draw,
        font,
        session.opp_nuisance(),
        opponent_x,
        offset_y - 28.0,
        board_w,
    );

    draw.text(font, &format!("Score: {}", me.score))
        .position(ui_x, offset_y + 20.0)
        .size(30.0)
        .color(Color::WHITE);
    draw.text(font, &format!("Level: {}", me.level()))
        .position(ui_x, offset_y + 60.0)
        .size(30.0)
        .color(Color::YELLOW);

    draw.text(font, "Next:")
        .position(ui_x, offset_y + 110.0)
        .size(30.0)
        .color(Color::GRAY);
    draw.rect((ui_x, offset_y + 140.0), (config::CELL_SIZE, config::CELL_SIZE * 2.1))
        .color(Color::from_rgb(0.2, 0.2, 0.2));
    draw_puyo(&mut draw, 0.0, 0.0, me.next_types.1, ui_x, offset_y + 140.0, 1.0, 0.0);
    draw_puyo(&mut draw, 1.0, 0.0, me.next_types.0, ui_x, offset_y + 140.0, 1.0, 0.0);

    let next_next_y = offset_y + 170.0 + (config::CELL_SIZE * 2.5);
    draw.text(font, "Next Next:")
        .position(ui_x, next_next_y - 25.0)
        .size(20.0)
        .color(Color::GRAY);
    draw.rect((ui_x, next_next_y), (config::CELL_SIZE, config::CELL_SIZE * 2.1))
        .color(Color::from_rgb(0.15, 0.15, 0.15));
    draw_puyo(&mut draw, 0.0, 0.0, me.next_next_types.1, ui_x, next_next_y, 1.0, 0.0);
    draw_puyo(&mut draw, 1.0, 0.0, me.next_next_types.0, ui_x, next_next_y, 1.0, 0.0);

    draw_chain_anim(
        &mut draw,
        font,
        &session.chain_display,
        start_x,
        offset_y,
        board_w,
        board_h,
    );

    if me.state == GameState::Playing && me.active_piece.is_some() && !me.can_fall() {
        let used = me.ground_frames as f32 / config::GRACE_FRAMES as f32;
        let ratio = (1.0 - used).max(0.0);
        let col = if used > 0.75 { Color::RED } else { Color::ORANGE };
        draw.rect((ui_x, offset_y + 350.0), (100.0 * ratio, 10.0)).color(col);
    }

    if session.opponent_disconnected {
        draw.rect((0.0, 0.0), (win_w, win_h))
            .color(Color::from_rgba(0.5, 0.0, 0.0, 0.5));
        draw.text(font, "OPPONENT DISCONNECTED")
            .position(win_w / 2.0, win_h / 2.0 - 20.0)
            .size(40.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::RED);
        draw_exit_buttons(&mut draw, app, font, win_w, win_h, is_host, leaving_forfeits);
    }

    let i_lost = session.board.state == GameState::GameOver;
    let opponent_lost = session.other_board.state == GameState::GameOver;
    if i_lost || opponent_lost {
        draw.rect((0.0, 0.0), (win_w, win_h))
            .color(Color::from_rgba(0.0, 0.0, 0.0, 0.7));
        if i_lost {
            draw.text(font, "GAME OVER")
                .position(win_w / 2.0, win_h / 2.0 - 20.0)
                .size(60.0)
                .h_align_center()
                .v_align_middle()
                .color(Color::RED);
        } else {
            draw.text(font, "YOU WIN !")
                .position(win_w / 2.0, win_h / 2.0 - 20.0)
                .size(80.0)
                .h_align_center()
                .v_align_middle()
                .color(Color::YELLOW);
        }
        draw.text(font, "Press R to Restart")
            .position(win_w / 2.0, win_h / 2.0 + 50.0)
            .size(28.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::WHITE);
        draw_exit_buttons(&mut draw, app, font, win_w, win_h, is_host, leaving_forfeits);
    }

    if session.board.state == GameState::Paused && !session.opponent_disconnected {
        draw.rect((0.0, 0.0), (win_w, win_h))
            .color(Color::from_rgba(0.0, 0.0, 0.0, 0.5));
        let alpha = (app.timer.elapsed_f32() * 2.0).sin().abs();
        let visible_alpha = 0.2 + (alpha * 0.8);
        draw.text(font, "PAUSED")
            .position(win_w / 2.0, win_h / 2.0 - 40.0)
            .size(60.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgba(1.0, 1.0, 1.0, visible_alpha));
        let hint = if can_pause {
            "Press ESC to continue"
        } else {
            "Seul l'hôte peut reprendre"
        };
        draw.text(font, hint)
            .position(win_w / 2.0, win_h / 2.0 + 30.0)
            .size(28.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgba(1.0, 1.0, 1.0, visible_alpha));
        draw_exit_buttons(&mut draw, app, font, win_w, win_h, is_host, leaving_forfeits);
    }

    if session.all_clear_timer > 0.0 {
        let alpha = (session.all_clear_timer / 3.0).min(1.0);
        draw.text(font, "ALL CLEAR!")
            .position(start_x + board_w / 2.0, offset_y + board_h / 2.0)
            .size(48.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgba(1.0, 1.0, 0.0, alpha));
    }

    #[cfg(debug_assertions)]
    {
        draw.text(
            font,
            &format!(
                "DEBUG NET: srv tick={} (+{}) {}",
                session.server_tick,
                session.local_tick as i64 - session.server_tick as i64,
                session.last_server_msg
            ),
        )
        .position(10.0, win_h - 30.0)
        .size(20.0)
        .color(Color::MAGENTA);
        let ping = match session.ping_rtt_ms {
            Some(ms) => format!("{ms:.0} ms"),
            None => "--".to_string(),
        };
        draw.text(
            font,
            &format!("RTT ping: {ping} | input->ack: {:.0} ms", session.last_rtt_ms),
        )
        .position(10.0, win_h - 55.0)
        .size(20.0)
        .color(Color::MAGENTA);

        let me = &session.predicted_board;
        let opp = &session.other_board;
        draw.text(
            font,
            &format!(
                "ME  state={:?} pid={} pend_garb={} nuis={}",
                me.state, me.piece_id, me.pending_garbage, me.nuisance_points
            ),
        )
        .position(10.0, win_h - 80.0)
        .size(20.0)
        .color(Color::from_rgb(0.0, 1.0, 1.0));
        draw.text(
            font,
            &format!(
                "OPP state={:?} pid={} pend_garb={} nuis={}",
                opp.state, opp.piece_id, opp.pending_garbage, opp.nuisance_points
            ),
        )
        .position(10.0, win_h - 105.0)
        .size(20.0)
        .color(Color::from_rgb(0.0, 1.0, 1.0));
    }

    gfx.render(&draw);
}

fn draw_exit_buttons(draw: &mut Draw, app: &App, font: &Font, ww: f32, wh: f32, is_host: bool, forfeits: bool) {
    let (leave, back) = if forfeits {
        ("Abandonner (défaite)", "Lobby (défaite)")
    } else {
        ("Leave Room", "Back to Lobby")
    };
    crate::rooms::leave_room_button(ww, wh).draw(draw, app, font, leave);
    if is_host {
        crate::rooms::back_to_lobby_button(ww, wh).draw(draw, app, font, back);
    }
}

/// Notches a falling piece shows per cell it falls: Tsu's 16 pixels, so the
/// natural fall moves a pixel a frame and a soft drop half a cell a frame.
/// 2 gives half-cell steps, 1 whole cells.
const FALL_STEPS_PER_CELL: f32 = config::CELL_PX as f32;

/// How far below its row to draw the falling piece: its fall progress,
/// rounded down to a notch. Drawing only; the simulation knows whole rows.
pub fn fall_step(board: &Board) -> f32 {
    (board.fall_progress() * FALL_STEPS_PER_CELL).floor() / FALL_STEPS_PER_CELL
}

#[allow(clippy::too_many_arguments)]
fn draw_board(
    draw: &mut Draw,
    board: &Board,
    offset_x: f32,
    offset_y: f32,
    board_w: f32,
    board_h: f32,
    piece_offset: (f32, f32),
    satellite: (f32, f32),
) {
    draw.rect((offset_x, offset_y), (board_w, board_h))
        .color(Color::from_rgb(0.12, 0.12, 0.12));

    let x_cross = offset_x + (config::SPAWN_COL as f32 * config::CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw.line((x_cross, y_cross), (x_cross + 20.0, y_cross + 20.0))
        .width(3.0)
        .color(Color::RED);
    draw.line((x_cross + 20.0, y_cross), (x_cross, y_cross + 20.0))
        .width(3.0)
        .color(Color::RED);

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
            draw_puyo(draw, row, c as f32, pt, offset_x, offset_y, alpha, bounce);
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
                // Where the piece will land does not glide with it; only a
                // sideways correction applies.
                let draw_r = pos.0 as f32 - hidden;
                let draw_c = pos.1 as f32 + piece_offset.1;
                draw_puyo(draw, draw_r, draw_c, p_type, offset_x, offset_y, 0.3, 0.0);
            }
        }
        if let Some(ref piece) = board.active_piece {
            let axis_r = piece.row as f32 - hidden + piece_offset.0;
            let axis_c = piece.col as f32 + piece_offset.1;
            draw_puyo(draw, axis_r, axis_c, piece.axis_type, offset_x, offset_y, 1.0, 0.0);
            // The satellite as drawn turns around the axis.
            let (sr, sc) = satellite;
            draw_puyo(
                draw,
                axis_r + sr,
                axis_c + sc,
                piece.sat_type,
                offset_x,
                offset_y,
                1.0,
                0.0,
            );
        }
    }

    let visible_height = (board.height - config::VISIBLE_ROW_OFFSET) as f32;
    for i in 0..=board.width {
        let x = offset_x + (i as f32 * config::CELL_SIZE);
        draw.line((x, offset_y), (x, offset_y + board_h))
            .width(1.0)
            .color(Color::GRAY);
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as f32 * config::CELL_SIZE);
        draw.line((offset_x, y), (offset_x + board_w, y))
            .width(1.0)
            .color(Color::GRAY);
    }
}

/// A popping group blinks, then fades out over its last frames.
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

/// One puyo. `bounce`, 0 to 1 through a landing, squashes it against the
/// floor and lets it spring back.
#[allow(clippy::too_many_arguments)]
fn draw_puyo(draw: &mut Draw, row: f32, col: f32, pt: PuyoType, dx: f32, dy: f32, alpha: f32, bounce: f32) {
    if row < 0.0 || alpha <= 0.0 {
        return;
    }
    let mut color = get_puyo_color(pt);
    color.a = alpha;

    let squash = (bounce * std::f32::consts::PI).sin() * 0.22;
    let size = config::CELL_SIZE - 2.0;
    let (w, h) = (size * (1.0 + squash * 0.6), size * (1.0 - squash));
    // Anchored on the cell's floor, centred horizontally.
    let x = dx + col * config::CELL_SIZE + 1.0 + (size - w) / 2.0;
    let y = dy + row * config::CELL_SIZE + 1.0 + (size - h);

    draw.rect((x, y), (w, h)).color(color);

    if pt == PuyoType::Garbage {
        draw.rect((x + w * 0.25, y + h * 0.25), (w * 0.5, h * 0.5))
            .color(Color::from_rgba(0.0, 0.0, 0.0, alpha));
    }
}

fn get_puyo_color(puyo_type: PuyoType) -> Color {
    match puyo_type {
        PuyoType::Red => Color::RED,
        PuyoType::Blue => Color::BLUE,
        PuyoType::Yellow => Color::YELLOW,
        PuyoType::Green => Color::GREEN,
        PuyoType::Purple => Color::MAGENTA,
        PuyoType::Garbage => Color::GRAY,
    }
}

fn draw_nuisance_bar(draw: &mut Draw, font: &Font, nuisance: u32, board_x: f32, bar_y: f32, board_w: f32) {
    if nuisance == 0 {
        return;
    }

    let block_w = board_w / config::GRID_WIDTH as f32;
    let block_h = 20.0;
    let rocks = (nuisance / 6).min(config::GRID_WIDTH as u32);
    let leftover = nuisance % 6;

    let color = match nuisance {
        1..=12 => Color::from_rgb(0.3, 0.9, 0.3),
        13..=30 => Color::from_rgb(1.0, 0.8, 0.1),
        31..=60 => Color::from_rgb(1.0, 0.5, 0.0),
        _ => Color::from_rgb(1.0, 0.15, 0.15),
    };

    for i in 0..rocks {
        let x = board_x + i as f32 * block_w + 1.0;
        draw.rect((x, bar_y), (block_w - 2.0, block_h)).color(color);
    }

    if leftover > 0 && rocks < config::GRID_WIDTH as u32 {
        let x = board_x + rocks as f32 * block_w + 1.0;
        let partial_w = (block_w - 2.0) * leftover as f32 / 6.0;
        draw.rect((x, bar_y), (partial_w, block_h))
            .color(Color::from_rgba(color.r, color.g, color.b, 0.5));
    }

    if nuisance > config::GRID_WIDTH as u32 * 6 {
        draw.text(font, &format!("+{}", nuisance))
            .position(board_x + board_w - 2.0, bar_y - 1.0)
            .size(14.0)
            .h_align_right()
            .color(Color::WHITE);
    }
}

fn draw_chain_anim(
    draw: &mut Draw,
    font: &Font,
    chain_display: &Option<(u32, f32)>,
    board_x: f32,
    board_y: f32,
    board_w: f32,
    board_h: f32,
) {
    let Some((count, t)) = chain_display else {
        return;
    };
    let alpha = (*t / 0.5).min(1.0_f32);
    let scale = 1.0 + ((*t - 1.7).max(0.0) / 0.3 * 0.4).min(0.4_f32);
    let size = 42.0 * scale;

    let cx = board_x + board_w / 2.0;
    let cy = board_y + board_h * 0.35;

    let chain_color = match count {
        1 => Color::from_rgba(0.4, 1.0, 0.4, alpha),
        2 => Color::from_rgba(0.4, 0.8, 1.0, alpha),
        3 => Color::from_rgba(1.0, 0.9, 0.2, alpha),
        4 => Color::from_rgba(1.0, 0.5, 0.1, alpha),
        _ => Color::from_rgba(1.0, 0.2, 1.0, alpha),
    };
    draw.text(font, &format!("{}  CHAIN!", count))
        .position(cx, cy)
        .size(size)
        .h_align_center()
        .v_align_middle()
        .color(chain_color);
}
