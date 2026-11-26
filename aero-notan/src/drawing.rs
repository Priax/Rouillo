//! notan's drawing: a list of shapes, recorded with builders that draw when dropped, then played into Aero.

use std::ops::{Deref, DerefMut};

use aero::geom::{Brush, Gradient, Rect};
use aero::math::{Affine2, Mat3, Vec2};
use aero::path::{Cap, Join, Stroke};
use aero::text::Fonts;

use crate::color::{BlendMode, Color};
use crate::graphics::{Font, Texture};
use crate::texts::Texts;

/// notan's draw extension, which Aero needs no setup for.
pub struct DrawConfig;

/// Shapes to draw, in logical pixels for a window and in pixels for a render texture.
#[derive(Clone)]
pub struct Draw {
    size: (f32, f32),
    alpha: f32,
    transform: Transform,
    pub(crate) items: Vec<Item>,
}

/// The matrices applied to every shape drawn after them.
#[derive(Clone)]
pub struct Transform {
    stack: Vec<Mat3>,
}

impl Transform {
    pub fn matrix(&self) -> &Mat3 {
        self.stack.last().unwrap_or(&Mat3::IDENTITY)
    }

    /// Applies `matrix` within the current one, until [`Transform::pop`].
    pub fn push(&mut self, matrix: Mat3) -> &mut Self {
        let combined = *self.matrix() * matrix;
        self.stack.push(combined);
        self
    }

    pub fn pop(&mut self) -> &mut Self {
        self.stack.pop();
        self
    }
}

#[derive(Clone)]
pub(crate) struct Item {
    matrix: Mat3,
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    Clear(Color),
    Rect { min: Vec2, size: Vec2, radii: [f32; 4], paint: Ink },
    Ellipse { center: Vec2, radii: Vec2, rotation: f32, paint: Ink },
    Path { commands: Vec<PathCommand>, paint: Ink },
    Text { font: Font, text: String, at: Vec2, size: f32, color: Color, h: HAlign, v: VAlign, max_width: Option<f32> },
    Image { image: aero::image::Image, at: Vec2, size: Vec2, crop: Option<Rect>, color: Color },
}

/// A fill, or a stroke of some width, with its color.
#[derive(Clone, Copy)]
enum Ink {
    Fill(Brush),
    Stroke(Brush, Stroke),
}

/// Aero's shapes and paths, filled or stroked alike.
trait Painted: Sized {
    fn color(self, brush: Brush);
    fn stroke(self, brush: Brush, stroke: Stroke);

    fn paint(self, paint: Ink) {
        match paint {
            Ink::Fill(brush) => self.color(brush),
            Ink::Stroke(brush, stroke) => self.stroke(brush, stroke),
        }
    }
}

impl Painted for aero::draw::Shape<'_> {
    fn color(self, brush: Brush) {
        Self::color(self, brush);
    }

    /// Rectangles and ellipses have no ends nor corners to shape.
    fn stroke(self, brush: Brush, stroke: Stroke) {
        Self::stroke(self, brush, stroke.width);
    }
}

impl Painted for aero::path::PathShape<'_> {
    fn color(self, brush: Brush) {
        Self::color(self, brush);
    }

    fn stroke(self, brush: Brush, stroke: Stroke) {
        self.stroke_with(brush, stroke);
    }
}

/// lyon's corners, which notan strokes with, are pointed. notan lifts their limit, which lets a near reversal point
/// far out: past 10 half widths, these are bevelled.
const MITER: Join = Join::Miter(10.0);

#[derive(Clone, Copy)]
enum PathCommand {
    Move(Vec2),
    Line(Vec2),
    Quad(Vec2, Vec2),
    Cubic(Vec2, Vec2, Vec2),
    Close,
}

impl Draw {
    pub fn new(width: f32, height: f32) -> Self {
        Self { size: (width, height), alpha: 1.0, transform: Transform { stack: Vec::new() }, items: Vec::new() }
    }

