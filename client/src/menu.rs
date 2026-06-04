use notan::prelude::*;
use notan::draw::*;
use crate::state::{State, Screen, Settings, GameSession};

#[derive(Clone, Copy)]
pub struct Btn {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Btn {
    pub fn contains(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x + self.w && my >= self.y && my <= self.y + self.h
    }

    pub fn clicked(&self, app: &App) -> bool {
        self.contains(app.mouse.x, app.mouse.y) && app.mouse.left_was_pressed()
    }

    pub fn draw(&self, draw: &mut Draw, app: &App, font: &crate::Font, label: &str) {
        let hover = self.contains(app.mouse.x, app.mouse.y);
        let bg = if hover { Color::from_rgb(0.28, 0.30, 0.42) } else { Color::from_rgb(0.18, 0.19, 0.26) };
        draw.rect((self.x, self.y), (self.w, self.h)).color(bg);
        draw.rect((self.x, self.y), (self.w, self.h)).stroke(2.0).color(Color::from_rgb(0.45, 0.47, 0.6));
        draw.text(font, label)
            .position(self.x + self.w / 2.0, self.y + self.h / 2.0)
            .size(28.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::WHITE);
    }
}

struct MenuLayout {
    play_1v1: Btn,
    settings: Btn,
}

fn menu_layout(win_w: f32, win_h: f32) -> MenuLayout {
    let w = 280.0;
    let h = 70.0;
    let x = (win_w - w) / 2.0;
    let cy = win_h / 2.0;
    MenuLayout {
        play_1v1: Btn { x, y: cy - 20.0, w, h },
        settings: Btn { x, y: cy + 70.0, w, h },
    }
}

pub fn update_menu(app: &mut App, state: &mut State) {
    let layout = menu_layout(win_w(app), win_h(app));
    if layout.play_1v1.clicked(app) {
        start_1v1(state);
    } else if layout.settings.clicked(app) {
        state.screen = Screen::Settings;
    }
}

pub fn draw_menu(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (win_w(app), win_h(app));
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, "Rouillo")
        .position(ww / 2.0, wh / 2.0 - 140.0)
        .size(80.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.9, 0.7, 1.0));

    let layout = menu_layout(ww, wh);
    layout.play_1v1.draw(&mut draw, app, &state.font, "1 vs 1");
    layout.settings.draw(&mut draw, app, &state.font, "Settings");

    gfx.render(&draw);
}

struct SettingsLayout {
    minus: [Btn; Settings::COUNT],
    plus: [Btn; Settings::COUNT],
    back: Btn,
}

fn settings_layout(win_w: f32, win_h: f32) -> SettingsLayout {
    let row_h = 70.0;
    let btn = 50.0;
    let first_y = win_h / 2.0 - (Settings::COUNT as f32 * row_h) / 2.0;
    let center_x = win_w / 2.0;
    let minus_x = center_x + 60.0;
    let plus_x = center_x + 200.0;

    let mut minus = [Btn { x: 0.0, y: 0.0, w: btn, h: btn }; Settings::COUNT];
    let mut plus = minus;
    for i in 0..Settings::COUNT {
        let y = first_y + i as f32 * row_h;
        minus[i] = Btn { x: minus_x, y, w: btn, h: btn };
        plus[i] = Btn { x: plus_x, y, w: btn, h: btn };
    }

    SettingsLayout {
        minus,
        plus,
        back: Btn { x: center_x - 100.0, y: first_y + Settings::COUNT as f32 * row_h + 40.0, w: 200.0, h: 60.0 },
    }
}

pub fn update_settings(app: &mut App, state: &mut State) {
    let layout = settings_layout(win_w(app), win_h(app));
    for i in 0..Settings::COUNT {
        if layout.minus[i].clicked(app) {
            state.settings.adjust(i, -1);
        }
        if layout.plus[i].clicked(app) {
            state.settings.adjust(i, 1);
        }
    }
    if layout.back.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::Menu;
    }
}

pub fn draw_settings(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (win_w(app), win_h(app));
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, "SETTINGS")
        .position(ww / 2.0, wh / 2.0 - 170.0)
        .size(50.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::WHITE);

    let layout = settings_layout(ww, wh);
    let center_x = ww / 2.0;
    for i in 0..Settings::COUNT {
        let y = layout.minus[i].y;
        let mid = y + layout.minus[i].h / 2.0;
        draw.text(&state.font, Settings::label(i))
            .position(center_x - 100.0, mid)
            .size(24.0)
            .h_align_right()
            .v_align_middle()
            .color(Color::from_rgb(0.8, 0.8, 0.85));
        layout.minus[i].draw(&mut draw, app, &state.font, "-");
        layout.plus[i].draw(&mut draw, app, &state.font, "+");
        draw.text(&state.font, &format!("{:.0} ms", state.settings.value(i) * 1000.0))
            .position(center_x + 175.0, mid)
            .size(22.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::YELLOW);
    }

    layout.back.draw(&mut draw, app, &state.font, "Back");
    gfx.render(&draw);
}

pub fn back_to_menu_button(win_w: f32, win_h: f32) -> Btn {
    let w = 240.0;
    let h = 56.0;
    Btn { x: (win_w - w) / 2.0, y: win_h / 2.0 + 130.0, w, h }
}

fn start_1v1(state: &mut State) {
    match ewebsock::connect(&crate::server_url(), ewebsock::Options::default()) {
        Ok((ws_sender, ws_receiver)) => {
            state.session = Some(GameSession::new(ws_sender, ws_receiver));
            state.screen = Screen::Game;
        }
        Err(e) => {
            eprintln!("Connexion au serveur impossible : {e}");
        }
    }
}

fn win_w(app: &mut App) -> f32 {
    app.window().width() as f32
}

fn win_h(app: &mut App) -> f32 {
    app.window().height() as f32
}
