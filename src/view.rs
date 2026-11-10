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
    pub sticky: bool,
    /// Pressing on the view (outside interactive children) moves the window.
    pub drag: bool,
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
            sticky: false,
            drag: false,
        }
    }
}

/// A run of text with its own style inside `rich_text`.
#[derive(Clone, Debug, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Monospaced, for code.
    pub mono: bool,
    pub color: Option<Color>,
}

impl Span {
    pub fn new(text: impl Into<String>) -> Self {
        Span { text: text.into(), ..Default::default() }
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub fn code(mut self) -> Self {
        self.mono = true;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }
}

/// What the screen looks like right now; lets one `view` function lay itself out
/// differently on a phone, a tablet and a desktop window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Env {
    /// Window size in points.
    pub width: f32,
    pub height: f32,
    /// Height of the title bar zone the content is drawn under (`App::titlebar(Titlebar::Overlay)`
    /// on macOS); 0 when the system draws its own title bar. Put a `.window_drag()` strip here.
    pub titlebar: f32,
    /// Width at the leading edge of that zone taken by the window buttons (the macOS traffic
    /// lights): start your own toolbar buttons after it.
    pub window_buttons: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SizeClass {
    /// Narrow: one column, bottom tab bar, drawers and sheets.
    Phone,
    /// Medium: room for a sidebar next to the content.
    Tablet,
    /// Wide: sidebar, content and extra panels side by side.
    Desktop,
}

impl Env {
    pub fn class(&self) -> SizeClass {
        match self.width {
            w if w < 640.0 => SizeClass::Phone,
            w if w < 1024.0 => SizeClass::Tablet,
            _ => SizeClass::Desktop,
        }
    }
    pub fn is_phone(&self) -> bool {
        self.class() == SizeClass::Phone
    }
}

/// The app's view function: either `Fn(&S) -> View<S>` or, to adapt to the screen,
/// `Fn(&S, &Env) -> View<S>`.
pub trait ViewFn<S, Marker>: 'static {
    fn build(&self, state: &S, env: &Env) -> View<S>;
}
impl<S, F: Fn(&S) -> View<S> + 'static> ViewFn<S, ()> for F {
    fn build(&self, state: &S, _: &Env) -> View<S> {
        self(state)
    }
}
impl<S, F: Fn(&S, &Env) -> View<S> + 'static> ViewFn<S, Env> for F {
    fn build(&self, state: &S, env: &Env) -> View<S> {
        self(state, env)
    }
}

pub(crate) struct TextData {
    pub spans: Vec<Span>,
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
    #[cfg(feature = "decode")]
    pub fn decode(bytes: &[u8]) -> Result<Image, String> {
        let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
        Ok(Image::from_rgba(img.width(), img.height(), img.into_raw()))
    }

    /// From RGBA8 pixels that are already premultiplied by alpha.
    pub fn from_premultiplied(width: u32, height: u32, pixels: Vec<u8>) -> Image {
        assert_eq!(pixels.len(), (width * height * 4) as usize, "pixel buffer size");
        static NEXT: AtomicU64 = AtomicU64::new(1 << 32);
        Image(Arc::new(ImageData { id: NEXT.fetch_add(1, Ordering::Relaxed), width, height, pixels }))
    }

    /// Rasterizes an SVG so that its longer side is `size_px` pixels. For icons use 2–4x
    /// the point size you will show it at; mipmaps take care of smaller sizes.
    #[cfg(feature = "svg")]
    pub fn from_svg(svg: &[u8], size_px: u32) -> Result<Image, String> {
        use resvg::{tiny_skia, usvg};
        let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
        let size = tree.size();
        let k = size_px.max(1) as f32 / size.width().max(size.height()).max(0.001);
        let (w, h) = ((size.width() * k).ceil().max(1.0) as u32, (size.height() * k).ceil().max(1.0) as u32);
        let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("svg is too large")?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
        Ok(Image::from_premultiplied(w, h, pixmap.take()))
    }

    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }
}

/// When a scroll view shows its scroll bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Indicator {
    /// Appears while scrolling and fades out afterwards, like macOS overlay scrollers.
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OverlayMode {
    Modal,
    Sheet,
    Toast,
    Drawer,
}

pub(crate) enum Kind<S> {
    /// Picture; `true` = template (only its alpha is used, tinted with the text color).
    Image(Image, bool),
    TextField { value: String, placeholder: String, on_change: Box<dyn Fn(&mut S, String)> },
    Picker { options: Vec<String>, selected: usize, on_select: Rc<dyn Fn(&mut S, usize)> },
    Tabs { labels: Vec<String>, selected: usize, on_select: Box<dyn Fn(&mut S, usize)> },
    Empty,
    Spacer,
    Stack { axis: Axis, spacing: f32, align: Align, children: Vec<View<S>> },
    Text(TextData),
    Scroll { child: Box<View<S>>, refresh: Option<(bool, Box<dyn Fn(&mut S)>)>, indicator: Indicator, stick: bool },
    List { count: usize, row_h: f32, row: Box<dyn Fn(usize) -> View<S>> },
    Overlay { open: bool, mode: OverlayMode, child: Box<View<S>>, on_dismiss: Option<Box<dyn Fn(&mut S)>> },
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

