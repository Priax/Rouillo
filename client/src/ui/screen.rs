use std::ops::{Deref, DerefMut};

use aero::math::{vec2, Affine2};

use super::{Face, Rect, Stage, Triangles, Typeset};
use crate::theme;

/// How far a new screen slides in from, rightwards.
const SLIDE: f32 = 40.0;

impl Stage {
    /// The canvas a screen draws on: its section's background with triangles
    /// drifting up through it, then the screen's own content, which fades and
    /// slides in while a transition runs. The backdrop stays put under it.
    pub fn screen_canvas<'a, 'u>(&self, ui: &'a mut aero::ui::Ui<'u>) -> ScreenCanvas<'a, 'u> {
        ui.cx.request_redraw();
        let pal = self.palette();
        let view = self.view();
        ui.draw.background(pal.background);
        let backdrop =
            Triangles { seed: 0x5eed, size: 220.0, density: 0.35, speed: 0.015, color: pal.raised.with_alpha(0.35) };
        backdrop.draw(ui.draw, view.rect(), self.time());

        let t = self.transition();
        ui.draw.push_alpha(t);
        ui.draw.push_transform(Affine2::from_translation(vec2((1.0 - t) * SLIDE, 0.0)));
        ScreenCanvas(ui)
    }

    /// The header band along the top of a page, with its title.
    pub fn page_header(&self, ui: &mut aero::ui::Ui<'_>, title: &str) {
        self.header_band(ui, Rect::at(0.0, 0.0, self.view().w, theme::HEADER_H));
        ui.cx
            .fonts
            .write(ui.draw, Face::Display, title)
            .left(60.0, theme::HEADER_H / 2.0)
            .size(theme::size::TITLE)
            .color(self.palette().text);
    }

    /// A page header: a band in the section's colour with triangles rising
    /// through it and a line of accent along its foot.
    pub fn header_band(&self, ui: &mut aero::ui::Ui<'_>, rect: Rect) {
        ui.cx.request_redraw();
        let pal = self.palette();
        let draw = &mut *ui.draw;
        draw.rect(rect).color(pal.banner);
        let triangles = Triangles {
            seed: 0xba5d,
            size: rect.h() * 0.9,
            density: 0.6,
            speed: 0.03,
            color: pal.accent.with_alpha(0.08),
        };
        triangles.draw_clipped(draw, rect, self.time(), Some(&rect.corners().map(Into::into)));
        draw.rect(Rect::at(rect.x(), rect.y() + rect.h() - 2.0, rect.w(), 2.0)).color(pal.accent);
    }
}

/// The frame while a screen draws on it, as the transition fades and slides it.
pub struct ScreenCanvas<'a, 'u>(&'a mut aero::ui::Ui<'u>);

impl<'u> Deref for ScreenCanvas<'_, 'u> {
    type Target = aero::ui::Ui<'u>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl DerefMut for ScreenCanvas<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl Drop for ScreenCanvas<'_, '_> {
    fn drop(&mut self) {
        self.0.draw.pop_transform();
        self.0.draw.pop_alpha();
    }
}
