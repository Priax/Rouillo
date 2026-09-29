use std::sync::Arc;

use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::{ClientMessage, LobbyInfo, RoomInfo, RoomSettings};

use crate::state::{ApiFriendsResponse, Screen, State};
use crate::ui::{self, divider, list_row, text_field, Face, Field, Pill, Rect, SharpText, Stepper, View};
use crate::{http, theme};

fn send(state: &mut State, msg: &ClientMessage) {
    state.conn.send(msg);
}

const ROOM_ROW_H: f32 = 56.0;

fn room_list_card(view: View) -> Rect {
    Rect::at(
        view.w / 2.0 - 420.0,
        theme::HEADER_H + 30.0,
        840.0,
        view.h - theme::HEADER_H - 30.0 - 110.0,
    )
}

/// How many rooms fit in the list card.
fn visible_rooms(view: View) -> usize {
    ((room_list_card(view).h - 24.0) / ROOM_ROW_H).floor().max(0.0) as usize
}

fn room_row(view: View, i: usize) -> Rect {
    let card = room_list_card(view);
    Rect::at(
        card.x + 12.0,
        card.y + 12.0 + i as f32 * ROOM_ROW_H,
        card.w - 24.0,
        ROOM_ROW_H - 6.0,
    )
}

struct BrowserButtons {
    create: Rect,
    join_id: Rect,
    refresh: Rect,
    back: Rect,
}

fn browser_buttons(view: View) -> BrowserButtons {
    let (bw, gap, y) = (190.0, 12.0, (theme::HEADER_H - 44.0) / 2.0);
    let right = view.w - 60.0;
    let at = |i: f32| Rect::at(right - (3.0 - i) * bw - (2.0 - i) * gap, y, bw, 44.0);
    BrowserButtons {
        create: at(0.0),
        join_id: at(1.0),
        refresh: at(2.0),
        back: Rect::at(40.0, view.h - 80.0, 200.0, 54.0),
    }
}