    pub const fn size(&self) -> (f32, f32) {
        self.size
    }

    pub const fn width(&self) -> f32 {
        self.size.0
    }

    pub const fn height(&self) -> f32 {
        self.size.1
    }

    pub const fn set_size(&mut self, width: f32, height: f32) {
        self.size = (width, height);
    }

    /// Multiplies the alpha of every shape drawn after.
    pub const fn set_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }

    pub const fn alpha(&self) -> f32 {
        self.alpha
    }

    pub fn set_alpha_mode(&mut self, _mode: Option<BlendMode>) {}

    pub fn set_blend_mode(&mut self, _mode: Option<BlendMode>) {}

    pub fn transform(&mut self) -> &mut Transform {
        &mut self.transform
    }

    /// Wipes what the target holds before anything is drawn.
    pub fn clear(&mut self, color: Color) {
        self.items.push(Item { matrix: Mat3::IDENTITY, kind: Kind::Clear(color) });
    }

    fn add(&mut self, matrix: Option<Mat3>, kind: Kind) {
        let matrix = matrix.map_or(*self.transform.matrix(), |m| *self.transform.matrix() * m);
        self.items.push(Item { matrix, kind });
    }

    /// Plays the shapes into Aero's `draw`; a clear becomes its background.
    pub(crate) fn play(&self, draw: &mut aero::draw::Draw, fonts: &mut Fonts, texts: &mut Texts) {
        for item in &self.items {
            if let Kind::Clear(color) = item.kind {
                draw.background(color.aero(1.0));
                continue;
            }
            draw.push_transform(Affine2::from_mat3(item.matrix));
            play(&item.kind, draw, fonts, texts);
            draw.pop_transform();
        }
    }
}

fn play(kind: &Kind, draw: &mut aero::draw::Draw, fonts: &mut Fonts, texts: &mut Texts) {
    match kind {
        Kind::Clear(_) => {}
        Kind::Rect { min, size, radii, paint } => {
            draw.rect(Rect::new(min.x, min.y, size.x, size.y)).radii(*radii).paint(*paint);
        }
        Kind::Ellipse { center, radii, rotation, paint } => {
            draw.push_transform(Affine2::from_angle_translation(*rotation, *center));
            draw.ellipse(Vec2::ZERO, *radii).paint(*paint);
            draw.pop_transform();
        }
        Kind::Path { commands, paint } => {
            draw.path(|p| {
                for command in commands {
                    match *command {
                        PathCommand::Move(at) => p.move_to(at),
                        PathCommand::Line(at) => p.line_to(at),
                        PathCommand::Quad(c, at) => p.quad_to(c, at),
                        PathCommand::Cubic(c0, c1, at) => p.cubic_to(c0, c1, at),
                        PathCommand::Close => p.close(),
                    };
                }
            })
            .paint(*paint);
        }
        Kind::Text { font, text, at, size, color, h, v, max_width } => {
            let layout = texts.layout(fonts, font, *size, *max_width, text);
            let shown = layout.size();
            let x = match h {
                HAlign::Left => 0.0,
                HAlign::Center => shown.x * 0.5,
                HAlign::Right => shown.x,
            };
            let y = match v {
                VAlign::Top => 0.0,
                VAlign::Middle => shown.y * 0.5,
                VAlign::Bottom => shown.y,
            };
            draw.text_colored(fonts, layout, *at - Vec2::new(x, y), color.aero(1.0));
        }
        Kind::Image { image, at, size, crop, color } => {
            let shape = draw.image(image, Rect::new(at.x, at.y, size.x, size.y)).tint(color.aero(1.0));
            if let Some(crop) = crop {
                let _ = shape.crop(*crop);
            }
        }
    }
}

/// Records its shape into the draw when dropped, as notan's builders do.
pub struct DrawBuilder<'a, T: Shape> {
    draw: &'a mut Draw,
    shape: Option<T>,
}

