use std::sync::Arc;

use notan::draw::DrawShapes;
use notan::prelude::*;
use shared::{ClientMessage, LobbyInfo, RoomSettings};

use crate::state::{ApiFriendsResponse, Screen, State};
use crate::ui::{text_field, Field, Rect, SharpText, Stepper};
use crate::{http, theme};

fn send(state: &mut State, msg: &ClientMessage) {
    state.conn.send(msg);
}

pub fn leave_room_button(w: f32, h: f32) -> Rect {
    Rect::at(w / 2.0 - 130.0, h / 2.0 + 110.0, 260.0, 50.0)
}

pub fn back_to_lobby_button(w: f32, h: f32) -> Rect {
    Rect::at(w / 2.0 - 130.0, h / 2.0 + 175.0, 260.0, 50.0)
}

fn room_row(i: usize, w: f32) -> Rect {
    Rect::at(w / 2.0 - 250.0, 150.0 + i as f32 * 56.0, 500.0, 48.0)
}

struct BrowserButtons {
    create: Rect,
    join_id: Rect,
    refresh: Rect,
    back: Rect,
}

fn browser_buttons(w: f32, h: f32) -> BrowserButtons {
    let bw = 190.0;
    let gap = 15.0;
    let start = w / 2.0 - f32::midpoint(4.0 * bw, 3.0 * gap);
    let y = h - 90.0;
    let at = |i: f32| Rect::at(start + i * (bw + gap), y, bw, 50.0);
    BrowserButtons {
        create: at(0.0),
        join_id: at(1.0),
        refresh: at(2.0),
        back: at(3.0),
    }
}

pub fn update_browser(state: &mut State) {
    let (w, h) = state.ui.view().size();
    let b = browser_buttons(w, h);

    for i in 0..state.rooms.len() {
        let id = state.rooms[i].id;
        if state.ui.clicked(room_row(i, w)) {
            state.notice.clear();
            send(state, &ClientMessage::JoinRoom { id });
            return;
        }
    }

    if state.ui.clicked(b.create) {
        state.text_input.clear();
        state.notice.clear();
        state.screen = Screen::CreateRoom;
    } else if state.ui.clicked(b.join_id) {
        state.text_input.clear();
        state.notice.clear();
        state.screen = Screen::JoinById;
    } else if state.ui.clicked(b.refresh) {
        send(state, &ClientMessage::RequestRoomList);
    } else if state.ui.clicked(b.back) {
        state.conn.disconnect();
        state.rooms.clear();
        state.screen = Screen::Menu;
    }
}

