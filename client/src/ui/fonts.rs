use notan::draw::{CreateFont, Font};
use notan::prelude::Graphics;

pub struct Fonts {
    pub text: Font,
    pub display: Font,
}

impl Fonts {
    pub fn load(gfx: &mut Graphics) -> Result<Self, String> {
        Ok(Self {
            text: gfx.create_font(include_bytes!("../../../assets/fonts/Nunito-ExtraBold.ttf"))?,
            display: gfx.create_font(include_bytes!("../../../assets/fonts/SairaCondensed-Bold.ttf"))?,
        })
    }
}
