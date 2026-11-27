use aero::draw::Draw;
use aero::geom::Color;
use aero::input::KeyCode;
use aero::math::vec2;

use crate::controls::{Action, Bindings};
use crate::demo::Demo;
use crate::nav::Transition;
use crate::pads::Pad;
use crate::state::{Screen, State};
use crate::tick::Tick;
use crate::ui::{self, px, widgets, Face, Fonts, Frame, Gui, Icon, Rect, Stage, Typeset, View, FILL};
use crate::{draw as game_draw, theme};

const RULES: [(&str, &str); 7] = [
    ("Le but", "Empilez les paires qui tombent. Quatre Puyos de la même couleur qui se touchent disparaissent."),
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
    ("All Clear", "Videz tout le plateau: votre chaîne suivante enverra une grosse attaque en plus."),
    ("La défaite", "La partie est perdue quand la case marquée d'une croix, en haut du plateau, est occupée."),
    ("Les niveaux", "Avec le temps, les paires tombent plus vite et les attaques deviennent plus fortes."),
    ("Le classé", "Une série se joue en trois manches gagnantes. Seul le classé fait bouger l'ELO."),
];

/// The help screen: a demo that plays lessons, next to a card showing either
/// the controls or the rules.
pub struct HelpView {
    pub demo: Demo,
    rules: bool,
}

/// Wide enough for the match between two CPUs to stay readable.
const DEMO_W: f32 = 440.0;
const DEMO_H: f32 = 580.0;
const INFO_H_UPRIGHT: f32 = 640.0;
const TAB_H: f32 = 56.0;
const ROW_H: f32 = 52.0;
const CHIP_H: f32 = 38.0;
const LINE_H: f32 = 25.0;

/// The tabbed card and the demo card: side by side, or the demo below.
fn cards(view: View) -> (Rect, Rect) {
    let area = Rect::at(0.0, theme::HEADER_H + 20.0, view.w, view.h).inset([0.0, 20.0, 0.0, 20.0]);

    if view.portrait() {
        let [info, demo] = area.rows(20.0, [px(INFO_H_UPRIGHT), px(DEMO_H)]);
        return (info, demo.max_width(DEMO_W));
    }

    let row = area.top_strip(DEMO_H).max_width(1220.0);
    let demo_w = DEMO_W.min(row.w() * 0.36);
    let [info, demo] = row.columns(25.0, [FILL, px(demo_w)]);
    (info, demo)
}

fn tabs(info: Rect) -> [Rect; 2] {
    info.inset([8.0, 0.0, 0.0, 20.0]).top_strip(TAB_H - 8.0).left_strip(420.0).columns(20.0, [FILL, FILL])
}

/// The buttons to the lesson before and after, around its title.
fn lesson_arrows(demo_card: Rect) -> [Rect; 2] {
    let row = demo_card.inset([6.0, 8.0, 0.0, 8.0]).top_strip(32.0);
    [row.left_strip(40.0), row.right_strip(40.0)]
}

fn back_btn(view: View) -> Rect {
    Frame::of(view).back
}

pub fn enter() -> Transition {
    Transition::To(Screen::Help(Box::new(HelpView { demo: Demo::new(), rules: false })))
}

