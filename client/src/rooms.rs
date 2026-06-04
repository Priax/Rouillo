use notan::prelude::*;
use notan::draw::*;
use shared::{ClientMessage, RoomSettings};
use crate::state::{State, Screen};
use crate::menu::Btn;

fn win_w(app: &mut App) -> f32 { app.window().width() as f32 }
fn win_h(app: &mut App) -> f32 { app.window().height() as f32 }

fn send(state: &mut State, msg: &ClientMessage) {
    if let Some(net) = state.net.as_mut() {
        net.send(msg);
    }
}

pub fn leave_room_button(w: f32, h: f32) -> Btn {
    Btn { x: w / 2.0 - 130.0, y: h / 2.0 + 110.0, w: 260.0, h: 50.0 }
}

pub fn back_to_lobby_button(w: f32, h: f32) -> Btn {
    Btn { x: w / 2.0 - 130.0, y: h / 2.0 + 175.0, w: 260.0, h: 50.0 }
}

fn room_row(i: usize, w: f32) -> Btn {
    Btn { x: w / 2.0 - 250.0, y: 150.0 + i as f32 * 56.0, w: 500.0, h: 48.0 }
}

struct BrowserButtons { create: Btn, join_id: Btn, refresh: Btn, back: Btn }

fn browser_buttons(w: f32, h: f32) -> BrowserButtons {
    let bw = 190.0;
    let gap = 15.0;
    let start = w / 2.0 - (4.0 * bw + 3.0 * gap) / 2.0;
    let y = h - 90.0;
    let at = |i: f32| Btn { x: start + i * (bw + gap), y, w: bw, h: 50.0 };
    BrowserButtons { create: at(0.0), join_id: at(1.0), refresh: at(2.0), back: at(3.0) }
}

pub fn update_browser(app: &mut App, state: &mut State) {
    let (w, h) = (win_w(app), win_h(app));
    let b = browser_buttons(w, h);

    let ids: Vec<u32> = state.rooms.iter().map(|r| r.id).collect();
    for (i, id) in ids.iter().enumerate() {
        if room_row(i, w).clicked(app) {
            state.notice.clear();
            send(state, &ClientMessage::JoinRoom { id: *id });
            return;
        }
    }

    if b.create.clicked(app) {
        state.text_input.clear();
        state.notice.clear();
        state.screen = Screen::CreateRoom;
    } else if b.join_id.clicked(app) {
        state.text_input.clear();
        state.notice.clear();
        state.screen = Screen::JoinById;
    } else if b.refresh.clicked(app) {
        send(state, &ClientMessage::RequestRoomList);
    } else if b.back.clicked(app) {
        state.net = None;
        state.rooms.clear();
        state.screen = Screen::Menu;
    }
}

pub fn draw_browser(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (w, h) = (app.window().width() as f32, app.window().height() as f32);
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, "ROOMS").position(w / 2.0, 70.0).size(50.0)
        .h_align_center().v_align_middle().color(Color::WHITE);

    if state.rooms.is_empty() {
        draw.text(&state.font, "Aucune room. Crees-en une !").position(w / 2.0, 200.0).size(24.0)
            .h_align_center().v_align_middle().color(Color::GRAY);
    }
    for (i, room) in state.rooms.iter().enumerate() {
        let btn = room_row(i, w);
        let label = format!("#{}  {}   {}/{}{}",
            room.id, room.name, room.players, room.max,
            if room.in_game { "  (en jeu)" } else { "" });
        btn.draw(&mut draw, app, &state.font, &label);
    }

    let b = browser_buttons(w, h);
    b.create.draw(&mut draw, app, &state.font, "Create Room");
    b.join_id.draw(&mut draw, app, &state.font, "Join by ID");
    b.refresh.draw(&mut draw, app, &state.font, "Refresh");
    b.back.draw(&mut draw, app, &state.font, "Back");

    if !state.notice.is_empty() {
        draw.text(&state.font, &state.notice).position(w / 2.0, h - 130.0).size(22.0)
            .h_align_center().v_align_middle().color(Color::RED);
    }

    gfx.render(&draw);
}

