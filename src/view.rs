//! The declarative view tree: `View<S>` plus constructors and modifiers.
//!
//! A view is a cheap description rebuilt from app state `S` whenever something
//! changes. Event handlers are closures that mutate `&mut S`.

use crate::types::Color;
use crate::ui::popup::{MenuItem, MenuStyle, Placement, Shortcut};
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

/// Modifier keys held down, as the runner reports them with [`Ui::set_modifiers`](crate::Ui::set_modifiers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    /// Cmd on macOS, Ctrl elsewhere.
    pub command: bool,
    /// Option on macOS.
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// A soft shadow, as in CSS `box-shadow`: offset (`x`, `y`), `blur`, `spread` (grows the
/// shape, or shrinks it when negative) and `color`, all in points. An `inset` one falls inside
/// the view from its edge: with no blur and a `spread` of a hairline it is the light rim
/// along the inner edge of a macOS menu.
///
/// ```ignore
/// panel
///     .shadow_with(Shadow::new(Color::BLACK.alpha(0.30), 6.0, 2.0))           // key: close and sharp
///     .shadow_with(Shadow::new(Color::BLACK.alpha(0.25), 40.0, 16.0))         // ambient: wide and soft
///     .shadow_with(Shadow::new(Color::WHITE.alpha(0.12), 0.0, 0.0).spread(0.5).inset()) // rim
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub blur: f32,
    pub x: f32,
    pub y: f32,
    pub spread: f32,
    pub inset: bool,
}

impl Shadow {
    /// A drop shadow `y` points below the view (no sideways offset, no spread).
    pub const fn new(color: Color, blur: f32, y: f32) -> Self {
        Shadow { color, blur, x: 0.0, y, spread: 0.0, inset: false }
    }
    pub const fn offset(mut self, x: f32, y: f32) -> Self {
        self.x = x;
        self.y = y;
        self
    }
    pub const fn spread(mut self, s: f32) -> Self {
        self.spread = s;
        self
    }
    /// Inside the view instead of under it: drawn over the background and the children.
    pub const fn inset(mut self) -> Self {
        self.inset = true;
        self
    }
    /// How far it reaches outside the view's rect.
    pub(crate) fn reach(&self) -> f32 {
        if self.inset {
            0.0
        } else {
            self.blur * 2.0 + self.x.abs().max(self.y.abs()) + self.spread.max(0.0)
        }
    }
}

/// Look of a [`slider`] (`.slider_style()`). The track and its filled part are capsules
/// `track_height` high; the knob is a disc `knob_size` across, which is also the slider's
/// height. The theme gives the default; change the fields you need:
/// `SliderStyle { fill: Color::GREEN, ..SliderStyle::from(&*env.theme) }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderStyle {
    pub track_height: f32,
    /// The empty part of the track.
    pub track: Color,
    /// Outline of the track (under the filled part).
    pub track_border: Option<(f32, Color)>,
    /// The part of the track left of the knob.
    pub fill: Color,
    pub knob: Color,
    pub knob_size: f32,
    pub knob_shadow: Option<Shadow>,
    /// Stop marks (`.steps()`): diameter, color on the filled part, color on the empty part
    /// (default: the text color at 35 %).
    pub tick_size: f32,
    pub tick_on: Color,
    pub tick_off: Option<Color>,
}

/// Glass material: the content behind it is lightly frosted and refracted through a
/// bevelled rim. Defaults follow iOS "regular" glass.
///
/// Start from a preset and turn three knobs, each 0..=1:
/// ```
/// # use ruru::{Glass, Color};
/// let g = Glass::dark().blur(0.4).distortion(0.6).opacity(0.5);
/// let tinted = Glass::light().tint(Color::BLUE).opacity(0.2);
/// ```
/// The fields stay public for fine tuning.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// Smoked glass: darkens what is behind it, for light text over busy content.
    pub const fn dark() -> Self {
        Glass { tint: Color::rgba(0.06, 0.06, 0.08, 0.45), saturation: 1.25, frost: 16.0, rim: 0.25, ..Self::regular() }
    }
    /// Milky glass: brightens what is behind it, for dark text over busy content.
    pub const fn light() -> Self {
        Glass { tint: Color::WHITE.alpha(0.45), saturation: 1.25, frost: 16.0, rim: 0.4, ..Self::regular() }
    }

    /// Blur of what is behind, 0..=1: 0 is clear glass, 1 heavy frost (`frost` 0..=50 pt).
    pub fn blur(mut self, amount: f32) -> Self {
        self.frost = BLUR_MAX * amount.clamp(0.0, 1.0);
        self
    }
    /// Bending of the backdrop at the rim, 0..=1: 0 is a flat pane, 1 a strong lens
    /// (sets `refraction` and the colour fringing that comes with it, `dispersion`).
    pub fn distortion(mut self, amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        self.refraction = a;
        self.dispersion = 0.3 * a;
        self
    }
    /// Colour of the tint. Its alpha, when below 1, is taken as the [`opacity`](Self::opacity).
    pub fn tint(mut self, color: Color) -> Self {
        let a = if color.a < 1.0 { color.a } else { self.tint.a };
        self.tint = color.alpha(a);
        self
    }
    /// How dense the tint is, 0..=1: 0 shows only the (blurred) backdrop, 1 is solid tint.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.tint.a = opacity.clamp(0.0, 1.0);
        self
    }
}

