use notan::prelude::*;
use notan::draw::*;
use shared::*;
use crate::state::State;
use crate::config;

pub fn draw(app: &mut App, gfx: &mut Graphics, state: &mut State) {
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

    let me = &state.predicted_board;
    draw_board(&mut draw, me, start_x, offset_y, board_w, board_h);
    draw.text(&state.font, "YOU").position(start_x, offset_y - 30.0).size(20.0).color(Color::WHITE);

    let opponent_x = start_x + board_w + gap;
    draw_board(&mut draw, &state.other_board, opponent_x, offset_y, board_w, board_h);
    draw.text(&state.font, "OPPONENT").position(opponent_x, offset_y - 30.0).size(20.0).color(Color::GRAY);

    draw.text(&state.font, &format!("Score: {}", me.score)).position(ui_x, offset_y + 20.0).size(30.0).color(Color::WHITE);
    draw.text(&state.font, &format!("Level: {}", me.level())).position(ui_x, offset_y + 60.0).size(30.0).color(Color::YELLOW);

    draw.text(&state.font, "Next:").position(ui_x, offset_y + 110.0).size(30.0).color(Color::GRAY);
    draw.rect((ui_x, offset_y + 140.0), (config::CELL_SIZE, config::CELL_SIZE * 2.1)).color(Color::from_rgb(0.2, 0.2, 0.2));
    draw_cell(&mut draw, 0.0, 0.0, Some(me.next_types.1), ui_x, offset_y + 140.0, 1.0);
    draw_cell(&mut draw, 1.0, 0.0, Some(me.next_types.0), ui_x, offset_y + 140.0, 1.0);

    let next_next_y = offset_y + 170.0 + (config::CELL_SIZE * 2.5);
    draw.text(&state.font, "Next Next:").position(ui_x, next_next_y - 25.0).size(20.0).color(Color::GRAY);
    draw.rect((ui_x, next_next_y), (config::CELL_SIZE, config::CELL_SIZE * 2.1)).color(Color::from_rgb(0.15, 0.15, 0.15));
    draw_cell(&mut draw, 0.0, 0.0, Some(me.next_next_types.1), ui_x, next_next_y, 1.0);
    draw_cell(&mut draw, 1.0, 0.0, Some(me.next_next_types.0), ui_x, next_next_y, 1.0);

    if me.chain_count > 0 {
        draw.text(&state.font, &format!("Chain: {}", me.chain_count)).position(ui_x, offset_y + 380.0).size(30.0).color(Color::GREEN);
    }

    if me.is_touching_ground && me.state == GameState::Playing {
        let ratio_std = 1.0 - (me.lock_timer / config::MAX_LOCK_TIME);
        let ratio_hard = 1.0 - (me.total_ground_timer / config::MAX_TOTAL_GROUND_TIME);
        let ratio = ratio_std.min(ratio_hard).max(0.0);
        let col = if me.total_ground_timer > 1.5 { Color::RED } else { Color::ORANGE };
        draw.rect((ui_x, offset_y + 350.0), (100.0 * ratio, 10.0)).color(col);
    }

    if state.room_full {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.9));
        draw.text(&state.font, "SALLE PLEINE").position(win_w / 2.0, win_h / 2.0 - 20.0).size(60.0).h_align_center().v_align_middle().color(Color::RED);
        draw.text(&state.font, "Une partie est deja en cours.").position(win_w / 2.0, win_h / 2.0 + 40.0).size(25.0).h_align_center().v_align_middle().color(Color::WHITE);
    } else if state.waiting_for_opponent {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.8));
        draw.text(&state.font, "WAITING FOR PLAYER 2...").position(win_w / 2.0, win_h / 2.0).size(40.0).h_align_center().v_align_middle().color(Color::WHITE);
    }

    if state.opponent_disconnected {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.5, 0.0, 0.0, 0.5));
        draw.text(&state.font, "OPPONENT DISCONNECTED").position(win_w / 2.0, win_h / 2.0 - 20.0).size(40.0).h_align_center().v_align_middle().color(Color::RED);
        draw.text(&state.font, "Waiting for reconnection...").position(win_w / 2.0, win_h / 2.0 + 30.0).size(20.0).h_align_center().v_align_middle().color(Color::WHITE);
    }

    let i_lost = state.board.state == GameState::GameOver;
    let opponent_lost = state.other_board.state == GameState::GameOver;
    if i_lost || opponent_lost {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.7));
        if i_lost {
            draw.text(&state.font, "GAME OVER").position(win_w / 2.0, win_h / 2.0 - 20.0).size(60.0).h_align_center().v_align_middle().color(Color::RED);
        } else {
            draw.text(&state.font, "YOU WIN !").position(win_w / 2.0, win_h / 2.0 - 20.0).size(80.0).h_align_center().v_align_middle().color(Color::YELLOW);
        }
        draw.text(&state.font, "Press R to Restart").position(win_w / 2.0, win_h / 2.0 + 60.0).size(30.0).h_align_center().v_align_middle().color(Color::WHITE);
    }

    if state.board.state == GameState::Paused && !state.opponent_disconnected {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.5));
        let alpha = (app.timer.elapsed_f32() * 2.0).sin().abs();
        let visible_alpha = 0.2 + (alpha * 0.8);
        draw.text(&state.font, "PAUSED").position(win_w / 2.0, win_h / 2.0 - 40.0).size(60.0).h_align_center().v_align_middle().color(Color::from_rgba(1.0, 1.0, 1.0, visible_alpha));
        draw.text(&state.font, "Press ESC to continue").position(win_w / 2.0, win_h / 2.0 + 40.0).size(30.0).h_align_center().v_align_middle().color(Color::from_rgba(1.0, 1.0, 1.0, visible_alpha));
    }

    #[cfg(debug_assertions)]
    {
        draw.text(&state.font, &format!("DEBUG NET: {}", state.last_server_msg))
            .position(10.0, win_h - 30.0)
            .size(20.0)
            .color(Color::MAGENTA);
    }

    gfx.render(&draw);
}

