use std::collections::HashMap;
use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::state::{ApiFriendsResponse, FriendEntry, FriendsData, Screen, State, UserSearchEntry};
use crate::ui::{
    self, divider, field_clicked, list_row, page_count, text_field, Face, Field, Fonts, Pager, Pill, Rect, SharpText,
    Status, TextInput, Ui, View,
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

/// How many friends a column lists per page: as many as fit above the
/// bottom buttons, with a row left for the pager. Upright, the three
/// columns are stacked and share that height.
pub fn rows_for(view: View) -> usize {
    let room = back_btn(view.h).y - 8.0 - COLUMNS_TOP;
    let card_h = if view.portrait() {
        (room - 2.0 * COL_GAP_UPRIGHT) / 3.0
    } else {
        room
    };
    let list_h = card_h - CARD_TITLE_H - 6.0 - 10.0;
    ((list_h / ROW_H) as usize).saturating_sub(1).clamp(1, MAX_ROWS)
}

/// A column's card: three side by side, or stacked when upright.
fn column_card(view: View, col: usize) -> Rect {
    let h = CARD_TITLE_H + 6.0 + (rows_for(view) + 1) as f32 * ROW_H + 10.0;
    if view.portrait() {
        let y = COLUMNS_TOP + col as f32 * (h + COL_GAP_UPRIGHT);
        return Rect::at(20.0, y, view.w - 40.0, h);
    }
    let total = 3.0 * COL_W + 2.0 * COL_GAP;
    let x = (view.w - total) / 2.0 + col as f32 * (COL_W + COL_GAP);
    Rect::at(x, COLUMNS_TOP, COL_W, h)
}

fn list_top(card: Rect) -> f32 {
    card.y + CARD_TITLE_H + 6.0
}

fn list_row_rect(view: View, col: usize, row: usize) -> Rect {
    let card = column_card(view, col);
    Rect::at(
        card.x + 8.0,
        list_top(card) + row as f32 * ROW_H,
        card.w - 16.0,
        ROW_H - 4.0,
    )
}

fn pager_area(view: View, col: usize) -> Rect {
    let card = column_card(view, col);
    Rect::at(card.x, list_top(card) + rows_for(view) as f32 * ROW_H, card.w, ROW_H)
}

fn lists(f: &FriendsData) -> [&[FriendEntry]; 3] {
    [&f.friends, &f.received, &f.sent]
}

fn row_button(row: Rect, from_right: f32, w: f32) -> Rect {
    Rect::at(
        row.x + row.w - from_right - w,
        row.y + (row.h - SMALL_BTN_H) / 2.0,
        w,
        SMALL_BTN_H,
    )
}

fn search_submit_btn(view: View) -> Rect {
    Rect::at(
        view.w - 60.0 - 160.0,
        (theme::HEADER_H - FIELD_H) / 2.0 + 12.0,
        160.0,
        FIELD_H,
    )
}

fn search_box(view: View) -> Rect {
    let submit = search_submit_btn(view);
    let w = (submit.x - 12.0 - 200.0).min(430.0);
    Rect::at(submit.x - 12.0 - w, submit.y, w, FIELD_H)
}

fn results_card(view: View) -> Rect {
    let w = (view.w - 40.0).min(840.0);
    Rect::at(
        (view.w - w) / 2.0,
        RESULTS_TOP,
        w,
        16.0 + MAX_SEARCH_RESULTS as f32 * RESULT_ROW_H,
    )
}

fn result_row(view: View, row: usize) -> Rect {
    let card = results_card(view);
    Rect::at(
        card.x + 8.0,
        card.y + 8.0 + row as f32 * RESULT_ROW_H,
        card.w - 16.0,
        RESULT_ROW_H - 4.0,
    )
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

type Pictures = HashMap<String, Texture>;

fn pictures<'a>(gfx: &mut Graphics, state: &State, urls: impl Iterator<Item = &'a Option<String>>) -> Pictures {
    urls.flatten()
        .filter_map(|url| Some((url.clone(), state.images.get(gfx, url)?)))
        .collect()
}

fn avatar(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &Fonts,
    (x, cy): (f32, f32),
    name: &str,
    url: Option<&String>,
    pics: &Pictures,
) {
    let who = ui::Persona {
        name,
        glow: 0.0,
        picture: url.and_then(|u| pics.get(u)),
    };
    ui::portrait(draw, &ui.palette(), fonts, (x + AVATAR_R, cy), AVATAR_R, &who);
}

const AVATAR_R: f32 = 15.0;
const AVATAR_ROOM: f32 = 2.0 * AVATAR_R + 10.0;

fn name_zone(view: View, col: usize, row: usize) -> Rect {
    let rect = list_row_rect(view, col, row);
    let first_button = [watch_btn(view, row), accept_btn(view, row), remove_btn(view, 2, row)][col];
    Rect::at(rect.x, rect.y, first_button.x - rect.x - 8.0, rect.h)
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

fn back_btn(wh: f32) -> Rect {
    Rect::at(40.0, wh - 80.0, 200.0, 54.0)
}

fn refresh_btn(wh: f32) -> Rect {
    Rect::at(254.0, wh - 80.0, 160.0, 54.0)
}

/// The focused page field, or else the search field.
pub fn typing(state: &mut State) -> Option<&mut TextInput> {
    let f = state.friends.as_mut()?;
    if let Some(pager) = f.pages.iter_mut().find(|p| p.focused()) {
        return pager.typing();
    }
    Some(&mut f.search_input)
}

pub fn enter_friends(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, Arc::clone(&slot));
    state.friends = Some(FriendsData {
        friends: Vec::new(),
        sent: Vec::new(),
        received: Vec::new(),
        list_slot: Some(slot),
        search_input: TextInput::default()
            .max_chars(36)
            .only(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' ')),
        search_results: Vec::new(),
        search_slot: None,
        search_status: Status::Empty,
        add_pending: None,
        add_status: Status::Empty,
        confirm_remove: None,
        action_pending: None,
        action_status: Status::Empty,
        pages: Default::default(),
    });
}

