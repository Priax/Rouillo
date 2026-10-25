use std::sync::Arc;

use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::{config, ClientMessage, LobbyInfo, PausePolicy, RoomInfo, RoomSetting, RoomSettings};

use crate::state::{ApiFriendsResponse, Invite, Screen, State};
use crate::ui::{
    self, field_clicked, list_row, text_field, Face, Field, Frame, ListCard, Modal, Pill, Rect, SettingsGrid,
    SharpText, Size, Stepper, TextInput, View, FILL, PAGER_H,
};
use crate::{http, theme};

fn send(state: &mut State, msg: &ClientMessage) {
    state.conn.send(msg);
}

const ROOM_ROW_H: f32 = 56.0;

fn room_list(view: View) -> ListCard {
    ListCard::new(view, ROOM_ROW_H)
}

fn room_list_card(view: View) -> Rect {
    room_list(view).card
}

fn visible_rooms(view: View) -> usize {
    room_list(view).per_page()
}

fn pager_area(view: View) -> Rect {
    room_list(view).pager()
}

fn room_pages(state: &State) -> usize {
    ui::page_count(state.rooms.len(), visible_rooms(state.ui.view()))
}

/// The rooms on the current page, with their row on screen.
fn shown_rooms(state: &State) -> impl Iterator<Item = (usize, &RoomInfo)> {
    state.room_pager.shown(&state.rooms, visible_rooms(state.ui.view()))
}

fn room_row(view: View, i: usize) -> Rect {
    room_list(view).row(i)
}

struct BrowserButtons {
    create: Rect,
    join_id: Rect,
    refresh: Rect,
    back: Rect,
}

fn browser_buttons(view: View) -> BrowserButtons {
    let frame = Frame::of(view);
    let gap = 12.0;
    let tools = frame.header_tools(200.0).middle(44.0);
    // Narrower when upright, so the title keeps its place on the left.
    let bw = ((tools.w - 2.0 * gap) / 3.0).min(190.0);
    let [create, join_id, refresh] = tools.right(3.0 * bw + 2.0 * gap).columns(gap, [Size::Px(bw); 3]);
    BrowserButtons {
        create,
        join_id,
        refresh,
        back: frame.back,
    }
}

