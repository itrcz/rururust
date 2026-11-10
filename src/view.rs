//! The declarative view tree: `View<S>` plus constructors and modifiers.
//!
//! A view is a cheap description rebuilt from app state `S` whenever something
//! changes. Event handlers are closures that mutate `&mut S`.

use crate::types::Color;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dim {
    /// Hug content.
    Auto,
    /// Exact size in points.
    Fixed(f32),
    /// Take all the space the parent offers.
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Axis {
    H,
    V,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weight {
    Regular = 400,
    Medium = 500,
    Semibold = 600,
    Bold = 700,
    Heavy = 800,
}

#[derive(Clone, Copy, Debug)]
pub enum Fill {
    Solid(Color),
    /// Two-stop linear gradient; angle in degrees, CSS convention (180 = top to bottom).
    Linear(Color, Color, f32),
}

impl From<Color> for Fill {
    fn from(c: Color) -> Self {
        Fill::Solid(c)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub color: Color,
    pub blur: f32,
    pub y: f32,
}

/// Glass material: the content behind it is lightly frosted and refracted through a
/// bevelled rim. Defaults follow iOS "regular" glass.
#[derive(Clone, Copy, Debug)]
pub struct Glass {
    /// Color mixed over the backdrop; alpha is the mix strength.
    pub tint: Color,
    /// Saturation boost of the backdrop (1 = unchanged).
    pub saturation: f32,
    /// Blur of the backdrop in points (0 = perfectly clear glass).
    pub frost: f32,
    /// How strongly the rim bends the backdrop, 0..=1 (0 = flat pane).
    pub refraction: f32,
    /// Width of the bevelled rim in points (capped at half the pane's smaller side).
    pub depth: f32,
    /// Colour fringing inside the bevel, 0..=1 (costs extra texture samples there).
    pub dispersion: f32,
    /// Brightness of the specular rim line.
    pub rim: f32,
}

impl Glass {
    /// iOS-like regular glass: light frost, strong lensing at the rim.
    pub const fn regular() -> Self {
        Glass {
            tint: Color::WHITE.alpha(0.06),
            saturation: 1.35,
            frost: 10.0,
            refraction: 0.7,
            depth: 30.0,
            dispersion: 0.2,
            rim: 0.3,
        }
    }
    /// Nearly transparent glass for small controls over rich content.
    pub const fn clear() -> Self {
        Glass { tint: Color::WHITE.alpha(0.02), saturation: 1.15, frost: 5.0, rim: 0.4, ..Self::regular() }
    }
    /// Heavily blurred "frosted" material for panels with a lot of text.
    pub const fn frosted() -> Self {
        Glass { tint: Color::WHITE.alpha(0.10), saturation: 1.7, frost: 44.0, refraction: 0.5, ..Self::regular() }
    }
}

impl Glass {
    /// Scales the whole effect: 0 is an invisible pane, 1 leaves the material as it is,
    /// larger values make it frostier and more opaque.
    pub fn strength(mut self, k: f32) -> Self {
        let k = k.max(0.0);
        self.frost *= k * k;
        self.refraction = (self.refraction * k).min(1.0);
        self.dispersion = (self.dispersion * k).min(1.0);
        self.rim = (self.rim * k).min(1.0);
        self.saturation = 1.0 + (self.saturation - 1.0) * k;
        self.tint.a = (self.tint.a * k).min(1.0);
        self
    }
}

impl Default for Glass {
    fn default() -> Self {
        Self::regular()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Style {
    pub padding: [f32; 4], // left, top, right, bottom
    pub width: Dim,
    pub height: Dim,
    pub bg: Option<Fill>,
    pub radius: f32,
    pub border: Option<(f32, Color)>,
    pub shadow: Option<Shadow>,
    pub glass: Option<Glass>,
    pub opacity: f32,
    pub fg: Option<Color>,
    pub place: Option<(Align, Align)>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            padding: [0.0; 4],
            width: Dim::Auto,
            height: Dim::Auto,
            bg: None,
            radius: 0.0,
            border: None,
            shadow: None,
            glass: None,
            opacity: 1.0,
            fg: None,
            place: None,
        }
    }
}

pub(crate) struct TextData {
    pub content: String,
    pub size: f32,
    pub weight: Weight,
    pub center: bool,
}

/// Decoded picture, cheap to clone. Create it once (not inside `view`) and keep it in
/// your state or a static.
#[derive(Clone)]
pub struct Image(pub(crate) Arc<ImageData>);

pub(crate) struct ImageData {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    /// RGBA8, premultiplied alpha.
    pub pixels: Vec<u8>,
}

impl Image {
    /// From straight-alpha RGBA8 pixels, row by row.
    pub fn from_rgba(width: u32, height: u32, mut pixels: Vec<u8>) -> Image {
        assert_eq!(pixels.len(), (width * height * 4) as usize, "pixel buffer size");
        static NEXT: AtomicU64 = AtomicU64::new(1);
        for px in pixels.chunks_exact_mut(4) {
            let a = px[3] as u16;
            if a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u16 * a + 127) / 255) as u8;
                }
            }
        }
        Image(Arc::new(ImageData { id: NEXT.fetch_add(1, Ordering::Relaxed), width, height, pixels }))
    }

    /// Decodes a PNG or JPEG file, e.g. `Image::decode(include_bytes!("photo.jpg"))`.
    pub fn decode(bytes: &[u8]) -> Result<Image, String> {
        let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
        Ok(Image::from_rgba(img.width(), img.height(), img.into_raw()))
    }

    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }
}