    /// Inside the vertical stack of a `scroll`: this child sticks to the top of the
    /// viewport while its section is on screen and is pushed away by the next sticky one.
    pub fn sticky(mut self) -> Self {
        self.style.sticky = true;
        self
    }
    /// Scroll only: pull-to-refresh. `on_refresh` runs when the list is pulled past the
    /// threshold and released; keep `refreshing` true until the new data has arrived.
    pub fn on_refresh(mut self, refreshing: bool, on_refresh: impl Fn(&mut S) + 'static) -> Self {
        if let Kind::Scroll { refresh, .. } = &mut self.kind {
            *refresh = Some((refreshing, Box::new(on_refresh)));
        }
        self
    }
    /// Scroll only: start at the end and stay there while content is appended, unless the
    /// user has scrolled up. For chats and logs.
    pub fn stick_to_bottom(mut self) -> Self {
        if let Kind::Scroll { stick, .. } = &mut self.kind {
            *stick = true;
        }
        self
    }
    /// Scroll only: scroll bar visibility (default `Indicator::Auto`).
    pub fn indicator(mut self, mode: Indicator) -> Self {
        if let Kind::Scroll { indicator, .. } = &mut self.kind {
            *indicator = mode;
        }
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
    /// Pressing on this view moves the window, as a title bar does; a double click zooms it.
    /// Buttons and fields inside keep working: they sit above the drag area.
    /// Has no effect where windows don't move (web, mobile, touch).
    ///
    /// ```ignore
    /// hstack((spacer(), button("New", |s| ..))).height(env.titlebar).window_drag()
    /// ```
    pub fn window_drag(mut self) -> Self {
        self.style.drag = true;
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
    View::new(Kind::Text(TextData {
        spans: Vec::new(),
        content: content.into(),
        size: 17.0,
        weight: Weight::Regular,
        center: false,
    }))
}
/// Text made of differently styled runs (bold, italic, code, colored) that wrap together.
pub fn rich_text<S>(spans: Vec<Span>) -> View<S> {
    View::new(Kind::Text(TextData { spans, content: String::new(), size: 17.0, weight: Weight::Regular, center: false }))
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
    View::new(Kind::Scroll { child: Box::new(child), refresh: None, indicator: Indicator::Auto, stick: false }).fill()
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
    View::new(Kind::Image(img.clone(), false))
}
/// A glyph, like an SF Symbol: only the picture's shape is used and it is painted in the
/// current text color (`.color()`), so one file works in every theme and state.
/// 22 pt by default; resize with `.size()`.
pub fn icon<S>(img: &Image) -> View<S> {
    View::new(Kind::Image(img.clone(), true)).size(22.0, 22.0)
}
/// A lazily built list for long data: only the rows on screen are created, laid out and
/// drawn, so 100 000 rows cost the same as 20. Rows may have different heights;
/// `row_height` is the estimate used for rows that have not been seen yet (the scroll
/// range is corrected as they are measured). Put it inside a `scroll`.
pub fn list<S>(count: usize, row_height: f32, row: impl Fn(usize) -> View<S> + 'static) -> View<S> {
    View::new(Kind::List { count, row_h: row_height.max(1.0), row: Box::new(row) }).fill_width()
}
fn overlay<S>(open: bool, mode: OverlayMode, on_dismiss: Option<Box<dyn Fn(&mut S)>>, content: View<S>) -> View<S> {
    View::new(Kind::Overlay { open, mode, child: Box::new(content), on_dismiss }).fill()
}
/// A centered dialog over a dimmed screen. Put it last in the root `zstack`. It blocks
/// input to everything below; tapping outside calls `on_dismiss`.
pub fn modal<S>(open: bool, on_dismiss: impl Fn(&mut S) + 'static, content: View<S>) -> View<S> {
    overlay(open, OverlayMode::Modal, Some(Box::new(on_dismiss)), content)
}
/// A panel that slides up from the bottom edge over a dimmed screen.
pub fn sheet<S>(open: bool, on_dismiss: impl Fn(&mut S) + 'static, content: View<S>) -> View<S> {
    overlay(open, OverlayMode::Sheet, Some(Box::new(on_dismiss)), content)
}
/// A side panel that slides in from the left edge over a dimmed screen: the phone-sized
/// stand-in for a sidebar.
pub fn drawer<S>(open: bool, on_dismiss: impl Fn(&mut S) + 'static, content: View<S>) -> View<S> {
    overlay(open, OverlayMode::Drawer, Some(Box::new(on_dismiss)), content)
}
/// A small floating message near the bottom that does not block the UI. Hide it with
/// `ruru::after(seconds, ...)`.
pub fn snackbar<S>(open: bool, content: View<S>) -> View<S> {
    overlay(open, OverlayMode::Toast, None, content)
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
        .fill_width()
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
    View::new(Kind::Slider(value.clamp(0.0, 1.0), Box::new(on_change))).fill_width()
}