pub fn update_browser(app: &App, state: &mut State) {
    let view = state.ui.view();
    let b = browser_buttons(view);

    let pages = room_pages(state);
    state.room_pager.clamp(pages);
    state
        .room_pager
        .update(app, &state.ui, &state.fonts, &state.keys, pager_area(view), pages);

    let clicked = shown_rooms(state)
        .find(|&(i, _)| state.ui.clicked(room_row(view, i)))
        .map(|(_, room)| (room.id, watch_only(room)));
    if let Some((id, watch)) = clicked {
        state.notice.clear();
        let msg = if watch {
            ClientMessage::Spectate { id }
        } else {
            ClientMessage::JoinRoom { id }
        };
        send(state, &msg);
        return;
    }

    if state.ui.clicked(b.create) {
        state.text_input = TextInput::default().max_chars(24);
        state.notice.clear();
        state.screen = Screen::CreateRoom;
    } else if state.ui.clicked(b.join_id) {
        state.text_input = TextInput::default().max_chars(9).only(|c| c.is_ascii_digit());
        state.notice.clear();
        state.screen = Screen::JoinById;
    } else if state.ui.clicked(b.refresh) {
        send(state, &ClientMessage::RequestRoomList);
    } else if state.ui.clicked(b.back) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.conn.disconnect();
        state.rooms.clear();
        state.screen = Screen::PlayMenu;
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
        draw.sharp_text(&state.fonts.text, "Aucune room. Créez-en une !")
            .position(card.x + card.w / 2.0, card.y + 60.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
    }
    for (i, room) in shown_rooms(state) {
        draw_room_row(&mut draw, state, room_row(view, i), i, room);
    }
    let pages = room_pages(state);
    if pages > 1 {
        state
            .room_pager
            .draw(&mut draw, &state.ui, &state.fonts, pager_area(view), pages);
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

/// A full room, or one already playing, is joined as a spectator.
fn watch_only(room: &RoomInfo) -> bool {
    room.ranked || (!room.friends_only && (room.players >= room.max || room.in_game))
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
    if room.ranked {
        pills.push(Pill {
            text: "Classé",
            color: theme::bar::ORANGE,
            size: theme::size::SMALL,
        });
    }
    let watchers = format!(
        "{} spectateur{}",
        room.spectators,
        if room.spectators > 1 { "s" } else { "" }
    );
    if room.spectators > 0 {
        pills.push(Pill {
            text: &watchers,
            color: pal.text_dim,
            size: theme::size::SMALL,
        });
    }
    if watch_only(room) {
        pills.push(Pill {
            text: "Regarder",
            color: theme::LINK,
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

fn entry_card(w: f32, h: f32) -> Rect {
    Rect::at(0.0, 0.0, w, h).centered(560.0, 300.0)
}

fn entry_box(w: f32, h: f32) -> Rect {
    entry_card(w, h).pad(110.0, 40.0, 0.0, 40.0).top(50.0)
}

fn entry_buttons(w: f32, h: f32) -> (Rect, Rect) {
    let band = entry_card(w, h).pad(0.0, 40.0, 40.0, 40.0).bottom_band(56.0);
    let [confirm, back] = band.columns(20.0, [FILL, FILL]);
    (confirm, back)
}

fn update_entry(app: &mut App, state: &mut State) -> Option<String> {
    state.text_input.edit(&state.keys);
    let (w, h) = state.ui.view().size();
    field_clicked(&state.ui, &state.fonts, entry_box(w, h), &mut state.text_input);
    let (confirm, back) = entry_buttons(w, h);
    if state.ui.clicked(confirm) || app.keyboard.was_pressed(KeyCode::Enter) {
        return Some(state.text_input.trim().to_string());
    }
    if state.ui.clicked(back) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::RoomBrowser;
    }
    None
}

pub fn update_create_room(app: &mut App, state: &mut State) {
    if let Some(name) = update_entry(app, state).filter(|name| !name.is_empty()) {
        state.notice.clear();
        send(state, &ClientMessage::CreateRoom { name });
        state.text_input.clear();
    }
}

pub fn update_join_by_id(app: &mut App, state: &mut State) {
    if let Some(id) = update_entry(app, state).and_then(|text| text.parse::<u32>().ok()) {
        state.notice.clear();
        send(state, &ClientMessage::JoinRoom { id });
        state.text_input.clear();
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
        input: &state.text_input,
        focused: true,
    };
    text_field(&mut draw, &state.ui, &state.fonts, entry_box(w, h), &field);

    let (cbtn, back) = entry_buttons(w, h);
    state.ui.button(&mut draw, &state.fonts, cbtn, confirm);
    state.ui.button(&mut draw, &state.fonts, back, "Retour");

    if let Some((msg, color)) = state.notice.shown(&pal) {
        draw.sharp_text(&state.fonts.text, msg)
            .position(card.x + card.w / 2.0, card.y + card.h + 40.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    state.ui.render(gfx, &draw);
}

pub fn draw_create_room(gfx: &mut Graphics, state: &State) {
    draw_entry(gfx, state, "Créer une room", "Créer", "Nom de la room...");
}

pub fn draw_join_by_id(gfx: &mut Graphics, state: &State) {
    draw_entry(gfx, state, "Rejoindre par ID", "Rejoindre", "Numéro de la room...");
}

const INVITE_ROW_H: f32 = 52.0;

pub fn setting_label(setting: RoomSetting) -> &'static str {
    match setting {
        RoomSetting::StartingLevel => "Niveau de départ",
        RoomSetting::Colors => "Couleurs",
        RoomSetting::Columns => "Largeur",
        RoomSetting::Rows => "Hauteur",
        RoomSetting::FriendsOnly => "Amis seulement",
        RoomSetting::Pause => "Pause",
        RoomSetting::MaxPlayers => "Joueurs max",
    }
}

pub fn setting_value(settings: &RoomSettings, setting: RoomSetting) -> String {
    let rules = &settings.rules;
    match setting {
        RoomSetting::StartingLevel => match config::starting_garbage_rows(rules.starting_level) {
            0 => rules.starting_level.to_string(),
            1 => format!("{} (+1 ligne)", rules.starting_level),
            rows => format!("{} (+{rows} lignes)", rules.starting_level),
        },
        RoomSetting::Colors => rules.colors.to_string(),
        RoomSetting::Columns => rules.columns.to_string(),
        RoomSetting::Rows => rules.rows.to_string(),
        RoomSetting::FriendsOnly => if settings.friends_only { "Oui" } else { "Non" }.into(),
        RoomSetting::Pause => match settings.pause {
            PausePolicy::Everyone => "Tous",
            PausePolicy::HostOnly => "Hôte",
            PausePolicy::Nobody => "Personne",
        }
        .into(),
        RoomSetting::MaxPlayers => settings.max_players.to_string(),
    }
}

/// The lobby's settings, by column: the game played, then the room.
const LOBBY_SETTINGS: [(&str, &[RoomSetting]); 2] = [
    (
        "Partie",
        &[
            RoomSetting::StartingLevel,
            RoomSetting::Colors,
            RoomSetting::Columns,
            RoomSetting::Rows,
        ],
    ),
    (
        "Room",
        &[RoomSetting::MaxPlayers, RoomSetting::Pause, RoomSetting::FriendsOnly],
    ),
];

fn lobby_panel(view: View) -> SettingsGrid {
    let rows = LOBBY_SETTINGS.iter().map(|(_, s)| s.len()).max().unwrap_or(0);
    SettingsGrid::new(view, rows)
}

/// Each setting of the lobby with its tile.
fn lobby_tiles(view: View) -> impl Iterator<Item = (RoomSetting, Rect)> {
    let grid = lobby_panel(view);
    LOBBY_SETTINGS
        .into_iter()
        .enumerate()
        .flat_map(move |(column, (_, settings))| {
            settings
                .iter()
                .enumerate()
                .map(move |(row, &setting)| (setting, grid.tile(column, row)))
        })
}

fn lobby_launch(view: View) -> Rect {
    lobby_panel(view).action_row(0)
}

fn lobby_invite(view: View) -> Rect {
    lobby_panel(view).action_row(1)
}

fn lobby_leave(view: View) -> Rect {
    lobby_panel(view).action_row(2)
}

/// The players' card, left of the settings as the chat is right of them.
fn lobby_players(view: View) -> Rect {
    let card = lobby_panel(view).card;
    let beside = Rect::at(0.0, card.y, card.x, card.h).pad(0.0, 24.0, 0.0, 24.0);
    beside.right(beside.w.min(460.0))
}

const PLAYERS_TITLE_H: f32 = 56.0;
const PLAYER_ROW_H: f32 = 44.0;

fn lobby_player_row(view: View, i: usize, players: usize) -> Rect {
    let list = lobby_players(view).below(PLAYERS_TITLE_H).pad(0.0, 12.0, 12.0, 12.0);
    let h = (list.h / players.max(1) as f32).min(PLAYER_ROW_H);
    list.nth_row(i, h - 4.0, 4.0)
}

/// One's own row, while one may still change team on it.
fn team_picker(info: &LobbyInfo) -> Option<usize> {
    let open = info.ranked.is_none() && info.countdown.is_none();
    usize::from(info.your_slot).checked_sub(1).filter(|_| open)
}

/// Solo, then each team in turn, then Solo again.
fn next_team(team: Option<u8>) -> Option<u8> {
    match team {
        None => Some(1),
        Some(t) if t < shared::MAX_TEAMS => Some(t + 1),
        Some(_) => None,
    }
}

fn team_look(team: Option<u8>, pal: &theme::Palette) -> (&'static str, Color) {
    team.and_then(theme::team).unwrap_or(("Solo", pal.text_dim))
}

/// Whether the players make two sides at least: one without a team is a side.
fn has_sides(info: &LobbyInfo) -> bool {
    let mut teams = info.teams.iter();
    let Some(first) = teams.next() else { return false };
    first.is_none() && info.teams.len() > 1 || teams.any(|t| t.is_none() || t != first)
}

fn draw_lobby_players(draw: &mut Draw, state: &State, info: &LobbyInfo) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let fonts = &state.fonts;
    let card = lobby_players(view);
    if card.w < 160.0 {
        return;
    }
    ui::card(draw, &pal, card);
    draw.sharp_text(&fonts.display, "Joueurs")
        .position(card.x + 16.0, card.y + PLAYERS_TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text_dim);
    ui::divider(draw, &pal, card.x + 16.0, card.y + PLAYERS_TITLE_H, card.w - 32.0);
    let own = usize::from(info.your_slot).checked_sub(1);
    let picker = team_picker(info);
    for (i, name) in info.names.iter().enumerate() {
        let row = lobby_player_row(view, i, info.names.len());
        let mine = own == Some(i);
        if picker == Some(i) {
            state.ui.row(draw, row, i, &format!("team:{i}"));
        } else {
            list_row(draw, &pal, row, i);
        }
        let (label, color) = team_look(info.teams.get(i).copied().flatten(), &pal);
        let pill = Pill {
            text: label,
            color,
            size: theme::size::SMALL,
        };
        let pill_w = pill.width(fonts);
        let name = fonts.fit(Face::Text, name, theme::size::BODY, row.w - pill_w - 36.0);
        draw.sharp_text(&fonts.text, &name)
            .position(row.x + 10.0, row.y + row.h / 2.0)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(if mine { pal.text } else { pal.text_dim });
        pill.draw(draw, fonts, (row.x + row.w - 10.0 - pill_w, row.y + row.h / 2.0));
    }
}

fn lobby_chat(view: View) -> Rect {
    let card = lobby_panel(view).card;
    let beside = Rect::at(0.0, card.y, view.w, card.h).pad(0.0, 24.0, 0.0, card.x + card.w + 24.0);
    beside.left(beside.w.min(460.0))
}

fn invite_modal(view: View) -> Modal {
    Modal::new(view, 600.0)
}

fn invite_list(view: View) -> Rect {
    invite_modal(view).card.pad(70.0, 16.0, PAGER_H + 22.0, 16.0)
}

fn visible_invites(view: View) -> usize {
    invite_list(view).rows_fitting(INVITE_ROW_H - 6.0, 6.0).max(1)
}

fn invite_pager_area(view: View) -> Rect {
    invite_modal(view).card.above(8.0).bottom_band(PAGER_H)
}

fn invite_row(view: View, i: usize) -> Rect {
    invite_list(view).nth_row(i, INVITE_ROW_H - 6.0, 6.0)
}

fn invite_button(view: View, i: usize) -> Rect {
    invite_row(view, i).pad(0.0, 8.0, 0.0, 0.0).right(120.0).middle(32.0)
}

pub fn update_lobby(app: &mut App, state: &mut State) {
    let view = state.ui.view();
    let State {
        room,
        ui,
        fonts,
        keys,
        conn,
        auth,
        maintenance,
        ..
    } = state;
    let Some(room) = room.as_mut() else { return };
    if let Some(Ok(data)) = http::take_json::<ApiFriendsResponse>(&mut room.invite.slot) {
        room.invite.friends = data.friends;
    }
    let info = &room.info;

    if room.invite.open {
        let invite = &mut room.invite;
        if invite_modal(view).dismissed(ui) || app.keyboard.was_pressed(KeyCode::Escape) {
            invite.open = false;
            return;
        }
        let per_page = visible_invites(view);
        let pages = ui::page_count(invite.friends.len(), per_page);
        invite.pager.clamp(pages);
        invite
            .pager
            .update(app, ui, fonts, keys, invite_pager_area(view), pages);
        let loaded = invite.slot.is_none();
        let invited = invite
            .pager
            .shown(&invite.friends, per_page)
            .find(|&(i, _)| loaded && ui.clicked(invite_button(view, i)))
            .map(|(_, friend)| friend.user_id.clone());
        if let Some(user_id) = invited {
            conn.send(&ClientMessage::InviteFriend { user_id });
            invite.open = false;
        }
        return;
    }

    if let Some(me) = team_picker(info) {
        if ui.clicked(lobby_player_row(view, me, info.names.len())) {
            let team = next_team(info.teams.get(me).copied().flatten());
            conn.send(&ClientMessage::SetTeam { team });
            return;
        }
    }

    let chatting = room.chat.focused;
    if let Some(text) = room.chat.update_lobby(&app.keyboard, keys) {
        conn.send(&ClientMessage::Chat { text });
    }
    room.chat.click(ui, fonts, lobby_chat(view));

    if info.is_host && info.countdown.is_none() {
        for (setting, tile) in lobby_tiles(view) {
            let stepper = Stepper::in_tile(tile);
            let dir = if setting == RoomSetting::FriendsOnly {
                if ui.clicked(Stepper::tile_band(tile)) {
                    1
                } else {
                    continue;
                }
            } else if ui.clicked(stepper.minus) {
                -1
            } else if ui.clicked(stepper.plus) {
                1
            } else {
                continue;
            };
            conn.send(&ClientMessage::SetRoomSetting { setting, dir });
            return;
        }
    }

    let (_, _, launch_enabled) = launch_bar(info, *maintenance);
    if launch_enabled && ui.bar_clicked(lobby_launch(view)) {
        let msg = if info.your_slot == 0 {
            ClientMessage::JoinRoom { id: info.id }
        } else {
            ClientMessage::ToggleCountdown
        };
        conn.send(&msg);
        return;
    }

    let escape = !chatting && app.keyboard.was_pressed(KeyCode::Escape);
    if ui.bar_clicked(lobby_leave(view)) || escape {
        conn.send(&ClientMessage::LeaveRoom);
        room.invite = Invite::default();
        return;
    }

    if info.your_slot != 0 && auth.is_some() && ui.bar_clicked(lobby_invite(view)) {
        room.invite.open = true;
        if room.invite.friends.is_empty() && room.invite.slot.is_none() {
            let token = auth.as_ref().map(|a| a.token.clone());
            let slot = http::new_slot();
            http::get(http::api_url("friends"), token, Arc::clone(&slot));
            room.invite.slot = Some(slot);
        }
    }
}

pub fn draw_lobby(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let Some(room) = &state.room else {
        return;
    };
    let info = &room.info;
    let view = state.ui.view();
    let fonts = &state.fonts;
    let mut draw = state.ui.screen_canvas(gfx);
    state.ui.set_input(!room.invite.open);

    draw_lobby_header(&mut draw, state, info);
    draw_lobby_players(&mut draw, state, info);

    lobby_panel(view).draw(
        &mut draw,
        &pal,
        fonts,
        "Réglages de la room",
        LOBBY_SETTINGS.map(|(section, _)| section),
    );
    let editable = info.is_host && info.countdown.is_none();
    for (setting, tile) in lobby_tiles(view) {
        let label = setting_label(setting);
        if setting == RoomSetting::FriendsOnly {
            let on = info.settings.friends_only;
            state.ui.switch_tile(&mut draw, fonts, tile, label, on, editable);
        } else {
            let value = setting_value(&info.settings, setting);
            state.ui.stepper_tile(&mut draw, fonts, tile, label, &value, editable);
        }
    }

    let (label, color, enabled) = launch_bar(info, state.maintenance);
    state
        .ui
        .menu_bar_enabled(&mut draw, fonts, lobby_launch(view), label, color, enabled);
    if info.your_slot != 0 && state.auth.is_some() {
        state
            .ui
            .menu_bar(&mut draw, fonts, lobby_invite(view), "Inviter un ami", theme::bar::BLUE);
    }
    room.chat.draw_card(&mut draw, &state.ui, fonts, lobby_chat(view));
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

    if room.invite.open {
        draw_invite_overlay(&mut draw, state, &room.invite);
    }

    state.ui.render(gfx, &draw);
}

fn launch_bar(info: &LobbyInfo, maintenance: bool) -> (&'static str, Color, bool) {
    if maintenance {
        ("Le serveur redémarre...", theme::bar::GREEN, false)
    } else if info.your_slot == 0 && info.players < info.settings.max_players && info.countdown.is_none() {
        ("Prendre la place", theme::bar::GREEN, true)
    } else if info.your_slot == 0 {
        ("Vous regardez cette room", theme::bar::GREEN, false)
    } else if !info.is_host {
        ("En attente de l'hôte...", theme::bar::GREEN, false)
    } else if info.countdown.is_some() {
        ("Annuler", theme::bar::YELLOW, true)
    } else if info.players < 2 {
        ("En attente d'un 2e joueur...", theme::bar::GREEN, false)
    } else if !has_sides(info) {
        ("Tout le monde est dans la même équipe", theme::bar::GREEN, false)
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
    let players = format!("{}/{} joueurs", info.players, info.settings.max_players);
    let watchers = format!(
        "{} spectateur{}",
        info.spectators,
        if info.spectators > 1 { "s" } else { "" }
    );
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
    if info.spectators > 0 {
        pills.push(Pill {
            text: &watchers,
            color: pal.text_dim,
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

fn draw_invite_overlay(draw: &mut Draw, state: &State, invite: &Invite) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let fonts = &state.fonts;
    let modal = invite_modal(view);
    let card = modal.card;
    modal.draw(draw, &state.ui, fonts, "Inviter un ami");

    let message = if invite.slot.is_some() {
        Some("Chargement...")
    } else if invite.friends.is_empty() {
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
    let per_page = visible_invites(view);
    let pages = ui::page_count(invite.friends.len(), per_page);
    if pages > 1 {
        invite
            .pager
            .draw(draw, &state.ui, fonts, invite_pager_area(view), pages);
    }
    for (i, friend) in invite.pager.shown(&invite.friends, per_page) {
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

#[cfg(test)]
mod team_tests {
    use super::*;

    fn lobby(teams: Vec<Option<u8>>) -> LobbyInfo {
        LobbyInfo {
            id: 1,
            name: "r".into(),
            settings: RoomSettings::default(),
            players: teams.len() as u8,
            connected: teams.len() as u8,
            your_slot: 1,
            is_host: true,
            countdown: None,
            ranked: None,
            names: teams.iter().map(|_| "p".into()).collect(),
            teams,
            spectators: 0,
        }
    }

    #[test]
    fn sides_are_teams_or_players_on_their_own() {
        assert!(!has_sides(&lobby(vec![None])));
        assert!(has_sides(&lobby(vec![None, None])));
        assert!(has_sides(&lobby(vec![Some(1), None])));
        assert!(has_sides(&lobby(vec![Some(1), Some(2)])));
        assert!(!has_sides(&lobby(vec![Some(3), Some(3), Some(3)])));
        assert!(has_sides(&lobby(vec![Some(3), Some(3), None])));
    }

    #[test]
    fn the_team_button_goes_round() {
        let mut team = None;
        let mut seen = Vec::new();
        for _ in 0..=shared::MAX_TEAMS {
            team = next_team(team);
            seen.push(team);
        }
        assert_eq!(seen, [Some(1), Some(2), Some(3), Some(4), None]);
    }
}