pub trait Shape {
    fn record(self, draw: &mut Draw);
}

impl<'a, T: Shape> DrawBuilder<'a, T> {
    const fn new(draw: &'a mut Draw, shape: T) -> Self {
        Self { draw, shape: Some(shape) }
    }
}

impl<T: Shape> Deref for DrawBuilder<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.shape.as_ref().expect("a shape until dropped")
    }
}

impl<T: Shape> DerefMut for DrawBuilder<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.shape.as_mut().expect("a shape until dropped")
    }
}

impl<T: Shape> Drop for DrawBuilder<'_, T> {
    fn drop(&mut self) {
        if let Some(shape) = self.shape.take() {
            shape.record(self.draw);
        }
    }
}

/// A matrix of the shape's own, applied before the draw's.
pub trait DrawTransform {
    fn matrix(&mut self) -> &mut Option<Mat3>;

    fn transform(&mut self, matrix: Mat3) -> &mut Self {
        let current = self.matrix().unwrap_or(Mat3::IDENTITY);
        *self.matrix() = Some(current * matrix);
        self
    }

    fn translate(&mut self, x: f32, y: f32) -> &mut Self {
        self.transform(Mat3::from_translation(Vec2::new(x, y)))
    }

    fn scale(&mut self, x: f32, y: f32) -> &mut Self {
        self.transform(Mat3::from_scale(Vec2::new(x, y)))
    }

    fn rotate(&mut self, angle: f32) -> &mut Self {
        self.transform(Mat3::from_angle(angle))
    }

    fn rotate_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate(degrees.to_radians())
    }

    fn rotate_from(&mut self, point: (f32, f32), angle: f32) -> &mut Self {
        let at = Vec2::from(point);
        self.transform(Mat3::from_translation(at) * Mat3::from_angle(angle) * Mat3::from_translation(-at))
    }

    fn rotate_degrees_from(&mut self, point: (f32, f32), degrees: f32) -> &mut Self {
        self.rotate_from(point, degrees.to_radians())
    }

    fn scale_from(&mut self, point: (f32, f32), scale: (f32, f32)) -> &mut Self {
        let at = Vec2::from(point);
        self.transform(Mat3::from_translation(at) * Mat3::from_scale(Vec2::from(scale)) * Mat3::from_translation(-at))
    }
}

/// Fill and stroke as notan resolves them: fill unless told otherwise, in the order asked, each in its own color.
#[derive(Clone, Copy)]
struct Paint {
    color: Color,
    alpha: f32,
    fill_color: Option<Color>,
    stroke_color: Option<Color>,
    stroke: Stroke,
    modes: [Option<Mode>; 2],
    matrix: Option<Mat3>,
    /// A triangle's corners and their colors, blended across its fill.
    corners: Option<([Vec2; 3], [Color; 3])>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Fill,
    Stroke,
}

impl Paint {
    const fn new() -> Self {
        Self {
            color: Color::WHITE,
            alpha: 1.0,
            fill_color: None,
            stroke_color: None,
            stroke: Stroke { width: 1.0, cap: Cap::Butt, join: MITER },
            modes: [None; 2],
            matrix: None,
            corners: None,
        }
    }

    fn add_mode(&mut self, mode: Mode) {
        if let Some(free) = self.modes.iter_mut().find(|m| m.is_none()) {
            *free = Some(mode);
        }
    }

    /// The draw's alpha applies too.
    fn resolve(&self, draw_alpha: f32) -> Vec<Ink> {
        let alpha = self.alpha * draw_alpha;
        let modes: Vec<Mode> = match self.modes {
            [None, _] => vec![Mode::Fill],
            modes => modes.into_iter().flatten().collect(),
        };
        let fill = match self.corners {
            Some((points, colors)) => corner_blend(points, colors.map(|c| c.aero(alpha))),
            None => self.fill_color.unwrap_or(self.color).aero(alpha).into(),
        };
        modes
            .into_iter()
            .map(|mode| match mode {
                Mode::Fill => Ink::Fill(fill),
                Mode::Stroke => Ink::Stroke(self.stroke_color.unwrap_or(self.color).aero(alpha).into(), self.stroke),
            })
            .collect()
    }
}

