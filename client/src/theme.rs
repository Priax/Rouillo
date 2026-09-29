use notan::prelude::Color;

const HUE: f32 = 255.0;

// HSL to RGB, `h` in degrees, `s` and `l` in `0..=1`. `const` so the palette
// below is computed at compile time.
const fn hsl(h: f32, s: f32, l: f32) -> Color {
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

const fn tone(s: f32, l: f32) -> Color {
    hsl(HUE, s, l)
}

pub const BACKGROUND: Color = tone(0.10, 0.08);
pub const SURFACE_ALT: Color = tone(0.10, 0.11);
pub const SURFACE: Color = tone(0.10, 0.15);
pub const DISABLED: Color = tone(0.08, 0.13);
pub const RAISED: Color = tone(0.12, 0.22);
pub const RAISED_HOVER: Color = tone(0.15, 0.30);
pub const DIVIDER: Color = tone(0.15, 0.24);
pub const BORDER: Color = tone(0.20, 0.38);

pub const ACCENT: Color = tone(1.0, 0.70);
pub const TITLE: Color = tone(0.90, 0.85);
pub const AVATAR: Color = tone(0.35, 0.30);
pub const AVATAR_HOVER: Color = tone(0.40, 0.40);
pub const BANNER: Color = tone(0.35, 0.14).with_alpha(0.96);

pub const TEXT: Color = tone(0.30, 0.95);
pub const TEXT_DIM: Color = tone(0.25, 0.75);
pub const TEXT_MUTED: Color = tone(0.10, 0.55);
pub const TEXT_DISABLED: Color = tone(0.08, 0.38);

pub const GOLD: Color = Color::from_rgb(1.0, 0.8, 0.13);
pub const SUCCESS: Color = Color::from_rgb(0.45, 0.88, 0.5);
pub const SUCCESS_BG: Color = Color::from_rgba(0.1, 0.3, 0.1, 0.6);
pub const DANGER: Color = Color::from_rgb(0.95, 0.42, 0.42);
pub const DANGER_BG: Color = Color::from_rgba(0.3, 0.1, 0.1, 0.6);
pub const WARNING: Color = Color::from_rgb(0.95, 0.62, 0.22);
pub const WARNING_TEXT: Color = Color::from_rgb(1.0, 0.85, 0.6);
pub const WARNING_BANNER: Color = Color::from_rgba(0.35, 0.18, 0.05, 0.96);
pub const LINK: Color = Color::from_rgb(0.6, 0.8, 1.0);

pub const SCRIM_LIGHT: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
pub const SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);
pub const SCRIM_DARK: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.7);
pub const SCRIM_STRONG: Color = tone(0.30, 0.03).with_alpha(0.88);

pub const RADIUS: f32 = 8.0;

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
    fn surfaces_get_lighter_in_order() {
        let luma = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let order = [BACKGROUND, SURFACE_ALT, SURFACE, RAISED, RAISED_HOVER, BORDER];
        assert!(order.windows(2).all(|w| luma(w[0]) < luma(w[1])));
    }
}