/// Frost at `.blur(1.0)`, in points.
const BLUR_MAX: f32 = 50.0;

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
    /// Drop shadows, key then ambient (`.shadow()`, `.shadow_with()`).
    pub shadow: [Option<Shadow>; 2],
    /// Inner shadow or rim (`.shadow_with(Shadow::..inset())`).
    pub inset: Option<Shadow>,
    pub glass: Option<Glass>,
    pub opacity: f32,
    pub fg: Option<Color>,
    pub place: Option<(Align, Align)>,
    pub sticky: bool,
    /// Pressing on the view (outside interactive children) moves the window.
    pub drag: bool,
    /// Size limits (width, height); `max` is infinite when unset.
    pub min: (f32, f32),
    pub max: (f32, f32),
    /// Children are cut to this view's rounded shape.
    pub clip: bool,
    /// Continuous (squircle) corners instead of circular arcs (`.smooth()`).
    pub smooth: bool,
    /// Size changes glide to the new value instead of jumping.
    pub animate: bool,
    /// Highlight laid over the background while the pointer is over the view.
    pub hover: Option<Color>,
    /// Text color while the pointer is over the view (`.hover_color()`).
    pub hover_fg: Option<Color>,
    pub cursor: Option<Cursor>,
    /// Share of the free space along the parent stack's axis (`.flex()`); 0 = not set.
    pub flex: f32,
    /// Text inside can be selected and copied (`.selectable()`).
    pub select: bool,
    /// Highlight of selected text in this subtree (`.selection_color()`).
    pub sel_color: Option<Color>,
    /// Copied instead of the subtree's text (`.copy_as()`).
    pub copy_as: Option<String>,
    /// Put between the children's texts when copying (`.copy_sep()`).
    pub copy_sep: Option<&'static str>,
    /// Which built-in widget this is: the theme fills in what its modifiers left unset.
    pub role: Role,
    /// Fields set by a modifier that have no `None` of their own (`PAD_*`, `SET_RADIUS`).
    pub set: u8,
}

/// A built-in widget whose padding, fill and radius come from the theme (`theme::apply`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    None,
    Button,
    Field,
    Picker,
    Tabs,
}

pub(crate) const PAD_L: u8 = 1;
pub(crate) const PAD_T: u8 = 2;
pub(crate) const PAD_R: u8 = 4;
pub(crate) const PAD_B: u8 = 8;
const PAD_ALL: u8 = PAD_L | PAD_T | PAD_R | PAD_B;
pub(crate) const SET_RADIUS: u8 = 16;

