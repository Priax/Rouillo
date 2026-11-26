use std::sync::Arc;

use aero::draw::Draw;
use aero::geom::Color;
use aero::input::KeyCode;
use shared::{config, ClientMessage, LobbyInfo, PausePolicy, RoomInfo, RoomSetting, RoomSettings};

use crate::connection::Connection;
use crate::cpu_seat::{self, CpuSeat};
use crate::nav::Transition;
use crate::state::{ApiFriendsResponse, Input, Invite, Network, Screen, State};
use crate::tick::Tick;
use crate::ui::{
    self, field_clicked, list_row, text_field, EditKeys, Face, Field, Fonts, Frame, Gui, Icon, ListCard, Modal, Pill,
    Rect, SettingsGrid, Size, Stepper, TextInput, Ui, View, FILL, PAGER_H,
};
use crate::{board_size, http, theme};

const ROOM_ROW_H: f32 = 56.0;

fn room_list(view: View) -> ListCard {
    ListCard::new(view, ROOM_ROW_H)
}

fn room_pages(rooms: &[RoomInfo], view: View) -> usize {
    ui::page_count(rooms.len(), room_list(view).per_page())
}

/// The rooms on the current page, with their row on screen.
fn shown_rooms(view: View, state: &State) -> impl Iterator<Item = (usize, &RoomInfo)> {
    state.room_pager.shown(&state.rooms, room_list(view).per_page())
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
    BrowserButtons { create, join_id, refresh, back: frame.back }
}

pub fn update_browser(tick: &Tick, gui: &mut Gui, state: &mut State) -> Transition {
    let view = gui.ui.view();
    let b = browser_buttons(view);
    let pages = room_pages(&state.rooms, view);

    state.room_pager.clamp(pages);
    state.room_pager.update(tick, &gui.ui, &mut gui.fonts, &state.input.keys, room_list(view).pager(), pages);

    let clicked = shown_rooms(gui.ui.view(), state)
        .find(|&(i, _)| gui.ui.clicked(room_list(view).row(i)))
        .map(|(_, room)| (room.id, watch_only(room)));
    if let Some((id, watch)) = clicked {
        state.notice.clear();
        let msg = if watch { ClientMessage::Spectate { id } } else { ClientMessage::JoinRoom { id } };
        state.net.conn.send(&msg);
        return Transition::Stay;
    }

    if gui.ui.clicked(b.create) {
        state.text_input = TextInput::default().max_chars(24);
        state.notice.clear();
        return Transition::To(Screen::CreateRoom);
    } else if gui.ui.clicked(b.join_id) {
        state.text_input = TextInput::default().max_chars(9).only(|c| c.is_ascii_digit());
        state.notice.clear();
        return Transition::To(Screen::JoinById);
    } else if gui.ui.clicked(b.refresh) {
        state.net.conn.send(&ClientMessage::RequestRoomList);
    } else if gui.ui.clicked(b.back) || tick.back() {
        state.net.conn.disconnect();
        state.rooms.clear();
        return Transition::To(Screen::PlayMenu);
    }

    Transition::Stay
}

pub fn draw_browser(draw: &mut Draw, gui: &mut Gui, state: &State) {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let card = room_list(view).card;
    let pages = room_pages(&state.rooms, view);
    let b = browser_buttons(view);

    let mut canvas = gui.ui.screen_canvas(draw);
    let draw = &mut *canvas;
    gui.ui.page_header(draw, &mut gui.fonts, "Rooms");

    ui::card(draw, &pal, card);
    if state.rooms.is_empty() {
        gui.fonts
            .write(draw, Face::Text, "Aucune room. Créez-en une !")
            .center(card.x + card.w / 2.0, card.y + 60.0)
            .size(theme::size::EMPHASIS)
            .color(pal.text_muted);
    }
    for (i, room) in shown_rooms(gui.ui.view(), state) {
        draw_room_row(draw, gui, room_list(view).row(i), i, room);
    }
    if pages > 1 {
        state.room_pager.draw(draw, &mut gui.ui, &mut gui.fonts, room_list(view).pager(), pages);
    }

    gui.ui.button(draw, &mut gui.fonts, b.create, "Créer une room");
    gui.ui.button(draw, &mut gui.fonts, b.join_id, "Rejoindre par ID");
    gui.ui.button(draw, &mut gui.fonts, b.refresh, "Rafraîchir");
    gui.ui.button(draw, &mut gui.fonts, b.back, "Retour");

    if let Some((msg, color)) = state.notice.shown(&pal) {
        gui.fonts
            .write(draw, Face::Text, msg)
            .center(view.w / 2.0, b.back.y + b.back.h / 2.0)
            .size(theme::size::EMPHASIS)
            .color(color);
    }
}

