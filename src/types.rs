//! Basic geometry and color types. All sizes are in logical points.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub const EVERYTHING: Rect = Rect::new(-1e6, -1e6, 2e6, 2e6);

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
    pub fn inflate(&self, d: f32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
    pub(crate) fn arr(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }
}

/// sRGB color with straight (non-premultiplied) alpha, components in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
    /// `Color::hex(0x0A84FF)`
    pub const fn hex(v: u32) -> Self {
        Self {
            r: ((v >> 16) & 0xFF) as f32 / 255.0,
            g: ((v >> 8) & 0xFF) as f32 / 255.0,
            b: (v & 0xFF) as f32 / 255.0,
            a: 1.0,
        }
    }
    pub const fn alpha(mut self, a: f32) -> Self {
        self.a = a;
        self
    }
    pub fn lerp(self, o: Color, t: f32) -> Color {
        Color::rgba(
            self.r + (o.r - self.r) * t,
            self.g + (o.g - self.g) * t,
            self.b + (o.b - self.b) * t,
            self.a + (o.a - self.a) * t,
        )
    }
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }
    pub(crate) fn arr(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub const CLEAR: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::hex(0xFFFFFF);
    pub const BLACK: Color = Color::hex(0x000000);
    // iOS system palette
    pub const BLUE: Color = Color::hex(0x0A84FF);
    pub const GREEN: Color = Color::hex(0x30D158);
    pub const RED: Color = Color::hex(0xFF453A);
    pub const ORANGE: Color = Color::hex(0xFF9F0A);
    pub const YELLOW: Color = Color::hex(0xFFD60A);
    pub const PINK: Color = Color::hex(0xFF375F);
    pub const PURPLE: Color = Color::hex(0xBF5AF2);
    pub const INDIGO: Color = Color::hex(0x5E5CE6);
    pub const TEAL: Color = Color::hex(0x40C8E0);
    pub const GRAY: Color = Color::hex(0x8E8E93);
}
