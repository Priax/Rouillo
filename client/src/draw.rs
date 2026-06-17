use notan::draw::*;
use notan::prelude::*;
use shared::*;

use crate::state::GameSession;
use crate::{config, Font};

pub fn draw_game(app: &mut App, gfx: &mut Graphics, session: &GameSession, font: &Font, is_host: bool) {
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
    draw_board(
        &mut draw,
        me,
        start_x,
        offset_y,
        board_w,
        board_h,
        session.piece_visual_offset,
    );
    draw.text(font, "YOU")
        .position(start_x, offset_y - 50.0)
        .size(20.0)
        .color(Color::WHITE);
    draw_nuisance_bar(
        &mut draw,
        font,
        session.board.pending_garbage,
        start_x,
        offset_y - 28.0,
        board_w,
    );

    let opponent_x = start_x + board_w + gap;
    draw_board(
        &mut draw,
        &session.other_board,
        opponent_x,
        offset_y,
        board_w,
        board_h,
        session.opponent_piece_offset,
    );
    draw.text(font, "OPPONENT")
        .position(opponent_x, offset_y - 50.0)
        .size(20.0)
        .color(Color::GRAY);
    draw_nuisance_bar(
        &mut draw,
        font,
        session.other_board.pending_garbage,
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
    draw_cell(&mut draw, 0.0, 0.0, Some(me.next_types.1), ui_x, offset_y + 140.0, 1.0);
    draw_cell(&mut draw, 1.0, 0.0, Some(me.next_types.0), ui_x, offset_y + 140.0, 1.0);

    let next_next_y = offset_y + 170.0 + (config::CELL_SIZE * 2.5);
    draw.text(font, "Next Next:")
        .position(ui_x, next_next_y - 25.0)
        .size(20.0)
        .color(Color::GRAY);
    draw.rect((ui_x, next_next_y), (config::CELL_SIZE, config::CELL_SIZE * 2.1))
        .color(Color::from_rgb(0.15, 0.15, 0.15));
    draw_cell(&mut draw, 0.0, 0.0, Some(me.next_next_types.1), ui_x, next_next_y, 1.0);
    draw_cell(&mut draw, 1.0, 0.0, Some(me.next_next_types.0), ui_x, next_next_y, 1.0);

    draw_chain_anim(
        &mut draw,
        font,
        &session.chain_display,
        start_x,
        offset_y,
        board_w,
        board_h,
    );

    if me.is_touching_ground && me.state == GameState::Playing {
        let ratio_std = 1.0 - (me.lock_timer / config::MAX_LOCK_TIME);
        let ratio_hard = 1.0 - (me.total_ground_timer / config::MAX_TOTAL_GROUND_TIME);
        let ratio = ratio_std.min(ratio_hard).max(0.0);
        let col = if me.total_ground_timer > 1.5 {
            Color::RED
        } else {
            Color::ORANGE
        };
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
        draw_exit_buttons(&mut draw, app, font, win_w, win_h, is_host);
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
        draw_exit_buttons(&mut draw, app, font, win_w, win_h, is_host);
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
        draw.text(font, "Press ESC to continue")
            .position(win_w / 2.0, win_h / 2.0 + 30.0)
            .size(28.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgba(1.0, 1.0, 1.0, visible_alpha));
        draw_exit_buttons(&mut draw, app, font, win_w, win_h, is_host);
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
        draw.text(font, &format!("DEBUG NET: {}", session.last_server_msg))
            .position(10.0, win_h - 30.0)
            .size(20.0)
            .color(Color::MAGENTA);
        draw.text(font, &format!("RTT input->ack: {:.0} ms", session.last_rtt_ms))
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

fn draw_exit_buttons(draw: &mut Draw, app: &App, font: &Font, ww: f32, wh: f32, is_host: bool) {
    crate::rooms::leave_room_button(ww, wh).draw(draw, app, font, "Leave Room");
    if is_host {
        crate::rooms::back_to_lobby_button(ww, wh).draw(draw, app, font, "Back to Lobby");
    }
}

fn draw_board(
    draw: &mut Draw,
    board: &Board,
    offset_x: f32,
    offset_y: f32,
    board_w: f32,
    board_h: f32,
    piece_offset: (f32, f32),
) {
    draw.rect((offset_x, offset_y), (board_w, board_h))
        .color(Color::from_rgb(0.12, 0.12, 0.12));

    let x_cross = offset_x + (2.0 * config::CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw.line((x_cross, y_cross), (x_cross + 20.0, y_cross + 20.0))
        .width(3.0)
        .color(Color::RED);
    draw.line((x_cross + 20.0, y_cross), (x_cross, y_cross + 20.0))
        .width(3.0)
        .color(Color::RED);

    for r in config::VISIBLE_ROW_OFFSET..board.height {
        for c in 0..board.width {
            let draw_r = (r - config::VISIBLE_ROW_OFFSET) as f32;
            draw_cell(draw, draw_r, c as f32, board.cells[r][c], offset_x, offset_y, 1.0);
        }
    }

    if board.state == GameState::Playing || board.state == GameState::Paused {
        if let Some(ghost) = board.get_ghost_piece() {
            for pos in ghost.get_positions().iter() {
                if pos.0 >= 0 && (board.cells[pos.0 as usize][pos.1 as usize]).is_some() {
                    continue;
                }
                let p_type = if pos.0 == ghost.row && pos.1 == ghost.col {
                    ghost.axis_type
                } else {
                    ghost.sat_type
                };
                let draw_r = pos.0 as f32 - config::VISIBLE_ROW_OFFSET as f32 + piece_offset.0;
                let draw_c = pos.1 as f32 + piece_offset.1;
                draw_cell(draw, draw_r, draw_c, Some(p_type), offset_x, offset_y, 0.3);
            }
        }
        if let Some(ref piece) = board.active_piece {
            for pos in piece.get_positions().iter() {
                let p_type = if pos.0 == piece.row && pos.1 == piece.col {
                    piece.axis_type
                } else {
                    piece.sat_type
                };
                let draw_r = pos.0 as f32 - config::VISIBLE_ROW_OFFSET as f32 + piece_offset.0;
                let draw_c = pos.1 as f32 + piece_offset.1;
                draw_cell(draw, draw_r, draw_c, Some(p_type), offset_x, offset_y, 1.0);
            }
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

fn draw_cell(draw: &mut Draw, row: f32, col: f32, puyo_type: Option<PuyoType>, dx: f32, dy: f32, alpha: f32) {
    if let Some(pt) = puyo_type {
        if row >= 0.0 {
            let mut color = get_puyo_color(pt);
            color.a = alpha;

            let x = dx + col * config::CELL_SIZE + 1.0;
            let y = dy + row * config::CELL_SIZE + 1.0;
            let size = config::CELL_SIZE - 2.0;

            draw.rect((x, y), (size, size)).color(color);

            if pt == PuyoType::Garbage {
                draw.rect((x + size * 0.25, y + size * 0.25), (size * 0.5, size * 0.5))
                    .color(Color::BLACK);
            }
        }
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
