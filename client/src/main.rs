use notan::prelude::*;
use notan::draw::*;
use shared::*;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};

struct State {
    board: Board,
    other_board: Board,
    my_player_id: Option<u8>,
    initial_seed: u64,
    ws_sender: WsSender,
    ws_receiver: WsReceiver,
    
    waiting_for_opponent: bool,
    opponent_disconnected: bool,

    game_over_sent: bool,
    did_i_win: bool,

    last_fall_time: f32,
    last_resolve_time: f32,
    played_time: f32, 
    
    key_timer_left: f32,
    key_timer_right: f32,
    key_timer_down: f32,

    last_server_msg: String,
    font: Font, 
}
impl AppState for State {}

fn setup(app: &mut App, gfx: &mut Graphics) -> State {
    let (ws_sender, ws_receiver) = ewebsock::connect("ws://ADRESSE_IP:8080/ws").unwrap();
    let font = gfx.create_font(include_bytes!("arcadeFont.ttf")).unwrap();

    let mut board = Board::new(GRID_WIDTH, GRID_HEIGHT, 12345);
    board.spawn_piece();
    let other_board = Board::new(GRID_WIDTH, GRID_HEIGHT, 12345);
    let now = app.timer.elapsed_f32();

    State {
        board, other_board, my_player_id: None, initial_seed: 12345,
        ws_sender, ws_receiver,
        waiting_for_opponent: true,
        opponent_disconnected: false,
        game_over_sent: false, did_i_win: false,
        last_fall_time: now, last_resolve_time: now, played_time: 0.0,
        key_timer_left: 0.0, key_timer_right: 0.0, key_timer_down: 0.0,
        last_server_msg: String::from("Connexion..."), font,
    }
}

fn update_opponent_board(board: &mut Board, col: i32, rot: usize, c1: u8, c2: u8) {
    let piece = ActivePuyo { row: 1, col, rotation: rot, axis_type: PuyoType::from_u8(c1), sat_type: PuyoType::from_u8(c2) };
    let mut ghost = piece.clone();
    if board.check_collision(&ghost) { return; }
    while !board.check_collision(&ghost) { ghost.row += 1; }
    ghost.row -= 1; 
    for (r, c) in ghost.get_positions().iter() {
        if *r >= 0 && *r < board.height as i32 && *c >= 0 && *c < board.width as i32 {
            let p_type = if *r == ghost.row && *c == ghost.col { ghost.axis_type } else { ghost.sat_type };
            board.cells[*r as usize][*c as usize] = Some(p_type);
        }
    }
    board.apply_board_gravity();
    loop {
        if board.check_matches() { board.apply_board_gravity(); } else { break; }
    }
}