/// The colors of a triangle's corners, blended over it as notan does: a gradient when two corners share a color, which
/// is then constant along their side; three different colors, which no gradient makes, give their average.
fn corner_blend(points: [Vec2; 3], colors: [aero::geom::Color; 3]) -> Brush {
    for odd in 0..3 {
        let (a, b) = ((odd + 1) % 3, (odd + 2) % 3);
        if colors[a] != colors[b] {
            continue;
        }
        let side = points[b] - points[a];
        let foot = points[a] + side * ((points[odd] - points[a]).dot(side) / side.length_squared().max(f32::EPSILON));
        return Gradient::linear(points[odd], colors[odd], foot, colors[a]).into();
    }
    let sum = colors.iter().fold([0.0; 4], |acc, c| {
        let c = c.to_array();
        [acc[0] + c[0], acc[1] + c[1], acc[2] + c[2], acc[3] + c[3]]
    });
    aero::geom::Color::srgba(sum[0] / 3.0, sum[1] / 3.0, sum[2] / 3.0, sum[3] / 3.0).into()
}

macro_rules! paint_methods {
    ($shape:ty) => {
        impl $shape {
            pub fn color(&mut self, color: Color) -> &mut Self {
                self.paint.color = color;
                self
            }

            pub fn fill_color(&mut self, color: Color) -> &mut Self {
                self.paint.fill_color = Some(color);
                self
            }

            pub fn stroke_color(&mut self, color: Color) -> &mut Self {
                self.paint.stroke_color = Some(color);
                self
            }

            pub fn alpha(&mut self, alpha: f32) -> &mut Self {
                self.paint.alpha = alpha;
                self
            }

            pub fn fill(&mut self) -> &mut Self {
                self.paint.add_mode(Mode::Fill);
                self
            }

            pub fn stroke(&mut self, width: f32) -> &mut Self {
                self.paint.stroke.width = width;
                self.paint.add_mode(Mode::Stroke);
                self
            }

            pub fn blend_mode(&mut self, _mode: BlendMode) -> &mut Self {
                self
            }

            pub fn alpha_mode(&mut self, _mode: BlendMode) -> &mut Self {
                self
            }
        }

        impl DrawTransform for $shape {
            fn matrix(&mut self) -> &mut Option<Mat3> {
                &mut self.paint.matrix
            }
        }
    };
}

pub struct Rectangle {
    min: Vec2,
    size: Vec2,
    radii: [f32; 4],
    paint: Paint,
}

paint_methods!(Rectangle);

impl Rectangle {
    pub fn corner_radius(&mut self, radius: f32) -> &mut Self {
        self.radii = [radius; 4];
        self
    }

    pub fn top_left_radius(&mut self, radius: f32) -> &mut Self {
        self.radii[0] = radius;
        self
    }

    pub fn top_right_radius(&mut self, radius: f32) -> &mut Self {
        self.radii[1] = radius;
        self
    }

    pub fn bottom_right_radius(&mut self, radius: f32) -> &mut Self {
        self.radii[2] = radius;
        self
    }

    pub fn bottom_left_radius(&mut self, radius: f32) -> &mut Self {
        self.radii[3] = radius;
        self
    }
}

impl Shape for Rectangle {
    fn record(self, draw: &mut Draw) {
        for paint in self.paint.resolve(draw.alpha) {
            draw.add(self.paint.matrix, Kind::Rect { min: self.min, size: self.size, radii: self.radii, paint });
        }
    }
}

/// A circle or an ellipse.
pub struct Ellipse {
    center: Vec2,
    radii: Vec2,
    rotation: f32,
    paint: Paint,
}

paint_methods!(Ellipse);

