use notan::draw::Draw;
use notan::prelude::*;

use crate::controls::{Action, Bindings};
use crate::demo::Demo;
use crate::state::{Screen, State};
use crate::ui::{self, Face, Fonts, Rect, SharpText, Ui, View};
use crate::{draw as game_draw, theme};

const RULES: [(&str, &str); 7] = [
    (
        "Le but",
        "Empilez les paires qui tombent. Quatre Puyos de la même couleur qui se touchent disparaissent.",
    ),
    (
        "Les chaînes",
        "Quand des Puyos disparaissent, ceux du dessus tombent et peuvent en faire disparaître d'autres: \
         c'est une chaîne. Plus elle est longue, plus elle rapporte.",
    ),
    (
        "Les nuisances",
        "Vos chaînes envoient des Puyos gris chez l'adversaire, après un court délai. Une chaîne \
         annule d'abord les nuisances qui vous attendent. Un gris disparaît quand un groupe éclate à côté.",
    ),
    (
        "All Clear",
        "Videz tout le plateau: votre chaîne suivante enverra une grosse attaque en plus.",
    ),
    (
        "La défaite",
        "La partie est perdue quand la case marquée d'une croix, en haut du plateau, est occupée.",
    ),
    (
        "Les niveaux",
        "Avec le temps, les paires tombent plus vite et les attaques deviennent plus fortes.",
    ),
    (
        "Le classé",
        "Une série se joue en trois manches gagnantes. Seul le classé fait bouger l'ELO.",
    ),
];

const LINE_H: f32 = 23.0;

const DEMO_W: f32 = game_draw::BOARD_W + 40.0;

fn cards(view: View) -> (Rect, Rect, Rect) {
    let (top, bottom) = (theme::HEADER_H + 30.0, view.h - 110.0);
    let left = Rect::at(view.w / 2.0 - 610.0, top, 380.0, bottom - top);
    let demo = Rect::at(left.x + left.w + 25.0, top, DEMO_W, bottom - top);
    let right = Rect::at(
        demo.x + demo.w + 25.0,
        top,
        1220.0 - left.w - DEMO_W - 50.0,
        bottom - top,
    );
    (left, demo, right)
}

fn back_btn(view: View) -> Rect {
    Rect::at(40.0, view.h - 80.0, 200.0, 54.0)
}

pub fn enter(state: &mut State) {
    state.demo = Some(Demo::new());
    state.screen = Screen::Help;
}

pub fn update(app: &App, state: &mut State) {
    if let Some(demo) = state.demo.as_mut() {
        demo.update(app.timer.delta_f32());
    }
    if state.ui.clicked(back_btn(state.ui.view())) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.demo = None;
        state.screen = Screen::Menu;
    }
}

fn draw_demo(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect, demo: &Demo, time: f32) {
    let pal = ui.palette();
    let board = Rect::at(card.x + 20.0, card.y + 30.0, game_draw::BOARD_W, game_draw::BOARD_H);
    game_draw::draw_demo(draw, fonts, demo, board, time);
    if let Some((text, left)) = demo.caption {
        draw.sharp_text(&fonts.display, text)
            .position(card.x + card.w / 2.0, board.y + board.h + 32.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text.with_alpha(left.min(1.0)));
    }
}

fn binding_text(bindings: &Bindings, action: Action) -> String {
    let keys = (0..2).filter_map(|slot| bindings.key(action, slot).map(|k| k.label.clone()));
    let pad = bindings.pad(action).map(|p| format!("manette {}", p.label()));
    let all: Vec<String> = keys.chain(pad).collect();
    if all.is_empty() {
        "aucune touche".to_owned()
    } else {
        all.join(", ")
    }
}

fn card_title(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect, title: &str) {
    let pal = ui.palette();
    ui::card(draw, &pal, card);
    draw.sharp_text(&fonts.display, title)
        .position(card.x + 20.0, card.y + 28.0)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text_dim);
    ui::divider(draw, &pal, card.x + 20.0, card.y + 56.0, card.w - 40.0);
}

fn draw_controls(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect, bindings: &Bindings) {
    let pal = ui.palette();
    card_title(draw, ui, fonts, card, "Contrôles");
    let mut rows: Vec<(String, String)> = Action::ALL
        .into_iter()
        .map(|a| (a.label().to_owned(), binding_text(bindings, a)))
        .collect();
    rows.push(("Pause".to_owned(), "Échap, manette Start".to_owned()));
    rows.push(("Rejouer (partie finie)".to_owned(), "R, manette Select".to_owned()));
    let mut y = card.y + 80.0;
    for (label, keys) in rows {
        draw.sharp_text(&fonts.text, &label)
            .position(card.x + 20.0, y)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(pal.text);
        y += LINE_H;
        let keys = fonts.fit(Face::Text, &keys, theme::size::SMALL, card.w - 60.0);
        draw.sharp_text(&fonts.text, &keys)
            .position(card.x + 36.0, y)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(theme::GOLD);
        y += LINE_H + 8.0;
    }
    let note = "Les touches se changent dans Paramètres. Sur téléphone, des boutons apparaissent \
                à l'écran dès que vous le touchez.";
    for line in fonts.wrap(Face::Text, note, theme::size::SMALL, card.w - 40.0) {
        draw.sharp_text(&fonts.text, line)
            .position(card.x + 20.0, y)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(pal.text_muted);
        y += LINE_H - 3.0;
    }
}

fn draw_rules(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect) {
    let pal = ui.palette();
    card_title(draw, ui, fonts, card, "Règles");
    let mut y = card.y + 80.0;
    for (title, text) in RULES {
        draw.sharp_text(&fonts.display, title)
            .position(card.x + 20.0, y)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.accent);
        y += LINE_H;
        for line in fonts.wrap(Face::Text, text, theme::size::SMALL, card.w - 40.0) {
            draw.sharp_text(&fonts.text, line)
                .position(card.x + 20.0, y)
                .size(theme::size::SMALL)
                .v_align_middle()
                .color(pal.text);
            y += LINE_H - 3.0;
        }
        y += 8.0;
    }
}

pub fn draw(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let mut draw = state.ui.screen_canvas(gfx);
    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Comment jouer")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);
    let (left, middle, right) = cards(view);
    draw_controls(&mut draw, &state.ui, &state.fonts, left, &state.controls.bindings);
    if let Some(demo) = &state.demo {
        draw_demo(&mut draw, &state.ui, &state.fonts, middle, demo, state.ui.time() as f32);
    }
    draw_rules(&mut draw, &state.ui, &state.fonts, right);
    state.ui.button(&mut draw, &state.fonts, back_btn(view), "Retour");
    state.ui.render(gfx, &draw);
}
