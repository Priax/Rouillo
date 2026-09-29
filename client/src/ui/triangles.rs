use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;

use super::Rect;

/// Triangles drifting upwards through an area, osu!lazer style. The field is
/// a pure function of its seed and the time, so it needs no state and looks
/// the same from one frame to the next.
#[derive(Clone, Copy)]
pub struct Triangles {
    pub seed: u64,
    /// Triangles per 10 000 square units of area.
    pub density: f32,
    /// Cycles through the area per second, before per-triangle variation.
    /// Positions are derived from `time * speed`, so a field's speed must
    /// stay constant: changing it would jump every triangle at once.
    pub speed: f32,
    pub color: Color,
}

impl Triangles {
    pub fn draw(self, draw: &mut Draw, area: Rect, time: f32) {
        let count = ((area.w * area.h / 10_000.0) * self.density).round().max(1.0) as u64;
        for i in 0..count {
            let mut rng = self.seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let mut next = || unit(splitmix(&mut rng));
            let (u, phase, size, speed, alpha) = (next(), next(), next(), next(), next());

            let side = area.h * (0.3 + 0.9 * size);
            let travel = area.h + 2.0 * side;
            let progress = (phase + time * self.speed * (0.5 + speed)).fract();
            let (x, y) = (area.x + u * area.w, area.y + area.h + side - progress * travel);
            let half = side * 0.577;
            draw.triangle(
                (x, y - side / 2.0),
                (x - half, y + side / 2.0),
                (x + half, y + side / 2.0),
            )
            .color(self.color.with_alpha(self.color.a * (0.35 + 0.65 * alpha)));
        }
    }
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(bits: u64) -> f32 {
    (bits >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_stay_in_range_and_vary() {
        let mut s = 42;
        let values: Vec<f32> = (0..1000).map(|_| unit(splitmix(&mut s))).collect();
        assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
    }
}