pub(crate) enum Kind<S> {
    Image(Image),
    TextField { value: String, placeholder: String, on_change: Box<dyn Fn(&mut S, String)> },
    Picker { options: Vec<String>, selected: usize, on_select: Rc<dyn Fn(&mut S, usize)> },
    Tabs { labels: Vec<String>, selected: usize, on_select: Box<dyn Fn(&mut S, usize)> },
    Empty,
    Spacer,
    Stack { axis: Axis, spacing: f32, align: Align, children: Vec<View<S>> },
    Text(TextData),
    Scroll(Box<View<S>>),
    Toggle(bool, Box<dyn Fn(&mut S, bool)>),
    Slider(f32, Box<dyn Fn(&mut S, f32)>),
}

/// A node of the UI tree, generic over the app state `S`.
pub struct View<S> {
    pub(crate) kind: Kind<S>,
    pub(crate) style: Style,
    pub(crate) on_tap: Option<Box<dyn Fn(&mut S)>>,
    pub(crate) key: Option<u64>,
}

impl<S> View<S> {
    fn new(kind: Kind<S>) -> Self {
        View { kind, style: Style::default(), on_tap: None, key: None }
    }

    // ---- layout ----
    pub fn padding(mut self, p: f32) -> Self {
        self.style.padding = [p; 4];
        self
    }
    pub fn padding_xy(mut self, x: f32, y: f32) -> Self {
        self.style.padding = [x, y, x, y];
        self
    }
    /// left, top, right, bottom
    pub fn padding_ltrb(mut self, l: f32, t: f32, r: f32, b: f32) -> Self {
        self.style.padding = [l, t, r, b];
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.style.width = Dim::Fixed(w);
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.style.height = Dim::Fixed(h);
        self
    }
    pub fn size(self, w: f32, h: f32) -> Self {
        self.width(w).height(h)
    }
    pub fn fill_width(mut self) -> Self {
        self.style.width = Dim::Fill;
        self
    }
    pub fn fill_height(mut self) -> Self {
        self.style.height = Dim::Fill;
        self
    }
    pub fn fill(self) -> Self {
        self.fill_width().fill_height()
    }
    /// Position inside a parent `zstack` (horizontal, vertical).
    pub fn place(mut self, x: Align, y: Align) -> Self {
        self.style.place = Some((x, y));
        self
    }
    /// Stack only: gap between children.
    pub fn spacing(mut self, s: f32) -> Self {
        if let Kind::Stack { spacing, .. } = &mut self.kind {
            *spacing = s;
        }
        self
    }
    /// Stack only: cross-axis alignment of children.
    pub fn align(mut self, a: Align) -> Self {
        if let Kind::Stack { align, .. } = &mut self.kind {
            *align = a;
        }
        self
    }