impl Ellipse {
    /// The center, for a circle.
    pub fn position(&mut self, x: f32, y: f32) -> &mut Self {
        self.center = Vec2::new(x, y);
        self
    }

    /// Turns the ellipse about its center.
    pub fn rotate(&mut self, radians: f32) -> &mut Self {
        self.rotation = radians;
        self
    }
}

impl Shape for Ellipse {
    fn record(self, draw: &mut Draw) {
        for paint in self.paint.resolve(draw.alpha) {
            let kind = Kind::Ellipse { center: self.center, radii: self.radii, rotation: self.rotation, paint };
            draw.add(self.paint.matrix, kind);
        }
    }
}

/// Lines and curves, and the triangles and stars made of them.
pub struct Path {
    commands: Vec<PathCommand>,
    paint: Paint,
    /// Where a star goes, its points being relative to it.
    offset: Vec2,
}

paint_methods!(Path);

impl Path {
    fn of(commands: Vec<PathCommand>) -> Self {
        Self { commands, paint: Paint::new(), offset: Vec2::ZERO }
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand::Move(Vec2::new(x, y)));
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand::Line(Vec2::new(x, y)));
        self
    }

    pub fn quadratic_bezier_to(&mut self, control: (f32, f32), to: (f32, f32)) -> &mut Self {
        self.commands.push(PathCommand::Quad(control.into(), to.into()));
        self
    }

    pub fn cubic_bezier_to(&mut self, c0: (f32, f32), c1: (f32, f32), to: (f32, f32)) -> &mut Self {
        self.commands.push(PathCommand::Cubic(c0.into(), c1.into(), to.into()));
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.commands.push(PathCommand::Close);
        self
    }

    /// For a star: where its center goes.
    pub fn position(&mut self, x: f32, y: f32) -> &mut Self {
        self.offset = Vec2::new(x, y);
        self
    }

    /// For a triangle: a color at each corner, blended over its fill.
    pub fn color_vertex(&mut self, a: Color, b: Color, c: Color) -> &mut Self {
        let corners = self.commands.iter().filter_map(|c| match *c {
            PathCommand::Move(p) | PathCommand::Line(p) => Some(p),
            _ => None,
        });
        if let [p0, p1, p2] = corners.collect::<Vec<_>>()[..] {
            self.paint.corners = Some(([p0, p1, p2], [a, b, c]));
        }
        self
    }

    pub fn round_cap(&mut self) -> &mut Self {
        self.paint.stroke.cap = Cap::Round;
        self
    }

    pub fn butt_cap(&mut self) -> &mut Self {
        self.paint.stroke.cap = Cap::Butt;
        self
    }

    pub fn square_cap(&mut self) -> &mut Self {
        self.paint.stroke.cap = Cap::Square;
        self
    }

    pub fn round_join(&mut self) -> &mut Self {
        self.paint.stroke.join = Join::Round;
        self
    }

    pub fn bevel_join(&mut self) -> &mut Self {
        self.paint.stroke.join = Join::Bevel;
        self
    }

    pub fn miter_join(&mut self) -> &mut Self {
        self.paint.stroke.join = MITER;
        self
    }
}

impl Shape for Path {
    fn record(self, draw: &mut Draw) {
        let moved = |p: Vec2| p + self.offset;
        let commands: Vec<PathCommand> = self
            .commands
            .iter()
            .map(|c| match *c {
                PathCommand::Move(p) => PathCommand::Move(moved(p)),
                PathCommand::Line(p) => PathCommand::Line(moved(p)),
                PathCommand::Quad(c, p) => PathCommand::Quad(moved(c), moved(p)),
                PathCommand::Cubic(c0, c1, p) => PathCommand::Cubic(moved(c0), moved(c1), moved(p)),
                PathCommand::Close => PathCommand::Close,
            })
            .collect();
        let mut placed = self.paint;
        placed.corners = placed.corners.map(|(points, colors)| (points.map(moved), colors));
        for paint in placed.resolve(draw.alpha) {
            draw.add(self.paint.matrix, Kind::Path { commands: commands.clone(), paint });
        }
    }
}