fn entry_buttons(w: f32, h: f32, confirm: &str) -> (Btn, Btn) {
    let _ = confirm;
    let bw = 200.0;
    let y = h / 2.0 + 60.0;
    (
        Btn { x: w / 2.0 - bw - 10.0, y, w: bw, h: 56.0 },
        Btn { x: w / 2.0 + 10.0, y, w: bw, h: 56.0 },
    )
}

pub fn update_create_room(app: &mut App, state: &mut State) {
    if app.keyboard.was_pressed(KeyCode::Backspace) {
        state.text_input.pop();
    }
    let (w, h) = (win_w(app), win_h(app));
    let (confirm, back) = entry_buttons(w, h, "Create");
    let submit = confirm.clicked(app) || app.keyboard.was_pressed(KeyCode::Enter);
    if submit {
        let name = state.text_input.trim().to_string();
        if !name.is_empty() {
            send(state, &ClientMessage::CreateRoom { name });
            state.text_input.clear();
        }
    } else if back.clicked(app) {
        state.screen = Screen::RoomBrowser;
    }
}

pub fn update_join_by_id(app: &mut App, state: &mut State) {
    if app.keyboard.was_pressed(KeyCode::Backspace) {
        state.text_input.pop();
    }
    let (w, h) = (win_w(app), win_h(app));
    let (confirm, back) = entry_buttons(w, h, "Join");
    let submit = confirm.clicked(app) || app.keyboard.was_pressed(KeyCode::Enter);
    if submit {
        if let Ok(id) = state.text_input.trim().parse::<u32>() {
            state.notice.clear();
            send(state, &ClientMessage::JoinRoom { id });
            state.text_input.clear();
        }
    } else if back.clicked(app) {
        state.screen = Screen::RoomBrowser;
    }
}

fn draw_entry(app: &mut App, gfx: &mut Graphics, state: &State, title: &str, confirm: &str, placeholder: &str) {
    let (w, h) = (app.window().width() as f32, app.window().height() as f32);
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, title).position(w / 2.0, h / 2.0 - 120.0).size(44.0)
        .h_align_center().v_align_middle().color(Color::WHITE);

    let box_w = 420.0;
    let box_x = w / 2.0 - box_w / 2.0;
    let box_y = h / 2.0 - 40.0;
    draw.rect((box_x, box_y), (box_w, 50.0)).color(Color::from_rgb(0.15, 0.16, 0.22));
    draw.rect((box_x, box_y), (box_w, 50.0)).stroke(2.0).color(Color::from_rgb(0.45, 0.47, 0.6));
    let shown = if state.text_input.is_empty() { placeholder } else { &state.text_input };
    let col = if state.text_input.is_empty() { Color::GRAY } else { Color::WHITE };
    draw.text(&state.font, shown).position(box_x + 12.0, box_y + 25.0).size(26.0)
        .v_align_middle().color(col);

    let (cbtn, back) = entry_buttons(w, h, confirm);
    cbtn.draw(&mut draw, app, &state.font, confirm);
    back.draw(&mut draw, app, &state.font, "Back");

    gfx.render(&draw);
}

pub fn draw_create_room(app: &mut App, gfx: &mut Graphics, state: &State) {
    draw_entry(app, gfx, state, "Create Room", "Create", "Room name...");
}

pub fn draw_join_by_id(app: &mut App, gfx: &mut Graphics, state: &State) {
    draw_entry(app, gfx, state, "Join by ID", "Join", "Room ID...");
}

const LOBBY_FIRST_Y: f32 = 230.0;
const LOBBY_ROW_H: f32 = 64.0;

