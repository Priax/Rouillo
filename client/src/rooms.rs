use aero::draw::Draw;
use aero::edit::TextField;
use aero::geom::Color;
use aero::input::KeyCode;
use aero::ui::Sense;
use shared::api::Uuid;
use shared::{config, ClientMessage, LobbyInfo, PausePolicy, RoomInfo, RoomSetting, RoomSettings};

use crate::connection::Connection;
use crate::cpu_seat::{self, CpuSeat};
use crate::nav::Transition;
use crate::state::{Invite, Network, Room, Screen, State};
use crate::tick::Tick;
use crate::ui::{
    self, list_row, px, widgets, Face, Fonts, Frame, Gui, Icon, ListCard, Modal, Pill, Rect, SettingsGrid, Typeset,
    View, FILL, PAGER_H,
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
    let bw = ((tools.w() - 2.0 * gap) / 3.0).min(190.0);
    let [create, join_id, refresh] = tools.right_strip(3.0 * bw + 2.0 * gap).columns(gap, [px(bw); 3]);
    BrowserButtons { create, join_id, refresh, back: frame.back }
}

/// What the room list does with the keys; its clicks are in [`show_browser`].
pub fn update_browser(tick: &Tick, gui: &mut Gui, state: &mut State) -> Transition {
    let view = gui.stage.view();
    let pages = room_pages(&state.rooms, view);
    state.room_pager.clamp(pages);
    if tick.back() {
        return leave_browser(state);
    }
    Transition::Stay
}

fn leave_browser(state: &mut State) -> Transition {
    state.net.conn.disconnect();
    state.rooms.clear();
    Transition::To(Screen::PlayMenu)
}

pub fn show_browser(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let pal = gui.stage.palette();
    let view = gui.stage.view();
    let card = room_list(view).card;
    let pages = room_pages(&state.rooms, view);
    let b = browser_buttons(view);

    let mut canvas = gui.stage.screen_canvas(ui);
    let ui = &mut *canvas;
    gui.stage.page_header(ui, "Rooms");

    ui::card(ui.draw, &pal, card);
    if state.rooms.is_empty() {
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, "Aucune room. Créez-en une !")
            .center(card.x() + card.w() / 2.0, card.y() + 60.0)
            .size(theme::size::EMPHASIS)
            .color(pal.text_muted);
    }
    let mut chosen = None;
    for (i, room) in shown_rooms(view, state) {
        let row = room_list(view).row(i);
        if widgets::row(ui, gui, ("room", room.id), row, i) {
            chosen = Some((room.id, watch_only(room)));
        }
        draw_room_row(ui.draw, &mut ui.cx.fonts, gui, row, room);
    }
    if pages > 1 {
        state.room_pager.show(ui, gui, "pages", room_list(view).pager(), pages);
    }

    let create = widgets::button(ui, "create", b.create, "Créer une room", true);
    let join_id = widgets::button(ui, "join id", b.join_id, "Rejoindre par ID", true);
    let refresh = widgets::button(ui, "refresh", b.refresh, "Rafraîchir", true);
    let back = widgets::button(ui, "back", b.back, "Retour", true);
    if let Some((msg, color)) = state.notice.shown(&pal) {
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, msg)
            .center(view.w / 2.0, b.back.y() + b.back.h() / 2.0)
            .size(theme::size::EMPHASIS)
            .color(color);
    }

    if let Some((id, watch)) = chosen {
        state.notice.clear();
        let msg = if watch { ClientMessage::Spectate { id } } else { ClientMessage::JoinRoom { id } };
        state.net.conn.send(&msg);
    } else if create {
        state.text_input.clear();
        state.notice.clear();
        return Transition::To(Screen::CreateRoom);
    } else if join_id {
        state.text_input.clear();
        state.notice.clear();
        return Transition::To(Screen::JoinById);
    } else if refresh {
        state.net.conn.send(&ClientMessage::RequestRoomList);
    } else if back {
        return leave_browser(state);
    }
    Transition::Stay
}