pub fn draw_browser(gfx: &mut Graphics, state: &State) {
    let (w, h) = state.ui.view().size();
    let mut draw = state.ui.canvas(gfx);
    draw.clear(theme::BACKGROUND);

    draw.sharp_text(&state.fonts.display, "ROOMS")
        .position(w / 2.0, 70.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(theme::TEXT);

    if state.rooms.is_empty() {
        draw.sharp_text(&state.fonts.text, "Aucune room. Crees-en une !")
            .position(w / 2.0, 200.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
    }
    for (i, room) in state.rooms.iter().enumerate() {
        let btn = room_row(i, w);
        let label = format!(
            "#{}  {}   {}/{}{}",
            room.id,
            room.name,
            room.players,
            room.max,
            if room.in_game { "  (en jeu)" } else { "" }
        );
        state.ui.button(&mut draw, &state.fonts, btn, label.as_str());
    }

    let b = browser_buttons(w, h);
    state.ui.button(&mut draw, &state.fonts, b.create, "Create Room");
    state.ui.button(&mut draw, &state.fonts, b.join_id, "Join by ID");
    state.ui.button(&mut draw, &state.fonts, b.refresh, "Refresh");
    state.ui.button(&mut draw, &state.fonts, b.back, "Back");

    if let Some((msg, color)) = state.notice.shown() {
        draw.sharp_text(&state.fonts.text, msg)
            .position(w / 2.0, h - 130.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    gfx.render(&draw);
}

fn entry_box(w: f32, h: f32) -> Rect {
    Rect::at(w / 2.0 - 210.0, h / 2.0 - 40.0, 420.0, 50.0)
}

fn entry_buttons(w: f32, h: f32) -> (Rect, Rect) {
    let bw = 200.0;
    let y = h / 2.0 + 60.0;
    (
        Rect::at(w / 2.0 - bw - 10.0, y, bw, 56.0),
        Rect::at(w / 2.0 + 10.0, y, bw, 56.0),
    )
}

pub fn update_create_room(app: &mut App, state: &mut State) {
    if state.backspace.fired() {
        state.text_input.pop();
    }
    let (w, h) = state.ui.view().size();
    let (confirm, back) = entry_buttons(w, h);
    let submit = state.ui.clicked(confirm) || app.keyboard.was_pressed(KeyCode::Enter);
    if submit {
        let name = state.text_input.trim().to_string();
        if !name.is_empty() {
            send(state, &ClientMessage::CreateRoom { name });
            state.text_input.clear();
        }
    } else if state.ui.clicked(back) {
        state.screen = Screen::RoomBrowser;
    }
}

pub fn update_join_by_id(app: &mut App, state: &mut State) {
    if state.backspace.fired() {
        state.text_input.pop();
    }
    let (w, h) = state.ui.view().size();
    let (confirm, back) = entry_buttons(w, h);
    let submit = state.ui.clicked(confirm) || app.keyboard.was_pressed(KeyCode::Enter);
    if submit {
        if let Ok(id) = state.text_input.trim().parse::<u32>() {
            state.notice.clear();
            send(state, &ClientMessage::JoinRoom { id });
            state.text_input.clear();
        }
    } else if state.ui.clicked(back) {
        state.screen = Screen::RoomBrowser;
    }
}

fn draw_entry(gfx: &mut Graphics, state: &State, title: &str, confirm: &str, placeholder: &str) {
    let (w, h) = state.ui.view().size();
    let mut draw = state.ui.canvas(gfx);
    draw.clear(theme::BACKGROUND);

    draw.sharp_text(&state.fonts.display, title)
        .position(w / 2.0, h / 2.0 - 120.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(theme::TEXT);

    let field = Field {
        placeholder,
        value: &state.text_input,
        focused: true,
        secret: false,
    };
    text_field(&mut draw, &state.fonts, entry_box(w, h), &field);

    let (cbtn, back) = entry_buttons(w, h);
    state.ui.button(&mut draw, &state.fonts, cbtn, confirm);
    state.ui.button(&mut draw, &state.fonts, back, "Back");

    gfx.render(&draw);
}

pub fn draw_create_room(gfx: &mut Graphics, state: &State) {
    draw_entry(gfx, state, "Create Room", "Create", "Room name...");
}

pub fn draw_join_by_id(gfx: &mut Graphics, state: &State) {
    draw_entry(gfx, state, "Join by ID", "Join", "Room ID...");
}

fn lobby_ready(info: &LobbyInfo) -> bool {
    info.players >= 2 && info.connected >= info.players
}

fn lobby_headline(info: &LobbyInfo) -> String {
    let away = info.players.saturating_sub(info.connected);
    let away = if away > 0 {
        format!(" ({away} déconnecté)")
    } else {
        String::new()
    };
    format!("Room #{}   -   {}/2 joueurs{away}", info.id, info.players)
}

const LOBBY_FIRST_Y: f32 = 230.0;
const LOBBY_ROW_H: f32 = 64.0;

fn lobby_stepper(i: usize, w: f32) -> Stepper {
    Stepper::at(w / 2.0 + 20.0, LOBBY_FIRST_Y + i as f32 * LOBBY_ROW_H)
}
fn lobby_launch(w: f32) -> Rect {
    let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 30.0;
    Rect::at(w / 2.0 - 110.0, y, 220.0, 56.0)
}
fn lobby_leave(w: f32) -> Rect {
    let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 100.0;
    Rect::at(w / 2.0 - 110.0, y, 220.0, 50.0)
}

fn lobby_invite(w: f32) -> Rect {
    let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 220.0;
    Rect::at(w / 2.0 - 110.0, y, 220.0, 44.0)
}

fn invite_close(w: f32) -> Rect {
    Rect::at(w - 70.0, 10.0, 60.0, 40.0)
}

fn invite_row(w: f32, i: usize) -> Rect {
    Rect::at(w / 2.0 - 80.0, 180.0 + i as f32 * 52.0, 160.0, 40.0)
}

pub fn update_lobby(app: &mut App, state: &mut State) {
    if let Some(Ok(resp)) = http::take(&mut state.invite_slot) {
        if let Some(data) = http::json::<ApiFriendsResponse>(&resp) {
            state.invite_friends = data.friends;
        }
    }

    let info = match &state.lobby {
        Some(l) => l.clone(),
        None => return,
    };
    let (w, h) = state.ui.view().size();

    if state.invite_overlay {
        if state.ui.clicked(invite_close(w)) || app.keyboard.was_pressed(KeyCode::Escape) {
            state.invite_overlay = false;
            return;
        }
        let loaded = state.invite_slot.is_none();
        let invited = (0..state.invite_friends.len()).find(|&i| loaded && state.ui.clicked(invite_row(w, i)));
        if let Some(i) = invited {
            let user_id = state.invite_friends[i].user_id.clone();
            send(state, &ClientMessage::InviteFriend { user_id });
            state.invite_overlay = false;
        }
        return;
    }

    if info.is_host && info.countdown.is_none() {
        for i in 0..RoomSettings::COUNT {
            if state.ui.clicked(lobby_stepper(i, w).minus) {
                send(
                    state,
                    &ClientMessage::SetRoomSetting {
                        index: i as u8,
                        dir: -1,
                    },
                );
                return;
            }
            if state.ui.clicked(lobby_stepper(i, w).plus) {
                send(state, &ClientMessage::SetRoomSetting { index: i as u8, dir: 1 });
                return;
            }
        }
    }

    let launch_enabled = lobby_ready(&info) || info.countdown.is_some();
    if info.is_host && launch_enabled && state.ui.clicked(lobby_launch(w)) {
        send(state, &ClientMessage::ToggleCountdown);
        return;
    }

    if state.ui.clicked(lobby_leave(w)) {
        send(state, &ClientMessage::LeaveRoom);
        state.invite_friends.clear();
        state.invite_slot = None;
        state.invite_overlay = false;
        return;
    }

    if state.auth.is_some() && state.ui.clicked(lobby_invite(w)) {
        state.invite_overlay = true;
        if state.invite_friends.is_empty() && state.invite_slot.is_none() {
            let token = state.auth.as_ref().map(|a| a.token.clone());
            let slot = http::new_slot();
            http::get(http::api_url("friends"), token, Arc::clone(&slot));
            state.invite_slot = Some(slot);
        }
    }

    let _ = h;
}

pub fn draw_lobby(gfx: &mut Graphics, state: &State) {
    let Some(info) = &state.lobby else {
        return;
    };
    let (w, h) = state.ui.view().size();
    let mut draw = state.ui.canvas(gfx);
    state.ui.set_input(!state.invite_overlay);
    draw.clear(theme::BACKGROUND);

    draw.sharp_text(&state.fonts.display, &info.name)
        .position(w / 2.0, 70.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(theme::TITLE);
    draw.sharp_text(&state.fonts.text, &lobby_headline(info))
        .position(w / 2.0, 120.0)
        .size(theme::size::EMPHASIS)
        .h_align_center()
        .v_align_middle()
        .color(theme::TEXT_MUTED);

    let editable = info.is_host && info.countdown.is_none();
    for i in 0..RoomSettings::COUNT {
        let value = info.settings.value(i);
        state.ui.stepper(
            &mut draw,
            &state.fonts,
            lobby_stepper(i, w),
            RoomSettings::label(i),
            &value,
            editable,
        );
    }

    if info.is_host {
        let enabled = lobby_ready(info) || info.countdown.is_some();
        let label = if info.countdown.is_some() { "Cancel" } else { "Launch" };
        state
            .ui
            .button_enabled(&mut draw, &state.fonts, lobby_launch(w), label, enabled);
        let waiting = if info.players < 2 {
            Some("En attente d'un 2e joueur...")
        } else if info.connected < info.players {
            Some("Adversaire déconnecté, en attente de son retour...")
        } else {
            None
        };
        if let (Some(msg), None) = (waiting, info.countdown) {
            let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 165.0;
            draw.sharp_text(&state.fonts.text, msg)
                .position(w / 2.0, y)
                .size(theme::size::LABEL)
                .h_align_center()
                .v_align_middle()
                .color(theme::TEXT_MUTED);
        }
    } else {
        let y = LOBBY_FIRST_Y + RoomSettings::COUNT as f32 * LOBBY_ROW_H + 58.0;
        draw.sharp_text(&state.fonts.text, "En attente du host...")
            .position(w / 2.0, y)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
    }
    state.ui.button(&mut draw, &state.fonts, lobby_leave(w), "Leave");

    if state.auth.is_some() {
        state
            .ui
            .button(&mut draw, &state.fonts, lobby_invite(w), "Inviter un ami");
    }

    if let Some(n) = info.countdown {
        draw.rect((0.0, 0.0), (w, h)).color(theme::SCRIM);
        draw.sharp_text(&state.fonts.display, &n.to_string())
            .position(w / 2.0, h / 2.0)
            .size(theme::size::HUGE)
            .h_align_center()
            .v_align_middle()
            .color(theme::GOLD);
    }

    if state.invite_overlay {
        state.ui.set_input(true);
        draw.rect((0.0, 0.0), (w, h)).color(theme::SCRIM_STRONG);
        draw.sharp_text(&state.fonts.display, "Inviter un ami")
            .position(w / 2.0, 100.0)
            .size(theme::size::HEADING)
            .h_align_center()
            .v_align_middle()
            .color(theme::TITLE);
        state.ui.button(&mut draw, &state.fonts, invite_close(w), "X");
        if state.invite_slot.is_some() {
            draw.sharp_text(&state.fonts.text, "Chargement...")
                .position(w / 2.0, 200.0)
                .size(theme::size::EMPHASIS)
                .h_align_center()
                .v_align_middle()
                .color(theme::TEXT_MUTED);
        } else if state.invite_friends.is_empty() {
            draw.sharp_text(&state.fonts.text, "Aucun ami pour l'instant.")
                .position(w / 2.0, 200.0)
                .size(theme::size::EMPHASIS)
                .h_align_center()
                .v_align_middle()
                .color(theme::TEXT_MUTED);
        } else {
            for (i, friend) in state.invite_friends.iter().enumerate() {
                let row = invite_row(w, i);
                draw.sharp_text(&state.fonts.text, &friend.username)
                    .position(row.x - 20.0, row.y + row.h / 2.0)
                    .size(theme::size::LABEL)
                    .h_align_right()
                    .v_align_middle()
                    .color(theme::TEXT);
                state.ui.button(&mut draw, &state.fonts, row, "Inviter");
            }
        }
    }

    gfx.render(&draw);
}