/// The demo's time and what the arrows and Escape do; the clicks are in [`show`].
pub fn update(tick: &Tick, help: &mut HelpView) -> Transition {
    help.demo.update(tick.dt);
    if tick.pressed(KeyCode::ArrowLeft) {
        help.demo.previous();
    } else if tick.pressed(KeyCode::ArrowRight) {
        help.demo.next();
    }
    if tick.back() {
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}

/// A small game controller, drawn in front of a pad button's name.
fn pad_icon(draw: &mut Draw, (x, cy): (f32, f32), color: Color, hole: Color) {
    draw.rect(Rect::at(x, cy - 8.0, 26.0, 16.0)).radius(7.0).color(color);
    draw.rect(Rect::at(x + 5.0, cy - 1.0, 7.0, 2.0)).color(hole);
    draw.rect(Rect::at(x + 7.5, cy - 3.5, 2.0, 7.0)).color(hole);
    draw.circle(vec2(x + 19.0, cy - 2.0), 1.8).color(hole);
    draw.circle(vec2(x + 21.5, cy + 2.0), 1.8).color(hole);
}

/// A key or pad button as a small cap; returns where the next one goes.
fn chip(draw: &mut Draw, stage: &Stage, fonts: &mut Fonts, (x, cy): (f32, f32), label: &str, pad: bool) -> f32 {
    let pal = stage.palette();
    let size = theme::size::LABEL;
    let icon = if pad { 32.0 } else { 0.0 };
    let w = fonts.width(Face::Text, label, size) + 24.0 + icon;

    draw.rect(Rect::at(x, cy - CHIP_H / 2.0, w, CHIP_H)).radius(8.0).color(pal.surface_alt);
    draw.rect(Rect::at(x, cy - CHIP_H / 2.0, w, CHIP_H)).radius(8.0).stroke(pal.border, 1.5);
    if pad {
        pad_icon(draw, (x + 12.0, cy), theme::GOLD, pal.surface_alt);
    }
    fonts.write(draw, Face::Text, label).left(x + 12.0 + icon, cy).size(size).color(theme::GOLD);

    x + w + 8.0
}

fn controls_rows(bindings: &Bindings) -> Vec<(&'static str, Vec<String>, Option<&'static str>)> {
    let mut rows: Vec<_> = Action::ALL
        .into_iter()
        .map(|a| {
            let keys = (0..2).filter_map(|slot| bindings.key(a, slot).map(|k| k.label.clone())).collect();
            (a.label(), keys, bindings.pad(a).map(Pad::label))
        })
        .collect();
    rows.push(("Pause", vec!["Échap".to_owned()], Some(Pad::Start.label())));
    rows.push(("Rejouer (partie finie)", vec!["R".to_owned()], Some(Pad::Select.label())));

    rows
}

fn draw_controls(draw: &mut Draw, stage: &Stage, fonts: &mut Fonts, card: Rect, bindings: &Bindings) {
    let pal = stage.palette();
    let label_w = 300.0_f32.min(card.w() * 0.42);
    let note = "Les touches se changent dans Paramètres. Sur téléphone, des boutons apparaissent \
                à l'écran dès que vous le touchez.";
    let note = fonts.wrap(Face::Text, note, theme::size::BODY, card.w() - 48.0);
    let rows = controls_rows(bindings);
    let room = card.h() - TAB_H - 36.0 - note.len() as f32 * LINE_H - 16.0;
    let row_h = ROW_H.min(room / rows.len() as f32);

    let mut y = card.y() + TAB_H + 36.0;
    for (label, keys, pad) in rows {
        fonts.write(draw, Face::Text, label).left(card.x() + 24.0, y).size(theme::size::EMPHASIS).color(pal.text);
        let mut x = card.x() + 24.0 + label_w;
        if keys.is_empty() && pad.is_none() {
            chip(draw, stage, fonts, (x, y), "aucune touche", false);
        }
        for key in &keys {
            x = chip(draw, stage, fonts, (x, y), key, false);
        }
        if let Some(pad) = pad {
            chip(draw, stage, fonts, (x, y), pad, true);
        }
        y += row_h;
    }

    y += 4.0;
    for line in note {
        fonts.write(draw, Face::Text, line).left(card.x() + 24.0, y).size(theme::size::BODY).color(pal.text_muted);
        y += LINE_H;
    }
}

fn draw_rules(draw: &mut Draw, stage: &Stage, fonts: &mut Fonts, card: Rect) {
    let pal = stage.palette();
    let mut y = card.y() + TAB_H + 30.0;
    for (title, text) in RULES {
        fonts.write(draw, Face::Display, title).left(card.x() + 24.0, y).size(theme::size::EMPHASIS).color(pal.accent);
        y += LINE_H + 4.0;
        for line in fonts.wrap(Face::Text, text, theme::size::BODY, card.w() - 48.0) {
            fonts.write(draw, Face::Text, line).left(card.x() + 24.0, y).size(theme::size::BODY).color(pal.text);
            y += LINE_H;
        }
        y += 8.0;
    }
}

/// The demo: its lesson above the board, the key just pressed below it, and
/// the explanation over the board while the game stands still.
/// The demo's card; returns the lesson its arrows step to: -1, 1, or 0.
fn show_demo(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, card: Rect, demo: &Demo, time: f32) -> i32 {
    let pal = gui.stage.palette();
    let title = demo.title();
    let title = ui.cx.fonts.fit(Face::Display, &title, theme::size::LABEL, card.w() - 110.0);
    let board = if demo.rival.is_some() {
        Rect::at(card.x() + 20.0, card.y() + 44.0, card.w() - 40.0, game_draw::BOARD_H)
    } else {
        let x = card.x() + (card.w() - game_draw::BOARD_W) / 2.0;
        Rect::at(x, card.y() + 44.0, game_draw::BOARD_W, game_draw::BOARD_H)
    };
    let [previous, next] = lesson_arrows(card);

    ui::card(ui.draw, &pal, card);
    ui.cx
        .fonts
        .write(ui.draw, Face::Display, &title)
        .center(card.x() + card.w() / 2.0, card.y() + 22.0)
        .size(theme::size::LABEL)
        .color(pal.text);
    game_draw::draw_demo(ui.draw, &mut ui.cx.fonts, demo, board, time);
    let step = i32::from(widgets::button(ui, "next lesson", next, Icon::Next, true))
        - i32::from(widgets::button(ui, "previous lesson", previous, Icon::Previous, true));
    if let Some((key, left)) = demo.key {
        ui.cx
            .fonts
            .write(ui.draw, Face::Display, key)
            .center(card.x() + card.w() / 2.0, board.y() + board.h() + 24.0)
            .size(theme::size::EMPHASIS)
            .color(pal.text.with_alpha(left.min(1.0)));
    }
    if let Some(text) = demo.text() {
        let lines = ui.cx.fonts.wrap(Face::Text, text, theme::size::BODY, board.w() - 28.0);
        let h = lines.len() as f32 * LINE_H + 24.0;
        let (boards, scale) = game_draw::demo_boards(demo, board);
        let bubble = Rect::at(boards.x() + 6.0, boards.y() + 50.0 * scale, boards.w() - 12.0, h);

        ui.draw.rect(bubble).radius(10.0).color(Color::BLACK.with_alpha(0.78));
        for (i, line) in lines.iter().enumerate() {
            ui.cx
                .fonts
                .write(ui.draw, Face::Text, line)
                .center(bubble.x() + bubble.w() / 2.0, bubble.y() + 12.0 + LINE_H * (i as f32 + 0.5))
                .size(theme::size::BODY)
                .color(Color::WHITE);
        }
    }
    step
}

pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &State, help: &mut HelpView) -> Transition {
    let pal = gui.stage.palette();
    let view = gui.stage.view();
    let (info, demo_card) = cards(view);
    let [controls_tab, rules_tab] = tabs(info);

    let mut canvas = gui.stage.screen_canvas(ui);
    let ui = &mut *canvas;
    gui.stage.page_header(ui, "Comment jouer");
    ui::card(ui.draw, &pal, info);
    if widgets::tab(ui, controls_tab, "Contrôles", !help.rules) {
        help.rules = false;
    }
    if widgets::tab(ui, rules_tab, "Règles", help.rules) {
        help.rules = true;
    }
    if help.rules {
        draw_rules(ui.draw, &gui.stage, &mut ui.cx.fonts, info);
    } else {
        draw_controls(ui.draw, &gui.stage, &mut ui.cx.fonts, info, &state.input.controls.bindings);
    }
    let time = gui.stage.time() as f32;
    match show_demo(ui, gui, demo_card, &help.demo, time) {
        -1 => help.demo.previous(),
        1 => help.demo.next(),
        _ => {}
    }
    if widgets::button(ui, "back", back_btn(view), "Retour", true) {
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}
