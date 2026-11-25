//! The part of notan's API that Rouillo uses, drawn by Aero: imported as `notan`, it runs the client unchanged.
//! notan runs continuously, so this does too: every update asks for the next frame.

mod color;
mod drawing;
mod graphics;
mod input;

use aero::{Config, Cx, WindowIcon};

pub mod app {
    pub use crate::input::{App, Event};
}

pub mod math {
    pub use aero::math::{vec2, Mat3, Vec2};
}

pub mod draw {
    pub use crate::drawing::{
        Draw, DrawBuilder, DrawConfig, DrawImages, DrawShapes, DrawTextSection, DrawTransform, Ellipse, Image, Line,
        Path, Rectangle, Shape, Text, Transform,
    };
    pub use crate::graphics::{CreateDraw, CreateFont, Font};
}

pub mod prelude {
    pub use crate::color::{BlendMode, Color, TextureFilter};
    pub use crate::graphics::{Graphics, RenderTexture, RenderTextureBuilder, Texture, TextureBuilder};
    pub use crate::input::{App, AppWindow, KeyCode, Keyboard, Mouse, Timer, Touch};
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

impl Default for WindowConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What [`AppBuilder::add_config`] takes.
pub trait AppConfig {
    fn apply(self, config: &mut Option<aero::WindowConfig>);
}

impl AppConfig for draw::DrawConfig {
    fn apply(self, _config: &mut Option<aero::WindowConfig>) {}
}

impl AppConfig for WindowConfig {
    fn apply(self, config: &mut Option<aero::WindowConfig>) {
        *config = Some(self.window);
    }
}

type Setup<S> = Box<dyn FnOnce(&mut graphics::Graphics<'_>) -> S>;
type OnEvent<S> = fn(&mut S, input::Event);
type OnUpdate<S> = fn(&mut input::App, &mut S);
type OnDraw<S> = fn(&mut input::App, &mut graphics::Graphics<'_>, &mut S);

pub struct AppBuilder<S> {
    setup: Setup<S>,
    window: Option<aero::WindowConfig>,
    event: Option<OnEvent<S>>,
    update: Option<OnUpdate<S>>,
    draw: Option<OnDraw<S>>,
}

pub fn init_with<S: 'static>(setup: impl FnOnce(&mut graphics::Graphics<'_>) -> S + 'static) -> AppBuilder<S> {
    AppBuilder { setup: Box::new(setup), window: None, event: None, update: None, draw: None }
}

impl<S: 'static> AppBuilder<S> {
    #[must_use]
    pub fn add_config(mut self, config: impl AppConfig) -> Self {
        config.apply(&mut self.window);
        self
    }

    #[must_use]
    pub fn event(mut self, event: OnEvent<S>) -> Self {
        self.event = Some(event);
        self
    }

    #[must_use]
    pub fn update(mut self, update: OnUpdate<S>) -> Self {
        self.update = Some(update);
        self
    }

    #[must_use]
    pub fn draw(mut self, draw: OnDraw<S>) -> Self {
        self.draw = Some(draw);
        self
    }

    pub fn build(self) -> Result<(), String> {
        let Self { setup, window, event, update, draw } = self;
        let config = Config::new().window(window.unwrap_or_default());
        aero::run(config, move |cx| {
            let state = setup(&mut graphics::Graphics { cx, frame: None });
            Runner { state, app: input::App::new(), stale: false, event, update, draw }
        })
        .map_err(|e| e.to_string())
    }
}

struct Runner<S> {
    state: S,
    app: input::App,
    /// An update ran and no frame cleared its input yet.
    stale: bool,
    event: Option<OnEvent<S>>,
    update: Option<OnUpdate<S>>,
    draw: Option<OnDraw<S>>,
}

impl<S> Runner<S> {
    /// notan clears the input once a frame is drawn: when none was, the next event or update does.
    fn freshen(&mut self) {
        if std::mem::replace(&mut self.stale, false) {
            self.app.end_frame();
        }
    }
}

impl<S: 'static> aero::App for Runner<S> {
    fn event(&mut self, _cx: &mut Cx, event: &aero::input::Event) {
        self.freshen();
        for typed in self.app.apply(event) {
            if let Some(on_event) = self.event {
                on_event(&mut self.state, typed);
            }
        }
    }

    fn update(&mut self, cx: &mut Cx) {
        self.freshen();
        self.app.timer.tick();
        if let Some(info) = cx.window(aero::WindowId::MAIN) {
            self.app.set_window(info);
        }
        if let Some(update) = self.update {
            update(&mut self.app, &mut self.state);
        }
        self.stale = true;
        cx.request_redraw();
    }

    fn draw(&mut self, cx: &mut Cx, frame: &mut aero::draw::Draw) {
        if let Some(draw) = self.draw {
            draw(&mut self.app, &mut graphics::Graphics { cx, frame: Some(frame) }, &mut self.state);
        }
        self.freshen();
    }
}