/// A line: stroked, `width` wide.
pub struct Line {
    from: Vec2,
    to: Vec2,
    width: f32,
    color: Color,
    alpha: f32,
    matrix: Option<Mat3>,
}

impl Line {
    pub fn color(&mut self, color: Color) -> &mut Self {
        self.color = color;
        self
    }

    pub fn width(&mut self, width: f32) -> &mut Self {
        self.width = width;
        self
    }

    pub fn alpha(&mut self, alpha: f32) -> &mut Self {
        self.alpha = alpha;
        self
    }
}

impl DrawTransform for Line {
    fn matrix(&mut self) -> &mut Option<Mat3> {
        &mut self.matrix
    }
}

impl Shape for Line {
    fn record(self, draw: &mut Draw) {
        let color = self.color.aero(self.alpha * draw.alpha);
        let commands = vec![PathCommand::Move(self.from), PathCommand::Line(self.to)];
        let stroke = Stroke::new(self.width).cap(Cap::Butt).join(MITER);
        draw.add(self.matrix, Kind::Path { commands, paint: Ink::Stroke(color.into(), stroke) });
    }
}

#[derive(Clone, Copy)]
enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy)]
enum VAlign {
    Top,
    Middle,
    Bottom,
}

pub struct Text {
    font: Font,
    text: String,
    at: Vec2,
    size: f32,
    color: Color,
    alpha: f32,
    h: HAlign,
    v: VAlign,
    max_width: Option<f32>,
    matrix: Option<Mat3>,
}

impl Text {
    pub fn position(&mut self, x: f32, y: f32) -> &mut Self {
        self.at = Vec2::new(x, y);
        self
    }

    pub fn size(&mut self, size: f32) -> &mut Self {
        self.size = size;
        self
    }

    pub fn color(&mut self, color: Color) -> &mut Self {
        self.color = color;
        self
    }

    pub fn alpha(&mut self, alpha: f32) -> &mut Self {
        self.alpha = alpha;
        self
    }

    pub fn max_width(&mut self, width: f32) -> &mut Self {
        self.max_width = Some(width);
        self
    }

    pub fn h_align_left(&mut self) -> &mut Self {
        self.h = HAlign::Left;
        self
    }

    pub fn h_align_center(&mut self) -> &mut Self {
        self.h = HAlign::Center;
        self
    }

    pub fn h_align_right(&mut self) -> &mut Self {
        self.h = HAlign::Right;
        self
    }

    pub fn v_align_top(&mut self) -> &mut Self {
        self.v = VAlign::Top;
        self
    }

    pub fn v_align_middle(&mut self) -> &mut Self {
        self.v = VAlign::Middle;
        self
    }

    pub fn v_align_bottom(&mut self) -> &mut Self {
        self.v = VAlign::Bottom;
        self
    }
}

impl DrawTransform for Text {
    fn matrix(&mut self) -> &mut Option<Mat3> {
        &mut self.matrix
    }
}

impl Shape for Text {
    fn record(self, draw: &mut Draw) {
        let color = self.color.with_alpha(self.color.a * self.alpha * draw.alpha);
        let kind = Kind::Text {
            font: self.font,
            text: self.text,
            at: self.at,
            size: self.size,
            color,
            h: self.h,
            v: self.v,
            max_width: self.max_width,
        };
        draw.add(self.matrix, kind);
    }
}

pub struct Image {
    image: aero::image::Image,
    at: Vec2,
    size: Option<Vec2>,
    crop: Option<Rect>,
    color: Color,
    alpha: f32,
    matrix: Option<Mat3>,
}

impl Image {
    pub fn position(&mut self, x: f32, y: f32) -> &mut Self {
        self.at = Vec2::new(x, y);
        self
    }