fn draw_board(draw: &mut Draw, board: &Board, offset_x: f32, offset_y: f32, board_w: f32, board_h: f32) {
    draw.rect((offset_x, offset_y), (board_w, board_h)).color(Color::from_rgb(0.12, 0.12, 0.12));

    let x_cross = offset_x + (2.0 * config::CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw.line((x_cross, y_cross), (x_cross + 20.0, y_cross + 20.0)).width(3.0).color(Color::RED);
    draw.line((x_cross + 20.0, y_cross), (x_cross, y_cross + 20.0)).width(3.0).color(Color::RED);

    for r in config::VISIBLE_ROW_OFFSET..board.height {
        for c in 0..board.width {
            let draw_r = (r - config::VISIBLE_ROW_OFFSET) as f32;
            draw_cell(draw, draw_r, c as f32, board.cells[r][c], offset_x, offset_y, 1.0);
        }
    }

    if board.state == GameState::Playing || board.state == GameState::Paused {
        if let Some(ghost) = board.get_ghost_piece() {
            for pos in ghost.get_positions().iter() {
                let p_type = if pos.0 == ghost.row && pos.1 == ghost.col { ghost.axis_type } else { ghost.sat_type };
                draw_cell(draw, pos.0 as f32 - config::VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 0.3);
            }
        }
        if let Some(ref piece) = board.active_piece {
            for pos in piece.get_positions().iter() {
                let p_type = if pos.0 == piece.row && pos.1 == piece.col { piece.axis_type } else { piece.sat_type };
                draw_cell(draw, pos.0 as f32 - config::VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 1.0);
            }
        }
    }

    let visible_height = (board.height - config::VISIBLE_ROW_OFFSET) as f32;
    for i in 0..=board.width {
        let x = offset_x + (i as f32 * config::CELL_SIZE);
        draw.line((x, offset_y), (x, offset_y + board_h)).width(1.0).color(Color::GRAY);
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as f32 * config::CELL_SIZE);
        draw.line((offset_x, y), (offset_x + board_w, y)).width(1.0).color(Color::GRAY);
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
