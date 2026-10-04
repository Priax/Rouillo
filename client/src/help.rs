use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;

use crate::controls::{Action, Bindings};
use crate::demo::Demo;
use crate::pads::Pad;
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

/// The help screen: a demo that plays lessons, next to a card showing either
/// the controls or the rules.
pub struct HelpView {
    pub demo: Demo,
    rules: bool,
}

const DEMO_W: f32 = game_draw::BOARD_W + 40.0;
const DEMO_H: f32 = 580.0;
const INFO_H_UPRIGHT: f32 = 640.0;
const TAB_H: f32 = 56.0;
const ROW_H: f32 = 52.0;
const CHIP_H: f32 = 38.0;
const LINE_H: f32 = 25.0;

/// The tabbed card and the demo card: side by side, or the demo below.
fn cards(view: View) -> (Rect, Rect) {
    let top = theme::HEADER_H + 20.0;
    if view.portrait() {
        let info = Rect::at(20.0, top, view.w - 40.0, INFO_H_UPRIGHT);
        let demo = Rect::at((view.w - DEMO_W) / 2.0, top + INFO_H_UPRIGHT + 20.0, DEMO_W, DEMO_H);
        return (info, demo);
    }
    let width = 1220.0_f32.min(view.w - 40.0);
    let left = (view.w - width) / 2.0;
    let info = Rect::at(left, top, width - DEMO_W - 25.0, DEMO_H);
    let demo = Rect::at(info.x + info.w + 25.0, top, DEMO_W, DEMO_H);
    (info, demo)
}

fn tabs(info: Rect) -> [Rect; 2] {
    [0.0, 1.0].map(|i| Rect::at(info.x + 20.0 + i * 220.0, info.y + 8.0, 200.0, TAB_H - 8.0))
}

fn back_btn(view: View) -> Rect {
    Rect::at(40.0, view.h - 80.0, 200.0, 54.0)
}

pub fn enter(state: &mut State) {
    state.help = Some(HelpView {
        demo: Demo::new(),
        rules: false,
    });
    state.screen = Screen::Help;
}

