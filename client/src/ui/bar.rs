use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;

use super::{Area, Fonts, Rect, SharpText, Triangles, Ui};

const WIDTH: f32 = 0.56;
const HOVER_WIDTH: f32 = 0.10;
const SLANT: f32 = 0.28;
const GLOW_WIDTH: f32 = 90.0;
const BAND: Color = Color::new(0.0, 0.0, 0.0, 0.28);
const TEXT_HEIGHT: f32 = 0.42;

impl Ui {
    pub fn menu_bar(&self, draw: &mut Draw, fonts: &Fonts, row: Rect, label: &str, color: Color) {
        self.menu_bar_enabled(draw, fonts, row, label, color, true);
    }

    pub fn menu_bar_enabled(
        &self,
        draw: &mut Draw,
        fonts: &Fonts,
        row: Rect,
        label: &str,
        color: Color,
        enabled: bool,
    ) {
        let pal = self.palette();
        let color = if enabled { color } else { pal.disabled };
        let r = self.interact_in(label, row, Area::Bar, enabled);
        if r.entered {
            crate::audio::play_ui_hover();
        }

        draw.rect((row.x, row.y), (row.w, row.h)).color(BAND);

        let cx = row.x + row.w / 2.0;
        let bar = Slanted::of(row, r.hover);
        let slant = bar.slant;

        glow(draw, bar, color.with_alpha(0.45 * r.hover));
        bar.fill(draw, color);

        let triangles = Triangles {
            seed: seed_of(label),
            size: row.h * 1.2,
            density: 2.5,
            speed: 0.15,
            color: Color::WHITE.with_alpha(0.13),
        };
        let area = Rect::at(bar.left - slant, row.y, bar.right - bar.left + 2.0 * slant, row.h);
        triangles.draw_clipped(draw, area, self.time(), Some(&bar.corners()));

        if r.flash > 0.0 {
            bar.fill(draw, Color::WHITE.with_alpha(0.3 * r.flash));
        }
        let shrink = 1.0 - 0.03 * r.press;
        draw.sharp_text(&fonts.display, label)
            .position(cx, row.y + row.h / 2.0)
            .size(row.h * TEXT_HEIGHT * shrink)
            .h_align_center()
            .v_align_middle()
            .color(if enabled { Color::WHITE } else { pal.text_disabled });
    }
}

#[derive(Clone, Copy)]
struct Slanted {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    slant: f32,
}

impl Slanted {
    fn of(row: Rect, hover: f32) -> Self {
        let cx = row.x + row.w / 2.0;
        let half = row.w * (WIDTH + HOVER_WIDTH * hover) / 2.0;
        Self {
            left: cx - half,
            right: cx + half,
            top: row.y,
            bottom: row.y + row.h,
            slant: row.h * SLANT,
        }
    }

    fn contains(self, x: f32, y: f32) -> bool {
        if y < self.top || y >= self.bottom {
            return false;
        }
        let lean = self.slant * (1.0 - 2.0 * (y - self.top) / (self.bottom - self.top));
        x >= self.left + lean && x < self.right + lean
    }

    fn corners(self) -> [(f32, f32); 4] {
        let Self {
            left,
            right,
            top,
            bottom,
            slant,
        } = self;
        [
            (left + slant, top),
            (right + slant, top),
            (right - slant, bottom),
            (left - slant, bottom),
        ]
    }

    fn fill(self, draw: &mut Draw, color: Color) {
        let [a, b, c, d] = self.corners();
        draw.path()
            .move_to(a.0, a.1)
            .line_to(b.0, b.1)
            .line_to(c.0, c.1)
            .line_to(d.0, d.1)
            .close()
            .fill()
            .color(color);
    }
}

pub(super) fn contains(row: Rect, hover: f32, x: f32, y: f32) -> bool {
    Slanted::of(row, hover).contains(x, y)
}

fn glow(draw: &mut Draw, bar: Slanted, color: Color) {
    if color.a <= 0.0 {
        return;
    }
    let Slanted {
        left,
        right,
        top,
        bottom,
        slant,
    } = bar;
    glow_quad(draw, (left + slant, top), (left - slant, bottom), -GLOW_WIDTH, color);
    glow_quad(draw, (right + slant, top), (right - slant, bottom), GLOW_WIDTH, color);
}

fn glow_quad(draw: &mut Draw, top: (f32, f32), bottom: (f32, f32), reach: f32, color: Color) {
    let clear = color.with_alpha(0.0);
    let (far_top, far_bottom) = ((top.0 + reach, top.1), (bottom.0 + reach, bottom.1));
    draw.triangle(far_top, top, bottom).color_vertex(clear, color, color);
    draw.triangle(far_top, bottom, far_bottom)
        .color_vertex(clear, color, clear);
}

fn seed_of(label: &str) -> u64 {
    label.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}
