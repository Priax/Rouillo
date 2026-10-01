use notan::prelude::*;
use shared::ClientMessage;

use crate::state::{Screen, State};
use crate::theme;
use crate::ui::{Rect, SharpText, View};

fn cancel_btn(view: View) -> Rect {
    Rect::at(view.w / 2.0 - 110.0, view.h / 2.0 + 90.0, 220.0, 54.0)
}

pub fn enter(state: &mut State) {
    state.notice.clear();
    state.queued_at = Some(crate::connection::now_secs());
    state.lobby = None;
    state.series_over = None;
    if state.conn.is_live() {
        state.conn.send(&ClientMessage::JoinQueue);
    } else {
        state.conn.connect(crate::connection::now_secs());
        state.pending_queue = true;
    }
    state.screen = Screen::Ranked;
}

pub fn leave(state: &mut State) {
    state.queued_at = None;
    state.pending_queue = false;
    state.lobby = None;
    state.session = None;
    state.series_over = None;
    state.screen = Screen::PlayMenu;
}

fn matched(state: &State) -> bool {
    state.lobby.as_ref().is_some_and(|l| l.ranked.is_some())
}

pub fn update(app: &App, state: &mut State) {
    if matched(state) {
        return;
    }
    let view = state.ui.view();
    if state.ui.clicked(cancel_btn(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.conn.send(&ClientMessage::LeaveQueue);
        state.conn.disconnect();
        leave(state);
    }
}

pub fn draw(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let (cx, cy) = (view.w / 2.0, view.h / 2.0);
    let mut draw = state.ui.screen_canvas(gfx);

    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Classé")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    let line = |draw: &mut notan::draw::Draw, text: &str, y: f32, size: f32, color: Color| {
        draw.sharp_text(&state.fonts.text, text)
            .position(cx, y)
            .size(size)
            .h_align_center()
            .v_align_middle()
            .color(color);
    };

    match state.lobby.as_ref().and_then(|l| Some((l, l.ranked.as_ref()?))) {
        Some((lobby, ranked)) => {
            line(
                &mut draw,
                "Adversaire trouvé",
                cy - 80.0,
                theme::size::HEADING,
                pal.text_dim,
            );
            let opponent = format!("{} ({} ELO)", ranked.opponent, ranked.opponent_elo);
            line(&mut draw, &opponent, cy - 30.0, theme::size::TITLE, pal.text);
            let first_to = format!("Premier à {} manches", shared::config::RANKED_WINS);
            line(&mut draw, &first_to, cy + 20.0, theme::size::LABEL, pal.text_muted);
            if let Some(n) = lobby.countdown {
                draw.sharp_text(&state.fonts.display, &n.to_string())
                    .position(cx, cy + 110.0)
                    .size(theme::size::HERO)
                    .h_align_center()
                    .v_align_middle()
                    .color(pal.title);
            }
        }
        None => {
            let dots = ".".repeat(1 + (state.ui.time() * 2.0) as usize % 3);
            line(
                &mut draw,
                &format!("Recherche d'un adversaire{dots}"),
                cy - 40.0,
                theme::size::HEADING,
                pal.text,
            );
            let waited = state
                .queued_at
                .map_or(0, |t| (crate::connection::now_secs() - t).max(0.0) as u64);
            let waited = format!("{}:{:02}", waited / 60, waited % 60);
            line(&mut draw, &waited, cy + 10.0, theme::size::EMPHASIS, pal.text_dim);
            if let Some(auth) = &state.auth {
                let elo = format!("Votre ELO: {}", auth.elo);
                line(&mut draw, &elo, cy + 50.0, theme::size::LABEL, pal.text_muted);
            }
            state.ui.button(&mut draw, &state.fonts, cancel_btn(view), "Annuler");
        }
    }

    state.ui.render(gfx, &draw);
}