    /// Stretched to it; the texture's size, or its crop's, by default.
    pub fn size(&mut self, width: f32, height: f32) -> &mut Self {
        self.size = Some(Vec2::new(width, height));
        self
    }

    /// The part shown, in the texture's pixels.
    pub fn crop(&mut self, at: (f32, f32), size: (f32, f32)) -> &mut Self {
        self.crop = Some(Rect::new(at.0, at.1, size.0, size.1));
        self
    }

    pub fn color(&mut self, color: Color) -> &mut Self {
        self.color = color;
        self
    }

    pub fn alpha(&mut self, alpha: f32) -> &mut Self {
        self.alpha = alpha;
        self
    }
}

impl DrawTransform for Image {
    fn matrix(&mut self) -> &mut Option<Mat3> {
        &mut self.matrix
    }
}

impl Shape for Image {
    fn record(self, draw: &mut Draw) {
        let (w, h) = self.image.size();
        let natural = self.crop.map_or(Vec2::new(w as f32, h as f32), |c| c.size());
        let color = self.color.with_alpha(self.color.a * self.alpha * draw.alpha);
        let kind =
            Kind::Image { image: self.image, at: self.at, size: self.size.unwrap_or(natural), crop: self.crop, color };
        draw.add(self.matrix, kind);
    }
}

pub trait DrawShapes {
    fn rect(&mut self, position: (f32, f32), size: (f32, f32)) -> DrawBuilder<'_, Rectangle>;
    fn circle(&mut self, radius: f32) -> DrawBuilder<'_, Ellipse>;
    fn ellipse(&mut self, center: (f32, f32), radii: (f32, f32)) -> DrawBuilder<'_, Ellipse>;
    fn line(&mut self, from: (f32, f32), to: (f32, f32)) -> DrawBuilder<'_, Line>;
    fn triangle(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> DrawBuilder<'_, Path>;
    fn path(&mut self) -> DrawBuilder<'_, Path>;
    /// Its first point straight up from the center, which [`Path::position`] sets.
    fn star(&mut self, spikes: u8, outer: f32, inner: f32) -> DrawBuilder<'_, Path>;
}

impl DrawShapes for Draw {
    fn rect(&mut self, position: (f32, f32), size: (f32, f32)) -> DrawBuilder<'_, Rectangle> {
        let rect = Rectangle { min: position.into(), size: size.into(), radii: [0.0; 4], paint: Paint::new() };
        DrawBuilder::new(self, rect)
    }

    fn circle(&mut self, radius: f32) -> DrawBuilder<'_, Ellipse> {
        let ellipse = Ellipse { center: Vec2::ZERO, radii: Vec2::splat(radius), rotation: 0.0, paint: Paint::new() };
        DrawBuilder::new(self, ellipse)
    }

    fn ellipse(&mut self, center: (f32, f32), radii: (f32, f32)) -> DrawBuilder<'_, Ellipse> {
        let ellipse = Ellipse { center: center.into(), radii: radii.into(), rotation: 0.0, paint: Paint::new() };
        DrawBuilder::new(self, ellipse)
    }

    fn line(&mut self, from: (f32, f32), to: (f32, f32)) -> DrawBuilder<'_, Line> {
        let line = Line { from: from.into(), to: to.into(), width: 1.0, color: Color::WHITE, alpha: 1.0, matrix: None };
        DrawBuilder::new(self, line)
    }

    fn triangle(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> DrawBuilder<'_, Path> {
        let commands = vec![
            PathCommand::Move(a.into()),
            PathCommand::Line(b.into()),
            PathCommand::Line(c.into()),
            PathCommand::Close,
        ];
        DrawBuilder::new(self, Path::of(commands))
    }

    fn path(&mut self) -> DrawBuilder<'_, Path> {
        DrawBuilder::new(self, Path::of(Vec::new()))
    }

    fn star(&mut self, spikes: u8, outer: f32, inner: f32) -> DrawBuilder<'_, Path> {
        let step = std::f32::consts::PI / f32::from(spikes.max(2));
        let mut commands = vec![PathCommand::Move(Vec2::new(0.0, -outer))];
        let mut angle = std::f32::consts::FRAC_PI_2 * 3.0;
        for _ in 0..spikes.max(2) {
            for radius in [outer, inner] {
                commands.push(PathCommand::Line(Vec2::new(angle.cos(), angle.sin()) * radius));
                angle += step;
            }
        }
        commands.push(PathCommand::Close);
        DrawBuilder::new(self, Path::of(commands))
    }
}