fn refresh_list(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, Arc::clone(&slot));
    if let Some(f) = state.friends.as_mut() {
        f.list_slot = Some(slot);
    }
}

fn looks_like_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn try_search(state: &mut State) {
    let busy = state.friends.as_ref().is_none_or(|f| f.search_slot.is_some());
    if busy {
        return;
    }
    let q = state
        .friends
        .as_ref()
        .map(|f| f.search_input.trim().to_owned())
        .unwrap_or_default();
    if q.len() < 2 {
        if let Some(f) = state.friends.as_mut() {
            f.search_status = Status::info("Saisir au moins 2 caractères.");
            f.search_results.clear();
        }
        return;
    }
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(
        http::api_url(&format!("users/search?q={}", http::encode_query(&q))),
        token,
        Arc::clone(&slot),
    );
    if let Some(f) = state.friends.as_mut() {
        f.search_slot = Some(slot);
        f.search_status.clear();
        f.search_results.clear();
    }
}

fn send_add_request(state: &mut State, user_id: &str) {
    let busy = state.friends.as_ref().is_none_or(|f| f.add_pending.is_some());
    if busy {
        return;
    }
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let body = serde_json::json!({ "user_id": user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, Arc::clone(&slot));
    if let Some(f) = state.friends.as_mut() {
        f.add_pending = Some(slot);
        f.add_status.clear();
    }
}

fn poll_list(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(Ok(data)) = http::take_json::<ApiFriendsResponse>(&mut f.list_slot) else {
        return;
    };
    f.friends = data.friends;
    f.sent = data.sent;
    f.received = data.received;
}

fn poll_search(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(result) = http::take_json::<Vec<UserSearchEntry>>(&mut f.search_slot) else {
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

fn poll_add(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(result) = http::take_done(&mut f.add_pending) else {
        return;
    };
    match result {
        Ok(()) => {
            f.add_status = Status::success("Demande envoyée !");
            refresh_list(state);
        }
        Err(msg) => f.add_status = Status::error(msg),
    }
}

fn poll_action(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(result) = http::take_done(&mut f.action_pending) else {
        return;
    };
    match result {
        Ok(()) => f.action_status.clear(),
        Err(msg) => f.action_status = Status::error(msg),
    }
    refresh_list(state);
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

pub fn update_friends(app: &mut App, state: &mut State) {
    poll_list(state);
    poll_search(state);
    poll_add(state);
    poll_action(state);

    let view = state.ui.view();
    let wh = view.h;

    let paging = state
        .friends
        .as_ref()
        .is_some_and(|f| f.pages.iter().any(Pager::focused));
    if let Some(f) = state.friends.as_mut() {
        if !paging {
            f.search_input.edit(&state.keys);
        }
        field_clicked(&state.ui, &state.fonts, search_box(view), &mut f.search_input);
        for col in 0..3 {
            let pages = page_count(lists(f)[col].len(), rows_for(view));
            f.pages[col].clamp(pages);
            f.pages[col].update(app, &state.ui, &state.fonts, &state.keys, pager_area(view, col), pages);
        }
    }

    if state.ui.clicked(search_submit_btn(view)) || (!paging && app.keyboard.was_pressed(KeyCode::Enter)) {
        let q = state
            .friends
            .as_ref()
            .map(|f| f.search_input.trim().to_owned())
            .unwrap_or_default();
        if looks_like_uuid(&q) {
            crate::profile::enter_other_profile(state, q, "...".to_owned(), Screen::Friends);
            state.screen = Screen::OtherProfile;
            return;
        }
        try_search(state);
    }

    if state.ui.clicked(back_btn(wh)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.friends = None;
        state.screen = Screen::Menu;
        return;
    }

    if state.friends.as_ref().is_some_and(can_refresh) && state.ui.clicked(refresh_btn(wh)) {
        refresh_list(state);
    }

    let Some(f) = &state.friends else {
        return;
    };
    if let Some((uid, uname)) = clicked_profile(f, &state.ui, view).or_else(|| clicked_listed(f, &state.ui, view)) {
        crate::profile::enter_other_profile(state, uid, uname, Screen::Friends);
        state.screen = Screen::OtherProfile;
        return;
    }
    let add = clicked_add(f, &state.ui, view);
    let action = clicked_action(f, &state.ui, view);
    if let Some(uid) = add {
        send_add_request(state, &uid);
    }
    match action {
        Some(FriendAction::Watch(user_id)) => watch(state, user_id),
        Some(action) => apply_action(state, action),
        None => {}
    }
}

fn can_refresh(f: &FriendsData) -> bool {
    f.list_slot.is_none() && f.action_pending.is_none()
}

fn clicked_profile(f: &FriendsData, ui: &Ui, view: View) -> Option<(String, String)> {
    let (_, e) = f
        .search_results
        .iter()
        .take(MAX_SEARCH_RESULTS)
        .enumerate()
        .find(|&(i, _)| ui.clicked(result_view_btn(view, i)))?;
    Some((e.user_id.clone(), e.username.clone()))
}

fn clicked_listed(f: &FriendsData, ui: &Ui, view: View) -> Option<(String, String)> {
    if f.list_slot.is_some() {
        return None;
    }
    lists(f).into_iter().enumerate().find_map(|(col, list)| {
        f.pages[col]
            .shown(list, rows_for(view))
            .find(|&(i, _)| ui.clicked(name_zone(view, col, i)))
            .map(|(_, e)| (e.user_id.clone(), e.username.clone()))
    })
}

fn clicked_add(f: &FriendsData, ui: &Ui, view: View) -> Option<String> {
    if f.add_pending.is_some() {
        return None;
    }
    f.search_results
        .iter()
        .take(MAX_SEARCH_RESULTS)
        .enumerate()
        .find(|&(i, e)| ui.clicked(result_add_btn(view, i)) && !f.friends.iter().any(|fr| fr.user_id == e.user_id))
        .map(|(_, e)| e.user_id.clone())
}

fn clicked_action(f: &FriendsData, ui: &Ui, view: View) -> Option<FriendAction> {
    // While the lists reload they are not drawn, so neither are their buttons.
    if f.action_pending.is_some() || f.list_slot.is_some() {
        return None;
    }
    if let Some(confirm_id) = &f.confirm_remove {
        let (row, _) = f.pages[0]
            .shown(&f.friends, rows_for(view))
            .find(|(_, e)| &e.user_id == confirm_id)?;
        if ui.clicked(confirm_yes_btn(view, row)) {
            return Some(FriendAction::ConfirmRemove(confirm_id.clone()));
        }
        if ui.clicked(confirm_no_btn(view, row)) {
            return Some(FriendAction::CancelRemove);
        }
        return None;
    }
    for (i, e) in f.pages[0].shown(&f.friends, rows_for(view)) {
        if ui.clicked(remove_btn(view, 0, i)) {
            return Some(FriendAction::StartRemove(e.user_id.clone()));
        }
        if e.playing && ui.clicked(watch_btn(view, i)) {
            return Some(FriendAction::Watch(e.user_id.clone()));
        }
    }
    for (i, e) in f.pages[1].shown(&f.received, rows_for(view)) {
        if ui.clicked(accept_btn(view, i)) {
            return Some(FriendAction::Accept(e.user_id.clone()));
        }
        if ui.clicked(reject_btn(view, i)) {
            return Some(FriendAction::Reject(e.user_id.clone()));
        }
    }
    for (i, e) in f.pages[2].shown(&f.sent, rows_for(view)) {
        if ui.clicked(remove_btn(view, 2, i)) {
            return Some(FriendAction::Cancel(e.user_id.clone()));
        }
    }
    None
}

/// Watches a friend's game: the server says where it is, or that there is none.
fn watch(state: &mut State, user_id: String) {
    state.notice.clear();
    if state.conn.is_live() {
        state.conn.send(&shared::ClientMessage::WatchFriend { user_id });
    } else {
        state.pending_watch = Some(user_id);
        state.conn.connect(crate::connection::now_secs());
        state.rooms.clear();
    }
    state.screen = Screen::RoomBrowser;
}

fn apply_action(state: &mut State, action: FriendAction) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let Some(f) = state.friends.as_mut() else {
        return;
    };
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

pub fn draw_friends(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let wh = view.h;

    let mut draw = state.ui.screen_canvas(gfx);
    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Amis")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    if let Some(f) = &state.friends {
        let urls = lists(f).into_iter().flatten().map(|e| &e.avatar_url);
        let pics = pictures(gfx, state, urls.chain(f.search_results.iter().map(|e| &e.avatar_url)));
        draw_search(&mut draw, &state.ui, &state.fonts, f, (view, &pics));
        draw_lists(&mut draw, &state.ui, &state.fonts, f, (view, &pics));

        state.ui.button(&mut draw, &state.fonts, back_btn(wh), "Retour");
        let label = if f.list_slot.is_some() { "..." } else { "Rafraîchir" };
        state
            .ui
            .button_enabled(&mut draw, &state.fonts, refresh_btn(wh), label, can_refresh(f));
    }
    state.ui.render(gfx, &draw);
}

fn draw_search(draw: &mut Draw, ui: &Ui, fonts: &Fonts, f: &FriendsData, (view, pics): (View, &Pictures)) {
    let pal = ui.palette();
    let field = search_box(view);
    draw.sharp_text(&fonts.text, "Rechercher un ami")
        .position(field.x, field.y - 14.0)
        .size(theme::size::SMALL)
        .v_align_middle()
        .color(pal.text_dim);

    let searching = f.search_slot.is_some();
    let search = Field {
        placeholder: "Pseudo ou UUID",
        input: &f.search_input,
        focused: true,
    };
    text_field(draw, ui, fonts, field, &search);
    ui.button_enabled(
        draw,
        fonts,
        search_submit_btn(view),
        if searching { "..." } else { "Rechercher" },
        !searching,
    );

    let status = f.search_status.shown(&pal);
    if f.search_results.is_empty() && status.is_none() {
        return;
    }
    let card = results_card(view);
    ui::card(draw, &pal, card);
    if let (true, Some((msg, color))) = (f.search_results.is_empty(), status) {
        draw.sharp_text(&fonts.text, msg)
            .position(card.x + card.w / 2.0, card.y + card.h / 2.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
    let adding = f.add_pending.is_some();
    for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let row = result_row(view, i);
        let mid = row.y + row.h / 2.0;
        list_row(draw, &pal, row, i);
        avatar(
            draw,
            ui,
            fonts,
            (row.x + 10.0, mid),
            &e.username,
            e.avatar_url.as_ref(),
            pics,
        );
        draw.sharp_text(&fonts.text, &e.username)
            .position(row.x + 10.0 + AVATAR_ROOM, mid)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.text);
        let elo = format!("ELO {}", e.elo);
        let rating = Pill {
            text: &elo,
            color: theme::GOLD,
            size: theme::size::SMALL,
        };
        rating.draw(draw, fonts, (row.x + 260.0, mid));
        if !f.friends.iter().any(|fr| fr.user_id == e.user_id) {
            ui.button_enabled(draw, fonts, result_add_btn(view, i), "Ajouter", !adding);
        }
        ui.button(draw, fonts, result_view_btn(view, i), "Profil");
    }

    if let Some((msg, color)) = f.add_status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(view.w / 2.0, ADD_STATUS_Y)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

fn draw_lists(draw: &mut Draw, ui: &Ui, fonts: &Fonts, f: &FriendsData, (view, pics): (View, &Pictures)) {
    let pal = ui.palette();
    let columns = [
        ("Amis", &f.friends, "Aucun ami pour l'instant"),
        ("Demandes reçues", &f.received, "Aucune demande reçue"),
        ("Envoyées", &f.sent, "Aucune demande envoyée"),
    ];
    for (col, (title, list, _)) in columns.iter().enumerate() {
        let card = column_card(view, col);
        ui::card(draw, &pal, card);
        let mid = card.y + CARD_TITLE_H / 2.0;
        draw.sharp_text(&fonts.display, title)
            .position(card.x + 16.0, mid)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(pal.text);
        let count = list.len().to_string();
        let tally = Pill {
            text: &count,
            color: pal.accent,
            size: theme::size::SMALL,
        };
        tally.draw(draw, fonts, (card.x + card.w - 16.0 - tally.width(fonts), mid));
        divider(draw, &pal, card.x + 16.0, card.y + CARD_TITLE_H, card.w - 32.0);
    }

    if f.list_slot.is_some() {
        draw.sharp_text(&fonts.text, "Chargement...")
            .position(view.w / 2.0, list_top(column_card(view, 0)) + 24.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
    } else {
        let busy = f.action_pending.is_some();
        let confirm = f.confirm_remove.as_deref();
        draw_col(
            draw,
            ui,
            fonts,
            (view, 0),
            (columns[0].1, &f.pages[0], pics),
            columns[0].2,
            |draw, fonts, entry, i| {
                if confirm == Some(entry.user_id.as_str()) {
                    ui.button_enabled(draw, fonts, confirm_yes_btn(view, i), "Oui", !busy);
                    ui.button(draw, fonts, confirm_no_btn(view, i), "Non");
                } else {
                    let active = !busy && confirm.is_none();
                    if entry.playing {
                        ui.button_enabled(draw, fonts, watch_btn(view, i), "Regarder", confirm.is_none());
                    }
                    ui.button_enabled(draw, fonts, remove_btn(view, 0, i), "Retirer", active);
                }
            },
        );
        draw_col(
            draw,
            ui,
            fonts,
            (view, 1),
            (columns[1].1, &f.pages[1], pics),
            columns[1].2,
            |draw, fonts, _, i| {
                ui.button_enabled(draw, fonts, accept_btn(view, i), "Accepter", !busy);
                ui.button_enabled(draw, fonts, reject_btn(view, i), "Refuser", !busy);
            },
        );
        draw_col(
            draw,
            ui,
            fonts,
            (view, 2),
            (columns[2].1, &f.pages[2], pics),
            columns[2].2,
            |draw, fonts, _, i| {
                ui.button_enabled(draw, fonts, remove_btn(view, 2, i), "Annuler", !busy);
            },
        );
    }

    for (col, list) in lists(f).into_iter().enumerate() {
        let pages = page_count(list.len(), rows_for(view));
        if pages > 1 && f.list_slot.is_none() {
            f.pages[col].draw(draw, ui, fonts, pager_area(view, col), pages);
        }
    }

    if let Some((msg, color)) = f.action_status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(view.w / 2.0, column_card(view, 2).bottom() + 10.0)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

fn draw_col<F>(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &Fonts,
    (view, col): (View, usize),
    (list, pager, pics): (&[FriendEntry], &Pager, &Pictures),
    empty_msg: &str,
    draw_buttons: F,
) where
    F: Fn(&mut Draw, &Fonts, &FriendEntry, usize),
{
    let pal = &ui.palette();
    if list.is_empty() {
        let card = column_card(view, col);
        draw.sharp_text(&fonts.text, empty_msg)
            .position(card.x + card.w / 2.0, list_top(card) + 24.0)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
        return;
    }
    let room = name_zone(view, col, 0).w - 12.0 - AVATAR_ROOM;
    for (i, e) in pager.shown(list, rows_for(view)) {
        let row = list_row_rect(view, col, i);
        list_row(draw, pal, row, i);
        ui.row(draw, name_zone(view, col, i), i, &format!("friend:{col}:{}", e.user_id));
        let mid = row.y + row.h / 2.0;
        avatar(
            draw,
            ui,
            fonts,
            (row.x + 8.0, mid),
            &e.username,
            e.avatar_url.as_ref(),
            pics,
        );
        let text_x = row.x + 8.0 + AVATAR_ROOM;
        draw.sharp_text(
            &fonts.text,
            &fonts.fit(Face::Text, &e.username, theme::size::BODY, room),
        )
        .position(text_x, mid - 8.0)
        .size(theme::size::BODY)
        .v_align_middle()
        .color(pal.text);
        draw.sharp_text(&fonts.text, &format!("ELO {}", e.elo))
            .position(text_x, mid + 10.0)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(theme::GOLD);
        draw_buttons(draw, fonts, e, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_columns_stay_above_the_bottom_buttons() {
        assert_eq!(
            rows_for(View::fit(1280.0, 800.0)),
            MAX_ROWS,
            "a desktop window keeps every row"
        );
        for (w, h) in [
            (852.0, 300.0),
            (852.0, 340.0),
            (1280.0, 800.0),
            (393.0, 740.0),
            (393.0, 600.0),
        ] {
            let view = View::fit(w, h);
            let last = column_card(view, 2);
            assert!(rows_for(view) >= 1);
            assert!(
                last.bottom() < back_btn(view.h).y,
                "{w}x{h}: the columns reach the buttons"
            );
            assert!(last.x + last.w <= view.w, "{w}x{h}: a column is cut on the right");
        }
    }
}