    // ---- appearance ----
    pub fn background(mut self, fill: impl Into<Fill>) -> Self {
        self.style.bg = Some(fill.into());
        self
    }
    pub fn gradient(mut self, from: Color, to: Color, angle_deg: f32) -> Self {
        self.style.bg = Some(Fill::Linear(from, to, angle_deg));
        self
    }
    /// Corner radius with iOS-style continuous (squircle) corners.
    pub fn radius(mut self, r: f32) -> Self {
        self.style.radius = r;
        self
    }
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.style.border = Some((width, color));
        self
    }
    pub fn shadow(mut self, color: Color, blur: f32, y: f32) -> Self {
        self.style.shadow = Some(Shadow { color, blur, y });
        self
    }
    /// Frosted "liquid glass": blurs and refracts whatever is behind this view.
    pub fn glass(mut self) -> Self {
        self.style.glass = Some(Glass::default());
        self
    }
    pub fn glass_with(mut self, glass: Glass) -> Self {
        self.style.glass = Some(glass);
        self
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.style.opacity = o;
        self
    }
    /// Text color, inherited by all descendants.
    pub fn color(mut self, c: Color) -> Self {
        self.style.fg = Some(c);
        self
    }

    // ---- text ----
    pub fn font_size(mut self, s: f32) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.size = s;
        }
        self
    }
    pub fn weight(mut self, w: Weight) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.weight = w;
        }
        self
    }
    /// Center lines of multi-line text.
    pub fn centered(mut self) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.center = true;
        }
        self
    }

    // ---- behavior ----
    /// Makes the view tappable, with a springy press animation.
    pub fn on_tap(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.on_tap = Some(Box::new(f));
        self
    }
    /// Stable identity for animation/scroll state when siblings are reordered.
    pub fn key(mut self, k: impl Hash) -> Self {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        k.hash(&mut h);
        self.key = Some(h.finish());
        self
    }
}

// ---- children ----

pub trait Children<S> {
    fn into_views(self) -> Vec<View<S>>;
}
impl<S> Children<S> for Vec<View<S>> {
    fn into_views(self) -> Vec<View<S>> {
        self
    }
}
impl<S> Children<S> for View<S> {
    fn into_views(self) -> Vec<View<S>> {
        vec![self]
    }
}
impl<S, const N: usize> Children<S> for [View<S>; N] {
    fn into_views(self) -> Vec<View<S>> {
        self.into()
    }
}
impl<S> Children<S> for () {
    fn into_views(self) -> Vec<View<S>> {
        Vec::new()
    }
}
macro_rules! tuple_children {
    ($($n:ident),+) => {
        impl<S> Children<S> for ($(tuple_children!(@ty $n S),)+) {
            #[allow(non_snake_case)]
            fn into_views(self) -> Vec<View<S>> {
                let ($($n,)+) = self;
                vec![$($n),+]
            }
        }
    };
    (@ty $n:ident $s:ident) => { View<$s> };
}
tuple_children!(A);
tuple_children!(A, B);
tuple_children!(A, B, C);
tuple_children!(A, B, C, D);
tuple_children!(A, B, C, D, E);
tuple_children!(A, B, C, D, E, F);
tuple_children!(A, B, C, D, E, F, G);
tuple_children!(A, B, C, D, E, F, G, H);
tuple_children!(A, B, C, D, E, F, G, H, I);
tuple_children!(A, B, C, D, E, F, G, H, I, J);
tuple_children!(A, B, C, D, E, F, G, H, I, J, K);
tuple_children!(A, B, C, D, E, F, G, H, I, J, K, L);

// ---- constructors ----