impl Default for Style {
    fn default() -> Self {
        Style {
            padding: [0.0; 4],
            width: Dim::Auto,
            height: Dim::Auto,
            bg: None,
            radius: 0.0,
            border: None,
            shadow: [None; 2],
            inset: None,
            glass: None,
            opacity: 1.0,
            fg: None,
            place: None,
            sticky: false,
            drag: false,
            min: (0.0, 0.0),
            max: (f32::INFINITY, f32::INFINITY),
            clip: false,
            smooth: false,
            animate: false,
            hover: None,
            hover_fg: None,
            cursor: None,
            flex: 0.0,
            select: false,
            sel_color: None,
            copy_as: None,
            copy_sep: None,
            role: Role::None,
            set: 0,
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
    /// Exact weight; wins over `bold`.
    pub weight: Option<Weight>,
    /// Font size in points; the text's own size when unset.
    pub size: Option<f32>,
    /// Rounded plate under the run (inline code, tags); see [`Span::background`].
    pub background: Option<Color>,
    /// A picture shown in the line instead of text; see [`Span::icon`].
    pub icon: Option<Image>,
    /// Where the run links to; see [`Span::link`].
    pub link: Option<String>,
    /// OpenType features of this run on top of the text's; see [`Span::feature`].
    pub features: Vec<([u8; 4], u32)>,
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
    /// Any weight, e.g. semibold for emphasis: `Span::new("Note").weight(Weight::Semibold)`.
    pub fn weight(mut self, w: Weight) -> Self {
        self.weight = Some(w);
        self
    }
    /// Font size of this run in points; its line grows to fit.
    pub fn size(mut self, pt: f32) -> Self {
        self.size = Some(pt.max(1.0));
        self
    }
    /// A rounded plate under the run that follows it across line wraps, with a little room
    /// on both sides: `Span::new("wgpu").code().background(Color::hex(0x303030))`.
    /// It never makes the line taller.
    pub fn background(mut self, c: Color) -> Self {
        self.background = Some(c);
        self
    }
    /// A glyph-like picture inside the line, one em wide, followed by a small gap and painted
    /// in the run's color (like [`icon`]): `Span::icon(&github).color(link)`, then the link text.
    pub fn icon(img: &Image) -> Self {
        Span { icon: Some(img.clone()), ..Default::default() }
    }
    /// Makes the run a link: the pointer turns into a hand over it, it is underlined while
    /// hovered, and a click calls the nearest [`View::on_link`] above it, or, without one,
    /// asks the runner to open the URL ([`WindowRequest::OpenUrl`](crate::WindowRequest::OpenUrl),
    /// http(s) and mailto only). Give it a color yourself: `Span::new("docs").link(url).color(blue)`.
    pub fn link(mut self, url: impl Into<String>) -> Self {
        self.link = Some(url.into());
        self
    }
    /// An OpenType feature for this run: `Span::new("1/2").feature(*b"frac", 1)`.
    pub fn feature(mut self, tag: [u8; 4], value: u32) -> Self {
        self.features.push((tag, value));
        self
    }
    /// Digits of equal width (`tnum`): counters and times that change in place do not jiggle.
    pub fn tabular(self) -> Self {
        self.feature(*b"tnum", 1)
    }
}

/// What the screen looks like right now; lets one `view` function lay itself out
/// differently on a phone, a tablet and a desktop window.
#[derive(Clone, Debug, PartialEq)]
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
    /// The theme in use (`App::theme`): `env.theme.color.accent`, `env.theme.text.body`.
    pub theme: Rc<crate::theme::Theme>,
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
    /// Most lines shown; 0 = no limit.
    pub lines: usize,
    pub truncate: Truncate,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Extra space between letters, in em.
    pub tracking: f32,
    /// OpenType features of the whole text (spans add their own).
    pub features: Vec<([u8; 4], u32)>,
    /// Theme text style for what no modifier set (`set`: `T_*` bits).
    pub role: TextRole,
    pub set: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextRole {
    Body,
    Headline,
}

const T_SIZE: u8 = 1;
const T_WEIGHT: u8 = 2;
const T_LH: u8 = 4;
const T_TRACK: u8 = 8;

impl TextData {
    /// Takes the theme's style for everything no modifier set.
    pub(crate) fn theme(&mut self, ts: &crate::theme::TextStyle) {
        if self.set & T_SIZE == 0 {
            self.size = ts.size;
        }
        if self.set & T_WEIGHT == 0 {
            self.weight = ts.weight;
        }
        if self.set & T_LH == 0 {
            self.line_height = ts.line_height;
        }
        if self.set & T_TRACK == 0 {
            self.tracking = ts.tracking;
        }
    }
}

/// How text that does not fit its lines ends (see [`View::lines`], [`View::truncate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Truncate {
    /// "A long title…"
    Ellipsis,
    /// The last line fades out towards its right edge, like titles in a sidebar.
    Fade,
}

/// Mouse pointer shape over a view (see [`View::cursor`]); the runner shows it on desktop
/// and on the web, touch screens ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    /// A hand: something to click.
    Pointer,
    /// An I-beam: editable or selectable text.
    Text,
    /// A left–right arrow: a vertical divider that can be dragged.
    ResizeH,
    /// An up–down arrow: a horizontal divider that can be dragged.
    ResizeV,
    Grab,
    Grabbing,
    NotAllowed,
}

/// Decoded picture, cheap to clone. Create it once (not inside `view`) and keep it in
/// your state or a static.
#[derive(Clone)]
pub struct Image(pub(crate) Arc<ImageData>);

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.0.width, self.0.height)
    }
}

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
    TextField(crate::ui::edit::Field<S>),
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
    /// Value, number of stops (0 = continuous), handler, look (default when `None`).
    Slider(f32, u32, Box<dyn Fn(&mut S, f32)>, Option<Box<SliderStyle>>),
    /// Look: the theme's unless `.menu_style()` set one.
    Menu { items: Vec<MenuItem<S>>, style: Option<Rc<MenuStyle>> },
}

