use notan::prelude::Color;

/// Section hues, after osu!lazer's colour schemes.
pub mod hue {
    pub const PURPLE: f32 = 255.0;
    pub const BLUE: f32 = 200.0;
    pub const GREEN: f32 = 125.0;
    pub const PINK: f32 = 333.0;
    pub const ORANGE: f32 = 45.0;
}

// HSL to RGB, `h` in degrees, `s` and `l` in `0..=1`. `const` so the palette
// below is computed at compile time.
const fn hsl(h: f32, s: f32, l: f32) -> Color {
    let mut h = h;
    while h < 0.0 {
        h += 360.0;
    }
    while h >= 360.0 {
        h -= 360.0;
    }
    const fn abs(v: f32) -> f32 {
        if v < 0.0 {
            -v
        } else {
            v
        }
    }
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let hp = h / 60.0;
    let mut hp_mod2 = hp;
    while hp_mod2 >= 2.0 {
        hp_mod2 -= 2.0;
    }
    let x = c * (1.0 - abs(hp_mod2 - 1.0));
    let (r, g, b) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c / 2.0;
    Color::from_rgb(r + m, g + m, b + m)
}

/// The colour `t` of the way from `a` to `b`.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Every colour that follows a section's hue. The tones differ only in
/// saturation and lightness, the way osu!lazer's colour provider works.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub surface_alt: Color,
    pub surface: Color,
    pub disabled: Color,
    pub raised: Color,
    pub raised_hover: Color,
    pub divider: Color,
    pub border: Color,
    pub accent: Color,
    pub title: Color,
    pub avatar: Color,
    pub avatar_hover: Color,
    pub banner: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_muted: Color,
    pub text_disabled: Color,
    pub scrim_strong: Color,
}

impl Palette {
    pub const fn new(hue: f32) -> Self {
        Self {
            background: hsl(hue, 0.10, 0.08),
            surface_alt: hsl(hue, 0.10, 0.11),
            surface: hsl(hue, 0.10, 0.15),
            disabled: hsl(hue, 0.08, 0.13),
            raised: hsl(hue, 0.12, 0.22),
            raised_hover: hsl(hue, 0.15, 0.30),
            divider: hsl(hue, 0.15, 0.24),
            border: hsl(hue, 0.20, 0.38),
            accent: hsl(hue, 1.0, 0.70),
            title: hsl(hue, 0.90, 0.85),
            avatar: hsl(hue, 0.35, 0.30),
            avatar_hover: hsl(hue, 0.40, 0.40),
            banner: hsl(hue, 0.35, 0.14).with_alpha(0.96),
            text: hsl(hue, 0.30, 0.95),
            text_dim: hsl(hue, 0.25, 0.75),
            text_muted: hsl(hue, 0.10, 0.55),
            text_disabled: hsl(hue, 0.08, 0.38),
            scrim_strong: hsl(hue, 0.30, 0.03).with_alpha(0.88),
        }
    }
}

pub const GOLD: Color = Color::from_rgb(1.0, 0.8, 0.13);
pub const SUCCESS: Color = Color::from_rgb(0.45, 0.88, 0.5);
pub const DANGER: Color = Color::from_rgb(0.95, 0.42, 0.42);
pub const WARNING: Color = Color::from_rgb(0.95, 0.62, 0.22);
pub const WARNING_TEXT: Color = Color::from_rgb(1.0, 0.85, 0.6);
pub const WARNING_BANNER: Color = Color::from_rgba(0.35, 0.18, 0.05, 0.96);
pub const LINK: Color = Color::from_rgb(0.6, 0.8, 1.0);

pub const SCRIM_LIGHT: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
pub const SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);
pub const SCRIM_DARK: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.7);

pub const RADIUS: f32 = 8.0;

/// Height of the header band that opens most screens.
pub const HEADER_H: f32 = 110.0;

/// Colours of the full-width menu bars, one per kind of action.
pub mod bar {
    use notan::prelude::Color;

    pub const GREEN: Color = Color::from_rgb(0.53, 0.72, 0.05);
    pub const BLUE: Color = Color::from_rgb(0.2, 0.55, 0.85);
    pub const YELLOW: Color = Color::from_rgb(0.95, 0.66, 0.05);
    pub const RED: Color = Color::from_rgb(0.74, 0.14, 0.2);
}