/// A full room, or one already playing, is joined as a spectator.
fn watch_only(room: &RoomInfo) -> bool {
    room.ranked || (!room.friends_only && (room.players >= room.max || room.in_game))
}

/// What a room's row holds, drawn over the row itself.
fn draw_room_row(draw: &mut Draw, fonts: &mut Fonts, gui: &mut Gui, row: Rect, room: &RoomInfo) {
    let pal = gui.stage.palette();
    let mid = row.y() + row.h() / 2.0;
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

    fonts
        .write(draw, Face::Text, &format!("#{}", room.id))
        .left(row.x() + 18.0, mid)
        .size(theme::size::BODY)
        .color(pal.text_muted);
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, row.x() + row.w() - 16.0, mid);
    let name_x = row.x() + 90.0;
    let name = fonts.fit(Face::Display, &room.name, theme::size::EMPHASIS, pills_start - 20.0 - name_x);
    fonts.write(draw, Face::Display, &name).left(name_x, mid).size(theme::size::EMPHASIS).color(pal.text);
}

fn entry_card(w: f32, h: f32) -> Rect {
    Rect::at(0.0, 0.0, w, h).centered(560.0, 300.0)
}

fn entry_box(w: f32, h: f32) -> Rect {
    entry_card(w, h).inset([110.0, 40.0, 0.0, 40.0]).top_strip(50.0)
}

fn entry_buttons(w: f32, h: f32) -> (Rect, Rect) {
    let band = entry_card(w, h).inset([0.0, 40.0, 40.0, 40.0]).bottom_strip(56.0);
    let [confirm, back] = band.columns(20.0, [FILL, FILL]);
    (confirm, back)
}

/// The outcome of a frame of a one-field screen.
enum Entry {
    Typing,
    Back,
    Confirmed(String),
}

/// Escape goes back from a one-field screen; the field and its Enter are in [`show_entry`].
pub fn update_entry(tick: &Tick) -> Transition {
    if tick.back() {
        return Transition::To(Screen::RoomBrowser);
    }
    Transition::Stay
}

