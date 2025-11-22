use macroquad::prelude::*;

mod logic;
use logic::*;

fn get_puyo_color(puyo_type: PuyoType) -> Color {
    match puyo_type {
        PuyoType::Red => RED,
        PuyoType::Blue => BLUE,
        PuyoType::Yellow => GOLD,
        PuyoType::Green => GREEN,
        PuyoType::Purple => PURPLE,
    }
}

fn draw_board(board: &Board, offset_x: f32, offset_y: f32, board_w: f32, board_h: f32) {
    draw_rectangle(offset_x, offset_y, board_w, board_h, Color::new(0.12, 0.12, 0.12, 1.0));

    let x_cross = offset_x + (2.0 * CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw_line(x_cross, y_cross, x_cross + 20.0, y_cross + 20.0, 3.0, RED);
    draw_line(x_cross + 20.0, y_cross, x_cross, y_cross + 20.0, 3.0, RED);

    for r in VISIBLE_ROW_OFFSET..board.height {
        for c in 0..board.width {
            let draw_r = (r - VISIBLE_ROW_OFFSET) as f32;
            draw_cell(draw_r, c as f32, board.cells[r][c], offset_x, offset_y, 1.0);
        }
    }

    if board.state == GameState::Playing {
        if let Some(ghost) = board.get_ghost_piece() {
            let positions = ghost.get_positions();
            for pos in positions.iter() {
                let p_type = if pos.0 == ghost.row && pos.1 == ghost.col { ghost.axis_type } else { ghost.sat_type };
                draw_cell(pos.0 as f32 - VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 0.3);
            }
        }
    }

    if let Some(ref piece) = board.active_piece {
        let positions = piece.get_positions();
        for pos in positions.iter() {
            let p_type = if pos.0 == piece.row && pos.1 == piece.col { piece.axis_type } else { piece.sat_type };
            draw_cell(pos.0 as f32 - VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 1.0);
        }
    }

    let visible_height = (board.height - VISIBLE_ROW_OFFSET) as f32;
    for i in 0..=board.width {
        let x = offset_x + (i as f32 * CELL_SIZE);
        draw_line(x, offset_y, x, offset_y + board_h, 1.0, DARKGRAY);
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as f32 * CELL_SIZE);
        draw_line(offset_x, y, offset_x + board_w, y, 1.0, DARKGRAY);
    }
}

fn draw_cell(row: f32, col: f32, puyo_type: Option<PuyoType>, dx: f32, dy: f32, alpha: f32) {
    if let Some(pt) = puyo_type {
        if row >= 0.0 {
            let mut color = get_puyo_color(pt);
            color.a = alpha;
            draw_rectangle(
                dx + col * CELL_SIZE + 1.0,
                dy + row * CELL_SIZE + 1.0,
                CELL_SIZE - 2.0,
                CELL_SIZE - 2.0,
                color
            );
        }
    }
}

