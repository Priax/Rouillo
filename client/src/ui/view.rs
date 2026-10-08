use notan::prelude::App;

const REFERENCE_W: f32 = 1280.0;
const REFERENCE_H: f32 = 800.0;
const MIN_SCALE: f32 = 0.5;
const MAX_SCALE: f32 = 4.0;

/// The space screens lay themselves out in: the window seen at a scale that
/// makes a 1280x800 layout fit it, so the interface grows with the window
/// instead of shrinking into a corner of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub w: f32,
    pub h: f32,
    pub scale: f32,
    /// Device pixels per window pixel: 2 or 3 on a phone, 1.25 on a scaled desktop.
    pub dpi: f32,
}

impl Default for View {
    fn default() -> Self {
        Self::fit(REFERENCE_W, REFERENCE_H)
    }
}

impl View {
    pub fn fit(window_w: f32, window_h: f32) -> Self {
        let scale = (window_w / REFERENCE_W)
            .min(window_h / REFERENCE_H)
            .clamp(MIN_SCALE, MAX_SCALE);
        Self {
            w: window_w / scale,
            h: window_h / scale,
            scale,
            dpi: 1.0,
        }
    }

    pub fn of(app: &mut App) -> Self {
        let (w, h) = app.window().size();
        let dpi = (app.window().dpi() as f32).max(1.0);
        Self {
            dpi,
            ..Self::fit(w as f32, h as f32)
        }
    }

    /// A window taller than wide, such as a phone held upright: screens stack
    /// what they would put side by side.
    pub fn portrait(self) -> bool {
        self.h > self.w
    }

    pub fn size(self) -> (f32, f32) {
        (self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_window_is_drawn_as_is() {
        assert_eq!(View::fit(1280.0, 800.0).scale, 1.0);
    }

    #[test]
    fn a_larger_window_scales_up_and_keeps_the_layout_inside() {
        let v = View::fit(1920.0, 1080.0);
        assert!((v.scale - 1.35).abs() < 1e-3, "height limits: {}", v.scale);
        assert!((v.h - 800.0).abs() < 1e-3 && v.w >= 1280.0);
    }

    #[test]
    fn a_smaller_window_scales_down_instead_of_cropping() {
        let v = View::fit(960.0, 600.0);
        assert!((v.scale - 0.75).abs() < 1e-3);
        assert!((v.w - 1280.0).abs() < 1e-3 && (v.h - 800.0).abs() < 1e-3);
    }
}