/// A full room, or one already playing, is joined as a spectator.
fn watch_only(room: &RoomInfo) -> bool {
    room.ranked || (!room.friends_only && (room.players >= room.max || room.in_game))
}

fn draw_room_row(draw: &mut Draw, gui: &mut Gui, row: Rect, index: usize, room: &RoomInfo) {
    let pal = gui.ui.palette();
    let fonts = &mut gui.fonts;
    let mid = row.y + row.h / 2.0;
    let players = format!("{}/{} joueurs", room.players, room.max);
    let watchers = format!("{} spectateur{}", room.spectators, if room.spectators > 1 { "s" } else { "" });
    let mut pills = vec![Pill::small(&players, pal.accent)];
    if room.in_game {
        pills.push(Pill::small("En jeu", theme::WARNING));
    }
    if room.friends_only {
        pills.push(Pill::small("Amis", theme::SUCCESS));
    }
    if room.ranked {
        pills.push(Pill::small("Classé", theme::bar::ORANGE));
    }
    if room.spectators > 0 {
        pills.push(Pill::small(&watchers, pal.text_dim));
    }
    if watch_only(room) {
        pills.push(Pill::small("Regarder", theme::LINK));
    }

    gui.ui.row(draw, row, index, &format!("room:{}", room.id));
    fonts
        .write(draw, Face::Text, &format!("#{}", room.id))
        .left(row.x + 18.0, mid)
        .size(theme::size::BODY)
        .color(pal.text_muted);
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, row.x + row.w - 16.0, mid);
    let name_x = row.x + 90.0;
    let name = fonts.fit(Face::Display, &room.name, theme::size::EMPHASIS, pills_start - 20.0 - name_x);
    fonts.write(draw, Face::Display, &name).left(name_x, mid).size(theme::size::EMPHASIS).color(pal.text);
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

/// The outcome of a frame of a one-field screen.
enum Entry {
    Typing,
    Back,
    Confirmed(String),
}

fn update_entry(tick: &Tick, gui: &mut Gui, state: &mut State) -> Entry {
    let (w, h) = gui.ui.view().size();
    let (confirm, back) = entry_buttons(w, h);

    state.text_input.edit(&state.input.keys);
    field_clicked(&gui.ui, &mut gui.fonts, entry_box(w, h), &mut state.text_input);
    if gui.ui.clicked(confirm) || tick.pressed(KeyCode::Enter) {
        return Entry::Confirmed(state.text_input.trim().to_string());
    }
    if gui.ui.clicked(back) || tick.back() {
        return Entry::Back;
    }
    Entry::Typing
}

pub fn update_create_room(tick: &Tick, gui: &mut Gui, state: &mut State) -> Transition {
    match update_entry(tick, gui, state) {
        Entry::Back => return Transition::To(Screen::RoomBrowser),
        Entry::Confirmed(name) if !name.is_empty() => {
            state.notice.clear();
            state.net.conn.send(&ClientMessage::CreateRoom { name });
            state.text_input.clear();
        }
        _ => {}
    }
    Transition::Stay
}