pub mod size {
    pub const SMALL: f32 = 15.0;
    pub const BODY: f32 = 18.0;
    pub const LABEL: f32 = 20.0;
    pub const EMPHASIS: f32 = 24.0;
    pub const HEADING: f32 = 30.0;
    pub const TITLE: f32 = 48.0;
    pub const HERO: f32 = 72.0;
    pub const HUGE: f32 = 140.0;
}

pub mod game {
    use notan::prelude::Color;
    use shared::PuyoType;

    pub const BACKGROUND: Color = Color::from_rgb(0.05, 0.05, 0.05);
    pub const BOARD: Color = Color::from_rgb(0.12, 0.12, 0.12);
    pub const GRID: Color = Color::GRAY;
    pub const PREVIEW: Color = Color::from_rgb(0.2, 0.2, 0.2);
    pub const PREVIEW_NEXT: Color = Color::from_rgb(0.15, 0.15, 0.15);
    pub const DEATH_CROSS: Color = Color::RED;
    pub const GARBAGE_CORE: Color = Color::BLACK;
    pub const ALL_CLEAR: Color = Color::from_rgb(1.0, 1.0, 0.0);
    pub const DISCONNECT_SCRIM: Color = Color::from_rgba(0.5, 0.0, 0.0, 0.5);
    #[cfg(debug_assertions)]
    pub const DEBUG_NET: Color = Color::MAGENTA;
    #[cfg(debug_assertions)]
    pub const DEBUG_BOARDS: Color = Color::from_rgb(0.0, 1.0, 1.0);

    pub const fn puyo(puyo_type: PuyoType) -> Color {
        match puyo_type {
            PuyoType::Red => Color::RED,
            PuyoType::Blue => Color::BLUE,
            PuyoType::Yellow => Color::YELLOW,
            PuyoType::Green => Color::GREEN,
            PuyoType::Purple => Color::MAGENTA,
            PuyoType::Garbage => Color::GRAY,
        }
    }

    pub const fn nuisance(points: u32) -> Color {
        match points {
            0..=12 => Color::from_rgb(0.3, 0.9, 0.3),
            13..=30 => Color::from_rgb(1.0, 0.8, 0.1),
            31..=60 => Color::from_rgb(1.0, 0.5, 0.0),
            _ => Color::from_rgb(1.0, 0.15, 0.15),
        }
    }

    pub const fn chain(count: u32) -> Color {
        match count {
            0 | 1 => Color::from_rgb(0.4, 1.0, 0.4),
            2 => Color::from_rgb(0.4, 0.8, 1.0),
            3 => Color::from_rgb(1.0, 0.9, 0.2),
            4 => Color::from_rgb(1.0, 0.5, 0.1),
            _ => Color::from_rgb(1.0, 0.2, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        [a.r - b.r, a.g - b.g, a.b - b.b].iter().all(|d| d.abs() < 1e-3)
    }

    #[test]
    fn hsl_matches_known_colours() {
        assert!(close(hsl(0.0, 1.0, 0.5), Color::from_rgb(1.0, 0.0, 0.0)));
        assert!(close(hsl(120.0, 1.0, 0.5), Color::from_rgb(0.0, 1.0, 0.0)));
        assert!(close(hsl(240.0, 1.0, 0.5), Color::from_rgb(0.0, 0.0, 1.0)));
        assert!(close(hsl(300.0, 1.0, 0.25), Color::from_rgb(0.5, 0.0, 0.5)));
        assert!(close(hsl(42.0, 0.0, 0.3), Color::from_rgb(0.3, 0.3, 0.3)));
    }

    #[test]
    fn hues_wrap_around() {
        assert!(close(hsl(-60.0, 1.0, 0.5), hsl(300.0, 1.0, 0.5)));
        assert!(close(hsl(420.0, 1.0, 0.5), hsl(60.0, 1.0, 0.5)));
    }

    #[test]
    fn surfaces_get_lighter_in_order_in_every_section() {
        let luma = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        for h in [hue::PURPLE, hue::BLUE, hue::GREEN, hue::PINK, hue::ORANGE] {
            let p = Palette::new(h);
            let order = [
                p.background,
                p.surface_alt,
                p.surface,
                p.raised,
                p.raised_hover,
                p.border,
            ];
            assert!(order.windows(2).all(|w| luma(w[0]) < luma(w[1])), "hue {h}");
        }
    }
}