/// How an attached popover opens and closes.
pub(crate) enum Opens<S> {
    /// The app holds the flag; a click outside or Escape calls the handler.
    State(bool, Rc<dyn Fn(&mut S)>),
    /// The framework opens it on a tap on the anchor (`.menu()`).
    Tap,
    /// The framework opens it at the pointer on a secondary click (`.context_menu()`).
    Context,
}

pub(crate) struct Pop<S> {
    pub opens: Opens<S>,
    pub placement: Placement,
    pub content: Box<View<S>>,
}

/// Rarely used attachments, boxed so that a plain view stays small.
pub(crate) struct Extras<S> {
    pub pop: Option<Pop<S>>,
    pub tip: Option<Box<View<S>>>,
    /// A default tooltip (label, key hint), built with the theme by `theme::apply`.
    pub tip_text: Option<(String, Option<String>)>,
    pub tip_at: Option<Placement>,
    pub shortcuts: Vec<(Shortcut, Box<dyn Fn(&mut S)>)>,
    pub secondary: Option<Box<dyn Fn(&mut S)>>,
}

/// A node of the UI tree, generic over the app state `S`.
pub struct View<S> {
    pub(crate) kind: Kind<S>,
    pub(crate) style: Style,
    pub(crate) on_tap: Option<Box<dyn Fn(&mut S)>>,
    pub(crate) on_link: Option<Rc<dyn Fn(&mut S, &str)>>,
    pub(crate) key: Option<u64>,
    pub(crate) extra: Option<Box<Extras<S>>>,
}

impl<S> View<S> {
    pub(crate) fn new(kind: Kind<S>) -> Self {
        View { kind, style: Style::default(), on_tap: None, on_link: None, key: None, extra: None }
    }

    fn extras(&mut self) -> &mut Extras<S> {
        self.extra.get_or_insert_with(|| Box::new(Extras { pop: None, tip: None, tip_text: None, tip_at: None, shortcuts: Vec::new(), secondary: None }))
    }