fn lobby_minus(i: usize, w: f32) -> Btn {
    Btn { x: w / 2.0 + 60.0, y: LOBBY_FIRST_Y + i as f32 * LOBBY_ROW_H, w: 50.0, h: 50.0 }
}
fn lobby_plus(i: usize, w: f32) -> Btn {
    Btn { x: w / 2.0 + 200.0, y: LOBBY_FIRST_Y + i as f32 * LOBBY_ROW_H, w: 50.0, h: 50.0 }
}
fn lobby_launch(w: f32) -> Btn {
    let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 30.0;
    Btn { x: w / 2.0 - 110.0, y, w: 220.0, h: 56.0 }
}
fn lobby_leave(w: f32) -> Btn {
    let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 100.0;
    Btn { x: w / 2.0 - 110.0, y, w: 220.0, h: 50.0 }
}

pub fn update_lobby(app: &mut App, state: &mut State) {
    let info = match &state.lobby {
        Some(l) => l.clone(),
        None => return,
    };
    let w = win_w(app);

    if info.is_host && info.countdown.is_none() {
        for i in 0..RoomSettings::COUNT {
            if lobby_minus(i, w).clicked(app) {
                send(state, &ClientMessage::SetRoomSetting { index: i as u8, dir: -1 });
                return;
            }
            if lobby_plus(i, w).clicked(app) {
                send(state, &ClientMessage::SetRoomSetting { index: i as u8, dir: 1 });
                return;
            }
        }
    }

    let launch_enabled = info.players >= 2 || info.countdown.is_some();
    if info.is_host && launch_enabled && lobby_launch(w).clicked(app) {
        send(state, &ClientMessage::ToggleCountdown);
        return;
    }

    if lobby_leave(w).clicked(app) {
        send(state, &ClientMessage::LeaveRoom);
    }
}

pub fn draw_lobby(app: &mut App, gfx: &mut Graphics, state: &State) {
    let info = match &state.lobby {
        Some(l) => l,
        None => return,
    };
    let (w, h) = (app.window().width() as f32, app.window().height() as f32);
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, &info.name).position(w / 2.0, 70.0).size(46.0)
        .h_align_center().v_align_middle().color(Color::from_rgb(0.9, 0.7, 1.0));
    draw.text(&state.font, &format!("Room #{}   -   {}/2 joueurs", info.id, info.players))
        .position(w / 2.0, 120.0).size(22.0).h_align_center().v_align_middle().color(Color::GRAY);

    for i in 0..RoomSettings::COUNT {
        let y = LOBBY_FIRST_Y + i as f32 * LOBBY_ROW_H + 25.0;
        draw.text(&state.font, RoomSettings::label(i)).position(w / 2.0 - 60.0, y).size(24.0)
            .h_align_right().v_align_middle().color(Color::from_rgb(0.8, 0.8, 0.85));
        draw.text(&state.font, &info.settings.value(i)).position(w / 2.0 + 145.0, y).size(26.0)
            .h_align_center().v_align_middle().color(Color::YELLOW);
        if info.is_host && info.countdown.is_none() {
            lobby_minus(i, w).draw(&mut draw, app, &state.font, "-");
            lobby_plus(i, w).draw(&mut draw, app, &state.font, "+");
        }
    }

    if info.is_host {
        let full = info.players >= 2;
        let enabled = full || info.countdown.is_some();
        let label = if info.countdown.is_some() { "Cancel" } else { "Launch" };
        lobby_launch(w).draw_styled(&mut draw, app, &state.font, label, enabled);
        if !full && info.countdown.is_none() {
            let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 165.0;
            draw.text(&state.font, "En attente d'un 2e joueur...").position(w / 2.0, y).size(20.0)
                .h_align_center().v_align_middle().color(Color::GRAY);
        }
    } else {
        let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 58.0;
        draw.text(&state.font, "En attente du host...").position(w / 2.0, y).size(24.0)
            .h_align_center().v_align_middle().color(Color::GRAY);
    }
    lobby_leave(w).draw(&mut draw, app, &state.font, "Leave");

    if let Some(n) = info.countdown {
        draw.rect((0.0, 0.0), (w, h)).color(Color::from_rgba(0.0, 0.0, 0.0, 0.6));
        draw.text(&state.font, &n.to_string()).position(w / 2.0, h / 2.0).size(140.0)
            .h_align_center().v_align_middle().color(Color::YELLOW);
    }

    gfx.render(&draw);
}