pub fn update_join_by_id(tick: &Tick, gui: &mut Gui, state: &mut State) -> Transition {
    match update_entry(tick, gui, state) {
        Entry::Back => return Transition::To(Screen::RoomBrowser),
        Entry::Confirmed(text) => {
            if let Ok(id) = text.parse::<u32>() {
                state.notice.clear();
                state.net.conn.send(&ClientMessage::JoinRoom { id });
                state.text_input.clear();
            }
        }
        Entry::Typing => {}
    }
    Transition::Stay
}

fn draw_entry(draw: &mut Draw, gui: &mut Gui, state: &State, title: &str, confirm: &str, placeholder: &str) {
    let pal = gui.ui.palette();
    let (w, h) = gui.ui.view().size();
    let card = entry_card(w, h);
    let field = Field { placeholder, input: &state.text_input, focused: true };
    let (cbtn, back) = entry_buttons(w, h);

    let mut canvas = gui.ui.screen_canvas(draw);
    let draw = &mut *canvas;
    ui::card(draw, &pal, card);
    gui.fonts
        .write(draw, Face::Display, title)
        .center(card.x + card.w / 2.0, card.y + 58.0)
        .size(theme::size::TITLE)
        .color(pal.text);
    text_field(draw, &mut gui.ui, &mut gui.fonts, entry_box(w, h), &field);
    gui.ui.button(draw, &mut gui.fonts, cbtn, confirm);
    gui.ui.button(draw, &mut gui.fonts, back, "Retour");

    if let Some((msg, color)) = state.notice.shown(&pal) {
        gui.fonts
            .write(draw, Face::Text, msg)
            .center(card.x + card.w / 2.0, card.y + card.h + 40.0)
            .size(theme::size::EMPHASIS)
            .color(color);
    }
}

pub fn draw_create_room(draw: &mut Draw, gui: &mut Gui, state: &State) {
    draw_entry(draw, gui, state, "Créer une room", "Créer", "Nom de la room...");
}

