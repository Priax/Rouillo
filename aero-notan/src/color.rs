/// Straight alpha, sRGB components from 0 to 1, as notan's.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    pub const RED: Self = Self::new(1.0, 0.0, 0.0, 1.0);
    pub const GREEN: Self = Self::new(0.0, 1.0, 0.0, 1.0);
    pub const BLUE: Self = Self::new(0.0, 0.0, 1.0, 1.0);
    pub const YELLOW: Self = Self::new(1.0, 1.0, 0.0, 1.0);
    pub const MAGENTA: Self = Self::new(1.0, 0.0, 1.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Self::new(r, g, b, 1.0)
    }

    pub const fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::new(r, g, b, a)
    }

    #[must_use]
    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// With its alpha multiplied by `alpha`.
    pub(crate) const fn aero(self, alpha: f32) -> aero::geom::Color {
        aero::geom::Color::srgba(self.r, self.g, self.b, self.a * alpha)
    }
}

/// Aero blends with premultiplied alpha throughout: every mode notan offers draws the same there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendMode(());

impl BlendMode {
    pub const NORMAL: Self = Self(());
    pub const OVER: Self = Self(());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    Linear,
    Nearest,
}

impl TextureFilter {
    pub(crate) const fn aero(self) -> aero::image::Filter {
        match self {
            Self::Linear => aero::image::Filter::Linear,
            Self::Nearest => aero::image::Filter::Nearest,
        }
    }
}