fn stack<S>(axis: Axis, children: impl Children<S>) -> View<S> {
    View::new(Kind::Stack { axis, spacing: 0.0, align: Align::Center, children: children.into_views() })
}
/// Children top to bottom.
pub fn vstack<S>(children: impl Children<S>) -> View<S> {
    stack(Axis::V, children)
}
/// Children left to right.
pub fn hstack<S>(children: impl Children<S>) -> View<S> {
    stack(Axis::H, children)
}
/// Children layered back to front.
pub fn zstack<S>(children: impl Children<S>) -> View<S> {
    stack(Axis::Z, children)
}
pub fn text<S>(content: impl Into<String>) -> View<S> {
    View::new(Kind::Text(TextData { content: content.into(), size: 17.0, weight: Weight::Regular, center: false }))
}
/// Flexible empty space inside a stack.
pub fn spacer<S>() -> View<S> {
    View::new(Kind::Spacer)
}
/// A plain box; style it with `.size()`, `.background()`, `.radius()`…
pub fn rect<S>() -> View<S> {
    View::new(Kind::Empty)
}
/// Vertically scrolling container.
pub fn scroll<S>(child: View<S>) -> View<S> {
    View::new(Kind::Scroll(Box::new(child))).fill()
}
pub fn button<S>(label: impl Into<String>, on_tap: impl Fn(&mut S) + 'static) -> View<S> {
    zstack(text(label).weight(Weight::Semibold))
        .padding_xy(20.0, 13.0)
        .background(Color::BLUE)
        .color(Color::WHITE)
        .radius(14.0)
        .on_tap(on_tap)
}
/// A picture. It keeps its aspect ratio; when both sides are set it fills the box and
/// crops the overflow. Combine with `.radius()` for rounded corners.
pub fn image<S>(img: &Image) -> View<S> {
    View::new(Kind::Image(img.clone()))
}
/// Single-line text input. Fills the available width.
pub fn text_field<S>(
    value: impl Into<String>,
    placeholder: impl Into<String>,
    on_change: impl Fn(&mut S, String) + 'static,
) -> View<S> {
    View::new(Kind::TextField { value: value.into(), placeholder: placeholder.into(), on_change: Box::new(on_change) })
        .padding_xy(14.0, 12.0)
        .background(Color::rgba(0.47, 0.47, 0.5, 0.24))
        .radius(12.0)
}
/// Drop-down choice: shows the selected option and opens a glass menu with all of them.
pub fn picker<S, T: AsRef<str>>(options: &[T], selected: usize, on_select: impl Fn(&mut S, usize) + 'static) -> View<S> {
    let options: Vec<String> = options.iter().map(|o| o.as_ref().to_string()).collect();
    let selected = selected.min(options.len().saturating_sub(1));
    View::new(Kind::Picker { options, selected, on_select: Rc::new(on_select) })
        .padding_xy(14.0, 10.0)
        .background(Color::rgba(0.47, 0.47, 0.5, 0.24))
        .radius(12.0)
}
/// Floating glass tab bar. A glass "drop" marks the selected tab: it flows to a tab when
/// you tap it, and while held it swells past the bar and can be dragged across the tabs,
/// bending everything underneath like a lens.
pub fn tab_bar<S, T: AsRef<str>>(labels: &[T], selected: usize, on_select: impl Fn(&mut S, usize) + 'static) -> View<S> {
    let labels: Vec<String> = labels.iter().map(|l| l.as_ref().to_string()).collect();
    let selected = selected.min(labels.len().saturating_sub(1));
    View::new(Kind::Tabs { labels, selected, on_select: Box::new(on_select) })
        .glass()
        .radius(28.0)
        .shadow(Color::BLACK.alpha(0.35), 30.0, 10.0)
}
/// iOS-style switch.
pub fn toggle<S>(on: bool, on_change: impl Fn(&mut S, bool) + 'static) -> View<S> {
    View::new(Kind::Toggle(on, Box::new(on_change)))
}
/// Slider with `value` in 0..=1. Fills the available width.
pub fn slider<S>(value: f32, on_change: impl Fn(&mut S, f32) + 'static) -> View<S> {
    View::new(Kind::Slider(value.clamp(0.0, 1.0), Box::new(on_change)))
}
