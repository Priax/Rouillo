mod bar;
mod button;
mod controls;
mod deco;
mod field;
mod fonts;
mod input;
mod keys;
mod modal;
mod pager;
mod panel;
mod rect;
mod screen;
mod status;
mod stepper;
mod text;
mod triangles;
mod view;

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub use button::Icon;
pub use deco::{banner, card, divider, list_row, pills_ending_at, portrait, Edge, Persona, Pill};
mod clipboard;
pub use clipboard::get as clipboard_text;
pub use field::{area_clicked, area_height, area_keys, field_clicked, text_area, text_field, Field};
pub use fonts::{Face, Fonts};
pub use input::TextInput;
#[cfg(not(target_arch = "wasm32"))]
pub use keys::ctrl_letter;
pub use keys::EditKeys;
#[cfg(target_arch = "wasm32")]
pub use keys::Shortcuts;
pub use modal::Modal;
use notan::draw::{CreateDraw, Draw, DrawImages};
use notan::math::{vec2, Mat3};
use notan::prelude::{App, BlendMode, Color, Graphics, RenderTexture, TextureFilter};
pub use pager::{page_count, Pager, PAGER_H};
pub use panel::SettingsPanel;
pub use rect::Rect;
pub use status::Status;
pub use stepper::Stepper;
pub use text::SharpText;
pub use triangles::Triangles;
pub use view::View;

use crate::theme::{hue, Palette};

const MAX_DT: f32 = 0.1;
const SUPERSAMPLE: f32 = 2.0;
const MAX_TARGET: f32 = 4096.0;
pub const TRANSITION: f32 = 0.3;

const HOVER_SPEED: f32 = 18.0;
const PRESS_SPEED: f32 = 30.0;
const FLASH_SPEED: f32 = 8.0;
const CARET_BLINK: f64 = 0.5;

#[derive(Default)]
pub struct Ui {
    inner: RefCell<Inner>,
}