pub trait DrawImages {
    fn image(&mut self, texture: &Texture) -> DrawBuilder<'_, Image>;
}

impl DrawImages for Draw {
    fn image(&mut self, texture: &Texture) -> DrawBuilder<'_, Image> {
        let image = Image {
            image: texture.image.clone(),
            at: Vec2::ZERO,
            size: None,
            crop: None,
            color: Color::WHITE,
            alpha: 1.0,
            matrix: None,
        };
        DrawBuilder::new(self, image)
    }
}

pub trait DrawTextSection {
    fn text(&mut self, font: &Font, text: &str) -> DrawBuilder<'_, Text>;
}

impl DrawTextSection for Draw {
    fn text(&mut self, font: &Font, text: &str) -> DrawBuilder<'_, Text> {
        let text = Text {
            font: font.clone(),
            text: text.to_owned(),
            at: Vec2::ZERO,
            size: 16.0,
            color: Color::WHITE,
            alpha: 1.0,
            h: HAlign::Left,
            v: VAlign::Top,
            max_width: None,
            matrix: None,
        };
        DrawBuilder::new(self, text)
    }
}

#[cfg(test)]
mod tests {
    use aero::geom::Brush;
    use aero::math::Vec2;

    use super::{corner_blend, Draw, DrawShapes, Ink, Kind};
    use crate::color::Color;

    fn inks(draw: &Draw) -> Vec<Ink> {
        draw.items
            .iter()
            .filter_map(|item| match &item.kind {
                Kind::Rect { paint, .. } | Kind::Path { paint, .. } => Some(*paint),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn shapes_fill_unless_told_and_keep_the_order_asked() {
        let mut draw = Draw::new(100.0, 100.0);
        draw.rect((0.0, 0.0), (10.0, 10.0)).color(Color::RED);
        draw.rect((0.0, 0.0), (10.0, 10.0)).stroke(2.0).fill_color(Color::BLUE).fill().stroke_color(Color::GREEN);
        draw.set_alpha(0.5);
        draw.rect((0.0, 0.0), (10.0, 10.0)).color(Color::WHITE);
        let red = Color::RED.aero(1.0);
        let (blue, green) = (Color::BLUE.aero(1.0), Color::GREEN.aero(1.0));
        let faded = Color::WHITE.aero(0.5);
        match inks(&draw)[..] {
            [Ink::Fill(Brush::Solid(a)), Ink::Stroke(Brush::Solid(b), w), Ink::Fill(Brush::Solid(c)), Ink::Fill(Brush::Solid(d))] =>
            {
                assert_eq!((a, b, w.width, c, d), (red, green, 2.0, blue, faded));
            }
            _ => panic!("fill, then stroke and fill as asked, then the faded fill"),
        }
    }

    #[test]
    fn two_corners_of_a_color_blend_toward_the_third() {
        let (clear, solid) = (Color::RED.with_alpha(0.0).aero(1.0), Color::RED.aero(1.0));
        let points = [Vec2::new(10.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0)];
        let Brush::Linear(g) = corner_blend(points, [clear, solid, solid]) else { panic!("a gradient") };
        assert_eq!((g.from, g.start, g.to, g.end), (points[0], clear, Vec2::new(0.0, 0.0), solid));
        let mixed = Color::BLUE.aero(1.0);
        assert!(matches!(corner_blend(points, [clear, solid, mixed]), Brush::Solid(_)), "three colors: their average");
    }
}
