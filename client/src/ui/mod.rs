mod button;
mod field;
mod fonts;
mod keys;
mod rect;
mod status;
mod stepper;
mod view;

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub use button::Icon;
pub use field::{text_field, Field};
pub use fonts::Fonts;
pub use keys::KeyRepeat;
use notan::draw::Draw;
use notan::prelude::{App, Graphics};
pub use rect::Rect;
pub use status::Status;
pub use stepper::Stepper;
pub use view::View;

const MAX_DT: f32 = 0.1;

const HOVER_SPEED: f32 = 18.0;
const PRESS_SPEED: f32 = 30.0;
const FLASH_SPEED: f32 = 8.0;

#[derive(Default)]
pub struct Ui {
    inner: RefCell<Inner>,
}

#[derive(Default)]
struct Inner {
    dt: f32,
    view: View,
    mouse: Mouse,
    input_off: bool,
    widgets: HashMap<(u64, u32), Widget>,
    drawn: HashMap<u64, u32>,
}

struct Widget {
    rect: Rect,
    live: bool,
    hover: f32,
    press: f32,
    flash: f32,
    hovered: bool,
    touched: bool,
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
        inner.view = view;
        inner.mouse = mouse;
        inner.input_off = false;
        inner.widgets.retain(|_, w| std::mem::take(&mut w.touched));
        inner.drawn.clear();

        let mut clicked = false;
        if mouse.pressed {
            for w in inner.widgets.values_mut() {
                if w.live && w.rect.contains(mouse.x, mouse.y) {
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

    pub fn view(&self) -> View {
        self.inner.borrow().view
    }

    pub fn canvas(&self, gfx: &mut Graphics) -> Draw {
        self.view().canvas(gfx)
    }

    pub fn mouse(&self) -> (f32, f32) {
        let m = self.inner.borrow().mouse;
        (m.x, m.y)
    }

    pub fn hovered(&self, r: Rect) -> bool {
        let (x, y) = self.mouse();
        r.contains(x, y)
    }

    pub fn pressed(&self) -> bool {
        self.inner.borrow().mouse.pressed
    }

    pub fn clicked(&self, r: Rect) -> bool {
        self.pressed() && self.hovered(r)
    }

    pub fn set_input(&self, on: bool) {
        self.inner.borrow_mut().input_off = !on;
    }

    fn interact(&self, label: &str, rect: Rect, enabled: bool) -> Response {
        let mut inner = self.inner.borrow_mut();
        let Inner {
            dt,
            view: _,
            mouse,
            input_off,
            widgets,
            drawn,
        } = &mut *inner;
        let live = enabled && !*input_off;
        let over = live && rect.contains(mouse.x, mouse.y);

        let mut h = DefaultHasher::new();
        label.hash(&mut h);
        let label = h.finish();
        let nth = drawn.entry(label).or_insert(0);
        let key = (label, *nth);
        *nth += 1;

        let w = widgets.entry(key).or_insert(Widget {
            rect,
            live,
            hover: 0.0,
            press: 0.0,
            flash: 0.0,
            hovered: over,
            touched: false,
        });
        let entered = over && !w.hovered;
        w.rect = rect;
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
        // The click switches screens: "Retour" now sits where the click was.
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