struct Inner {
    target: Option<RenderTexture>,
    dt: f32,
    time: f64,
    view: View,
    hue: f32,
    from_hue: f32,
    target_hue: f32,
    screen: Option<usize>,
    entered_at: f64,
    fade: f32,
    palette: Palette,
    mouse: Mouse,
    input_off: bool,
    widgets: HashMap<(u64, u32), Widget>,
    drawn: HashMap<u64, u32>,
    caret_text: Option<u64>,
    caret_since: f64,
    /// The widget a gamepad points at, and whether the pad is in use: the
    /// mouse moving hands control back to it.
    focus: Option<(u64, u32)>,
    pad: bool,
    pointer: (f32, f32),
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            target: None,
            dt: 0.0,
            time: 0.0,
            view: View::default(),
            hue: hue::PURPLE,
            from_hue: hue::PURPLE,
            target_hue: hue::PURPLE,
            screen: None,
            entered_at: f64::NEG_INFINITY,
            fade: TRANSITION,
            palette: Palette::new(hue::PURPLE),
            mouse: Mouse::default(),
            input_off: false,
            widgets: HashMap::new(),
            drawn: HashMap::new(),
            caret_text: None,
            caret_since: 0.0,
            focus: None,
            pad: false,
            pointer: (0.0, 0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    Whole,
    Bar,
    /// A whole rect the gamepad skips: it has its own button.
    Pointer,
}

struct Widget {
    rect: Rect,
    area: Area,
    live: bool,
    hover: f32,
    press: f32,
    flash: f32,
    hovered: bool,
    touched: bool,
}

impl Widget {
    fn contains(&self, x: f32, y: f32) -> bool {
        match self.area {
            Area::Whole | Area::Pointer => self.rect.contains(x, y),
            Area::Bar => bar::contains(self.rect, x, y),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Mouse {
    pub x: f32,
    pub y: f32,
    pub down: bool,
    pub pressed: bool,
}

impl Mouse {
    pub fn of(app: &App, view: View) -> Self {
        Self {
            x: app.mouse.x / view.scale,
            y: app.mouse.y / view.scale,
            down: app.mouse.left_is_down(),
            pressed: app.mouse.left_was_pressed(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Response {
    hover: f32,
    press: f32,
    flash: f32,
    entered: bool,
}

impl Ui {
    /// Called at the start of each frame, before the screen reacts to input:
    /// a click lands on the buttons drawn last frame, the ones the player saw,
    /// even when the screen it triggers is a different one. Returns whether a
    /// button was clicked.
    pub fn begin_frame(&mut self, dt: f32, view: View, mouse: Mouse) -> bool {
        let inner = self.inner.get_mut();
        inner.dt = dt.clamp(0.0, MAX_DT);
        inner.time += f64::from(inner.dt);
        inner.view = view;
        let turn = (inner.target_hue - inner.from_hue + 540.0).rem_euclid(360.0) - 180.0;
        let progress = ease_out(transition_progress(inner.time, inner.entered_at, inner.fade));
        inner.hue = (inner.from_hue + turn * progress).rem_euclid(360.0);
        inner.palette = Palette::new(inner.hue);
        inner.mouse = mouse;
        inner.input_off = false;
        inner.widgets.retain(|_, w| std::mem::take(&mut w.touched));
        inner.drawn.clear();

        let mut clicked = false;
        if mouse.pressed {
            for w in inner.widgets.values_mut() {
                if w.live && w.contains(mouse.x, mouse.y) {
                    w.flash = 1.0;
                    clicked = true;
                }
            }
        }
        if clicked {
            crate::audio::play_ui_click();
        }
        clicked
    }

    /// Tells which screen is shown, and its section's hue. A change of
    /// screen starts a transition of `fade` seconds: the new one fades and
    /// slides in while the palette moves to its hue, both along the same
    /// curve. The first screen shows up at once.
    pub fn set_screen(&self, screen: usize, hue: f32, fade: f32) {
        let mut inner = self.inner.borrow_mut();
        match inner.screen {
            Some(current) if current == screen => {}
            Some(_) => {
                inner.screen = Some(screen);
                inner.focus = None;
                inner.from_hue = inner.hue;
                inner.target_hue = hue;
                inner.entered_at = inner.time;
                inner.fade = fade;
            }
            None => {
                inner.screen = Some(screen);
                inner.hue = hue;
                inner.from_hue = hue;
                inner.target_hue = hue;
                inner.palette = Palette::new(hue);
            }
        }
    }

    /// How far the current screen is into its entrance, eased, in `0..=1`.
    fn transition(&self) -> f32 {
        let inner = self.inner.borrow();
        ease_out(transition_progress(inner.time, inner.entered_at, inner.fade))
    }

    pub fn waiting_dots(&self) -> String {
        ".".repeat(1 + (self.time() * 2.0) as usize % 3)
    }

    pub fn time(&self) -> f64 {
        self.inner.borrow().time
    }

    pub fn palette(&self) -> Palette {
        self.inner.borrow().palette
    }

    pub fn view(&self) -> View {
        self.inner.borrow().view
    }

    /// A canvas for this frame. Everything is drawn into a texture
    /// `SUPERSAMPLE` times the window's size and scaled down by `present`,
    /// which smooths every edge: notan draws shapes as bare triangles, and
    /// multisampling cannot be relied on (Wayland's EGL refuses it here).
    pub fn canvas(&self, gfx: &mut Graphics) -> Draw {
        let view = self.view();
        let mut draw = self.target(gfx).create_draw();
        let scale = view.scale * supersample(view);
        draw.transform().push(Mat3::from_scale(vec2(scale, scale)));
        draw.set_alpha_mode(Some(BlendMode::OVER));
        draw
    }

    pub fn render(&self, gfx: &mut Graphics, draw: &Draw) {
        let target = self.target(gfx);
        gfx.render_to(&target, draw);
    }

    pub fn present(&self, gfx: &mut Graphics) {
        let view = self.view();
        let target = self.target(gfx);
        let mut draw = gfx.create_draw();
        draw.clear(Color::BLACK);
        draw.image(&target).size(view.w * view.scale, view.h * view.scale);
        gfx.render(&draw);
    }

    fn target(&self, gfx: &mut Graphics) -> RenderTexture {
        let view = self.view();
        let factor = view.scale * supersample(view);
        let size = ((view.w * factor).ceil() as u32, (view.h * factor).ceil() as u32);
        let mut inner = self.inner.borrow_mut();
        if let Some(target) = inner
            .target
            .as_ref()
            .filter(|t| t.size() == (size.0 as f32, size.1 as f32))
        {
            return target.clone();
        }
        let target = gfx
            .create_render_texture(size.0.max(1), size.1.max(1))
            .with_filter(TextureFilter::Linear, TextureFilter::Linear)
            .build()
            .expect("a render texture the size of the window");
        inner.target = Some(target.clone());
        target
    }

    pub fn clicked(&self, r: Rect) -> bool {
        self.click_in(r).is_some()
    }

    pub fn click_in(&self, r: Rect) -> Option<(f32, f32)> {
        let m = self.inner.borrow().mouse;
        (m.pressed && r.contains(m.x, m.y)).then_some((m.x, m.y))
    }

    pub fn pressed(&self, label: &str) -> bool {
        let inner = self.inner.borrow();
        let (m, label) = (inner.mouse, hash_of(label));
        m.pressed
            && inner
                .widgets
                .iter()
                .any(|(&(l, _), w)| l == label && w.live && w.contains(m.x, m.y))
    }

    pub fn bar_clicked(&self, row: Rect) -> bool {
        let m = self.inner.borrow().mouse;
        m.pressed && bar::contains(row, m.x, m.y)
    }

    /// Whether the caret of the focused field, standing at `at` in `text`,
    /// is lit this frame. It blinks, and starts over lit whenever the text
    /// changes or the caret moves, so that it never vanishes under the typing.
    /// Leaves gamepad control as soon as the mouse moves.
    pub fn note_pointer(&mut self, real: Mouse) {
        let inner = self.inner.get_mut();
        if (real.x, real.y) != inner.pointer {
            inner.pointer = (real.x, real.y);
            inner.pad = false;
        }
    }

    /// A click on the widget the gamepad points at, to stand for the mouse.
    pub fn pad_click(&self) -> Option<Mouse> {
        let inner = self.inner.borrow();
        let w = inner.widgets.get(&inner.focus?).filter(|w| inner.pad && w.live)?;
        let (x, y) = w.rect.center();
        Some(Mouse {
            x,
            y,
            down: true,
            pressed: true,
        })
    }

    /// Moves the gamepad's pointer to the nearest live widget in `dir`, or
    /// onto the screen's first widget if it pointed at none.
    pub fn move_focus(&self, (dx, dy): (f32, f32)) {
        let mut inner = self.inner.borrow_mut();
        let live: Vec<((u64, u32), (f32, f32))> = inner
            .widgets
            .iter()
            .filter(|(_, w)| w.live && w.area != Area::Pointer)
            .map(|(k, w)| (*k, w.rect.center()))
            .collect();
        let current = inner.focus.and_then(|f| live.iter().find(|(k, _)| *k == f).copied());
        let next = match current.filter(|_| inner.pad) {
            None => live
                .iter()
                .min_by(|a, b| {
                    (a.1 .1, a.1 .0)
                        .partial_cmp(&(b.1 .1, b.1 .0))
                        .unwrap_or(Ordering::Equal)
                })
                .map(|(k, _)| *k),
            Some((_, (cx, cy))) => live
                .iter()
                .filter_map(|(k, (x, y))| {
                    let (ox, oy) = (x - cx, y - cy);
                    let along = ox * dx + oy * dy;
                    let across = (ox * dy - oy * dx).abs();
                    (along > 1.0).then_some((*k, along + 2.0 * across))
                })
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal))
                .map(|(k, _)| k)
                .or(inner.focus),
        };
        inner.focus = next;
        inner.pad = next.is_some();
    }

    pub fn pad_active(&self) -> bool {
        self.inner.borrow().pad
    }

    pub fn mouse(&self) -> Mouse {
        self.inner.borrow().mouse
    }

    pub fn caret_on(&self, text: &str, at: usize) -> bool {
        let mut inner = self.inner.borrow_mut();
        let text = Some(hash_of(&(text, at)));
        if inner.caret_text != text {
            inner.caret_text = text;
            inner.caret_since = inner.time;
        }
        (inner.time - inner.caret_since) % (2.0 * CARET_BLINK) < CARET_BLINK
    }

    pub fn set_input(&self, on: bool) {
        self.inner.borrow_mut().input_off = !on;
    }

    fn interact(&self, label: &str, rect: Rect, enabled: bool) -> Response {
        self.interact_in(label, rect, Area::Whole, enabled)
    }

    fn interact_in(&self, label: &str, rect: Rect, area: Area, enabled: bool) -> Response {
        let mut inner = self.inner.borrow_mut();
        let Inner {
            target: _,
            dt,
            time: _,
            view: _,
            hue: _,
            from_hue: _,
            target_hue: _,
            screen: _,
            entered_at: _,
            fade: _,
            palette: _,
            mouse,
            input_off,
            widgets,
            drawn,
            caret_text: _,
            caret_since: _,
            focus,
            pad,
            pointer: _,
        } = &mut *inner;
        let live = enabled && !*input_off;

        let label = hash_of(label);
        let nth = drawn.entry(label).or_insert(0);
        let key = (label, *nth);
        *nth += 1;

        let mut fresh = false;
        let w = widgets.entry(key).or_insert_with(|| {
            fresh = true;
            Widget {
                rect,
                area,
                live,
                hover: 0.0,
                press: 0.0,
                flash: 0.0,
                hovered: false,
                touched: false,
            }
        });
        w.rect = rect;
        w.area = area;
        let over = live
            && if *pad {
                *focus == Some(key)
            } else {
                w.contains(mouse.x, mouse.y)
            };
        let entered = over && !w.hovered && !fresh;
        w.live = live;
        w.hovered = over;
        w.touched = true;
        w.hover = approach(w.hover, f32::from(u8::from(over)), HOVER_SPEED, *dt);
        w.press = approach(w.press, f32::from(u8::from(over && mouse.down)), PRESS_SPEED, *dt);
        w.flash = approach(w.flash, 0.0, FLASH_SPEED, *dt);

        Response {
            hover: w.hover,
            press: w.press,
            flash: w.flash,
            entered,
        }
    }
}

fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn supersample(view: View) -> f32 {
    let largest = (view.w * view.scale).max(view.h * view.scale).max(1.0);
    SUPERSAMPLE.max(view.dpi).min(MAX_TARGET / largest)
}

fn transition_progress(time: f64, entered_at: f64, fade: f32) -> f32 {
    ((time - entered_at) / f64::from(fade)).clamp(0.0, 1.0) as f32
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn approach(current: f32, target: f32, speed: f32, dt: f32) -> f32 {
    current + (target - current) * (1.0 - (-speed * dt).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect::at(0.0, 0.0, 100.0, 40.0);
    const B: Rect = Rect::at(0.0, 100.0, 100.0, 40.0);
    const ON_A: Mouse = Mouse {
        x: 50.0,
        y: 20.0,
        down: false,
        pressed: false,
    };
    const AWAY: Mouse = Mouse {
        x: 500.0,
        y: 500.0,
        down: false,
        pressed: false,
    };
    const CLICK_A: Mouse = Mouse {
        down: true,
        pressed: true,
        ..ON_A
    };

    fn frame(ui: &mut Ui, mouse: Mouse, draws: &[(&str, Rect)]) -> Vec<Response> {
        ui.begin_frame(1.0 / 60.0, View::default(), mouse);
        draws.iter().map(|(label, r)| ui.interact(label, *r, true)).collect()
    }

    #[test]
    fn a_gamepad_walks_to_the_nearest_button_and_clicks_it() {
        let mut ui = Ui::default();
        let grid = [
            ("a", Rect::at(100.0, 100.0, 100.0, 40.0)),
            ("b", Rect::at(300.0, 100.0, 100.0, 40.0)),
            ("c", Rect::at(100.0, 200.0, 100.0, 40.0)),
            ("far", Rect::at(900.0, 600.0, 100.0, 40.0)),
        ];
        frame(&mut ui, AWAY, &grid);
        ui.note_pointer(AWAY);
        frame(&mut ui, AWAY, &grid);
        ui.move_focus((1.0, 0.0));
        let click = ui.pad_click().expect("the first button gets the focus");
        assert!(grid[0].1.contains(click.x, click.y), "starts at the top left");
        ui.move_focus((1.0, 0.0));
        let click = ui.pad_click().unwrap();
        assert!(grid[1].1.contains(click.x, click.y), "right goes to b");
        ui.move_focus((0.0, 1.0));
        let click = ui.pad_click().unwrap();
        assert!(
            grid[2].1.contains(click.x, click.y),
            "down from b lands on c, the nearest below"
        );
        ui.move_focus((0.0, -1.0));
        ui.move_focus((0.0, -1.0));
        let click = ui.pad_click().unwrap();
        assert!(grid[0].1.contains(click.x, click.y), "nothing above a: the focus stays");

        let hovered = frame(&mut ui, AWAY, &grid);
        assert!(
            hovered[0].hover > 0.0 && hovered[1].hover == 0.0,
            "the focus shows as a hover"
        );
        ui.note_pointer(ON_A);
        assert!(ui.pad_click().is_none(), "moving the mouse hands control back");
    }

    #[test]
    fn the_gamepad_skips_widgets_that_have_their_own_button() {
        let mut ui = Ui::default();
        let avatar = Rect::at(10.0, 10.0, 60.0, 60.0);
        let play = Rect::at(300.0, 300.0, 200.0, 50.0);
        let draw = |ui: &mut Ui| {
            ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
            ui.interact_in("avatar", avatar, Area::Pointer, true);
            ui.interact("play", play, true);
        };
        draw(&mut ui);
        ui.note_pointer(AWAY);
        draw(&mut ui);
        ui.move_focus((0.0, -1.0));
        let click = ui.pad_click().expect("a button gets the focus");
        assert!(play.contains(click.x, click.y), "the avatar, top left, is skipped");
        ui.move_focus((-1.0, -1.0));
        let click = ui.pad_click().unwrap();
        assert!(play.contains(click.x, click.y), "and never reached");
    }

    #[test]
    fn supersampling_stays_within_texture_limits() {
        assert!((supersample(View::fit(1280.0, 800.0)) - 2.0).abs() < f32::EPSILON);
        let big = View::fit(3840.0, 2160.0);
        assert!(3840.0 * supersample(big) <= MAX_TARGET + 0.5);
    }

    #[test]
    fn a_dense_screen_is_drawn_at_its_own_resolution() {
        let phone = View {
            dpi: 3.0,
            ..View::fit(393.0, 852.0)
        };
        assert!((supersample(phone) - 3.0).abs() < f32::EPSILON);
        let scaled = View {
            dpi: 1.25,
            ..View::fit(1536.0, 864.0)
        };
        assert!((supersample(scaled) - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_first_screen_shows_up_at_once() {
        let mut ui = Ui::default();
        ui.set_screen(0, hue::GREEN, TRANSITION);
        ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON);
        assert!((ui.inner.borrow().hue - hue::GREEN).abs() < f32::EPSILON);
    }

    #[test]
    fn a_new_screen_brings_its_colours_the_short_way_round() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        ui.set_screen(0, hue::PURPLE, TRANSITION);
        frame(&mut ui);
        ui.set_screen(1, hue::PINK, TRANSITION);
        assert!(ui.transition() < f32::EPSILON, "the new screen starts hidden");
        frame(&mut ui);
        let h = ui.inner.borrow().hue;
        assert!(h > hue::PURPLE && h < hue::PINK, "from 255 towards 333, got {h}");
        for _ in 0..30 {
            frame(&mut ui);
        }
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON, "done in {TRANSITION}s");
        assert!((ui.inner.borrow().hue - hue::PINK).abs() < 1e-3);
        ui.set_screen(2, hue::ORANGE, TRANSITION);
        frame(&mut ui);
        let h = ui.inner.borrow().hue;
        assert!(h > hue::PINK || h < hue::ORANGE, "333 to 45 crosses 360, got {h}");
        ui.set_screen(2, hue::ORANGE, TRANSITION);
        assert!(ui.transition() > 0.0, "staying on a screen does not restart it");
    }

    #[test]
    fn a_transition_lasts_its_own_fade() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        ui.set_screen(0, hue::PURPLE, TRANSITION);
        frame(&mut ui);
        ui.set_screen(1, hue::PURPLE, 1.2);
        for _ in 0..30 {
            frame(&mut ui);
        }
        assert!(ui.transition() < 1.0, "still fading after {TRANSITION}s");
        for _ in 0..45 {
            frame(&mut ui);
        }
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON, "done in 1.2s");
    }

    #[test]
    fn approach_does_not_depend_on_the_frame_rate() {
        let at_60 = (0..60).fold(0.0, |v, _| approach(v, 1.0, HOVER_SPEED, 1.0 / 60.0));
        let at_144 = (0..144).fold(0.0, |v, _| approach(v, 1.0, HOVER_SPEED, 1.0 / 144.0));
        assert!((at_60 - at_144).abs() < 1e-4);
        assert!(at_60 > 0.999, "one second must be plenty");
    }

    #[test]
    fn hover_eases_in_and_out() {
        let mut ui = Ui::default();
        let first = frame(&mut ui, ON_A, &[("Jouer", A)])[0].hover;
        assert!(first > 0.0 && first < 0.5, "a hover must ease in, got {first}");
        for _ in 0..60 {
            frame(&mut ui, ON_A, &[("Jouer", A)]);
        }
        let out = frame(&mut ui, AWAY, &[("Jouer", A)])[0].hover;
        assert!(out > 0.5 && out < 1.0, "and ease out, got {out}");
    }

    #[test]
    fn same_labels_animate_separately() {
        let mut ui = Ui::default();
        for _ in 0..10 {
            frame(&mut ui, ON_A, &[("+", A), ("+", B)]);
        }
        let r = frame(&mut ui, ON_A, &[("+", A), ("+", B)]);
        assert!(r[0].hover > 0.9);
        assert!(r[1].hover.abs() < f32::EPSILON);
    }

    #[test]
    fn a_widget_left_undrawn_is_forgotten() {
        let mut ui = Ui::default();
        for _ in 0..30 {
            frame(&mut ui, ON_A, &[("Back", A)]);
        }
        frame(&mut ui, ON_A, &[]);
        let back = frame(&mut ui, ON_A, &[("Back", A)])[0].hover;
        assert!(back < 0.5, "it came back half hovered: {back}");
    }

    #[test]
    fn entering_is_reported_once_and_not_on_appearance() {
        let mut ui = Ui::default();
        assert!(
            !frame(&mut ui, ON_A, &[("Amis", A)])[0].entered,
            "appeared under the pointer"
        );
        frame(&mut ui, AWAY, &[("Amis", A)]);
        assert!(frame(&mut ui, ON_A, &[("Amis", A)])[0].entered);
        assert!(!frame(&mut ui, ON_A, &[("Amis", A)])[0].entered);
    }

    #[test]
    fn a_click_lands_on_what_was_drawn_before_it() {
        let mut ui = Ui::default();
        frame(&mut ui, ON_A, &[("Jouer", A)]);
        assert!(
            ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A),
            "the button under the click"
        );
        let flash = ui.interact("Jouer", A, true).flash;
        assert!(flash > 0.8, "flashes, got {flash}");
    }

    #[test]
    fn a_button_appearing_under_a_click_is_not_clicked() {
        let mut ui = Ui::default();
        frame(&mut ui, ON_A, &[("Amis", B)]);
        assert!(!ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A));
        assert!(ui.interact("Retour", A, true).flash.abs() < f32::EPSILON);
    }

    #[test]
    fn a_click_flash_fades() {
        let mut ui = Ui::default();
        frame(&mut ui, ON_A, &[("Jouer", A)]);
        frame(&mut ui, CLICK_A, &[("Jouer", A)]);
        let later = (0..60).map(|_| frame(&mut ui, ON_A, &[("Jouer", A)])[0]).last();
        assert!(later.is_some_and(|r| r.flash < 0.01));
    }

    const ROW: Rect = Rect::at(0.0, 0.0, 1000.0, 60.0);

    fn at(x: f32, y: f32) -> Mouse {
        Mouse {
            x,
            y,
            down: false,
            pressed: false,
        }
    }

    fn bar_frame(ui: &mut Ui, mouse: Mouse) -> Response {
        ui.begin_frame(1.0 / 60.0, View::default(), mouse);
        ui.interact_in("Solo", ROW, Area::Bar, true)
    }

    #[test]
    fn a_menu_row_reacts_on_its_bar_only() {
        let mut ui = Ui::default();
        for _ in 0..30 {
            bar_frame(&mut ui, at(100.0, 30.0));
        }
        let band = bar_frame(&mut ui, at(100.0, 30.0));
        assert!(
            band.hover.abs() < f32::EPSILON,
            "the band beside the bar is not the button"
        );
        let click = Mouse {
            pressed: true,
            ..at(100.0, 30.0)
        };
        assert!(!ui.begin_frame(1.0 / 60.0, View::default(), click));
        assert!(!ui.bar_clicked(ROW));

        assert!(bar_frame(&mut ui, at(500.0, 30.0)).hover > 0.0);
        let click = Mouse {
            pressed: true,
            ..at(500.0, 30.0)
        };
        assert!(ui.begin_frame(1.0 / 60.0, View::default(), click));
        assert!(ui.bar_clicked(ROW));
    }

    #[test]
    fn a_menu_bar_follows_its_slant() {
        let mut ui = Ui::default();
        bar_frame(&mut ui, at(225.0, 1.0));
        assert!(bar_frame(&mut ui, at(225.0, 1.0)).hover.abs() < f32::EPSILON);
        assert!(bar_frame(&mut ui, at(225.0, 59.0)).hover > 0.0);
    }

    #[test]
    fn a_widened_menu_bar_reacts_where_it_did_at_rest() {
        let mut ui = Ui::default();
        let margin = at(800.0, 30.0);
        for _ in 0..60 {
            bar_frame(&mut ui, at(500.0, 30.0));
        }
        assert!(bar_frame(&mut ui, at(500.0, 30.0)).hover > 0.9);
        for _ in 0..60 {
            bar_frame(&mut ui, margin);
        }
        assert!(bar_frame(&mut ui, margin).hover < 0.01);
        ui.begin_frame(
            1.0 / 60.0,
            View::default(),
            Mouse {
                pressed: true,
                ..margin
            },
        );
        assert!(!ui.bar_clicked(ROW));
    }

    #[test]
    fn the_caret_blinks_and_typing_lights_it_again() {
        let mut ui = Ui::default();
        let at = |ui: &mut Ui, frames: u32, text: &str, at: usize| {
            for _ in 0..frames {
                ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
            }
            ui.caret_on(text, at)
        };
        let after = |ui: &mut Ui, frames: u32, text: &str| at(ui, frames, text, text.len());
        assert!(after(&mut ui, 1, "a"));
        assert!(after(&mut ui, 15, "a"), "lit for half a second");
        assert!(!after(&mut ui, 21, "a"), "then off");
        assert!(after(&mut ui, 1, "ab"), "a keystroke must not type blind");
        assert!(!after(&mut ui, 36, "ab"));
        assert!(after(&mut ui, 30, "ab"), "and back on");
        assert!(!after(&mut ui, 36, "ab"));
        assert!(at(&mut ui, 1, "ab", 1), "nor may it move unseen");
    }

    #[test]
    fn a_press_finds_the_widget_by_its_label() {
        let mut ui = Ui::default();
        frame(&mut ui, ON_A, &[("Voir plus", A), ("Retour", B)]);
        ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A);
        assert!(ui.pressed("Voir plus"));
        assert!(!ui.pressed("Retour"), "drawn, but not under the click");
        assert!(!ui.pressed("Jamais dessiné"));

        ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A);
        assert!(!ui.pressed("Voir plus"), "no longer drawn, no longer there");

        ui.set_input(false);
        ui.interact("Voir plus", A, true);
        ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A);
        assert!(!ui.pressed("Voir plus"), "covered by a window");
    }

    #[test]
    fn disabled_and_covered_buttons_ignore_the_pointer() {
        let mut ui = Ui::default();
        ui.begin_frame(1.0 / 60.0, View::default(), ON_A);
        ui.set_input(false);
        let under = ui.interact("Leave", A, true);
        ui.set_input(true);
        ui.interact("Off", A, false);
        assert!(under.hover.abs() < f32::EPSILON);
        assert!(
            !ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A),
            "neither was clickable"
        );
        ui.interact("Leave", A, true);
        assert!(
            ui.begin_frame(1.0 / 60.0, View::default(), CLICK_A),
            "input comes back the next frame"
        );
    }
}
