//! The part of notan's drawing API that Rouillo still uses, drawn by Aero, until the client draws with Aero's own.

mod color;
mod drawing;
mod graphics;
mod texts;

use aero::{Cx, WindowIcon};

pub mod math {
    pub use aero::math::{vec2, Mat3, Vec2};
}

pub mod draw {
    pub use crate::drawing::{
        Draw, DrawBuilder, DrawImages, DrawShapes, DrawTextSection, DrawTransform, Ellipse, Image, Line, Path,
        Rectangle, Shape, Text, Transform,
    };
    pub use crate::graphics::{CreateDraw, CreateFont, Font};
}

pub mod prelude {
    pub use crate::color::{BlendMode, Color, TextureFilter};
    pub use crate::graphics::{Graphics, RenderTexture, RenderTextureBuilder, Texture, TextureBuilder};
    pub use crate::WindowConfig;
}

/// notan's window options, those Rouillo sets.
#[derive(Clone)]
pub struct WindowConfig {
    window: aero::WindowConfig,
}

impl WindowConfig {
    pub fn new() -> Self {
        Self { window: aero::WindowConfig::new() }
    }

    #[must_use]
    pub fn set_title(mut self, title: &str) -> Self {
        self.window = self.window.title(title);
        self
    }

    #[must_use]
    pub fn set_size(mut self, width: u32, height: u32) -> Self {
        self.window = self.window.size(width, height);
        self
    }

    #[must_use]
    pub fn set_resizable(mut self, resizable: bool) -> Self {
        self.window = self.window.resizable(resizable);
        self
    }

    #[must_use]
    pub fn set_maximized(mut self, maximized: bool) -> Self {
        self.window = self.window.maximized(maximized);
        self
    }

    /// Aero always draws at the screen's density.
    #[must_use]
    pub const fn set_high_dpi(self, _high_dpi: bool) -> Self {
        self
    }

    #[must_use]
    pub fn set_app_id(mut self, id: &str) -> Self {
        self.window = self.window.app_id(id);
        self
    }

    /// An encoded picture, ICO or PNG: the largest image in an ICO.
    #[must_use]
    pub fn set_window_icon_data(mut self, data: Option<&'static [u8]>) -> Self {
        let decoded = data.map(graphics::decode).transpose().unwrap_or_else(|e| {
            eprintln!("window icon: {e}");
            None
        });
        if let Some((width, height, rgba)) = decoded {
            self.window = self.window.icon(WindowIcon { width, height, rgba });
        }
        self
    }

    /// Aero gives the task bar the window's icon.
    #[must_use]
    pub const fn set_taskbar_icon_data(self, _data: Option<&'static [u8]>) -> Self {
        self
    }
}

impl From<WindowConfig> for aero::WindowConfig {
    fn from(config: WindowConfig) -> Self {
        config.window
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Draws notan's way in an app that runs Aero's loop itself; kept between frames for the text it lays out.
#[derive(Default)]
pub struct Painter {
    texts: texts::Texts,
}

impl Painter {
    /// What notan drew with, for `frame` while a window draws, without it outside drawing.
    pub fn graphics<'a>(
        &'a mut self,
        cx: &'a mut Cx,
        frame: Option<&'a mut aero::draw::Draw>,
    ) -> graphics::Graphics<'a> {
        graphics::Graphics { cx, frame, texts: &mut self.texts }
    }

    /// Once a frame is drawn: drops the text it did not draw.
    pub fn end_frame(&mut self) {
        self.texts.end_frame();
    }
}