fn draw(app: &mut App, gfx: &mut Graphics, state: &mut State) {
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.05)); 

    while let Some(event) = state.ws_receiver.try_recv() {
        match event {
            WsEvent::Message(msg) => match msg {
                WsMessage::Text(text) => {
                    if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                       match server_msg {
                           ServerMessage::Welcome { random_seed, player_id } => {
                                state.my_player_id = Some(player_id);
                                state.initial_seed = random_seed;
                                state.board = Board::new(GRID_WIDTH, GRID_HEIGHT, random_seed);
                                state.board.spawn_piece();
                                state.other_board = Board::new(GRID_WIDTH, GRID_HEIGHT, random_seed);
                                state.played_time = 0.0;
                                state.game_over_sent = false;
                                state.did_i_win = false;
                                state.opponent_disconnected = false;
                           }
                           ServerMessage::GameStart => {
                               state.waiting_for_opponent = false;
                               state.opponent_disconnected = false;
                               state.played_time = 0.0;
                           }
                           ServerMessage::OpponentAction { player_id, col, rot, axis_color_idx, sat_color_idx } => {
                                if Some(player_id) != state.my_player_id {
                                    update_opponent_board(&mut state.other_board, col, rot, axis_color_idx, sat_color_idx);
                                }
                           }
                           ServerMessage::PlayerEliminated { player_id } => {
                               if Some(player_id) != state.my_player_id {
                                   state.did_i_win = true;
                                   state.board.state = GameState::GameOver; 
                               }
                           }
                           ServerMessage::Restart { new_seed } => {
                                state.initial_seed = new_seed;
                                state.board = Board::new(GRID_WIDTH, GRID_HEIGHT, new_seed);
                                state.board.spawn_piece();
                                state.other_board = Board::new(GRID_WIDTH, GRID_HEIGHT, new_seed);
                                state.played_time = 0.0;
                                state.last_fall_time = app.timer.elapsed_f32();
                                state.game_over_sent = false;
                                state.did_i_win = false;
                                if state.board.state == GameState::Paused { state.board.state = GameState::Playing; }
                           }
                           ServerMessage::GameStateChange { paused: _ } => {
                                state.board.toggle_pause();
                           }
                           ServerMessage::OpponentDisconnected => {
                               state.opponent_disconnected = true;
                               if state.board.state == GameState::Playing {
                                   state.board.toggle_pause();
                               }
                           }
                           
                           ServerMessage::RequestSnapshot { requester_id } => {
                               println!("Envoi snapshot...");
                               let msg = ClientMessage::FullGameState {
                                   my_board: state.board.clone(),
                                   opponent_board: state.other_board.clone(),
                                   scores: (state.board.score, state.other_board.score),
                                   requester_id
                               };
                               if let Ok(json) = serde_json::to_string(&msg) {
                                   state.ws_sender.send(WsMessage::Text(json));
                               }
                               state.opponent_disconnected = false;
                           }
                           
                           ServerMessage::SyncState { my_board, opponent_board, scores, target_player_id } => {
                               if Some(target_player_id) == state.my_player_id {
                                   println!("📦 REÇU SNAPSHOT !");
                                   state.board = my_board;
                                   state.other_board = opponent_board;
                                   state.board.score = scores.0;
                                   state.other_board.score = scores.1;

                                   if state.board.active_piece.is_none() && state.board.state == GameState::Playing {
                                       state.board.spawn_piece();
                                   }
                                   let now = app.timer.elapsed_f32();
                                   state.last_fall_time = now;
                                   state.last_resolve_time = now;
                                   state.waiting_for_opponent = false; 
                                   state.opponent_disconnected = false;
                                   if state.board.state == GameState::Paused {
                                   } else {
                                        state.board.state = GameState::Playing;
                                   }
                               } else {
                                   println!("Adversaire synchro.");
                                   state.opponent_disconnected = false;
                                   if state.board.state == GameState::Paused {
                                       state.board.toggle_pause(); 
                                   }
                               }
                           }
                       }
                    }
                }
                _ => {}
            },
            WsEvent::Opened => {
                let join_msg = ClientMessage::Join { name: "Joueur".to_string() };
                if let Ok(json) = serde_json::to_string(&join_msg) { state.ws_sender.send(WsMessage::Text(json)); }
            },
            _ => {}
        }
    }

    let time_now = app.timer.elapsed_f32();
    let delta_time = app.timer.delta_f32();

    let can_play = !state.waiting_for_opponent && !state.opponent_disconnected;

    if can_play {
        if state.board.state == GameState::Playing || state.board.state == GameState::ResolvingMatches {
            state.played_time += delta_time;
        }

        let level = 1 + (state.played_time / 15.0) as u32;
        let speed_decrease = (level as f64 - 1.0) * 0.05;
        let current_interval = if speed_decrease >= (0.8 - 0.1) { 0.1 } else { 0.8 - speed_decrease };

        if app.keyboard.was_pressed(KeyCode::R) && (state.board.state == GameState::GameOver || state.board.state == GameState::Paused) {
            let msg = ClientMessage::RequestRestart;
            if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); }
        }
        
        if app.keyboard.was_pressed(KeyCode::Escape) { 
            let msg = ClientMessage::TogglePause;
            if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); }
        }

        if state.board.state == GameState::GameOver && !state.game_over_sent && !state.did_i_win {
            let msg = ClientMessage::GameOver;
            if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); }
            state.game_over_sent = true;
        }

        if state.board.state == GameState::Playing {
            if app.keyboard.was_pressed(KeyCode::Up) || app.keyboard.was_pressed(KeyCode::Z) { state.board.rotate_piece(1); }
            if app.keyboard.was_pressed(KeyCode::X) || app.keyboard.was_pressed(KeyCode::W) { state.board.rotate_piece(3); }

            let piece_locked_now = if app.keyboard.was_pressed(KeyCode::Space) || app.keyboard.was_pressed(KeyCode::Return) {
                if let Some(piece) = &state.board.active_piece {
                    let action_msg = ClientMessage::PieceLocked { 
                        col: piece.col, rot: piece.rotation, 
                        axis_color_idx: piece.axis_type.to_u8(), sat_color_idx: piece.sat_type.to_u8() 
                    };
                    if let Ok(json) = serde_json::to_string(&action_msg) { state.ws_sender.send(WsMessage::Text(json)); }
                }
                state.board.hard_drop();
                state.last_fall_time = time_now;
                true
            } else { false };

            if !piece_locked_now {
                if app.keyboard.is_down(KeyCode::Left) {
                    if state.key_timer_left == 0.0 { state.board.move_piece(-1); state.key_timer_left = 0.0001; }
                    else {
                        state.key_timer_left += delta_time;
                        if state.key_timer_left > DAS_DELAY { while state.key_timer_left > DAS_DELAY + DAS_SPEED { state.board.move_piece(-1); state.key_timer_left -= DAS_SPEED; } }
                    }
                } else { state.key_timer_left = 0.0; }

                if app.keyboard.is_down(KeyCode::Right) {
                    if state.key_timer_right == 0.0 { state.board.move_piece(1); state.key_timer_right = 0.0001; }
                    else {
                        state.key_timer_right += delta_time;
                        if state.key_timer_right > DAS_DELAY { while state.key_timer_right > DAS_DELAY + DAS_SPEED { state.board.move_piece(1); state.key_timer_right -= DAS_SPEED; } }
                    }
                } else { state.key_timer_right = 0.0; }

                if app.keyboard.is_down(KeyCode::Down) {
                    state.key_timer_down += delta_time;
                    if state.key_timer_down > SOFT_DROP_SPEED {
                        state.board.force_drop();
                        state.last_fall_time = time_now;
                        state.key_timer_down = 0.0;
                    }
                } else { state.key_timer_down = 0.0; }
            }
        }

        match state.board.state {
            GameState::Playing => {
                let pending_lock_msg = if let Some(piece) = &state.board.active_piece {
                    Some(ClientMessage::PieceLocked { 
                        col: piece.col, rot: piece.rotation, axis_color_idx: piece.axis_type.to_u8(), sat_color_idx: piece.sat_type.to_u8() 
                    })
                } else { None };

                let locked = state.board.update_logic(delta_time);
                if locked {
                    if let Some(msg) = pending_lock_msg { if let Ok(json) = serde_json::to_string(&msg) { state.ws_sender.send(WsMessage::Text(json)); } }
                    state.last_fall_time = time_now;
                } else {
                    if !state.board.is_touching_ground && (time_now - state.last_fall_time > current_interval as f32) {
                        state.board.force_drop();
                        state.last_fall_time = time_now;
                    }
                }
            },
            GameState::ResolvingMatches => {
                if time_now - state.last_resolve_time > 0.15 { state.board.resolve_step(); state.last_resolve_time = time_now; }
            },
            _ => {}
        }
    }

    let board_w = GRID_WIDTH as f32 * CELL_SIZE;
    let board_h = (GRID_HEIGHT - VISIBLE_ROW_OFFSET) as f32 * CELL_SIZE;
    let gap = 250.0;
    let total_w = board_w * 2.0 + gap; 
    let start_x = (app.window().width() as f32 - total_w) / 2.0;
    let offset_y = (app.window().height() as f32 - board_h) / 2.0;
    let ui_x = start_x + board_w + 30.0; 

    draw_board(&mut draw, &state.board, start_x, offset_y, board_w, board_h);
    draw.text(&state.font, "YOU").position(start_x, offset_y - 30.0).size(20.0).color(Color::WHITE);

    let opponent_x = start_x + board_w + gap;
    draw_board(&mut draw, &state.other_board, opponent_x, offset_y, board_w, board_h);
    draw.text(&state.font, "OPPONENT").position(opponent_x, offset_y - 30.0).size(20.0).color(Color::GRAY);

    draw.text(&state.font, &format!("Score: {}", state.board.score)).position(ui_x, offset_y + 20.0).size(30.0).color(Color::WHITE);
    draw.text(&state.font, &format!("Level: {}", 1 + (state.played_time / 15.0) as u32)).position(ui_x, offset_y + 60.0).size(30.0).color(Color::YELLOW);

    draw.text(&state.font, "Next:").position(ui_x, offset_y + 110.0).size(30.0).color(Color::GRAY);
    draw.rect((ui_x, offset_y + 140.0), (CELL_SIZE, CELL_SIZE * 2.1)).color(Color::from_rgb(0.2, 0.2, 0.2));
    draw_cell(&mut draw, 0.0, 0.0, Some(state.board.next_types.1), ui_x, offset_y + 140.0, 1.0);
    draw_cell(&mut draw, 1.0, 0.0, Some(state.board.next_types.0), ui_x, offset_y + 140.0, 1.0);

    let next_next_y = offset_y + 170.0 + (CELL_SIZE * 2.5);
    draw.text(&state.font, "Next Next:").position(ui_x, next_next_y - 25.0).size(20.0).color(Color::GRAY);
    draw.rect((ui_x, next_next_y), (CELL_SIZE, CELL_SIZE * 2.1)).color(Color::from_rgb(0.15, 0.15, 0.15));
    draw_cell(&mut draw, 0.0, 0.0, Some(state.board.next_next_types.1), ui_x, next_next_y, 1.0);
    draw_cell(&mut draw, 1.0, 0.0, Some(state.board.next_next_types.0), ui_x, next_next_y, 1.0);

    if state.board.chain_count > 0 {
        draw.text(&state.font, &format!("Chain: {}", state.board.chain_count)).position(ui_x, offset_y + 380.0).size(30.0).color(Color::GREEN);
    }

    if state.board.is_touching_ground && state.board.state == GameState::Playing {
        let ratio_std = 1.0 - (state.board.lock_timer / MAX_LOCK_TIME);
        let ratio_hard = 1.0 - (state.board.total_ground_timer / MAX_TOTAL_GROUND_TIME);
        let ratio = ratio_std.min(ratio_hard).max(0.0);
        let col = if state.board.total_ground_timer > 1.5 { Color::RED } else { Color::ORANGE };
        draw.rect((ui_x, offset_y + 350.0), (100.0 * ratio, 10.0)).color(col);
    }

    let win_w = app.window().width() as f32;
    let win_h = app.window().height() as f32;

    if state.waiting_for_opponent {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.8));
        draw.text(&state.font, "WAITING FOR PLAYER 2...").position(win_w / 2.0, win_h / 2.0).size(40.0).h_align_center().v_align_middle().color(Color::WHITE);
    }

    if state.opponent_disconnected {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.5, 0.0, 0.0, 0.5));
        draw.text(&state.font, "OPPONENT DISCONNECTED").position(win_w / 2.0, win_h / 2.0 - 20.0).size(40.0).h_align_center().v_align_middle().color(Color::RED);
        draw.text(&state.font, "Waiting for reconnection...").position(win_w / 2.0, win_h / 2.0 + 30.0).size(20.0).h_align_center().v_align_middle().color(Color::WHITE);
    }

    if state.board.state == GameState::GameOver {
        draw.rect((0.0, 0.0), (win_w, win_h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.7));
        if state.did_i_win {
            draw.text(&state.font, "YOU WIN !").position(win_w / 2.0, win_h / 2.0 - 20.0).size(80.0).h_align_center().v_align_middle().color(Color::YELLOW);
        } else {
            draw.text(&state.font, "GAME OVER").position(win_w / 2.0, win_h / 2.0 - 20.0).size(60.0).h_align_center().v_align_middle().color(Color::RED);
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

    gfx.render(&draw);
}

fn get_puyo_color(puyo_type: PuyoType) -> Color {
    match puyo_type {
        PuyoType::Red => Color::RED, PuyoType::Blue => Color::BLUE,
        PuyoType::Yellow => Color::YELLOW, PuyoType::Green => Color::GREEN,
        PuyoType::Purple => Color::MAGENTA,
    }
}

fn draw_board(draw: &mut Draw, board: &Board, offset_x: f32, offset_y: f32, board_w: f32, board_h: f32) {
    draw.rect((offset_x, offset_y), (board_w, board_h)).color(Color::from_rgb(0.12, 0.12, 0.12));
    let x_cross = offset_x + (2.0 * CELL_SIZE) + 10.0;
    let y_cross = offset_y + 10.0;
    draw.line((x_cross, y_cross), (x_cross + 20.0, y_cross + 20.0)).width(3.0).color(Color::RED);
    draw.line((x_cross + 20.0, y_cross), (x_cross, y_cross + 20.0)).width(3.0).color(Color::RED);

    for r in VISIBLE_ROW_OFFSET..board.height {
        for c in 0..board.width {
            let draw_r = (r - VISIBLE_ROW_OFFSET) as f32;
            draw_cell(draw, draw_r, c as f32, board.cells[r][c], offset_x, offset_y, 1.0);
        }
    }

    if board.state == GameState::Playing || board.state == GameState::Paused {
        if let Some(ghost) = board.get_ghost_piece() {
            for pos in ghost.get_positions().iter() {
                let p_type = if pos.0 == ghost.row && pos.1 == ghost.col { ghost.axis_type } else { ghost.sat_type };
                draw_cell(draw, pos.0 as f32 - VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 0.3);
            }
        }
        if let Some(ref piece) = board.active_piece {
            for pos in piece.get_positions().iter() {
                let p_type = if pos.0 == piece.row && pos.1 == piece.col { piece.axis_type } else { piece.sat_type };
                draw_cell(draw, pos.0 as f32 - VISIBLE_ROW_OFFSET as f32, pos.1 as f32, Some(p_type), offset_x, offset_y, 1.0);
            }
        }
    }

    let visible_height = (board.height - VISIBLE_ROW_OFFSET) as f32;
    for i in 0..=board.width {
        let x = offset_x + (i as f32 * CELL_SIZE);
        draw.line((x, offset_y), (x, offset_y + board_h)).width(1.0).color(Color::GRAY);
    }
    for i in 0..=visible_height as usize {
        let y = offset_y + (i as f32 * CELL_SIZE);
        draw.line((offset_x, y), (offset_x + board_w, y)).width(1.0).color(Color::GRAY);
    }
}

fn draw_cell(draw: &mut Draw, row: f32, col: f32, puyo_type: Option<PuyoType>, dx: f32, dy: f32, alpha: f32) {
    if let Some(pt) = puyo_type {
        if row >= 0.0 {
            let mut color = get_puyo_color(pt);
            color.a = alpha;
            draw.rect((dx + col * CELL_SIZE + 1.0, dy + row * CELL_SIZE + 1.0), (CELL_SIZE - 2.0, CELL_SIZE - 2.0)).color(color);
        }
    }
}

#[notan_main]
fn main() -> Result<(), String> {
    notan::init_with(setup).add_config(DrawConfig).draw(draw).build()
}
