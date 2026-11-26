use std::collections::HashMap;
use std::sync::Arc;

use aero::draw::Draw;
use aero::image::Image;

use crate::images::Images;
use crate::nav::Transition;
use crate::state::{ApiFriendsResponse, FriendEntry, FriendsData, Screen, State};
use crate::tick::Tick;
use crate::ui::{
    self, divider, list_row, page_count, widgets, Face, Fonts, Frame, Gui, Label, Pager, Pill, Rect, Size, Status, Ui,
    View,
};
use crate::{http, theme};

const FIELD_H: f32 = 44.0;
const RESULTS_TOP: f32 = 128.0;
const RESULT_ROW_H: f32 = 44.0;
const MAX_SEARCH_RESULTS: usize = 4;
const ADD_STATUS_Y: f32 = 330.0;
const COLUMNS_TOP: f32 = 344.0;
const CARD_TITLE_H: f32 = 50.0;
const ROW_H: f32 = 48.0;
const MAX_ROWS: usize = 5;
const COL_W: f32 = 360.0;
const COL_GAP: f32 = 30.0;
const SMALL_BTN_H: f32 = 32.0;
const COL_GAP_UPRIGHT: f32 = 16.0;
const AVATAR_R: f32 = 15.0;
const AVATAR_ROOM: f32 = 2.0 * AVATAR_R + 10.0;

/// How many friends a column lists per page: as many as fit above the
/// bottom buttons, with a row left for the pager. Upright, the three
/// columns are stacked and share that height.
pub fn rows_for(view: View) -> usize {
    let room = back_btn(view).y - 8.0 - COLUMNS_TOP;
    let card_h = if view.portrait() { (room - 2.0 * COL_GAP_UPRIGHT) / 3.0 } else { room };
    let list_h = card_h - CARD_TITLE_H - 6.0 - 10.0;
    ((list_h / ROW_H) as usize).saturating_sub(1).clamp(1, MAX_ROWS)
}

/// A column's card: three side by side, or stacked when upright.
fn column_card(view: View, col: usize) -> Rect {
    let h = CARD_TITLE_H + 6.0 + (rows_for(view) + 1) as f32 * ROW_H + 10.0;
    let band = Rect::at(0.0, COLUMNS_TOP, view.w, h);
    if view.portrait() {
        return band.pad(0.0, 20.0, 0.0, 20.0).nth_row(col, h, COL_GAP_UPRIGHT);
    }
    let cards = band.centered(3.0 * COL_W + 2.0 * COL_GAP, h);
    cards.columns(COL_GAP, [Size::Px(COL_W); 3])[col]
}

/// The card under its title: the rows, then the pager.
fn card_list(card: Rect) -> Rect {
    card.below(CARD_TITLE_H + 6.0)
}

fn list_top(card: Rect) -> f32 {
    card_list(card).y
}

fn list_row_rect(view: View, col: usize, row: usize) -> Rect {
    let list = card_list(column_card(view, col)).pad(0.0, 8.0, 0.0, 8.0);
    list.nth_row(row, ROW_H - 4.0, 4.0)
}

fn pager_area(view: View, col: usize) -> Rect {
    card_list(column_card(view, col)).nth_row(rows_for(view), ROW_H, 0.0)
}

fn lists(f: &FriendsData) -> [&[FriendEntry]; 3] {
    [&f.friends, &f.received, &f.sent]
}

fn row_button(row: Rect, from_right: f32, w: f32) -> Rect {
    row.pad(0.0, from_right, 0.0, 0.0).right(w).middle(SMALL_BTN_H)
}

/// The header under the search label.
fn search_band(view: View) -> Rect {
    Frame::of(view).header_tools(200.0).pad(24.0, 0.0, 0.0, 0.0)
}

fn search_submit_btn(view: View) -> Rect {
    search_band(view).right(160.0).middle(FIELD_H)
}

fn search_box(view: View) -> Rect {
    let room = search_band(view).pad(0.0, 160.0 + 12.0, 0.0, 0.0).middle(FIELD_H);
    room.right(room.w.min(430.0))
}

