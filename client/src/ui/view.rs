use aero::geom::Rect;
use aero::math::Vec2;
use aero::{Reference, WindowInfo};

/// The layout every screen is drawn for: Aero zooms it to fit the window, so that the interface grows with the
/// window instead of shrinking into a corner of it.
pub const REFERENCE: Reference = Reference { size: Vec2::new(1280.0, 800.0), zoom: (0.5, 4.0) };

/// The space screens lay themselves out in: the window, in the units of [`REFERENCE`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub w: f32,
    pub h: f32,
}

impl Default for View {
    fn default() -> Self {
        Self { w: REFERENCE.size.x, h: REFERENCE.size.y }
    }
}

impl View {
    /// What a window this large in logical pixels shows.
    #[cfg(test)]
    pub fn fit(window_w: f32, window_h: f32) -> Self {
        let window = Vec2::new(window_w, window_h);
        let size = window / REFERENCE.zoom_for(window);
        Self { w: size.x, h: size.y }
    }

    pub fn of(window: WindowInfo) -> Self {
        Self { w: window.size.x, h: window.size.y }
    }

    /// A window taller than wide, such as a phone held upright: screens stack
    /// what they would put side by side.
    pub fn portrait(self) -> bool {
        self.h > self.w
    }

    pub fn size(self) -> (f32, f32) {
        (self.w, self.h)
    }

    /// The whole view, from its top left corner.
    pub fn rect(self) -> Rect {
        Rect::at(0.0, 0.0, self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_larger_window_shows_the_whole_layout_and_more_across() {
        let v = View::fit(1920.0, 1080.0);
        assert!((v.h - 800.0).abs() < 1e-3 && v.w >= 1280.0);
    }

    #[test]
    fn a_smaller_window_shows_the_layout_smaller_instead_of_cropping_it() {
        let v = View::fit(960.0, 600.0);
        assert!((v.w - 1280.0).abs() < 1e-3 && (v.h - 800.0).abs() < 1e-3);
    }
}