pub fn update_browser(state: &mut State) {
    let view = state.ui.view();
    let b = browser_buttons(view);

    for i in 0..state.rooms.len().min(visible_rooms(view)) {
        let id = state.rooms[i].id;
        if state.ui.clicked(room_row(view, i)) {
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
    let pal = state.ui.palette();
    let view = state.ui.view();
    let mut draw = state.ui.screen_canvas(gfx);

    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Rooms")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    let card = room_list_card(view);
    ui::card(&mut draw, &pal, card);
    if state.rooms.is_empty() {
        draw.sharp_text(&state.fonts.text, "Aucune room. Crées-en une !")
            .position(card.x + card.w / 2.0, card.y + 60.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
    }
    for (i, room) in state.rooms.iter().take(visible_rooms(view)).enumerate() {
        draw_room_row(&mut draw, state, room_row(view, i), i, room);
    }

    let b = browser_buttons(view);
    state.ui.button(&mut draw, &state.fonts, b.create, "Créer une room");
    state.ui.button(&mut draw, &state.fonts, b.join_id, "Rejoindre par ID");
    state.ui.button(&mut draw, &state.fonts, b.refresh, "Rafraîchir");
    state.ui.button(&mut draw, &state.fonts, b.back, "Retour");

    if let Some((msg, color)) = state.notice.shown(&pal) {
        draw.sharp_text(&state.fonts.text, msg)
            .position(view.w / 2.0, b.back.y + b.back.h / 2.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    state.ui.render(gfx, &draw);
}

fn draw_room_row(draw: &mut Draw, state: &State, row: Rect, index: usize, room: &RoomInfo) {
    let pal = state.ui.palette();
    let fonts = &state.fonts;
    state.ui.row(draw, row, index, &format!("room:{}", room.id));
    let mid = row.y + row.h / 2.0;
    draw.sharp_text(&fonts.text, &format!("#{}", room.id))
        .position(row.x + 18.0, mid)
        .size(theme::size::BODY)
        .v_align_middle()
        .color(pal.text_muted);
    let players = format!("{}/{} joueurs", room.players, room.max);
    let mut pills = vec![Pill {
        text: &players,
        color: pal.accent,
        size: theme::size::SMALL,
    }];
    if room.in_game {
        pills.push(Pill {
            text: "En jeu",
            color: theme::WARNING,
            size: theme::size::SMALL,
        });
    }
    if room.friends_only {
        pills.push(Pill {
            text: "Amis",
            color: theme::SUCCESS,
            size: theme::size::SMALL,
        });
    }
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, row.x + row.w - 16.0, mid);
    let name_x = row.x + 90.0;
    let name = fonts.fit(
        Face::Display,
        &room.name,
        theme::size::EMPHASIS,
        pills_start - 20.0 - name_x,
    );
    draw.sharp_text(&fonts.display, &name)
        .position(name_x, mid)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text);
}

/// The dialog card of the create and join screens, centred.
fn entry_card(w: f32, h: f32) -> Rect {
    Rect::at(w / 2.0 - 280.0, h / 2.0 - 160.0, 560.0, 300.0)
}

fn entry_box(w: f32, h: f32) -> Rect {
    let card = entry_card(w, h);
    Rect::at(card.x + 40.0, card.y + 110.0, card.w - 80.0, 50.0)
}

fn entry_buttons(w: f32, h: f32) -> (Rect, Rect) {
    let card = entry_card(w, h);
    let (bw, y) = ((card.w - 80.0 - 20.0) / 2.0, card.y + card.h - 40.0 - 56.0);
    (
        Rect::at(card.x + 40.0, y, bw, 56.0),
        Rect::at(card.x + 40.0 + bw + 20.0, y, bw, 56.0),
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
    let pal = state.ui.palette();
    let (w, h) = state.ui.view().size();
    let mut draw = state.ui.screen_canvas(gfx);

    let card = entry_card(w, h);
    ui::card(&mut draw, &pal, card);
    draw.sharp_text(&state.fonts.display, title)
        .position(card.x + card.w / 2.0, card.y + 58.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(pal.text);

    let field = Field {
        placeholder,
        value: &state.text_input,
        focused: true,
        secret: false,
    };
    text_field(&mut draw, &pal, &state.fonts, entry_box(w, h), &field);

    let (cbtn, back) = entry_buttons(w, h);
    state.ui.button(&mut draw, &state.fonts, cbtn, confirm);
    state.ui.button(&mut draw, &state.fonts, back, "Retour");

    state.ui.render(gfx, &draw);
}

pub fn draw_create_room(gfx: &mut Graphics, state: &State) {
    draw_entry(gfx, state, "Créer une room", "Créer", "Nom de la room...");
}

pub fn draw_join_by_id(gfx: &mut Graphics, state: &State) {
    draw_entry(gfx, state, "Rejoindre par ID", "Rejoindre", "Numéro de la room...");
}

fn lobby_ready(info: &LobbyInfo) -> bool {
    info.players >= 2 && info.connected >= info.players
}

const SETTINGS_TITLE_H: f32 = 56.0;
const SETTING_ROW_H: f32 = 64.0;
const ACTION_ROW_H: f32 = 60.0;
const INVITE_ROW_H: f32 = 52.0;

fn settings_card(view: View) -> Rect {
    let h = SETTINGS_TITLE_H + RoomSettings::COUNT as f32 * SETTING_ROW_H + 16.0;
    Rect::at(view.w / 2.0 - 320.0, theme::HEADER_H + 30.0, 640.0, h)
}

fn lobby_stepper(i: usize, view: View) -> Stepper {
    let card = settings_card(view);
    Stepper::at(
        card.x + card.w / 2.0 + 20.0,
        card.y + SETTINGS_TITLE_H + 8.0 + i as f32 * SETTING_ROW_H,
    )
}

/// The lobby's actions, as full-width bars under the settings card:
/// launching, inviting a friend, leaving.
fn action_row(view: View, k: usize) -> Rect {
    let card = settings_card(view);
    Rect::at(
        0.0,
        card.y + card.h + 30.0 + k as f32 * ACTION_ROW_H,
        view.w,
        ACTION_ROW_H,
    )
}

fn lobby_launch(view: View) -> Rect {
    action_row(view, 0)
}

fn lobby_invite(view: View) -> Rect {
    action_row(view, 1)
}

fn lobby_leave(view: View) -> Rect {
    action_row(view, 2)
}

fn invite_card(view: View) -> Rect {
    Rect::at(view.w / 2.0 - 300.0, 110.0, 600.0, view.h - 220.0)
}

fn invite_close(view: View) -> Rect {
    let card = invite_card(view);
    Rect::at(card.x + card.w - 64.0, card.y + 14.0, 48.0, 40.0)
}

fn visible_invites(view: View) -> usize {
    ((invite_card(view).h - 86.0) / INVITE_ROW_H).floor().max(0.0) as usize
}

fn invite_row(view: View, i: usize) -> Rect {
    let card = invite_card(view);
    Rect::at(
        card.x + 16.0,
        card.y + 70.0 + i as f32 * INVITE_ROW_H,
        card.w - 32.0,
        INVITE_ROW_H - 6.0,
    )
}

fn invite_button(view: View, i: usize) -> Rect {
    let row = invite_row(view, i);
    Rect::at(row.x + row.w - 128.0, row.y + (row.h - 32.0) / 2.0, 120.0, 32.0)
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
    let view = state.ui.view();

    if state.invite_overlay {
        if state.ui.clicked(invite_close(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
            state.invite_overlay = false;
            return;
        }
        let loaded = state.invite_slot.is_none();
        let shown = state.invite_friends.len().min(visible_invites(view));
        let invited = (0..shown).find(|&i| loaded && state.ui.clicked(invite_button(view, i)));
        if let Some(i) = invited {
            let user_id = state.invite_friends[i].user_id.clone();
            send(state, &ClientMessage::InviteFriend { user_id });
            state.invite_overlay = false;
        }
        return;
    }

    if info.is_host && info.countdown.is_none() {
        for i in 0..RoomSettings::COUNT {
            if state.ui.clicked(lobby_stepper(i, view).minus) {
                send(
                    state,
                    &ClientMessage::SetRoomSetting {
                        index: i as u8,
                        dir: -1,
                    },
                );
                return;
            }
            if state.ui.clicked(lobby_stepper(i, view).plus) {
                send(state, &ClientMessage::SetRoomSetting { index: i as u8, dir: 1 });
                return;
            }
        }
    }

    let launch_enabled = lobby_ready(&info) || info.countdown.is_some();
    if info.is_host && launch_enabled && state.ui.clicked(lobby_launch(view)) {
        send(state, &ClientMessage::ToggleCountdown);
        return;
    }

    if state.ui.clicked(lobby_leave(view)) {
        send(state, &ClientMessage::LeaveRoom);
        state.invite_friends.clear();
        state.invite_slot = None;
        state.invite_overlay = false;
        return;
    }

    if state.auth.is_some() && state.ui.clicked(lobby_invite(view)) {
        state.invite_overlay = true;
        if state.invite_friends.is_empty() && state.invite_slot.is_none() {
            let token = state.auth.as_ref().map(|a| a.token.clone());
            let slot = http::new_slot();
            http::get(http::api_url("friends"), token, Arc::clone(&slot));
            state.invite_slot = Some(slot);
        }
    }
}

pub fn draw_lobby(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let Some(info) = &state.lobby else {
        return;
    };
    let view = state.ui.view();
    let fonts = &state.fonts;
    let mut draw = state.ui.screen_canvas(gfx);
    state.ui.set_input(!state.invite_overlay);

    draw_lobby_header(&mut draw, state, info);

    let card = settings_card(view);
    ui::card(&mut draw, &pal, card);
    draw.sharp_text(&fonts.display, "Réglages de la room")
        .position(card.x + 20.0, card.y + SETTINGS_TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text_dim);
    divider(&mut draw, &pal, card.x + 20.0, card.y + SETTINGS_TITLE_H, card.w - 40.0);
    let editable = info.is_host && info.countdown.is_none();
    for i in 0..RoomSettings::COUNT {
        let value = info.settings.value(i);
        state.ui.stepper(
            &mut draw,
            fonts,
            lobby_stepper(i, view),
            RoomSettings::label(i),
            &value,
            editable,
        );
    }

    let (label, color, enabled) = launch_bar(info);
    state
        .ui
        .menu_bar_enabled(&mut draw, fonts, lobby_launch(view), label, color, enabled);
    if state.auth.is_some() {
        state
            .ui
            .menu_bar(&mut draw, fonts, lobby_invite(view), "Inviter un ami", theme::bar::BLUE);
    }
    state
        .ui
        .menu_bar(&mut draw, fonts, lobby_leave(view), "Quitter la room", theme::bar::RED);

    if let Some(n) = info.countdown {
        draw.rect((0.0, 0.0), (view.w, view.h)).color(theme::SCRIM);
        draw.sharp_text(&fonts.display, &n.to_string())
            .position(view.w / 2.0, view.h / 2.0)
            .size(theme::size::HUGE)
            .h_align_center()
            .v_align_middle()
            .color(theme::GOLD);
    }

    if state.invite_overlay {
        state.ui.set_input(true);
        draw_invite_overlay(&mut draw, state);
    }

    state.ui.render(gfx, &draw);
}

/// The launch bar's label, colour and whether it can be pressed: it says
/// why a game cannot start yet instead of showing a separate message.
fn launch_bar(info: &LobbyInfo) -> (&'static str, Color, bool) {
    if !info.is_host {
        ("En attente de l'hôte...", theme::bar::GREEN, false)
    } else if info.countdown.is_some() {
        ("Annuler", theme::bar::YELLOW, true)
    } else if info.players < 2 {
        ("En attente d'un 2e joueur...", theme::bar::GREEN, false)
    } else if info.connected < info.players {
        ("Adversaire déconnecté...", theme::bar::GREEN, false)
    } else {
        ("Lancer la partie", theme::bar::GREEN, true)
    }
}

fn draw_lobby_header(draw: &mut Draw, state: &State, info: &LobbyInfo) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let fonts = &state.fonts;
    state.ui.header_band(draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    let id = format!("Room #{}", info.id);
    let players = format!("{}/2 joueurs", info.players);
    let away = info.players.saturating_sub(info.connected);
    let away_text = format!("{away} déconnecté");
    let mut pills = vec![
        Pill {
            text: &id,
            color: pal.text_dim,
            size: theme::size::SMALL,
        },
        Pill {
            text: &players,
            color: pal.accent,
            size: theme::size::SMALL,
        },
    ];
    if away > 0 {
        pills.push(Pill {
            text: &away_text,
            color: theme::WARNING,
            size: theme::size::SMALL,
        });
    }
    let mid = theme::HEADER_H / 2.0;
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, view.w - 60.0, mid);
    let name = fonts.fit(Face::Display, &info.name, theme::size::TITLE, pills_start - 30.0 - 60.0);
    draw.sharp_text(&fonts.display, &name)
        .position(60.0, mid)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);
}

fn draw_invite_overlay(draw: &mut Draw, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let fonts = &state.fonts;
    draw.rect((0.0, 0.0), (view.w, view.h)).color(pal.scrim_strong);
    let card = invite_card(view);
    ui::card(draw, &pal, card);
    draw.sharp_text(&fonts.display, "Inviter un ami")
        .position(card.x + 24.0, card.y + 34.0)
        .size(theme::size::HEADING)
        .v_align_middle()
        .color(pal.text);
    state.ui.button(draw, fonts, invite_close(view), "X");

    let message = if state.invite_slot.is_some() {
        Some("Chargement...")
    } else if state.invite_friends.is_empty() {
        Some("Aucun ami pour l'instant.")
    } else {
        None
    };
    if let Some(message) = message {
        draw.sharp_text(&fonts.text, message)
            .position(card.x + card.w / 2.0, card.y + 110.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
        return;
    }
    for (i, friend) in state.invite_friends.iter().take(visible_invites(view)).enumerate() {
        let row = invite_row(view, i);
        list_row(draw, &pal, row, i);
        draw.sharp_text(&fonts.text, &friend.username)
            .position(row.x + 16.0, row.y + row.h / 2.0)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.text);
        state.ui.button(draw, fonts, invite_button(view, i), "Inviter");
    }
}