fn results_card(view: View) -> Rect {
    let h = 16.0 + MAX_SEARCH_RESULTS as f32 * RESULT_ROW_H;
    Rect::at(0.0, RESULTS_TOP, view.w, h).pad(0.0, 20.0, 0.0, 20.0).max_width(840.0)
}

fn result_row(view: View, row: usize) -> Rect {
    results_card(view).inset(8.0).nth_row(row, RESULT_ROW_H - 4.0, 4.0)
}

fn result_add_btn(view: View, row: usize) -> Rect {
    row_button(result_row(view, row), 128.0, 120.0)
}

fn result_view_btn(view: View, row: usize) -> Rect {
    row_button(result_row(view, row), 4.0, 116.0)
}

fn remove_btn(view: View, col: usize, row: usize) -> Rect {
    row_button(list_row_rect(view, col, row), 6.0, 110.0)
}

fn watch_btn(view: View, row: usize) -> Rect {
    row_button(list_row_rect(view, 0, row), 122.0, 110.0)
}

type Pictures = HashMap<String, Image>;

fn pictures<'a>(images: &mut Images, urls: impl Iterator<Item = &'a Option<String>>) -> Pictures {
    urls.flatten().filter_map(|url| Some((url.clone(), images.get(url)?))).collect()
}

fn avatar(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &mut Fonts,
    (x, cy): (f32, f32),
    name: &str,
    url: Option<&String>,
    pics: &Pictures,
) {
    let who = ui::Persona { name, glow: 0.0, picture: url.and_then(|u| pics.get(u)) };
    ui::portrait(draw, &ui.palette(), fonts, (x + AVATAR_R, cy), AVATAR_R, &who);
}

/// Up to the row's first button: "Regarder" only shows while the friend plays.
fn name_zone(view: View, col: usize, row: usize, playing: bool) -> Rect {
    let rect = list_row_rect(view, col, row);
    let friend = if playing { watch_btn(view, row) } else { remove_btn(view, 0, row) };
    let first_button = [friend, accept_btn(view, row), remove_btn(view, 2, row)][col];
    rect.left(first_button.x - rect.x - 8.0)
}

fn accept_btn(view: View, row: usize) -> Rect {
    row_button(list_row_rect(view, 1, row), 122.0, 116.0)
}

fn reject_btn(view: View, row: usize) -> Rect {
    row_button(list_row_rect(view, 1, row), 6.0, 110.0)
}

fn confirm_yes_btn(view: View, row: usize) -> Rect {
    row_button(list_row_rect(view, 0, row), 62.0, 52.0)
}

fn confirm_no_btn(view: View, row: usize) -> Rect {
    row_button(list_row_rect(view, 0, row), 6.0, 52.0)
}

fn back_btn(view: View) -> Rect {
    Frame::of(view).back
}

fn refresh_btn(view: View) -> Rect {
    let back = back_btn(view);
    Rect::at(back.x + back.w + 14.0, back.y, 160.0, back.h)
}

pub fn enter_friends(state: &State) -> Transition {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, Arc::clone(&slot));

    Transition::To(Screen::Friends(Box::new(FriendsData {
        friends: Vec::new(),
        sent: Vec::new(),
        received: Vec::new(),
        list_slot: Some(slot),
        search_input: String::new(),
        search_results: Vec::new(),
        search_slot: None,
        search_status: Status::Empty,
        add_pending: None,
        add_status: Status::Empty,
        confirm_remove: None,
        action_pending: None,
        action_status: Status::Empty,
        pages: Default::default(),
    })))
}

fn refresh_list(state: &State, f: &mut FriendsData) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, Arc::clone(&slot));
    f.list_slot = Some(slot);
}

fn looks_like_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(
            |(i, &c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c == b'-'
                } else {
                    c.is_ascii_hexdigit()
                }
            },
        )
}

fn search_query(f: &FriendsData) -> String {
    f.search_input.trim().to_owned()
}