fn create_room(entry: Entry, state: &mut State) -> Transition {
    match entry {
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

fn join_by_id(entry: Entry, state: &mut State) -> Transition {
    match entry {
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

/// Draws a one-field screen and tells what its buttons asked.
fn show_entry(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    state: &mut State,
    (title, confirm): (&str, &str),
    field: TextField<'_>,
) -> Entry {
    let pal = gui.stage.palette();
    let (w, h) = gui.stage.view().size();
    let card = entry_card(w, h);
    let (cbtn, back) = entry_buttons(w, h);

    let mut canvas = gui.stage.screen_canvas(ui);
    let ui = &mut *canvas;
    ui::card(ui.draw, &pal, card);
    ui.cx
        .fonts
        .write(ui.draw, Face::Display, title)
        .center(card.x() + card.w() / 2.0, card.y() + 58.0)
        .size(theme::size::TITLE)
        .color(pal.text);
    if !ui.cx.wants_keyboard() {
        widgets::focus_field(ui, "entry");
    }
    let typed = widgets::text_field(ui, gui, "entry", entry_box(w, h), field, &mut state.text_input);
    let confirmed = widgets::button(ui, "confirm", cbtn, confirm, true) || typed.submitted;
    let back = widgets::button(ui, "back", back, "Retour", true);
    if let Some((msg, color)) = state.notice.shown(&pal) {
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, msg)
            .center(card.x() + card.w() / 2.0, card.y() + card.h() + 40.0)
            .size(theme::size::EMPHASIS)
            .color(color);
    }
    if confirmed {
        Entry::Confirmed(state.text_input.trim().to_string())
    } else if back {
        Entry::Back
    } else {
        Entry::Typing
    }
}

pub fn show_create_room(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let field = widgets::field().placeholder("Nom de la room...").max_chars(24);
    let entry = show_entry(ui, gui, state, ("Créer une room", "Créer"), field);
    create_room(entry, state)
}

pub fn show_join_by_id(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let field = widgets::field().placeholder("Numéro de la room...").max_chars(9).accepts(|c| c.is_ascii_digit());
    let entry = show_entry(ui, gui, state, ("Rejoindre par ID", "Rejoindre"), field);
    join_by_id(entry, state)
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
    let beside = Rect::at(0.0, card.y(), card.x(), card.h()).inset([0.0, 24.0, 0.0, 24.0]);
    beside.right_strip(beside.w().min(460.0))
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
        let area = card.below(PLAYERS_TITLE_H).inset([0.0, 12.0, below, 12.0]);
        let count = info.names.len();
        let step = (area.h() / count.max(1) as f32).clamp(MIN_PLAYER_ROW_H, PLAYER_ROW_H);
        let shown = area.rows_fitting(step - PLAYER_ROW_GAP, PLAYER_ROW_GAP).clamp(1, count.max(1));

        Self {
            area,
            step,
            first: first.min(count.saturating_sub(shown)),
            shown,
            count,
            title: card.top_strip(PLAYERS_TITLE_H),
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
        let band = self.title.inset([0.0, 12.0, 0.0, 0.0]).middle(SCROLL_BUTTON);
        let down = band.right_strip(SCROLL_BUTTON);
        [Rect::at(down.x() - SCROLL_BUTTON - 6.0, down.y(), SCROLL_BUTTON, SCROLL_BUTTON), down]
    }

    /// The first row after scrolling by `dir` rows.
    fn scrolled(&self, dir: i32) -> usize {
        let last = self.count.saturating_sub(self.shown);
        self.first.saturating_add_signed(dir as isize).min(last)
    }
}

fn add_cpu_button(view: View) -> Rect {
    lobby_players(view).inset([0.0, 12.0, 12.0, 12.0]).bottom_strip(ADD_CPU_H).centered(220.0, ADD_CPU_H)
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

/// What a click in the players card asks.
enum PlayersClick {
    Scroll(i32),
    Row(usize),
    AddCpu,
}

fn show_players(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, info: &LobbyInfo, first: usize) -> Option<PlayersClick> {
    let pal = gui.stage.palette();
    let view = gui.stage.view();
    let card = lobby_players(view);
    if card.w() < 160.0 {
        return None;
    }

    let list = PlayerList::new(view, info, first);
    let own = usize::from(info.your_slot).checked_sub(1);
    let picker = team_picker(info);
    let mut clicked = None;
    let wheel = ui.interact(ui.id("players wheel"), card, Sense::SCROLL).wheel;

    ui::card(ui.draw, &pal, card);
    ui.cx
        .fonts
        .write(ui.draw, Face::Display, "Joueurs")
        .left(card.x() + 16.0, card.y() + PLAYERS_TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .color(pal.text_dim);
    ui::divider(ui.draw, &pal, card.x() + 16.0, card.y() + PLAYERS_TITLE_H, card.w() - 32.0);
    if list.scrolls() {
        if let Some((delta, _)) = wheel.filter(|(delta, _)| delta.y != 0.0) {
            clicked = Some(PlayersClick::Scroll(if delta.y > 0.0 { -1 } else { 1 }));
        }
        let [up, down] = list.scroll_buttons();
        if widgets::button(ui, "players up", up, Icon::Up, list.first > 0) {
            clicked = Some(PlayersClick::Scroll(-1));
        }
        if widgets::button(ui, "players down", down, Icon::Down, list.first + list.shown < list.count) {
            clicked = Some(PlayersClick::Scroll(1));
        }
    }

    for (i, name) in info.names.iter().enumerate() {
        let Some(row) = list.row(i) else { continue };

        let (label, color) = team_look(info.teams.get(i).copied().flatten(), &pal);
        let pill = Pill::small(label, color);
        let pill_w = pill.width(&mut ui.cx.fonts);
        let name = ui.cx.fonts.fit(Face::Text, name, theme::size::BODY, row.w() - pill_w - 36.0);
        let ink = if own == Some(i) { pal.text } else { pal.text_dim };

        let clickable = picker == Some(i) || edits_cpus(info) && cpu_seat_at(info, i).is_some();
        if clickable {
            if widgets::row(ui, gui, ("player", i), row, i) {
                clicked = Some(PlayersClick::Row(i));
            }
        } else {
            list_row(ui.draw, &pal, row, i);
        }
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, &name)
            .left(row.x() + 10.0, row.y() + row.h() / 2.0)
            .size(theme::size::BODY)
            .color(ink);
        pill.draw(ui.draw, &mut ui.cx.fonts, (row.x() + row.w() - 10.0 - pill_w, row.y() + row.h() / 2.0));
    }

    if may_add_cpu(info) && widgets::button(ui, "add cpu", add_cpu_button(view), "Ajouter un CPU", true) {
        clicked = Some(PlayersClick::AddCpu);
    }
    clicked
}

fn lobby_chat(view: View) -> Rect {
    let card = lobby_panel(view).card;
    let beside = Rect::at(0.0, card.y(), view.w, card.h()).inset([0.0, 24.0, 0.0, card.x() + card.w() + 24.0]);
    beside.left_strip(beside.w().min(460.0))
}

fn invite_modal(view: View) -> Modal {
    Modal::new(view, 600.0)
}

fn invite_list(view: View) -> Rect {
    invite_modal(view).card.inset([70.0, 16.0, PAGER_H + 22.0, 16.0])
}

fn visible_invites(view: View) -> usize {
    invite_list(view).rows_fitting(INVITE_ROW_H - 6.0, 6.0).max(1)
}

fn invite_pager_area(view: View) -> Rect {
    invite_modal(view).card.above(8.0).bottom_strip(PAGER_H)
}

fn invite_row(view: View, i: usize) -> Rect {
    invite_list(view).nth_row(i, INVITE_ROW_H - 6.0, 6.0)
}

fn invite_button(view: View, i: usize) -> Rect {
    invite_row(view, i).inset([0.0, 8.0, 0.0, 0.0]).right_strip(120.0).middle(32.0)
}

/// What the lobby does with the keys and the server's answers; its clicks are in [`show_lobby`].
pub fn update_lobby(tick: &Tick, gui: &mut Gui, state: &mut State) {
    let view = gui.stage.view();
    let State { room, net: Network { conn, .. }, .. } = state;
    let Some(room) = room.as_mut() else { return };
    let info = &room.info;
    let editable = info.is_host && info.countdown.is_none();

    if let Some(Ok(data)) = http::take(&mut room.invite.slot) {
        room.invite.friends = data.friends;
    }
    if room.invite.open {
        let pages = ui::page_count(room.invite.friends.len(), visible_invites(view));
        room.invite.pager.clamp(pages);
        if tick.back() {
            room.invite.open = false;
        }
        return;
    }
    if info.countdown.is_some() {
        room.sizing = false;
        room.cpu_edit = None;
    }
    if room.sizing {
        let action = board_size::update_keys(tick, editable);
        apply_size(room, conn, action);
        return;
    }
    if room.cpu_edit.is_some() {
        if cpu_seat::update_keys(tick).is_some() {
            room.cpu_edit = None;
        }
        return;
    }

    let list = PlayerList::new(view, info, room.players_first);
    room.players_first = list.first;

    if tick.typing() {
        if tick.back() {
            room.chat.release_keyboard();
        }
    } else if tick.pressed(KeyCode::Enter) || tick.pressed(KeyCode::NumpadEnter) {
        room.chat.take_keyboard();
    } else if tick.back() {
        leave_room(room, conn);
    }
}

fn apply_size(room: &mut Room, conn: &mut Connection, action: Option<board_size::Action>) {
    match action {
        Some(board_size::Action::Close) => room.sizing = false,
        Some(board_size::Action::Adjust(steps)) => {
            for (setting, dir) in steps {
                conn.send(&ClientMessage::SetRoomSetting { setting, dir });
            }
        }
        None => {}
    }
}

fn leave_room(room: &mut Room, conn: &mut Connection) {
    conn.send(&ClientMessage::LeaveRoom);
    room.invite = Invite::default();
}

fn open_invite(invite: &mut Invite, token: Option<String>) {
    invite.open = true;
    if invite.friends.is_empty() && invite.slot.is_none() {
        let slot = http::get(http::api_url("friends"), token);
        invite.slot = Some(slot.json());
    }
}

/// Runs the editor of the CPU in seat `i`, opened with `seats` taken; returns whether it stays open.
fn edit_cpu(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    info: &LobbyInfo,
    (i, seats): (usize, usize),
    conn: &mut Connection,
) -> bool {
    let Some(seat) = cpu_seat_at(info, i).filter(|_| edits_cpus(info) && seats == info.names.len()) else {
        return false;
    };
    match cpu_seat::show(ui, gui, seat) {
        Some(cpu_seat::Action::Close) => false,
        Some(cpu_seat::Action::Send(msg)) => {
            conn.send(&msg);
            !matches!(msg, ClientMessage::RemoveCpu { .. })
        }
        None => true,
    }
}

/// The settings' tiles: whether the size was clicked, and the setting a click steps.
fn show_tiles(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    info: &LobbyInfo,
    editable: bool,
) -> (bool, Option<(RoomSetting, i32)>) {
    let view = gui.stage.view();
    let mut sizing = false;
    let mut setting = None;
    for (tile, rect) in lobby_tiles(view) {
        let Tile::Setting(which) = tile else {
            let size = board_size::label(&info.settings.rules);
            sizing |= widgets::button_tile(ui, gui, "size", rect, ("Taille", &size));
            continue;
        };
        let label = setting_label(which);
        if which == RoomSetting::FriendsOnly {
            let on = info.settings.friends_only;
            if widgets::switch_tile(ui, gui, ("setting", label), rect, (label, on), editable) {
                setting = Some((which, 1));
            }
        } else {
            let value = setting_value(&info.settings, which);
            let step = widgets::stepper_tile(ui, gui, ("setting", label), rect, (label, &value), editable);
            if step != 0 {
                setting = Some((which, step));
            }
        }
    }

    (sizing, setting)
}

pub fn show_lobby(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let pal = gui.stage.palette();
    let view = gui.stage.view();
    let State { room, auth, net: Network { conn, maintenance, .. }, .. } = state;
    let Some(room) = room.as_mut() else { return Transition::Stay };
    let editable = room.info.is_host && room.info.countdown.is_none();
    let (label, color, enabled) = launch_bar(&room.info, *maintenance);
    let may_invite = room.info.your_slot != 0 && auth.is_some();

    let mut canvas = gui.stage.screen_canvas(ui);
    let ui = &mut *canvas;
    draw_lobby_header(ui, gui, &room.info);
    let players = show_players(ui, gui, &room.info, room.players_first);

    lobby_panel(view).draw(
        ui.draw,
        &pal,
        &mut ui.cx.fonts,
        "Réglages de la room",
        LOBBY_SETTINGS.map(|(section, _)| section),
    );
    let (sizing, setting) = show_tiles(ui, gui, &room.info, editable);

    let launch = widgets::menu_bar(ui, gui, lobby_launch(view), (label, color), enabled);
    let invite =
        may_invite && widgets::menu_bar(ui, gui, lobby_invite(view), ("Inviter un ami", theme::bar::BLUE), true);
    if let Some(text) = room.chat.show_card(ui, gui, lobby_chat(view)) {
        conn.send(&ClientMessage::Chat { text });
    }
    let leave = widgets::menu_bar(ui, gui, lobby_leave(view), ("Quitter la room", theme::bar::RED), true);

    if let Some(n) = room.info.countdown {
        ui.draw.rect(view.rect()).color(theme::SCRIM);
        ui.cx
            .fonts
            .write(ui.draw, Face::Display, &n.to_string())
            .center(view.w / 2.0, view.h / 2.0)
            .size(theme::size::HUGE)
            .color(theme::GOLD);
    }

    if room.invite.open {
        if let Some(user_id) = show_invite(ui, gui, &mut room.invite) {
            conn.send(&ClientMessage::InviteFriend { user_id });
        }
        return Transition::Stay;
    }
    if room.sizing {
        let action = board_size::show(ui, gui, &room.info.settings.rules, editable);
        apply_size(room, conn, action);
        return Transition::Stay;
    }
    if let Some(edit) = room.cpu_edit {
        if !edit_cpu(ui, gui, &room.info, edit, conn) {
            room.cpu_edit = None;
        }
        return Transition::Stay;
    }

    let info = &room.info;
    match players {
        Some(PlayersClick::Scroll(dir)) => {
            let list = PlayerList::new(view, info, room.players_first);
            room.players_first = list.scrolled(dir);
        }
        Some(PlayersClick::Row(i)) if team_picker(info) == Some(i) => {
            let team = step_team(info.teams.get(i).copied().flatten(), 1);
            conn.send(&ClientMessage::SetTeam { team });
        }
        Some(PlayersClick::Row(i)) => room.cpu_edit = Some((i, info.names.len())),
        Some(PlayersClick::AddCpu) => conn.send(&ClientMessage::AddCpu),
        None if sizing => room.sizing = true,
        None if setting.is_some() && editable => {
            if let Some((setting, dir)) = setting {
                conn.send(&ClientMessage::SetRoomSetting { setting, dir });
            }
        }
        None if launch => {
            let msg = if info.your_slot == 0 {
                ClientMessage::JoinRoom { id: info.id }
            } else {
                ClientMessage::ToggleCountdown
            };
            conn.send(&msg);
        }
        None if leave => leave_room(room, conn),
        None if invite => open_invite(&mut room.invite, auth.as_ref().map(|a| a.token.clone())),
        None => {}
    }
    Transition::Stay
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

fn draw_lobby_header(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, info: &LobbyInfo) {
    let pal = gui.stage.palette();
    let view = gui.stage.view();
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

    gui.stage.header_band(ui, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    let (draw, fonts) = (&mut *ui.draw, &mut ui.cx.fonts);
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, view.w - 60.0, mid);
    let name = fonts.fit(Face::Display, &info.name, theme::size::TITLE, pills_start - 30.0 - 60.0);
    fonts.write(draw, Face::Display, &name).left(60.0, mid).size(theme::size::TITLE).color(pal.text);
}

/// The invitation window: friends to invite, a page at a time. Returns the friend invited, which closes it.
fn show_invite(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, invite: &mut Invite) -> Option<Uuid> {
    let view = gui.stage.view();
    let modal = invite_modal(view);
    let card = modal.card;
    let message = if invite.slot.is_some() {
        Some("Chargement...")
    } else if invite.friends.is_empty() {
        Some("Aucun ami pour l'instant.")
    } else {
        None
    };
    let (invited, dismissed) = widgets::modal(ui, gui, (modal, "Inviter un ami"), |ui, gui| {
        let pal = gui.stage.palette();
        if let Some(message) = message {
            ui.cx
                .fonts
                .write(ui.draw, Face::Text, message)
                .center(card.x() + card.w() / 2.0, card.y() + 110.0)
                .size(theme::size::EMPHASIS)
                .color(pal.text_muted);
            return None;
        }
        let per_page = visible_invites(view);
        let pages = ui::page_count(invite.friends.len(), per_page);
        if pages > 1 {
            invite.pager.show(ui, gui, "invite pages", invite_pager_area(view), pages);
        }
        let mut invited = None;
        for (i, friend) in invite.pager.shown(&invite.friends, per_page) {
            let row = invite_row(view, i);
            list_row(ui.draw, &pal, row, i);
            ui.cx
                .fonts
                .write(ui.draw, Face::Text, &friend.username)
                .left(row.x() + 16.0, row.y() + row.h() / 2.0)
                .size(theme::size::LABEL)
                .color(pal.text);
            if widgets::button(ui, ("invite", &friend.user_id), invite_button(view, i), "Inviter", true) {
                invited = Some(friend.user_id);
            }
        }
        invited
    });
    if dismissed || invited.is_some() {
        invite.open = false;
    }
    invited
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
                assert!(row.h() >= MIN_PLAYER_ROW_H - PLAYER_ROW_GAP, "row {i} squeezed to {}", row.h());
                assert!(row.y() >= card.y() && row.bottom() <= card.bottom(), "row {i} out of the card");
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
