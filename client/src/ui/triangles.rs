use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;

use super::Rect;

/// Triangles drifting upwards through an area, osu!lazer style. The field is
/// a pure function of its seed and the time, so it needs no state and looks
/// the same from one frame to the next.
#[derive(Clone, Copy)]
pub struct Triangles {
    pub seed: u64,
    /// The side of the largest triangles; the smallest are a quarter of it.
    pub size: f32,
    /// Triangles per 10 000 square units of area.
    pub density: f32,
    /// Cycles through the area per second, before per-triangle variation.
    /// Positions are derived from `time * speed`, so a field's speed must
    /// stay constant: changing it would jump every triangle at once.
    pub speed: f32,
    pub color: Color,
}

impl Triangles {
    pub fn draw(self, draw: &mut Draw, area: Rect, time: f64) {
        self.draw_clipped(draw, area, time, None);
    }

    /// Draws the field, each triangle cut to the convex polygon `clip` when
    /// there is one. Clipping is geometric rather than a stencil mask, so it
    /// works on any target, render textures without a stencil buffer included.
    pub fn draw_clipped(self, draw: &mut Draw, area: Rect, time: f64, clip: Option<&[(f32, f32)]>) {
        let count = ((area.w * area.h / 10_000.0) * self.density).round().max(1.0) as u64;
        for i in 0..count {
            let mut rng = self.seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let mut next = || unit(splitmix(&mut rng));
            let (u, phase, size, speed, alpha) = (next(), next(), next(), next(), next());

            let side = self.size * (0.25 + 0.75 * size);
            let travel = area.h + 2.0 * side;
            let cycles = time * f64::from(self.speed * (0.5 + speed));
            let progress = (f64::from(phase) + cycles).fract() as f32;
            let (x, y) = (area.x + u * area.w, area.y + area.h + side - progress * travel);
            let half = side * 0.577;
            let corners = [
                (x, y - side / 2.0),
                (x - half, y + side / 2.0),
                (x + half, y + side / 2.0),
            ];
            let color = self.color.with_alpha(self.color.a * (0.35 + 0.65 * alpha));
            match clip {
                None => {
                    draw.triangle(corners[0], corners[1], corners[2]).color(color);
                }
                Some(clip) => fill_polygon(draw, &clip_convex(&corners, clip), color),
            }
        }
    }
}

fn fill_polygon(draw: &mut Draw, points: &[(f32, f32)], color: Color) {
    let Some((&first, rest)) = points.split_first() else {
        return;
    };
    if rest.len() < 2 {
        return;
    }
    let mut path = draw.path();
    path.move_to(first.0, first.1);
    for &(x, y) in rest {
        path.line_to(x, y);
    }
    path.close().fill().color(color);
}

/// The part of `subject` inside the convex polygon `clip` (Sutherland-
/// Hodgman). `clip`'s corners may run either way round.
fn clip_convex(subject: &[(f32, f32)], clip: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let area2: f32 = clip
        .iter()
        .zip(clip.iter().cycle().skip(1))
        .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
        .sum();
    let orientation = area2.signum();
    let mut output = subject.to_vec();
    for (&a, &b) in clip.iter().zip(clip.iter().cycle().skip(1)) {
        let side = |p: (f32, f32)| orientation * ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0));
        let input = std::mem::take(&mut output);
        for (&p, &q) in input.iter().zip(input.iter().cycle().skip(1)) {
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                output.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                output.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
            }
        }
        if output.is_empty() {
            break;
        }
    }
    output
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

    const SQUARE: [(f32, f32); 4] = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];

    #[test]
    fn a_shape_inside_the_clip_is_kept_whole() {
        let tri = [(2.0, 2.0), (8.0, 2.0), (5.0, 8.0)];
        assert_eq!(clip_convex(&tri, &SQUARE), tri.to_vec());
    }

    #[test]
    fn a_shape_outside_the_clip_vanishes() {
        assert!(clip_convex(&[(20.0, 20.0), (30.0, 20.0), (25.0, 30.0)], &SQUARE).is_empty());
    }

    #[test]
    fn a_shape_across_the_edge_is_cut_to_it_either_way_round() {
        let tri = [(5.0, -5.0), (15.0, 5.0), (5.0, 5.0)];
        let reversed: Vec<_> = SQUARE.iter().rev().copied().collect();
        for clip in [SQUARE.to_vec(), reversed] {
            let cut = clip_convex(&tri, &clip);
            assert!(cut.len() >= 3);
            assert!(cut
                .iter()
                .all(|&(x, y)| (-1e-4..=10.0001).contains(&x) && (-1e-4..=10.0001).contains(&y)));
        }
    }

    #[test]
    fn units_stay_in_range_and_vary() {
        let mut s = 42;
        let values: Vec<f32> = (0..1000).map(|_| unit(splitmix(&mut s))).collect();
        assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
    }
}
