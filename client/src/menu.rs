use notan::draw::*;
use notan::prelude::*;

use crate::http;
use crate::state::{AuthForm, Net, Screen, Settings, State};

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
        self.draw_styled(draw, app, font, label, true);
    }

    pub fn draw_styled(&self, draw: &mut Draw, app: &App, font: &crate::Font, label: &str, enabled: bool) {
        let hover = enabled && self.contains(app.mouse.x, app.mouse.y);
        let bg = if !enabled {
            Color::from_rgb(0.11, 0.11, 0.14)
        } else if hover {
            Color::from_rgb(0.28, 0.30, 0.42)
        } else {
            Color::from_rgb(0.18, 0.19, 0.26)
        };
        let border = if enabled {
            Color::from_rgb(0.45, 0.47, 0.6)
        } else {
            Color::from_rgb(0.28, 0.28, 0.33)
        };
        let text = if enabled {
            Color::WHITE
        } else {
            Color::from_rgb(0.45, 0.45, 0.5)
        };
        draw.rect((self.x, self.y), (self.w, self.h)).color(bg);
        draw.rect((self.x, self.y), (self.w, self.h)).stroke(2.0).color(border);
        let n = label.chars().count().max(1) as f32;
        let font_size = (28.0_f32 * (self.w - 20.0) / (n * 17.5)).min(28.0).max(11.0);
        draw.text(font, label)
            .position(self.x + self.w / 2.0, self.y + self.h / 2.0)
            .size(font_size)
            .h_align_center()
            .v_align_middle()
            .color(text);
    }
}

pub fn draw_text_box(
    draw: &mut Draw,
    font: &crate::Font,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    placeholder: &str,
    value: &str,
    focused: bool,
) {
    let border = if focused {
        Color::from_rgb(0.6, 0.5, 0.9)
    } else {
        Color::from_rgb(0.35, 0.37, 0.5)
    };
    draw.rect((x, y), (w, h)).color(Color::from_rgb(0.12, 0.13, 0.20));
    draw.rect((x, y), (w, h)).stroke(2.0).color(border);
    let (text, col) = if value.is_empty() {
        (placeholder, Color::from_rgb(0.45, 0.45, 0.55))
    } else {
        (value, Color::WHITE)
    };
    draw.text(font, text)
        .position(x + 12.0, y + h / 2.0)
        .size(20.0)
        .v_align_middle()
        .color(col);
}

struct MenuLayout {
    play: Btn,
    settings: Btn,
    friends: Option<Btn>,
    logout: Option<Btn>,
}

fn menu_layout(win_w: f32, win_h: f32, logged_in: bool) -> MenuLayout {
    let w = 280.0;
    let h = 70.0;
    let x = (win_w - w) / 2.0;
    let cy = win_h / 2.0;
    let (friends, logout) = if logged_in {
        (
            Some(Btn {
                x,
                y: cy + 160.0,
                w,
                h: 56.0,
            }),
            Some(Btn {
                x,
                y: cy + 240.0,
                w,
                h: 56.0,
            }),
        )
    } else {
        (None, None)
    };
    MenuLayout {
        play: Btn { x, y: cy - 20.0, w, h },
        settings: Btn { x, y: cy + 70.0, w, h },
        friends,
        logout,
    }
}

fn avatar_pos(ww: f32) -> (f32, f32, f32) {
    (ww - 70.0, 70.0, 38.0)
}

fn avatar_hovered(app: &App, cx: f32, cy: f32, r: f32) -> bool {
    let dx = app.mouse.x - cx;
    let dy = app.mouse.y - cy;
    dx * dx + dy * dy <= r * r
}

fn avatar_clicked(app: &App, cx: f32, cy: f32, r: f32) -> bool {
    avatar_hovered(app, cx, cy, r) && app.mouse.left_was_pressed()
}

