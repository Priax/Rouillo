#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn at(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn corners(&self) -> [(f32, f32); 4] {
        let (right, bottom) = (self.x + self.w, self.y + self.h);
        [(self.x, self.y), (right, self.y), (right, bottom), (self.x, bottom)]
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx < self.x + self.w && my >= self.y && my < self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacked_rows_never_share_a_point() {
        let (top, below) = (Rect::at(0.0, 0.0, 100.0, 64.0), Rect::at(0.0, 64.0, 100.0, 64.0));
        assert!(!top.contains(50.0, 64.0) && below.contains(50.0, 64.0));
        assert!(top.contains(0.0, 0.0) && !top.contains(100.0, 10.0));
    }
}