pub fn draw_join_by_id(draw: &mut Draw, gui: &mut Gui, state: &State) {
    draw_entry(draw, gui, state, "Rejoindre par ID", "Rejoindre", "Numéro de la room...");
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tile {
    Setting(RoomSetting),
    /// The board's columns and rows, changed in their own view.
    Size,
}

/// The lobby's settings, by column: the game played, then the room.
const LOBBY_SETTINGS: [(&str, &[Tile]); 2] = [
    ("Partie", &[Tile::Setting(RoomSetting::StartingLevel), Tile::Setting(RoomSetting::Colors), Tile::Size]),
    (
        "Room",
        &[
            Tile::Setting(RoomSetting::MaxPlayers),
            Tile::Setting(RoomSetting::Pause),
            Tile::Setting(RoomSetting::FriendsOnly),
        ],
    ),
];

fn lobby_panel(view: View) -> SettingsGrid {
    let rows = LOBBY_SETTINGS.iter().map(|(_, s)| s.len()).max().unwrap_or(0);
    SettingsGrid::new(view, rows)
}

/// Each setting of the lobby with its tile.
fn lobby_tiles(view: View) -> impl Iterator<Item = (Tile, Rect)> {
    let grid = lobby_panel(view);
    LOBBY_SETTINGS.into_iter().enumerate().flat_map(move |(column, (_, settings))| {
        settings.iter().enumerate().map(move |(row, &setting)| (setting, grid.tile(column, row)))
    })
}

fn lobby_size_button(view: View) -> Rect {
    lobby_tiles(view)
        .find_map(|(t, rect)| (t == Tile::Size).then(|| Stepper::tile_band(rect)))
        .expect("the lobby has a size tile")
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
const MIN_PLAYER_ROW_H: f32 = 34.0;
const PLAYER_ROW_GAP: f32 = 4.0;
const ADD_CPU_H: f32 = 40.0;
const SCROLL_BUTTON: f32 = 32.0;

/// The players' rows: as many as fit, the others reached by scrolling.
struct PlayerList {
    area: Rect,
    step: f32,
    first: usize,
    shown: usize,
    count: usize,
    title: Rect,
}

impl PlayerList {
    fn new(view: View, info: &LobbyInfo, first: usize) -> Self {
        let card = lobby_players(view);
        let below = if may_add_cpu(info) { ADD_CPU_H + 24.0 } else { 12.0 };
        let area = card.below(PLAYERS_TITLE_H).pad(0.0, 12.0, below, 12.0);
        let count = info.names.len();
        let step = (area.h / count.max(1) as f32).clamp(MIN_PLAYER_ROW_H, PLAYER_ROW_H);
        let shown = area.rows_fitting(step - PLAYER_ROW_GAP, PLAYER_ROW_GAP).clamp(1, count.max(1));

        Self {
            area,
            step,
            first: first.min(count.saturating_sub(shown)),
            shown,
            count,
            title: card.top(PLAYERS_TITLE_H),
        }
    }

    /// Row `i`, if it is in sight.
    fn row(&self, i: usize) -> Option<Rect> {
        (self.first..self.first + self.shown)
            .contains(&i)
            .then(|| self.area.nth_row(i - self.first, self.step - PLAYER_ROW_GAP, PLAYER_ROW_GAP))
    }

    fn scrolls(&self) -> bool {
        self.count > self.shown
    }

    /// The buttons that scroll up and down, right of the title.
    fn scroll_buttons(&self) -> [Rect; 2] {
        let band = self.title.pad(0.0, 12.0, 0.0, 0.0).middle(SCROLL_BUTTON);
        let down = band.right(SCROLL_BUTTON);
        [Rect::at(down.x - SCROLL_BUTTON - 6.0, down.y, SCROLL_BUTTON, SCROLL_BUTTON), down]
    }

    /// The first row after scrolling by `dir` rows.
    fn scrolled(&self, dir: i32) -> usize {
        let last = self.count.saturating_sub(self.shown);
        self.first.saturating_add_signed(dir as isize).min(last)
    }
}

fn add_cpu_button(view: View) -> Rect {
    lobby_players(view).pad(0.0, 12.0, 12.0, 12.0).bottom_band(ADD_CPU_H).centered(220.0, ADD_CPU_H)
}

/// Whether the host may seat, change and remove CPUs now.
fn edits_cpus(info: &LobbyInfo) -> bool {
    info.cpu_seats && info.is_host && info.countdown.is_none() && info.ranked.is_none()
}

fn may_add_cpu(info: &LobbyInfo) -> bool {
    edits_cpus(info) && info.names.len() < usize::from(info.settings.max_players)
}

/// The CPU in lobby seat `i`, if a CPU plays it.
fn cpu_seat_at(info: &LobbyInfo, i: usize) -> Option<CpuSeat> {
    let (difficulty, style) = info.cpus.get(i).copied().flatten()?;
    Some(CpuSeat { slot: u8::try_from(i + 1).ok()?, difficulty, style, team: info.teams.get(i).copied().flatten() })
}

/// One's own row, while one may still change team on it.
fn team_picker(info: &LobbyInfo) -> Option<usize> {
    let open = info.ranked.is_none() && info.countdown.is_none();
    usize::from(info.your_slot).checked_sub(1).filter(|_| open)
}

/// Solo, then each team in turn, then Solo again, the other way for `dir` < 0.
pub fn step_team(team: Option<u8>, dir: i32) -> Option<u8> {
    let teams: Vec<_> = std::iter::once(None).chain((1..=shared::MAX_TEAMS).map(Some)).collect();
    ui::cycle(&teams, team, dir)
}

pub fn team_look(team: Option<u8>, pal: &theme::Palette) -> (&'static str, Color) {
    team.and_then(theme::team).unwrap_or(("Solo", pal.text_dim))
}

/// Whether the players make two sides at least: one without a team is a side.
fn has_sides(info: &LobbyInfo) -> bool {
    let mut teams = info.teams.iter();
    let Some(first) = teams.next() else { return false };
    first.is_none() && info.teams.len() > 1 || teams.any(|t| t.is_none() || t != first)
}

fn draw_lobby_players(draw: &mut Draw, ui: &mut Ui, fonts: &mut Fonts, state: &State, info: &LobbyInfo) {
    let pal = ui.palette();
    let view = ui.view();
    let card = lobby_players(view);
    if card.w < 160.0 {
        return;
    }

    let list = PlayerList::new(view, info, state.room.as_ref().map_or(0, |r| r.players_first));
    let own = usize::from(info.your_slot).checked_sub(1);
    let picker = team_picker(info);

    ui::card(draw, &pal, card);
    fonts
        .write(draw, Face::Display, "Joueurs")
        .left(card.x + 16.0, card.y + PLAYERS_TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .color(pal.text_dim);
    ui::divider(draw, &pal, card.x + 16.0, card.y + PLAYERS_TITLE_H, card.w - 32.0);
    if list.scrolls() {
        let [up, down] = list.scroll_buttons();
        ui.button_enabled(draw, fonts, up, Icon::Up, list.first > 0);
        ui.button_enabled(draw, fonts, down, Icon::Down, list.first + list.shown < list.count);
    }

    for (i, name) in info.names.iter().enumerate() {
        let Some(row) = list.row(i) else { continue };

        let (label, color) = team_look(info.teams.get(i).copied().flatten(), &pal);
        let pill = Pill::small(label, color);
        let pill_w = pill.width(fonts);
        let name = fonts.fit(Face::Text, name, theme::size::BODY, row.w - pill_w - 36.0);
        let ink = if own == Some(i) { pal.text } else { pal.text_dim };

        if picker == Some(i) {
            ui.row(draw, row, i, &format!("team:{i}"));
        } else if edits_cpus(info) && cpu_seat_at(info, i).is_some() {
            ui.row(draw, row, i, &format!("cpu:{i}"));
        } else {
            list_row(draw, &pal, row, i);
        }
        fonts.write(draw, Face::Text, &name).left(row.x + 10.0, row.y + row.h / 2.0).size(theme::size::BODY).color(ink);
        pill.draw(draw, fonts, (row.x + row.w - 10.0 - pill_w, row.y + row.h / 2.0));
    }

    if may_add_cpu(info) {
        ui.button(draw, fonts, add_cpu_button(view), "Ajouter un CPU");
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

/// The friend picked in the invitation window, which then closes.
fn update_invite(
    tick: &Tick,
    ui: &Ui,
    fonts: &mut Fonts,
    keys: &EditKeys,
    invite: &mut Invite,
    view: View,
) -> Option<String> {
    if invite_modal(view).dismissed(ui) || tick.back() {
        invite.open = false;
        return None;
    }

    let per_page = visible_invites(view);
    let pages = ui::page_count(invite.friends.len(), per_page);
    let loaded = invite.slot.is_none();
    invite.pager.clamp(pages);
    invite.pager.update(tick, ui, fonts, keys, invite_pager_area(view), pages);

    let invited = invite
        .pager
        .shown(&invite.friends, per_page)
        .find(|&(i, _)| loaded && ui.clicked(invite_button(view, i)))
        .map(|(_, friend)| friend.user_id.clone());
    if invited.is_some() {
        invite.open = false;
    }
    invited
}

/// Runs the editor of the CPU in seat `i`, opened with `seats` taken; returns whether it stays open.
fn update_cpu_edit(tick: &Tick, ui: &Ui, info: &LobbyInfo, (i, seats): (usize, usize), conn: &mut Connection) -> bool {
    let Some(seat) = cpu_seat_at(info, i).filter(|_| edits_cpus(info) && seats == info.names.len()) else {
        return false;
    };
    match cpu_seat::update(tick, ui, seat) {
        Some(cpu_seat::Action::Close) => false,
        Some(cpu_seat::Action::Send(msg)) => {
            conn.send(&msg);
            !matches!(msg, ClientMessage::RemoveCpu { .. })
        }
        None => true,
    }
}

/// Which way the players list scrolls this frame, by its buttons or the wheel.
fn list_scroll(tick: &Tick, ui: &Ui, view: View, list: &PlayerList) -> Option<i32> {
    if !list.scrolls() {
        return None;
    }

    let [up, down] = list.scroll_buttons();
    let wheel = tick.input.wheel().y;
    let over = lobby_players(view).contains(ui.mouse().x, ui.mouse().y);
    if ui.clicked(up) || (over && wheel > 0.0) {
        Some(-1)
    } else if ui.clicked(down) || (over && wheel < 0.0) {
        Some(1)
    } else {
        None
    }
}

/// The setting a click changes, and which way.
fn clicked_setting(ui: &Ui, view: View) -> Option<(RoomSetting, i32)> {
    lobby_tiles(view).find_map(|(tile, rect)| {
        let Tile::Setting(setting) = tile else { return None };
        let stepper = Stepper::in_tile(rect);
        let dir = if setting == RoomSetting::FriendsOnly {
            ui.clicked(Stepper::tile_band(rect)).then_some(1)?
        } else if ui.clicked(stepper.minus) {
            -1
        } else if ui.clicked(stepper.plus) {
            1
        } else {
            return None;
        };
        Some((setting, dir))
    })
}

pub fn update_lobby(tick: &Tick, gui: &mut Gui, state: &mut State) {
    let view = gui.ui.view();
    let Gui { ui, fonts, .. } = gui;
    let State { room, auth, input: Input { keys, .. }, net: Network { conn, maintenance, .. }, .. } = state;
    let Some(room) = room.as_mut() else { return };
    let info = &room.info;
    let editable = info.is_host && info.countdown.is_none();

    if let Some(Ok(data)) = http::take_json::<ApiFriendsResponse>(&mut room.invite.slot) {
        room.invite.friends = data.friends;
    }

    if room.invite.open {
        if let Some(user_id) = update_invite(tick, ui, fonts, keys, &mut room.invite, view) {
            conn.send(&ClientMessage::InviteFriend { user_id });
        }
        return;
    }

    if info.countdown.is_some() {
        room.sizing = false;
        room.cpu_edit = None;
    }
    if room.sizing {
        match board_size::update(tick, ui, &info.settings.rules, editable) {
            Some(board_size::Action::Close) => room.sizing = false,
            Some(board_size::Action::Adjust(steps)) => {
                for (setting, dir) in steps {
                    conn.send(&ClientMessage::SetRoomSetting { setting, dir });
                }
            }
            None => {}
        }
        return;
    }

    if let Some(edit) = room.cpu_edit {
        if !update_cpu_edit(tick, ui, info, edit, conn) {
            room.cpu_edit = None;
        }
        return;
    }

    let list = PlayerList::new(view, info, room.players_first);
    room.players_first = list.first;
    if let Some(dir) = list_scroll(tick, ui, view, &list) {
        room.players_first = list.scrolled(dir);
        return;
    }
    let clicked_row = |i: usize| list.row(i).is_some_and(|row| ui.clicked(row));

    if edits_cpus(info) {
        let seats = info.names.len();
        let clicked = (0..seats).find(|&i| cpu_seat_at(info, i).is_some() && clicked_row(i));
        if let Some(i) = clicked {
            room.cpu_edit = Some((i, seats));
            return;
        }
        if may_add_cpu(info) && ui.clicked(add_cpu_button(view)) {
            conn.send(&ClientMessage::AddCpu);
            return;
        }
    }

    if let Some(me) = team_picker(info) {
        if clicked_row(me) {
            let team = step_team(info.teams.get(me).copied().flatten(), 1);
            conn.send(&ClientMessage::SetTeam { team });
            return;
        }
    }

    let chatting = room.chat.focused;
    if let Some(text) = room.chat.update_lobby(tick, keys) {
        conn.send(&ClientMessage::Chat { text });
    }
    room.chat.click(ui, fonts, lobby_chat(view));

    if ui.clicked(lobby_size_button(view)) {
        room.sizing = true;
        return;
    }

    if let Some((setting, dir)) = clicked_setting(ui, view).filter(|_| editable) {
        conn.send(&ClientMessage::SetRoomSetting { setting, dir });
        return;
    }

    let (_, _, launch_enabled) = launch_bar(info, *maintenance);
    if launch_enabled && ui.bar_clicked(lobby_launch(view)) {
        let msg =
            if info.your_slot == 0 { ClientMessage::JoinRoom { id: info.id } } else { ClientMessage::ToggleCountdown };
        conn.send(&msg);
        return;
    }

    let escape = !chatting && tick.back();
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

pub fn draw_lobby(draw: &mut Draw, gui: &mut Gui, state: &State) {
    let Gui { ui, fonts, .. } = gui;
    let pal = ui.palette();
    let Some(room) = &state.room else {
        return;
    };
    let info = &room.info;
    let view = ui.view();
    let editable = info.is_host && info.countdown.is_none();
    let (label, color, enabled) = launch_bar(info, state.net.maintenance);
    let editing = room.cpu_edit.filter(|&(_, seats)| seats == info.names.len());

    let mut canvas = ui.screen_canvas(draw);
    let draw = &mut *canvas;
    ui.set_input(!room.invite.open && !room.sizing && room.cpu_edit.is_none());
    draw_lobby_header(draw, ui, fonts, info);
    draw_lobby_players(draw, ui, fonts, state, info);

    lobby_panel(view).draw(draw, &pal, fonts, "Réglages de la room", LOBBY_SETTINGS.map(|(section, _)| section));
    for (setting, tile) in lobby_tiles(view) {
        let Tile::Setting(setting) = setting else {
            let size = board_size::label(&info.settings.rules);
            ui.button_tile(draw, fonts, tile, "Taille", &size);
            continue;
        };
        let label = setting_label(setting);
        if setting == RoomSetting::FriendsOnly {
            let on = info.settings.friends_only;
            ui.switch_tile(draw, fonts, tile, label, on, editable);
        } else {
            let value = setting_value(&info.settings, setting);
            ui.stepper_tile(draw, fonts, tile, label, &value, editable);
        }
    }

    ui.menu_bar_enabled(draw, fonts, lobby_launch(view), label, color, enabled);
    if info.your_slot != 0 && state.auth.is_some() {
        ui.menu_bar(draw, fonts, lobby_invite(view), "Inviter un ami", theme::bar::BLUE);
    }
    room.chat.draw_card(draw, ui, fonts, lobby_chat(view));
    ui.menu_bar(draw, fonts, lobby_leave(view), "Quitter la room", theme::bar::RED);

    if let Some(n) = info.countdown {
        draw.rect(Rect::at(0.0, 0.0, view.w, view.h)).color(theme::SCRIM);
        fonts
            .write(draw, Face::Display, &n.to_string())
            .center(view.w / 2.0, view.h / 2.0)
            .size(theme::size::HUGE)
            .color(theme::GOLD);
    }

    if room.invite.open {
        draw_invite_overlay(draw, ui, fonts, &room.invite);
    }
    if room.sizing {
        board_size::draw(draw, ui, fonts, &info.settings.rules, editable);
    }
    if let Some(seat) = editing.and_then(|(i, _)| cpu_seat_at(info, i)) {
        cpu_seat::draw(draw, ui, fonts, seat);
    }
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

fn draw_lobby_header(draw: &mut Draw, ui: &mut Ui, fonts: &mut Fonts, info: &LobbyInfo) {
    let pal = ui.palette();
    let view = ui.view();
    let mid = theme::HEADER_H / 2.0;
    let id = format!("Room #{}", info.id);
    let players = format!("{}/{} joueurs", info.players, info.settings.max_players);
    let watchers = format!("{} spectateur{}", info.spectators, if info.spectators > 1 { "s" } else { "" });
    let away = info.players.saturating_sub(info.connected);
    let away_text = format!("{away} déconnecté");
    let mut pills = vec![Pill::small(&id, pal.text_dim), Pill::small(&players, pal.accent)];
    if away > 0 {
        pills.push(Pill::small(&away_text, theme::WARNING));
    }
    if info.spectators > 0 {
        pills.push(Pill::small(&watchers, pal.text_dim));
    }

    ui.header_band(draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, view.w - 60.0, mid);
    let name = fonts.fit(Face::Display, &info.name, theme::size::TITLE, pills_start - 30.0 - 60.0);
    fonts.write(draw, Face::Display, &name).left(60.0, mid).size(theme::size::TITLE).color(pal.text);
}

fn draw_invite_overlay(draw: &mut Draw, ui: &mut Ui, fonts: &mut Fonts, invite: &Invite) {
    let pal = ui.palette();
    let view = ui.view();
    let modal = invite_modal(view);
    let card = modal.card;
    let message = if invite.slot.is_some() {
        Some("Chargement...")
    } else if invite.friends.is_empty() {
        Some("Aucun ami pour l'instant.")
    } else {
        None
    };

    modal.draw(draw, ui, fonts, "Inviter un ami");
    if let Some(message) = message {
        fonts
            .write(draw, Face::Text, message)
            .center(card.x + card.w / 2.0, card.y + 110.0)
            .size(theme::size::EMPHASIS)
            .color(pal.text_muted);
        return;
    }

    let per_page = visible_invites(view);
    let pages = ui::page_count(invite.friends.len(), per_page);
    if pages > 1 {
        invite.pager.draw(draw, ui, fonts, invite_pager_area(view), pages);
    }
    for (i, friend) in invite.pager.shown(&invite.friends, per_page) {
        let row = invite_row(view, i);

        list_row(draw, &pal, row, i);
        fonts
            .write(draw, Face::Text, &friend.username)
            .left(row.x + 16.0, row.y + row.h / 2.0)
            .size(theme::size::LABEL)
            .color(pal.text);
        ui.button(draw, fonts, invite_button(view, i), "Inviter");
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
            cpu_seats: true,
            cpus: vec![None; teams.len()],
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
    fn a_full_room_scrolls_its_players_instead_of_squeezing_them() {
        let info = lobby(vec![None; usize::from(shared::MAX_ROOM_PLAYERS)]);
        let view = View::default();
        let card = lobby_players(view);
        let mut list = PlayerList::new(view, &info, 0);
        assert!(list.scrolls() && list.shown >= 3, "{} rows in sight", list.shown);
        let mut seen = std::collections::HashSet::new();
        while seen.len() < info.names.len() {
            for i in 0..info.names.len() {
                let Some(row) = list.row(i) else { continue };
                assert!(row.h >= MIN_PLAYER_ROW_H - PLAYER_ROW_GAP, "row {i} squeezed to {}", row.h);
                assert!(row.y >= card.y && row.bottom() <= card.bottom(), "row {i} out of the card");
                seen.insert(i);
            }
            let next = list.scrolled(1);
            assert!(next > list.first || seen.len() == info.names.len(), "stuck at {}", list.first);
            list = PlayerList::new(view, &info, next);
        }
    }

    #[test]
    fn the_team_button_goes_round() {
        let mut team = None;
        let mut seen = Vec::new();
        for _ in 0..=shared::MAX_TEAMS {
            team = step_team(team, 1);
            seen.push(team);
        }
        assert_eq!(seen, [Some(1), Some(2), Some(3), Some(4), None]);
    }
}