pub fn update(app: &App, state: &mut State) {
    let view = state.ui.view();
    if let Some(help) = state.help.as_mut() {
        help.demo.update(app.timer.delta_f32());
        let [controls, rules] = tabs(cards(view).0);
        if state.ui.clicked(controls) {
            help.rules = false;
        } else if state.ui.clicked(rules) {
            help.rules = true;
        }
    }
    if state.ui.clicked(back_btn(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.help = None;
        state.screen = Screen::Menu;
    }
}

/// A small game controller, drawn in front of a pad button's name.
fn pad_icon(draw: &mut Draw, (x, cy): (f32, f32), color: Color, hole: Color) {
    draw.rect((x, cy - 8.0), (26.0, 16.0)).corner_radius(7.0).color(color);
    draw.rect((x + 5.0, cy - 1.0), (7.0, 2.0)).color(hole);
    draw.rect((x + 7.5, cy - 3.5), (2.0, 7.0)).color(hole);
    draw.circle(1.8).position(x + 19.0, cy - 2.0).color(hole);
    draw.circle(1.8).position(x + 21.5, cy + 2.0).color(hole);
}

/// A key or pad button as a small cap; returns where the next one goes.
fn chip(draw: &mut Draw, ui: &Ui, fonts: &Fonts, (x, cy): (f32, f32), label: &str, pad: bool) -> f32 {
    let pal = ui.palette();
    let size = theme::size::LABEL;
    let icon = if pad { 32.0 } else { 0.0 };
    let w = fonts.width(Face::Text, label, size) + 24.0 + icon;
    draw.rect((x, cy - CHIP_H / 2.0), (w, CHIP_H))
        .corner_radius(8.0)
        .color(pal.surface_alt);
    draw.rect((x, cy - CHIP_H / 2.0), (w, CHIP_H))
        .corner_radius(8.0)
        .stroke(1.5)
        .color(pal.border);
    if pad {
        pad_icon(draw, (x + 12.0, cy), theme::GOLD, pal.surface_alt);
    }
    draw.sharp_text(&fonts.text, label)
        .position(x + 12.0 + icon, cy)
        .size(size)
        .v_align_middle()
        .color(theme::GOLD);
    x + w + 8.0
}

fn controls_rows(bindings: &Bindings) -> Vec<(&'static str, Vec<String>, Option<&'static str>)> {
    let mut rows: Vec<_> = Action::ALL
        .into_iter()
        .map(|a| {
            let keys = (0..2)
                .filter_map(|slot| bindings.key(a, slot).map(|k| k.label.clone()))
                .collect();
            (a.label(), keys, bindings.pad(a).map(Pad::label))
        })
        .collect();
    rows.push(("Pause", vec!["Échap".to_owned()], Some(Pad::Start.label())));
    rows.push((
        "Rejouer (partie finie)",
        vec!["R".to_owned()],
        Some(Pad::Select.label()),
    ));
    rows
}

fn draw_controls(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect, bindings: &Bindings) {
    let pal = ui.palette();
    let label_w = 300.0_f32.min(card.w * 0.42);
    let mut y = card.y + TAB_H + 36.0;
    for (label, keys, pad) in controls_rows(bindings) {
        draw.sharp_text(&fonts.text, label)
            .position(card.x + 24.0, y)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(pal.text);
        let mut x = card.x + 24.0 + label_w;
        if keys.is_empty() && pad.is_none() {
            chip(draw, ui, fonts, (x, y), "aucune touche", false);
        }
        for key in &keys {
            x = chip(draw, ui, fonts, (x, y), key, false);
        }
        if let Some(pad) = pad {
            chip(draw, ui, fonts, (x, y), pad, true);
        }
        y += ROW_H;
    }
    let note = "Les touches se changent dans Paramètres. Sur téléphone, des boutons apparaissent \
                à l'écran dès que vous le touchez.";
    y += 4.0;
    for line in fonts.wrap(Face::Text, note, theme::size::BODY, card.w - 48.0) {
        draw.sharp_text(&fonts.text, line)
            .position(card.x + 24.0, y)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(pal.text_muted);
        y += LINE_H;
    }
}

fn draw_rules(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect) {
    let pal = ui.palette();
    let mut y = card.y + TAB_H + 30.0;
    for (title, text) in RULES {
        draw.sharp_text(&fonts.display, title)
            .position(card.x + 24.0, y)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(pal.accent);
        y += LINE_H + 4.0;
        for line in fonts.wrap(Face::Text, text, theme::size::BODY, card.w - 48.0) {
            draw.sharp_text(&fonts.text, line)
                .position(card.x + 24.0, y)
                .size(theme::size::BODY)
                .v_align_middle()
                .color(pal.text);
            y += LINE_H;
        }
        y += 8.0;
    }
}

/// The demo: its lesson above the board, the key just pressed below it, and
/// the explanation over the board while the game stands still.
fn draw_demo(draw: &mut Draw, ui: &Ui, fonts: &Fonts, card: Rect, demo: &Demo, time: f32) {
    let pal = ui.palette();
    ui::card(draw, &pal, card);
    let title = demo.title();
    let title = fonts.fit(Face::Display, &title, theme::size::LABEL, card.w - 24.0);
    draw.sharp_text(&fonts.display, &title)
        .position(card.x + card.w / 2.0, card.y + 22.0)
        .size(theme::size::LABEL)
        .h_align_center()
        .v_align_middle()
        .color(pal.text);
    let board = Rect::at(card.x + 20.0, card.y + 44.0, game_draw::BOARD_W, game_draw::BOARD_H);
    game_draw::draw_demo(draw, fonts, demo, board, time);
    if let Some((key, left)) = demo.key {
        draw.sharp_text(&fonts.display, key)
            .position(card.x + card.w / 2.0, board.y + board.h + 24.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text.with_alpha(left.min(1.0)));
    }
    if let Some(text) = demo.text() {
        let lines = fonts.wrap(Face::Text, text, theme::size::BODY, board.w - 28.0);
        let h = lines.len() as f32 * LINE_H + 24.0;
        let bubble = Rect::at(board.x + 6.0, board.y + 50.0, board.w - 12.0, h);
        draw.rect((bubble.x, bubble.y), (bubble.w, bubble.h))
            .corner_radius(10.0)
            .color(Color::BLACK.with_alpha(0.78));
        for (i, line) in lines.iter().enumerate() {
            draw.sharp_text(&fonts.text, line)
                .position(bubble.x + bubble.w / 2.0, bubble.y + 12.0 + LINE_H * (i as f32 + 0.5))
                .size(theme::size::BODY)
                .h_align_center()
                .v_align_middle()
                .color(Color::WHITE);
        }
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
    let (info, demo_card) = cards(view);
    ui::card(&mut draw, &pal, info);
    let rules = state.help.as_ref().is_some_and(|h| h.rules);
    let [controls_tab, rules_tab] = tabs(info);
    state.ui.tab(&mut draw, &state.fonts, controls_tab, "Contrôles", !rules);
    state.ui.tab(&mut draw, &state.fonts, rules_tab, "Règles", rules);
    if rules {
        draw_rules(&mut draw, &state.ui, &state.fonts, info);
    } else {
        draw_controls(&mut draw, &state.ui, &state.fonts, info, &state.controls.bindings);
    }
    if let Some(help) = &state.help {
        draw_demo(
            &mut draw,
            &state.ui,
            &state.fonts,
            demo_card,
            &help.demo,
            state.ui.time() as f32,
        );
    }
    state.ui.button(&mut draw, &state.fonts, back_btn(view), "Retour");
    state.ui.render(gfx, &draw);
}