    // ---- layout ----
    pub fn padding(mut self, p: f32) -> Self {
        self.style.padding = [p; 4];
        self.style.set |= PAD_ALL;
        self
    }
    pub fn padding_xy(mut self, x: f32, y: f32) -> Self {
        self.style.padding = [x, y, x, y];
        self.style.set |= PAD_ALL;
        self
    }
    /// Left and right only; top and bottom keep what was set before, so
    /// `.padding_h(8.0).padding_v(4.0)` gives both.
    pub fn padding_h(mut self, x: f32) -> Self {
        self.style.padding[0] = x;
        self.style.padding[2] = x;
        self.style.set |= PAD_L | PAD_R;
        self
    }
    /// Top and bottom only; left and right keep what was set before.
    pub fn padding_v(mut self, y: f32) -> Self {
        self.style.padding[1] = y;
        self.style.padding[3] = y;
        self.style.set |= PAD_T | PAD_B;
        self
    }
    /// left, top, right, bottom
    pub fn padding_ltrb(mut self, l: f32, t: f32, r: f32, b: f32) -> Self {
        self.style.padding = [l, t, r, b];
        self.style.set |= PAD_ALL;
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
    /// Never wider than `w`, whatever the parent offers: `.fill_width().max_width(720.0)` is a
    /// reading column that stops growing on a wide window (center it in a `zstack`).
    pub fn max_width(mut self, w: f32) -> Self {
        self.style.max.0 = w.max(0.0);
        self
    }
    /// Never narrower than `w`, even if that overflows the parent.
    pub fn min_width(mut self, w: f32) -> Self {
        self.style.min.0 = w.max(0.0);
        self
    }
    pub fn max_height(mut self, h: f32) -> Self {
        self.style.max.1 = h.max(0.0);
        self
    }
    pub fn min_height(mut self, h: f32) -> Self {
        self.style.min.1 = h.max(0.0);
        self
    }
    /// When this view's size changes it glides there with a spring, and its content is cut
    /// to the moving edge instead of being squeezed. Collapse a panel by setting its size
    /// to 0 and back; keep the view in the tree while it is hidden so it can grow back:
    ///
    /// ```ignore
    /// sidebar(s).width(if s.sidebar { 280.0 } else { 0.0 }).animate()
    /// ```
    ///
    /// Sizes set by `.fill*()` follow the parent (window resizes) without animating.
    pub fn animate(mut self) -> Self {
        self.style.animate = true;
        self
    }
    /// Inside an `hstack` / `vstack`: take a share of the space left after the other
    /// children, in proportion to `weight` among siblings with `.flex()` or `.fill_*()`
    /// (those count as 1). Proportional columns at any width:
    ///
    /// ```ignore
    /// hstack((name.flex(1.0), value.flex(1.5), note.flex(1.5)))
    /// ```
    pub fn flex(mut self, weight: f32) -> Self {
        self.style.flex = weight.max(0.0);
        self
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
        self.style.set |= SET_RADIUS;
        self
    }
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.style.border = Some((width, color));
        self
    }
    /// A soft drop shadow `y` points below the view. Replaces the first (key) shadow.
    pub fn shadow(mut self, color: Color, blur: f32, y: f32) -> Self {
        self.style.shadow[0] = Some(Shadow::new(color, blur, y));
        self
    }
    /// Adds a shadow layer: a view has up to two drop shadows, painted in the order given
    /// (a third replaces the second), and one inset shadow. Each costs one quad.
    ///
    /// ```ignore
    /// menu_panel
    ///     .shadow_with(Shadow::new(Color::BLACK.alpha(0.3), 6.0, 2.0))
    ///     .shadow_with(Shadow::new(Color::BLACK.alpha(0.25), 40.0, 16.0).spread(-4.0))
    ///     .shadow_with(Shadow::new(Color::WHITE.alpha(0.12), 0.0, 0.0).spread(0.5).inset())
    /// ```
    pub fn shadow_with(mut self, sh: Shadow) -> Self {
        let st = &mut self.style;
        if sh.inset {
            st.inset = Some(sh);
        } else if st.shadow[0].is_none() {
            st.shadow[0] = Some(sh);
        } else {
            st.shadow[1] = Some(sh);
        }
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
    /// Continuous corners, like iOS app icons: the curve eases into the straight edges instead
    /// of a circular arc meeting them (`.radius()` sets the size). Corners are circular by
    /// default, as on macOS; glass always has continuous ones.
    pub fn smooth(mut self) -> Self {
        self.style.smooth = true;
        self
    }
    /// Cuts the children to this view's rounded shape (`.radius()`): a list in a rounded
    /// panel, a photo in a card. Clips nest. Text is cut to the rectangle only, which is
    /// invisible unless glyphs reach into a corner.
    pub fn clip(mut self) -> Self {
        self.style.clip = true;
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
            t.set |= T_SIZE;
        }
        if let Kind::TextField(f) = &mut self.kind {
            f.size = s;
            f.sized = true;
        }
        self
    }
    pub fn weight(mut self, w: Weight) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.weight = w;
            t.set |= T_WEIGHT;
        }
        self
    }
    /// Text only: size, line height, weight and letter spacing at once, usually from the
    /// theme: `text("Settings").style(env.theme.text.title)`.
    pub fn style(mut self, ts: crate::theme::TextStyle) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.set = 0;
            t.theme(&ts);
            t.set = T_SIZE | T_WEIGHT | T_LH | T_TRACK;
        }
        if let Kind::TextField(f) = &mut self.kind {
            (f.size, f.line_height, f.tracking) = (ts.size, ts.line_height, ts.tracking);
            (f.sized, f.lh_set, f.track_set) = (true, true, true);
        }
        self
    }
    /// Text and text fields: line height as a multiple of the font size (1.3 unless set).
    /// Reading text usually wants 1.4–1.6: `text(body).font_size(14.0).line_height(1.625)`;
    /// a multi-line field grows by `round(size × k)` per row.
    pub fn line_height(mut self, k: f32) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.line_height = k.max(0.5);
            t.set |= T_LH;
        }
        if let Kind::TextField(f) = &mut self.kind {
            (f.line_height, f.lh_set) = (k.max(0.5), true);
        }
        self
    }
    /// Text and text fields: extra space between letters in em (0.01 = 1 % of the font
    /// size); negative tightens. Small text often reads better a little looser.
    pub fn letter_spacing(mut self, em: f32) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.tracking = em;
            t.set |= T_TRACK;
        }
        if let Kind::TextField(f) = &mut self.kind {
            (f.tracking, f.track_set) = (em, true);
        }
        self
    }
    /// Text only: digits of equal width (OpenType `tnum`), so a counter, a timer or a column
    /// of numbers does not jiggle as the digits change: `text(format!("{n}")).tabular()`.
    pub fn tabular(self) -> Self {
        self.font_feature(*b"tnum", 1)
    }
    /// Text only: turns an OpenType feature on (1), off (0) or picks an alternate (n):
    /// `.font_feature(*b"ss01", 1)`, `.font_feature(*b"liga", 0)`. Fonts without it ignore it.
    pub fn font_feature(mut self, tag: [u8; 4], value: u32) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.features.retain(|f| f.0 != tag);
            t.features.push((tag, value));
        }
        self
    }
    /// Text only: at most `n` lines; what does not fit ends with "…" (or fades, see
    /// [`View::truncate`]). The view never grows taller than `n` lines.
    pub fn lines(mut self, n: usize) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.lines = n;
        }
        self
    }
    /// Text only: how a text cut by `.lines()` ends; on its own it limits text to one line.
    /// `text(title).truncate(Truncate::Fade)` is a sidebar title that fades at the edge.
    pub fn truncate(mut self, how: Truncate) -> Self {
        if let Kind::Text(t) = &mut self.kind {
            t.truncate = how;
            t.lines = t.lines.max(1);
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
    /// Lays `color` over the background while the pointer is over the view (fades in and
    /// out in about a tenth of a second). Mouse only; touch has no hover.
    /// `.hover(Color::WHITE.alpha(0.06))` is a typical list-row highlight.
    pub fn hover(mut self, color: Color) -> Self {
        self.style.hover = Some(color);
        self
    }
    /// Text color of this view and everything in it while the pointer is over it, faded
    /// like [`hover`](View::hover): an icon button whose glyph lights up. Children that set
    /// their own `.color()` keep it, so set the resting color on this view too.
    ///
    /// ```ignore
    /// zstack(icon(&i.more)).size(28.0, 28.0).color(GRAY).hover(Color::hex(0x434343)).hover_color(Color::WHITE)
    /// ```
    pub fn hover_color(mut self, c: Color) -> Self {
        self.style.hover_fg = Some(c);
        self
    }
    /// Pointer shape over this view. Tappable views show `Cursor::Pointer` and text fields
    /// `Cursor::Text` without being told; `.cursor(Cursor::Default)` turns that off.
    pub fn cursor(mut self, c: Cursor) -> Self {
        self.style.cursor = Some(c);
        self
    }
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
    /// Text inside can be selected with the mouse (drag, double click for a word, triple for
    /// a paragraph, Shift+click to extend) and copied with Cmd/Ctrl+C as plain text; Cmd/Ctrl+A
    /// selects all of it. One selection spans every text in the subtree, across the rows of a
    /// lazy `list` too (rows off screen are built only when copying). Put it on the list of a
    /// chat or on any container; nested `.selectable()` inside one does nothing. `markdown()`
    /// is selectable by itself.
    ///
    /// ```ignore
    /// scroll(list(n, 80.0, move |i| bubble(markdown(&msgs[i]))).selectable())
    /// ```
    pub fn selectable(mut self) -> Self {
        self.style.select = true;
        self
    }
    /// Color of the selection highlight under selected text, here and in text fields of the
    /// subtree. Default: system blue at 30 %. On a dark theme with a blue accent try
    /// `Color::WHITE.alpha(0.22)`; set it once on the root.
    pub fn selection_color(mut self, c: Color) -> Self {
        self.style.sel_color = Some(c);
        self
    }
    /// What copying puts in place of this subtree's text when any of it is selected: a list
    /// marker `"- "` for a bullet drawn as `•`, a description of a picture, or `""` to leave
    /// it out.
    pub fn copy_as(mut self, s: impl Into<String>) -> Self {
        self.style.copy_as = Some(s.into());
        self
    }
    /// What copying puts between the texts of this stack's children (default: a line break
    /// in a `vstack`, a space in an `hstack`, an empty line between rows of a `list`).
    pub fn copy_sep(mut self, s: &'static str) -> Self {
        self.style.copy_sep = Some(s);
        self
    }
    /// Handles clicks on links ([`Span::link`]) anywhere inside: `.on_link(|s, url| s.open(url))`.
    /// Without it a click asks the runner to open http(s) and mailto links in the browser.
    pub fn on_link(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self {
        self.on_link = Some(Rc::new(f));
        self
    }
    /// Slider only: snaps to `n` evenly spaced stops (both ends included) and marks them with
    /// dots: `slider(s.effort, |s, v| s.effort = v).steps(5)` gives 0, 0.25, … 1.
    pub fn steps(mut self, n: u32) -> Self {
        if let Kind::Slider(v, steps, ..) = &mut self.kind {
            *steps = if n >= 2 { n } else { 0 };
            if *steps > 0 {
                *v = super::ui::popup::snap(*v, *steps);
            }
        }
        self
    }
    /// Slider only: its look (track, fill, knob, stop marks).
    ///
    /// ```ignore
    /// slider(s.effort, |s, v| s.effort = v).steps(5).slider_style(SliderStyle {
    ///     track_height: 24.0, fill: Color::hex(0x4D81EF), track: Color::hex(0x434242),
    ///     track_border: Some((1.0, Color::hex(0x535252))), ..Default::default()
    /// })
    /// ```
    pub fn slider_style(mut self, style: SliderStyle) -> Self {
        if let Kind::Slider(.., st) = &mut self.kind {
            *st = Some(Box::new(style));
        }
        self
    }
    /// Menu only (see [`menu`]): its look, and that of its submenus.
    pub fn menu_style(mut self, style: MenuStyle) -> Self {
        if let Kind::Menu { style: s, .. } = &mut self.kind {
            *s = Some(Rc::new(style));
        }
        self
    }

    // ---- popovers, menus, tooltips, shortcuts ----
    /// Shows `content` in a floating layer next to this view while `open` is true. It sits above
    /// everything, keeps out of the window edges (flipping to the other side when there is no
    /// room) and fades in. A click outside it or Escape calls `on_dismiss`; that first click only
    /// closes it, it never reaches what is underneath. Give the content a width.
    ///
    /// ```ignore
    /// button("Effort", |s| s.pop = true)
    ///     .popover(s.pop, Placement::top(), |s| s.pop = false, vstack((title, slider(..).steps(5))).width(260.0))
    /// ```
    pub fn popover(
        mut self,
        open: bool,
        placement: Placement,
        on_dismiss: impl Fn(&mut S) + 'static,
        content: View<S>,
    ) -> Self {
        self.extras().pop = Some(Pop { opens: Opens::State(open, Rc::new(on_dismiss)), placement, content: Box::new(content) });
        self
    }
    /// A tap on this view opens `content` (usually a [`menu`]) below it; choosing an item, a
    /// click outside or Escape closes it. The framework keeps the open state, so no flag is
    /// needed. One attached popover per view: the last of `.popover`, `.menu`, `.context_menu` wins.
    ///
    /// ```ignore
    /// button("…", |_| {}).menu(menu(vec![MenuItem::action("Rename", |s: &mut App| s.rename())]))
    /// ```
    pub fn menu(self, content: View<S>) -> Self {
        self.menu_at(Placement::bottom().start(), content)
    }
    /// [`menu`](View::menu) on another side, e.g. `Placement::top().start()` above an avatar.
    pub fn menu_at(mut self, placement: Placement, content: View<S>) -> Self {
        self.extras().pop = Some(Pop { opens: Opens::Tap, placement, content: Box::new(content) });
        self
    }
    /// A secondary (right) click on this view opens `content` at the pointer.
    pub fn context_menu(mut self, content: View<S>) -> Self {
        let placement = Placement::bottom().start().gap(2.0);
        self.extras().pop = Some(Pop { opens: Opens::Context, placement, content: Box::new(content) });
        self
    }
    /// Runs `f` on a secondary (right) click on this view.
    pub fn on_secondary(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.extras().secondary = Some(Box::new(f));
        self
    }
    /// A small label shown under this view after the mouse rests on it for half a second;
    /// it never takes clicks. Moving on to a neighbour while one is visible shows the next
    /// one at once.
    pub fn tooltip(mut self, label: impl Into<String>) -> Self {
        self.extras().tip = None;
        self.extras().tip_text = Some((label.into(), None));
        self
    }
    /// A tooltip with a key hint in a chip, like "Back ⌘[".
    pub fn tooltip_shortcut(mut self, label: impl Into<String>, keys: impl Into<String>) -> Self {
        self.extras().tip = None;
        self.extras().tip_text = Some((label.into(), Some(keys.into())));
        self
    }
    /// A tooltip made of any view (style it yourself: background, padding, radius).
    pub fn tooltip_with(mut self, content: View<S>) -> Self {
        self.extras().tip_text = None;
        self.extras().tip = Some(Box::new(content));
        self
    }
    /// Where this view's tooltip shows (default: below, centered, 6 pt away). Like a popover
    /// it flips and slides to stay in the window.
    ///
    /// ```ignore
    /// icon_button.tooltip("Back").tooltip_at(Placement::bottom().gap(2.0))
    /// rail_item.tooltip("Search").tooltip_at(Placement::right().gap(8.0))
    /// ```
    pub fn tooltip_at(mut self, placement: Placement) -> Self {
        self.extras().tip_at = Some(placement);
        self
    }
    /// Runs `f` when the key combination is pressed while this view is on screen, wherever the
    /// focus is; put app-wide ones on the root. In a text field, combinations that edit text
    /// (plain keys, Cmd+A/C/V/X/Z/Y) go to the field instead.
    ///
    /// ```ignore
    /// root.shortcut(Shortcut::cmd('k'), |s| s.palette = true)
    /// ```
    pub fn shortcut(mut self, keys: Shortcut, f: impl Fn(&mut S) + 'static) -> Self {
        self.extras().shortcuts.push((keys, Box::new(f)));
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
        lines: 0,
        truncate: Truncate::Ellipsis,
        line_height: crate::text::LINE_HEIGHT,
        tracking: 0.0,
        features: Vec::new(),
        role: TextRole::Body,
        set: 0,
    }))
}
/// Text made of differently styled runs (bold, italic, code, colored) that wrap together.
pub fn rich_text<S>(spans: Vec<Span>) -> View<S> {
    View::new(Kind::Text(TextData {
        spans,
        content: String::new(),
        size: 17.0,
        weight: Weight::Regular,
        center: false,
        lines: 0,
        truncate: Truncate::Ellipsis,
        line_height: crate::text::LINE_HEIGHT,
        tracking: 0.0,
        features: Vec::new(),
        role: TextRole::Body,
        set: 0,
    }))
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
    let mut label = text(label);
    if let Kind::Text(t) = &mut label.kind {
        t.role = TextRole::Headline;
    }
    let mut b = zstack(label).on_tap(on_tap);
    b.style.role = Role::Button;
    b
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
/// Text input. Fills the available width. Single-line by default; see
/// [`multiline`](View::multiline), [`on_submit`](View::on_submit) and [`plain`](View::plain).
///
/// ```ignore
/// text_field(&s.draft, "Message", |s, v| s.draft = v).multiline().max_lines(8).on_submit(|s| s.send())
/// ```
pub fn text_field<S>(
    value: impl Into<String>,
    placeholder: impl Into<String>,
    on_change: impl Fn(&mut S, String) + 'static,
) -> View<S> {
    let mut v = View::new(Kind::TextField(crate::ui::edit::Field::new(value.into(), placeholder.into(), Box::new(on_change))));
    v.style.role = Role::Field;
    v.fill_width()
}
/// Drop-down choice: shows the selected option and opens a glass menu with all of them.
pub fn picker<S, T: AsRef<str>>(options: &[T], selected: usize, on_select: impl Fn(&mut S, usize) + 'static) -> View<S> {
    let options: Vec<String> = options.iter().map(|o| o.as_ref().to_string()).collect();
    let selected = selected.min(options.len().saturating_sub(1));
    let mut v = View::new(Kind::Picker { options, selected, on_select: Rc::new(on_select) });
    v.style.role = Role::Picker;
    v
}
/// Floating glass tab bar. A glass "drop" marks the selected tab: it flows to a tab when
/// you tap it, and while held it swells past the bar and can be dragged across the tabs,
/// bending everything underneath like a lens.
pub fn tab_bar<S, T: AsRef<str>>(labels: &[T], selected: usize, on_select: impl Fn(&mut S, usize) + 'static) -> View<S> {
    let labels: Vec<String> = labels.iter().map(|l| l.as_ref().to_string()).collect();
    let selected = selected.min(labels.len().saturating_sub(1));
    let mut v = View::new(Kind::Tabs { labels, selected, on_select: Box::new(on_select) });
    v.style.role = Role::Tabs;
    v
}
/// iOS-style switch.
pub fn toggle<S>(on: bool, on_change: impl Fn(&mut S, bool) + 'static) -> View<S> {
    View::new(Kind::Toggle(on, Box::new(on_change)))
}
/// Slider with `value` in 0..=1. Fills the available width. [`steps`](View::steps) makes it snap.
pub fn slider<S>(value: f32, on_change: impl Fn(&mut S, f32) + 'static) -> View<S> {
    View::new(Kind::Slider(value.clamp(0.0, 1.0), 0, Box::new(on_change), None)).fill_width()
}
/// A menu panel: rows with icons, shortcuts, hints, separators, section titles and submenus,
/// highlighted under the mouse and driven by ↑↓ ←→ Enter Esc. Open it with
/// [`View::menu`] / [`View::context_menu`], or put it in a [`View::popover`]; choosing an item
/// runs it and closes the popover. Restyle it with [`View::menu_style`].
///
/// ```ignore
/// menu(vec![
///     MenuItem::action("Rename", |s: &mut App| s.rename()).icon(&pencil).shortcut("⌥⌘R"),
///     MenuItem::separator(),
///     MenuItem::submenu("Copy", vec![MenuItem::action("Link", |s| s.copy_link())]),
/// ])
/// ```
pub fn menu<S>(items: Vec<MenuItem<S>>) -> View<S> {
    View::new(Kind::Menu { items, style: None })
}
