use std::ops::Deref;

use aero::Cx;

use crate::color::TextureFilter;
use crate::drawing::Draw;

/// What notan's callbacks draw with: Aero's context, and while drawing, the window's frame.
pub struct Graphics<'a> {
    pub(crate) cx: &'a mut Cx,
    pub(crate) frame: Option<&'a mut aero::draw::Draw>,
}

impl Graphics<'_> {
    /// The window's size in logical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.cx.window(aero::WindowId::MAIN).map_or((0, 0), |w| (w.size.x as u32, w.size.y as u32))
    }

    /// Draws `draw` into the window, over what is already drawn this frame.
    pub fn render(&mut self, draw: &Draw) {
        if let Some(frame) = self.frame.as_deref_mut() {
            draw.play(frame, &mut self.cx.fonts);
        }
    }

    /// Draws `draw` into `target` right away, over what it holds.
    pub fn render_to(&mut self, target: &RenderTexture, draw: &Draw) {
        self.cx.render_to(&target.texture.image, |frame, fonts| draw.play(frame, fonts));
    }

    pub fn create_texture(&mut self) -> TextureBuilder {
        TextureBuilder { bytes: None, filter: TextureFilter::Linear }
    }

    pub fn create_render_texture(&mut self, width: u32, height: u32) -> RenderTextureBuilder {
        RenderTextureBuilder { width, height, filter: TextureFilter::Linear }
    }
}

/// A font added to Aero, drawn by its family name.
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    family: String,
    /// notan's sizes span the ascent and descent; Aero's, like CSS, span the em.
    em_per_size: f32,
}

impl Font {
    pub(crate) fn family(&self) -> &str {
        &self.family
    }

    /// Aero's size for text notan would draw at `size`.
    pub(crate) fn em(&self, size: f32) -> f32 {
        size * self.em_per_size
    }

    /// notan's lines span the ascent and descent, which puts the baseline an ascent below the top.
    pub(crate) fn line_height(&self) -> f32 {
        1.0 / self.em_per_size
    }
}

fn em_per_size(data: &[u8]) -> Result<f32, String> {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::new(data).map_err(|e| e.to_string())?;
    let metrics = font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
    let height = metrics.ascent - metrics.descent;
    if height <= 0.0 {
        return Err("a font without ascent nor descent".into());
    }
    Ok(f32::from(metrics.units_per_em) / height)
}

pub trait CreateFont {
    fn create_font(&mut self, data: &'static [u8]) -> Result<Font, String>;
}

impl CreateFont for Graphics<'_> {
    fn create_font(&mut self, data: &'static [u8]) -> Result<Font, String> {
        let em_per_size = em_per_size(data)?;
        let family = self.cx.fonts.add(data).into_iter().next().ok_or("no font in the data")?;
        Ok(Font { family, em_per_size })
    }
}

pub trait CreateDraw {
    fn create_draw(&self) -> Draw;
}

impl CreateDraw for Graphics<'_> {
    fn create_draw(&self) -> Draw {
        let (w, h) = self.size();
        Draw::new(w as f32, h as f32)
    }
}

#[derive(Clone)]
pub struct Texture {
    pub(crate) image: aero::image::Image,
}

impl Texture {
    pub fn size(&self) -> (f32, f32) {
        let (w, h) = self.image.size();
        (w as f32, h as f32)
    }

    pub fn width(&self) -> f32 {
        self.size().0
    }

    pub fn height(&self) -> f32 {
        self.size().1
    }
}

pub struct TextureBuilder {
    bytes: Option<Vec<u8>>,
    filter: TextureFilter,
}

impl TextureBuilder {
    /// An encoded picture: PNG, JPEG or ICO.
    #[must_use]
    pub fn from_image(mut self, bytes: &[u8]) -> Self {
        self.bytes = Some(bytes.to_vec());
        self
    }

    /// Aero filters both ways alike: `mag` decides.
    #[must_use]
    pub const fn with_filter(mut self, _min: TextureFilter, mag: TextureFilter) -> Self {
        self.filter = mag;
        self
    }

    pub fn build(self) -> Result<Texture, String> {
        let bytes = self.bytes.ok_or("no picture given")?;
        let (width, height, rgba) = decode(&bytes)?;
        let image =
            aero::image::Image::from_rgba(width, height, rgba, self.filter.aero()).map_err(|e| e.to_string())?;
        Ok(Texture { image })
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let picture = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
    Ok((picture.width(), picture.height(), picture.into_raw()))
}

/// A texture drawn into, which draws like any other.
#[derive(Clone)]
pub struct RenderTexture {
    texture: Texture,
}

impl Deref for RenderTexture {
    type Target = Texture;

    fn deref(&self) -> &Texture {
        &self.texture
    }
}

impl CreateDraw for RenderTexture {
    fn create_draw(&self) -> Draw {
        let (w, h) = self.size();
        Draw::new(w, h)
    }
}

pub struct RenderTextureBuilder {
    width: u32,
    height: u32,
    filter: TextureFilter,
}

impl RenderTextureBuilder {
    #[must_use]
    pub const fn with_filter(mut self, _min: TextureFilter, mag: TextureFilter) -> Self {
        self.filter = mag;
        self
    }

    pub fn build(self) -> Result<RenderTexture, String> {
        let image =
            aero::image::Image::target(self.width, self.height, self.filter.aero()).map_err(|e| e.to_string())?;
        Ok(RenderTexture { texture: Texture { image } })
    }
}