#[macroquad::main("Puyo Rust WASM")]
async fn main() {
    rand::srand(macroquad::miniquad::date::now() as u64);

    let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT);
    board.spawn_piece();

    let base_fall_interval = 0.8;
    let resolve_interval = 0.15;

    let mut last_fall_time = get_time();
    let mut last_resolve_time = get_time();
    let start_time = get_time();

    let mut key_timer_left: f32 = 0.0;
    let mut key_timer_right: f32 = 0.0;
    let mut key_timer_down: f32 = 0.0;

    loop {
        let delta_time = get_frame_time();
        let time_now = get_time();
        
        let seconds_played = time_now - start_time;
        let level = 1 + (seconds_played / 15.0) as u32;

        let speed_decrease = (level as f64 - 1.0) * 0.05;
        let current_interval = if speed_decrease >= (base_fall_interval - 0.1) {
            0.1
        } else {
            base_fall_interval - speed_decrease
        };

        if is_key_pressed(KeyCode::Escape) {
            board.toggle_pause();
        }

        if is_key_pressed(KeyCode::R) && (board.state == GameState::GameOver || board.state == GameState::Paused) {
            board = Board::new(GRID_WIDTH, GRID_HEIGHT);
            board.spawn_piece();
            last_fall_time = get_time();
        }

        if board.state == GameState::Playing {
            if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::Z) { board.rotate_piece(1); }
            if is_key_pressed(KeyCode::X) || is_key_pressed(KeyCode::W) { board.rotate_piece(3); }
            
            if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) {
                board.hard_drop();
                last_fall_time = get_time();
            }

            if is_key_down(KeyCode::Left) {
                if key_timer_left == 0.0 {
                    board.move_piece(-1);
                    key_timer_left = 0.0001;
                } else {
                    key_timer_left += delta_time;
                    if key_timer_left > DAS_DELAY {
                        while key_timer_left > DAS_DELAY + DAS_SPEED {
                            board.move_piece(-1);
                            key_timer_left -= DAS_SPEED;
                        }
                    }
                }
            } else {
                key_timer_left = 0.0;
            }

            if is_key_down(KeyCode::Right) {
                if key_timer_right == 0.0 {
                    board.move_piece(1);
                    key_timer_right = 0.0001;
                } else {
                    key_timer_right += delta_time;
                    if key_timer_right > DAS_DELAY {
                        while key_timer_right > DAS_DELAY + DAS_SPEED {
                            board.move_piece(1);
                            key_timer_right -= DAS_SPEED;
                        }
                    }
                }
            } else {
                key_timer_right = 0.0;
            }

            if is_key_down(KeyCode::Down) {
                key_timer_down += delta_time;
                if key_timer_down > SOFT_DROP_SPEED {
                    board.force_drop();
                    last_fall_time = get_time();
                    key_timer_down = 0.0;
                }
            } else {
                key_timer_down = 0.0;
            }
        }

        match board.state {
            GameState::Playing => {
                let locked = board.update_logic(delta_time);
                if locked {
                    last_fall_time = get_time();
                } else {
                    if !board.is_touching_ground {
                        if time_now - last_fall_time > current_interval {
                            board.force_drop();
                            last_fall_time = get_time();
                        }
                    }
                }
            },
            GameState::ResolvingMatches => {
                if time_now - last_resolve_time > resolve_interval {
                    board.resolve_step();
                    last_resolve_time = get_time();
                }
            },
            _ => {}
        }

        clear_background(Color::new(0.05, 0.05, 0.05, 1.0));

        let board_w = GRID_WIDTH as f32 * CELL_SIZE;
        let board_h = (GRID_HEIGHT - VISIBLE_ROW_OFFSET) as f32 * CELL_SIZE;
        let offset_x = (screen_width() - board_w) / 2.0;
        let offset_y = (screen_height() - board_h) / 2.0;
        let ui_x = offset_x + board_w + 20.0;

        draw_board(&board, offset_x, offset_y, board_w, board_h);

        draw_text(&format!("Score: {}", board.score), ui_x, offset_y + 20.0, 30.0, WHITE);
        draw_text(&format!("Level: {}", level), ui_x, offset_y + 50.0, 30.0, YELLOW);
        draw_text("Next:", ui_x, offset_y + 100.0, 30.0, GRAY);

        draw_rectangle(ui_x, offset_y + 110.0, CELL_SIZE, CELL_SIZE * 2.1, Color::new(0.2, 0.2, 0.2, 1.0));
        draw_cell(0.0, 0.0, Some(board.next_types.1), ui_x, offset_y + 110.0, 1.0);
        draw_cell(1.0, 0.0, Some(board.next_types.0), ui_x, offset_y + 110.0, 1.0);

        let nn_y = offset_y + 220.0;
        let nn_size = CELL_SIZE * 0.5;
        draw_rectangle(ui_x + 5.0, nn_y, nn_size, nn_size, get_puyo_color(board.next_next_types.1));
        draw_rectangle(ui_x + 5.0, nn_y + nn_size, nn_size, nn_size, get_puyo_color(board.next_next_types.0));

        if board.chain_count > 1 {
            draw_text(&format!("Chain: {}", board.chain_count), ui_x, nn_y + 80.0, 30.0, GREEN);
        }

        if board.is_touching_ground && board.state == GameState::Playing {
            let ratio_std = 1.0 - (board.lock_timer / MAX_LOCK_TIME);
            let ratio_hard = 1.0 - (board.total_ground_timer / MAX_TOTAL_GROUND_TIME);
            let ratio = ratio_std.min(ratio_hard);
            
            let bar_width = 100.0 * ratio;
            let col = if board.total_ground_timer > 1.5 { RED } else { ORANGE };
            draw_rectangle(ui_x, nn_y + 100.0, bar_width, 10.0, col);
        }

        if board.state == GameState::GameOver {
            draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.7));
            let text = "GAME OVER";
            let dims = measure_text(text, None, 60, 1.0);
            draw_text(text, screen_width()/2.0 - dims.width/2.0, screen_height()/2.0, 60.0, RED);
            
            let sub = "Press R to Restart";
            let sub_dims = measure_text(sub, None, 30, 1.0);
            draw_text(sub, screen_width()/2.0 - sub_dims.width/2.0, screen_height()/2.0 + 50.0, 30.0, WHITE);
        }

        if board.state == GameState::Paused {
            draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.5));
            let text = "PAUSED";
            let dims = measure_text(text, None, 60, 1.0);
            draw_text(text, screen_width()/2.0 - dims.width/2.0, screen_height()/2.0, 60.0, WHITE);
            let sub = "Press ESC to Resume";
            let sub_dims = measure_text(sub, None, 30, 1.0);
            draw_text(sub, screen_width()/2.0 - sub_dims.width/2.0, screen_height()/2.0 + 50.0, 30.0, LIGHTGRAY);
        }

        next_frame().await
    }
}