fn try_search(state: &State, f: &mut FriendsData) {
    if f.search_slot.is_some() {
        return;
    }

    let q = search_query(f);
    if q.len() < 2 {
        f.search_status = Status::info("Saisir au moins 2 caractères.");
        f.search_results.clear();
        return;
    }

    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url(&format!("users/search?q={}", http::encode_query(&q))), token, Arc::clone(&slot));
    f.search_slot = Some(slot);
    f.search_status.clear();
    f.search_results.clear();
}

fn send_add_request(state: &State, f: &mut FriendsData, user_id: &str) {
    if f.add_pending.is_some() {
        return;
    }

    let token = state.auth.as_ref().map(|a| a.token.clone());
    let body = serde_json::json!({ "user_id": user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, Arc::clone(&slot));
    f.add_pending = Some(slot);
    f.add_status.clear();
}

fn poll_list(f: &mut FriendsData) {
    let Some(Ok(data)) = http::take_json::<ApiFriendsResponse>(&mut f.list_slot) else {
        return;
    };
    f.friends = data.friends;
    f.sent = data.sent;
    f.received = data.received;
}

fn poll_search(f: &mut FriendsData) {
    let Some(result) = http::take_json::<Vec<FriendEntry>>(&mut f.search_slot) else {
        return;
    };
    match result {
        Ok(entries) => {
            if entries.is_empty() {
                f.search_status = Status::info("Aucun résultat.");
            }
            f.search_results = entries;
        }
        Err(msg) => f.search_status = Status::error(msg),
    }
}

fn poll_add(state: &State, f: &mut FriendsData) {
    let Some(result) = http::take_done(&mut f.add_pending) else {
        return;
    };
    match result {
        Ok(()) => {
            f.add_status = Status::success("Demande envoyée !");
            refresh_list(state, f);
        }
        Err(msg) => f.add_status = Status::error(msg),
    }
}

fn poll_action(state: &State, f: &mut FriendsData) {
    let Some(result) = http::take_done(&mut f.action_pending) else {
        return;
    };
    match result {
        Ok(()) => f.action_status.clear(),
        Err(msg) => f.action_status = Status::error(msg),
    }
    refresh_list(state, f);
}

enum FriendAction {
    Watch(String),
    StartRemove(String),
    ConfirmRemove(String),
    CancelRemove,
    Accept(String),
    Reject(String),
    Cancel(String),
}

/// What comes back, the search field and the keys; the clicks are in [`show_friends`].
pub fn update_friends(tick: &Tick, gui: &mut Gui, state: &mut State, f: &mut FriendsData) -> Transition {
    poll_list(f);
    poll_search(f);
    poll_add(state, f);
    poll_action(state, f);

    let view = gui.ui.view();
    for col in 0..3 {
        let pages = page_count(lists(f)[col].len(), rows_for(view));
        f.pages[col].clamp(pages);
    }
    if tick.back() {
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}

/// Opens the profile of the id typed, or searches for the name.
fn search(state: &State, f: &mut FriendsData) -> Transition {
    let q = search_query(f);
    if looks_like_uuid(&q) {
        return crate::profile::enter_other_profile(state, q, "...".to_owned(), Some(f));
    }
    try_search(state, f);
    Transition::Stay
}

fn can_refresh(f: &FriendsData) -> bool {
    f.list_slot.is_none() && f.action_pending.is_none()
}

/// Watches a friend's game: the server says where it is, or that there is none.
fn watch(state: &mut State, user_id: String) -> Transition {
    state.notice.clear();

    if state.net.conn.is_live() {
        state.net.conn.send(&shared::ClientMessage::WatchFriend { user_id });
    } else {
        state.net.pending_watch = Some(user_id);
        state.net.conn.connect(crate::connection::now_secs());
        state.rooms.clear();
    }

    Transition::To(Screen::RoomBrowser)
}

fn apply_action(state: &State, f: &mut FriendsData, action: FriendAction) {
    let token = state.auth.as_ref().map(|a| a.token.clone());

    match action {
        FriendAction::Watch(_) => {}
        FriendAction::StartRemove(id) => f.confirm_remove = Some(id),
        FriendAction::CancelRemove => f.confirm_remove = None,
        FriendAction::ConfirmRemove(id) | FriendAction::Reject(id) | FriendAction::Cancel(id) => {
            let slot = http::new_slot();
            http::delete_req(http::api_url(&format!("friends/{id}")), token, Arc::clone(&slot));
            f.action_pending = Some(slot);
            f.action_status.clear();
            f.confirm_remove = None;
        }
        FriendAction::Accept(id) => {
            let slot = http::new_slot();
            http::post_empty(http::api_url(&format!("friends/{id}/accept")), token, Arc::clone(&slot));
            f.action_pending = Some(slot);
            f.action_status.clear();
        }
    }
}

/// What a click on the friends screen asks for, kept until it is all drawn.
#[derive(Default)]
struct Clicks {
    action: Option<FriendAction>,
    profile: Option<(String, String)>,
    add: Option<String>,
}

pub fn show_friends(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State, f: &mut FriendsData) -> Transition {
    let view = gui.ui.view();
    let urls = lists(f).into_iter().flatten().map(|e| &e.avatar_url);
    let pics = pictures(&mut gui.images, urls.chain(f.search_results.iter().map(|e| &e.avatar_url)));
    let label = Label::busy_or(f.list_slot.is_some(), "Rafraîchir");

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    gui.ui.page_header(ui.draw, &mut gui.fonts, "Amis");
    let mut clicks = Clicks::default();
    let searched = show_search(ui, gui, f, (view, &pics), &mut clicks);
    show_lists(ui, gui, f, (view, &pics), &mut clicks);
    let back = widgets::button(ui, "back", back_btn(view), "Retour", true);
    let refreshed = widgets::button(ui, "refresh", refresh_btn(view), label, can_refresh(f));

    if searched {
        return search(state, f);
    }
    if back {
        return Transition::To(Screen::Menu);
    }
    if refreshed && can_refresh(f) {
        refresh_list(state, f);
    }
    if let Some((user_id, username)) = clicks.profile {
        return crate::profile::enter_other_profile(state, user_id, username, Some(f));
    }
    if let Some(user_id) = clicks.add {
        send_add_request(state, f, &user_id);
    }
    match clicks.action {
        Some(FriendAction::Watch(user_id)) => watch(state, user_id),
        Some(action) => {
            apply_action(state, f, action);
            Transition::Stay
        }
        None => Transition::Stay,
    }
}

/// Returns whether a search was asked.
fn show_search(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    f: &mut FriendsData,
    (view, pics): (View, &Pictures),
    clicks: &mut Clicks,
) -> bool {
    let pal = gui.ui.palette();
    let field = search_box(view);
    let searching = f.search_slot.is_some();
    let submit = Label::busy_or(searching, "Rechercher");
    let fonts = &mut gui.fonts;

    fonts
        .write(ui.draw, Face::Text, "Rechercher un ami")
        .left(field.x, field.y - 14.0)
        .size(theme::size::SMALL)
        .color(pal.text_dim);
    let search = widgets::field()
        .placeholder("Pseudo ou UUID")
        .max_chars(36)
        .accepts(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '));
    let typed = widgets::text_field(ui, gui, "search", field, search, &mut f.search_input);
    let searched = widgets::button(ui, "search button", search_submit_btn(view), submit, !searching) || typed.submitted;
    let fonts = &mut gui.fonts;

    let status = f.search_status.shown(&pal);
    if f.search_results.is_empty() && status.is_none() {
        return searched;
    }

    let card = results_card(view);
    let adding = f.add_pending.is_some();

    ui::card(ui.draw, &pal, card);
    if let (true, Some((msg, color))) = (f.search_results.is_empty(), status) {
        fonts
            .write(ui.draw, Face::Text, msg)
            .center(card.x + card.w / 2.0, card.y + card.h / 2.0)
            .size(theme::size::LABEL)
            .color(color);
    }
    for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let row = result_row(view, i);
        let mid = row.y + row.h / 2.0;
        let elo = format!("ELO {}", e.elo);
        let rating = Pill::small(&elo, theme::GOLD);

        list_row(ui.draw, &pal, row, i);
        avatar(ui.draw, &gui.ui, fonts, (row.x + 10.0, mid), &e.username, e.avatar_url.as_ref(), pics);
        fonts
            .write(ui.draw, Face::Text, &e.username)
            .left(row.x + 10.0 + AVATAR_ROOM, mid)
            .size(theme::size::LABEL)
            .color(pal.text);
        rating.draw(ui.draw, fonts, (row.x + 260.0, mid));
        let befriended = f.friends.iter().any(|fr| fr.user_id == e.user_id);
        if !befriended && widgets::button(ui, ("add", &e.user_id), result_add_btn(view, i), "Ajouter", !adding) {
            clicks.add = Some(e.user_id.clone());
        }
        if widgets::button(ui, ("profile", &e.user_id), result_view_btn(view, i), "Profil", true) {
            clicks.profile = Some((e.user_id.clone(), e.username.clone()));
        }
    }

    if let Some((msg, color)) = f.add_status.shown(&pal) {
        fonts.write(ui.draw, Face::Text, msg).center(view.w / 2.0, ADD_STATUS_Y).size(theme::size::BODY).color(color);
    }
    searched
}

fn show_lists(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    f: &mut FriendsData,
    (view, pics): (View, &Pictures),
    clicks: &mut Clicks,
) {
    let pal = gui.ui.palette();
    let columns = [
        ("Amis", "Aucun ami pour l'instant"),
        ("Demandes reçues", "Aucune demande reçue"),
        ("Envoyées", "Aucune demande envoyée"),
    ];
    for (col, ((title, _), list)) in columns.iter().zip(lists(f)).enumerate() {
        let card = column_card(view, col);
        let mid = card.y + CARD_TITLE_H / 2.0;
        let count = list.len().to_string();
        let tally = Pill::small(&count, pal.accent);
        let fonts = &mut gui.fonts;

        ui::card(ui.draw, &pal, card);
        fonts.write(ui.draw, Face::Display, title).left(card.x + 16.0, mid).size(theme::size::EMPHASIS).color(pal.text);
        let left = card.x + card.w - 16.0 - tally.width(fonts);
        tally.draw(ui.draw, fonts, (left, mid));
        divider(ui.draw, &pal, card.x + 16.0, card.y + CARD_TITLE_H, card.w - 32.0);
    }

    if f.list_slot.is_some() {
        gui.fonts
            .write(ui.draw, Face::Text, "Chargement...")
            .center(view.w / 2.0, list_top(column_card(view, 0)) + 24.0)
            .size(theme::size::LABEL)
            .color(pal.text_muted);
    } else {
        let busy = f.action_pending.is_some();
        let confirm = f.confirm_remove.as_deref();
        let friends = (&f.friends[..], &f.pages[0], pics);
        show_col(ui, gui, (view, 0), friends, (columns[0].1, clicks), |ui, entry, i| {
            let id = &entry.user_id;
            if confirm == Some(id.as_str()) {
                if widgets::button(ui, ("yes", id), confirm_yes_btn(view, i), "Oui", !busy) {
                    return Some(FriendAction::ConfirmRemove(id.clone()));
                }
                if widgets::button(ui, ("no", id), confirm_no_btn(view, i), "Non", true) {
                    return Some(FriendAction::CancelRemove);
                }
                return None;
            }
            let active = !busy && confirm.is_none();
            if entry.playing && widgets::button(ui, ("watch", id), watch_btn(view, i), "Regarder", confirm.is_none()) {
                return Some(FriendAction::Watch(id.clone()));
            }
            if widgets::button(ui, ("remove", id), remove_btn(view, 0, i), "Retirer", active) {
                return Some(FriendAction::StartRemove(id.clone()));
            }
            None
        });
        let received = (&f.received[..], &f.pages[1], pics);
        show_col(ui, gui, (view, 1), received, (columns[1].1, clicks), |ui, entry, i| {
            let id = &entry.user_id;
            if widgets::button(ui, ("accept", id), accept_btn(view, i), "Accepter", !busy) {
                return Some(FriendAction::Accept(id.clone()));
            }
            if widgets::button(ui, ("reject", id), reject_btn(view, i), "Refuser", !busy) {
                return Some(FriendAction::Reject(id.clone()));
            }
            None
        });
        let sent = (&f.sent[..], &f.pages[2], pics);
        show_col(ui, gui, (view, 2), sent, (columns[2].1, clicks), |ui, entry, i| {
            let id = &entry.user_id;
            widgets::button(ui, ("cancel", id), remove_btn(view, 2, i), "Annuler", !busy)
                .then(|| FriendAction::Cancel(id.clone()))
        });
        let FriendsData { friends, received, sent, pages, .. } = f;
        for (col, (list, pager)) in [&friends[..], received, sent].into_iter().zip(pages.iter_mut()).enumerate() {
            let pages = page_count(list.len(), rows_for(view));
            if pages > 1 {
                pager.show(ui, gui, ("pages", col), pager_area(view, col), pages);
            }
        }
    }

    if let Some((msg, color)) = f.action_status.shown(&pal) {
        gui.fonts
            .write(ui.draw, Face::Text, msg)
            .center(view.w / 2.0, column_card(view, 2).bottom() + 10.0)
            .size(theme::size::SMALL)
            .color(color);
    }
}

/// One column's rows: a click on a name opens the profile, `buttons` shows a row's buttons and says what they ask.
fn show_col(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    (view, col): (View, usize),
    (list, pager, pics): (&[FriendEntry], &Pager, &Pictures),
    (empty_msg, clicks): (&str, &mut Clicks),
    mut buttons: impl FnMut(&mut aero::ui::Ui<'_>, &FriendEntry, usize) -> Option<FriendAction>,
) {
    let pal = gui.ui.palette();
    if list.is_empty() {
        let card = column_card(view, col);
        gui.fonts
            .write(ui.draw, Face::Text, empty_msg)
            .center(card.x + card.w / 2.0, list_top(card) + 24.0)
            .size(theme::size::SMALL)
            .color(pal.text_muted);
        return;
    }
    for (i, e) in pager.shown(list, rows_for(view)) {
        let row = list_row_rect(view, col, i);
        let zone = name_zone(view, col, i, e.playing);
        let mid = row.y + row.h / 2.0;
        let text_x = row.x + 8.0 + AVATAR_ROOM;

        list_row(ui.draw, &pal, row, i);
        if widgets::row(ui, gui, ("friend", col, &e.user_id), zone, i) {
            clicks.profile = Some((e.user_id.clone(), e.username.clone()));
        }
        let fonts = &mut gui.fonts;
        let name = fonts.fit(Face::Text, &e.username, theme::size::BODY, zone.w - 12.0 - AVATAR_ROOM);
        avatar(ui.draw, &gui.ui, fonts, (row.x + 8.0, mid), &e.username, e.avatar_url.as_ref(), pics);
        fonts.write(ui.draw, Face::Text, &name).left(text_x, mid - 8.0).size(theme::size::BODY).color(pal.text);
        fonts
            .write(ui.draw, Face::Text, &format!("ELO {}", e.elo))
            .left(text_x, mid + 10.0)
            .size(theme::size::SMALL)
            .color(theme::GOLD);
        if let Some(action) = buttons(ui, e, i) {
            clicks.action = Some(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_columns_stay_above_the_bottom_buttons() {
        assert_eq!(rows_for(View::fit(1280.0, 800.0)), MAX_ROWS, "a desktop window keeps every row");
        for (w, h) in [(852.0, 300.0), (852.0, 340.0), (1280.0, 800.0), (393.0, 740.0), (393.0, 600.0)] {
            let view = View::fit(w, h);
            let last = column_card(view, 2);
            assert!(rows_for(view) >= 1);
            assert!(last.bottom() < back_btn(view).y, "{w}x{h}: the columns reach the buttons");
            assert!(last.x + last.w <= view.w, "{w}x{h}: a column is cut on the right");
        }
    }
}
