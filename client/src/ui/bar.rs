use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;

use super::{Fonts, Rect, SharpText, Triangles, Ui};

/// Share of the row's width the bar covers at rest, and how much wider it
/// grows when hovered.
const WIDTH: f32 = 0.56;
const HOVER_WIDTH: f32 = 0.10;
/// How far the slanted edges lean, relative to the bar's height.
const SLANT: f32 = 0.28;
const GLOW_WIDTH: f32 = 90.0;
const BAND: Color = Color::new(0.0, 0.0, 0.0, 0.28);
const TEXT_HEIGHT: f32 = 0.42;

impl Ui {
    /// A full-width menu entry in the style of osu!lazer's pause menu: a
    /// slanted bar of `color` over a dark band, with triangles drifting
    /// through it. The whole row reacts to the pointer.
    pub fn menu_bar(&self, draw: &mut Draw, fonts: &Fonts, row: Rect, label: &str, color: Color) {
        let r = self.interact(label, row, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }

        draw.rect((row.x, row.y), (row.w, row.h)).color(BAND);

        let cx = row.x + row.w / 2.0;
        let half = row.w * (WIDTH + HOVER_WIDTH * r.hover) / 2.0;
        let slant = row.h * SLANT;
        let bar = Slanted {
            left: cx - half,
            right: cx + half,
            top: row.y,
            bottom: row.y + row.h,
            slant,
        };

        glow(draw, bar, color.with_alpha(0.45 * r.hover));
        bar.fill(draw, color);

        let mut mask = Draw::new(draw.width() as u32, draw.height() as u32);
        mask.transform().set(*draw.transform().matrix());
        bar.fill(&mut mask, Color::WHITE);
        draw.mask(Some(&mask));
        let triangles = Triangles {
            seed: seed_of(label),
            density: 2.5,
            speed: 0.15,
            color: Color::WHITE.with_alpha(0.13),
        };
        triangles.draw(
            draw,
            Rect::at(bar.left - slant, row.y, bar.right - bar.left + 2.0 * slant, row.h),
            self.time(),
        );
        draw.mask(None);

        if r.flash > 0.0 {
            bar.fill(draw, Color::WHITE.with_alpha(0.3 * r.flash));
        }
        let shrink = 1.0 - 0.03 * r.press;
        draw.sharp_text(&fonts.display, label)
            .position(cx, row.y + row.h / 2.0)
            .size(row.h * TEXT_HEIGHT * shrink)
            .h_align_center()
            .v_align_middle()
            .color(Color::WHITE);
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
    fn fill(self, draw: &mut Draw, color: Color) {
        let Self {
            left,
            right,
            top,
            bottom,
            slant,
        } = self;
        draw.path()
            .move_to(left + slant, top)
            .line_to(right + slant, top)
            .line_to(right - slant, bottom)
            .line_to(left - slant, bottom)
            .close()
            .fill()
            .color(color);
    }
}

/// Light spilling out of both slanted ends of the bar, fading away from it.
/// Each glow is a quad lying against its edge, so it follows the slant.
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

/// A quad from the edge `top`-`bottom` out to `reach` (negative leftwards),
/// `color` along the edge and clear at the far side.
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