pub fn update_menu(app: &mut App, state: &mut State) {
    let (ww, wh) = (win_w(app), win_h(app));
    let logged_in = state.auth.is_some();
    let layout = menu_layout(ww, wh, logged_in);

    if layout.play.clicked(app) {
        start_play(state);
    } else if layout.settings.clicked(app) {
        state.screen = Screen::Settings;
    }

    if let Some(btn) = layout.friends {
        if btn.clicked(app) {
            crate::friends::enter_friends(state);
            state.screen = Screen::Friends;
        }
    }

    if let Some(btn) = layout.logout {
        if btn.clicked(app) {
            do_logout(state);
        }
    }

    if logged_in {
        let (acx, acy, ar) = avatar_pos(ww);
        if avatar_clicked(app, acx, acy, ar) {
            crate::profile::enter_profile(state);
            state.screen = Screen::Profile;
        }
    }
}

pub fn do_logout(state: &mut State) {
    if let Some(auth) = &state.auth {
        http::post_empty(http::api_url("logout"), Some(auth.token.clone()), http::new_slot());
    }
    crate::state::clear_stored_token();
    state.auth = None;
    state.auth_form = AuthForm::default();
    state.friends = None;
    state.profile = None;
    state.other_profile = None;
    state.screen = Screen::Auth;
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

    let logged_in = state.auth.is_some();
    let layout = menu_layout(ww, wh, logged_in);
    layout.play.draw(&mut draw, app, &state.font, "Jouer");
    layout.settings.draw(&mut draw, app, &state.font, "Paramètres");
    if let Some(btn) = layout.friends {
        btn.draw(&mut draw, app, &state.font, "Amis");
    }
    if let Some(btn) = layout.logout {
        btn.draw(&mut draw, app, &state.font, "Déconnexion");
    }

    if let Some(auth) = &state.auth {
        let (acx, acy, ar) = avatar_pos(ww);
        let hover = avatar_hovered(app, acx, acy, ar);
        let fill = if hover {
            Color::from_rgb(0.38, 0.28, 0.55)
        } else {
            Color::from_rgb(0.25, 0.18, 0.40)
        };
        draw.circle(ar).position(acx, acy).color(fill);
        draw.circle(ar)
            .position(acx, acy)
            .stroke(2.0)
            .color(Color::from_rgb(0.65, 0.45, 0.85));
        let initial: String = auth
            .username
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        draw.text(&state.font, &initial)
            .position(acx, acy)
            .size(30.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::WHITE);
        draw.text(&state.font, &auth.username)
            .position(acx, acy + ar + 16.0)
            .size(17.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.65, 0.65, 0.80));
    }

    if !state.notice.is_empty() {
        draw.text(&state.font, &state.notice)
            .position(ww / 2.0, wh - 60.0)
            .size(22.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.9, 0.4, 0.4));
    }

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

    let mut minus = [Btn {
        x: 0.0,
        y: 0.0,
        w: btn,
        h: btn,
    }; Settings::COUNT];
    let mut plus = minus;
    for i in 0..Settings::COUNT {
        let y = first_y + i as f32 * row_h;
        minus[i] = Btn {
            x: minus_x,
            y,
            w: btn,
            h: btn,
        };
        plus[i] = Btn {
            x: plus_x,
            y,
            w: btn,
            h: btn,
        };
    }

    SettingsLayout {
        minus,
        plus,
        back: Btn {
            x: center_x - 100.0,
            y: first_y + Settings::COUNT as f32 * row_h + 40.0,
            w: 200.0,
            h: 60.0,
        },
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
            .position(center_x + 130.0, mid)
            .size(22.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::YELLOW);
    }

    layout.back.draw(&mut draw, app, &state.font, "Back");
    gfx.render(&draw);
}

fn start_play(state: &mut State) {
    match ewebsock::connect(crate::server_url(), ewebsock::Options::default()) {
        Ok((ws_sender, ws_receiver)) => {
            state.net = Some(Net { ws_sender, ws_receiver });
            state.rooms.clear();
            state.notice.clear();
            state.screen = Screen::RoomBrowser;
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
