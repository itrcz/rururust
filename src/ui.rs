//! Platform-independent core: owns the app state, rebuilds the view tree,
//! lays it out, paints it into a `DrawList`, and routes pointer input.

use crate::layout::{child_id, layout, spec_in, LNode, Lx};
use crate::theme::{Look, Theme};
use crate::text::{TextSpec, TextSystem};
use crate::types::{Color, Rect};
use crate::view::{Axis, Cursor, Env, Fill, Glass, Image, Indicator, Kind, OverlayMode, Shadow, Truncate, View, ViewFn};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use web_time::Instant;

#[path = "reveal.rs"]
mod reveal;
#[path = "marquee.rs"]
mod marquee;
#[path = "cursor.rs"]
mod cursor;

#[path = "edit.rs"]
pub(crate) mod edit;
#[path = "select.rs"]
pub(crate) mod select;
#[path = "popup.rs"]
pub(crate) mod popup;

use popup::{Dismiss, Layer, LayerCtx, MenuNav, MenuState, Shortcut};

// ---------------------------------------------------------------- draw list

pub(crate) const KIND_FILL: f32 = 0.0;
pub(crate) const KIND_SHADOW: f32 = 1.0;
pub(crate) const KIND_GLASS: f32 = 2.0;
pub(crate) const KIND_IMAGE: f32 = 3.0;
pub(crate) const KIND_MORPH: f32 = 4.0;

// Flag bits packed into `np.w` (bits 0-3: square clip corners, 4-7: square shape corners,
// 10-13: hidden border edges). The shader reads the same numbers.
/// A fill that covers only the outside of its rounded corners (`.mask_corners()`).
pub(crate) const FLAG_CORNER_MASK: u32 = 256;
/// Glass lit from the sides rather than from above (`Glass::horizontal_light`).
pub(crate) const FLAG_HORIZONTAL_LIGHT: u32 = 512;
/// A border whose color fades along the view (`n0`, `n1` carry the fade, not geometry).
pub(crate) const FLAG_BORDER_FADE: u32 = 16384;

/// One GPU instance. Units are logical points; the shader applies the scale factor.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Quad {
    pub rect: [f32; 4],
    pub clip: [f32; 4],
    pub c0: [f32; 4],
    pub c1: [f32; 4],
    pub border: [f32; 4],
    /// radius, border width, shadow blur (glass: bottom fade height), kind
    pub p0: [f32; 4],
    /// gradient dir x, gradient dir y, clip radius, opacity
    pub p1: [f32; 4],
    /// Glass only: up to two neighbouring panes this one fuses with (x, y, w, h)…
    pub n0: [f32; 4],
    pub n1: [f32; 4],
    /// …their corner radii, and the fusing distance (0 = stands alone).
    pub np: [f32; 4],
}

pub struct TextCmd {
    pub key: u64,
    pub pos: (f32, f32),
    pub size: (f32, f32),
    pub scale: f32,
    pub color: Color,
    pub clip: Rect,
    /// Where the text fades out: alpha falls from 1 at the left of this rect to 0 at its
    /// right; text to the right of it is hidden. `Truncate::Fade`.
    pub fade: Option<Rect>,
    /// Fade in from the leading clip edge over this many points (full text height).
    pub leading_fade: f32,
}

pub enum Cmd {
    Quad(Quad),
    Text(TextCmd),
}

#[derive(Default)]
pub struct DrawList {
    pub cmds: Vec<Cmd>,
    /// Pictures used this frame; image quads refer to them by index (in `c1[0]`).
    pub images: Vec<Image>,
}

// ---------------------------------------------------------------- animation

#[derive(Clone, Copy)]
pub(crate) struct Spring {
    pub x: f32,
    v: f32,
    pub target: f32,
    pub used: u64,
    /// Stiffness and damping.
    k: f32,
    c: f32,
}

impl Spring {
    /// Parameters of a spring that only keeps a value (the theme's `motion` drives the rest).
    pub(crate) const SMOOTH: (f32, f32) = (260.0, 32.25);

    pub(crate) fn new(x: f32, target: f32, (k, c): (f32, f32)) -> Self {
        Spring { x, v: 0.0, target, used: 0, k, c }
    }

    pub(crate) fn snap_to(&mut self, target: f32) {
        self.x = target;
        self.target = target;
        self.v = 0.0;
    }

    fn step(&mut self, dt: f32) {
        let n = (dt / (1.0 / 240.0)).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as usize {
            let a = -self.k * (self.x - self.target) - self.c * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
        if self.settled_within(0.001) {
            self.x = self.target;
            self.v = 0.0;
        }
    }
    fn settled_within(&self, eps: f32) -> bool {
        (self.x - self.target).abs() < eps && self.v.abs() < eps * 10.0
    }
    fn moving(&self) -> bool {
        self.x != self.target || self.v != 0.0
    }
}

/// Spring slots: a node keeps its springs under `(id, slot)`. Every animated value of a
/// node has its own slot here, so two effects on one view never drive the same spring.
pub(crate) mod slot {
    /// Press feedback of a tappable view.
    pub const PRESS: u8 = 0;
    /// Toggle knob position.
    pub const TOGGLE: u8 = 1;
    /// Toggle and slider knob swelling while held.
    pub const CONTROL_PRESS: u8 = 2;
    /// Focus ring of a text field.
    pub const FOCUS: u8 = 3;
    /// Opening of the picker drop-down (on the picker's id).
    pub const PICKER_MENU: u8 = 4;
    /// Tab bar drop: position and swelling while held.
    pub const TAB_DROP: u8 = 5;
    pub const TAB_HELD: u8 = 6;
    pub const TAB_WIDTH: u8 = 24;
    pub const ICON_PROGRESS: u8 = 25;
    pub const COLLAPSE: u8 = 26;
    /// Fade of a layer (popover, menu, tooltip) or a modal.
    pub const FADE: u8 = 7;
    /// Scroll bar visibility.
    pub const BAR_SHOW: u8 = 8;
    /// `.hover()` / `.hover_color()` highlight (also a split divider's, on its own id).
    pub const HOVER: u8 = 9;
    /// `.animate()` size, and the last target it was laid out at (layout).
    pub const W: u8 = 10;
    pub const H: u8 = 11;
    pub const LAST_W: u8 = 12;
    pub const LAST_H: u8 = 13;
    /// Width of a split's panel region (layout and paint).
    pub const SPLIT: u8 = 14;
    /// `.reveal()` progress.
    pub const REVEAL: u8 = 15;
    /// `.animate_radius()`.
    pub const RADIUS: u8 = 16;
    /// `.text_cursor()` size.
    pub const CURSOR: u8 = 17;
    /// `.hover_offset()` amount.
    pub const HOVER_OFFSET: u8 = 18;
    /// Sheet, drawer and snackbar entrance.
    pub const OVERLAY: u8 = 19;
    /// Scroll bar widening under the pointer.
    pub const BAR_WIDE: u8 = 20;
    /// A split's resize grip while dragged (on the divider's id).
    pub const DIVIDER_HELD: u8 = 21;
    /// `.rotation()` angle.
    pub const ROTATION: u8 = 22;
    /// `progress_bar` / `progress_ring` value.
    pub const PROGRESS: u8 = 23;
}

/// The spring of `(id, slot)`: made at `initial` with `params` the first time it is seen,
/// then aimed at `target` (or snapped there while the window resizes) and kept this frame.
pub(crate) fn spring_with<'a>(
    springs: &'a mut HashMap<(u64, u8), Spring>,
    (frame, resizing): (u64, bool),
    (id, slot): (u64, u8),
    initial: f32,
    target: f32,
    params: (f32, f32),
) -> &'a mut Spring {
    let s = springs.entry((id, slot)).or_insert(Spring::new(initial, target, params));
    if resizing { s.snap_to(target); } else { s.target = target; }
    s.used = frame;
    s
}

#[derive(Clone, Copy, Default)]
struct ScrollState {
    request: Option<u64>,
    navigation: Option<(f32,Instant)>,
    offset: f32,
    vel: f32,
    max: f32,
    used: u64,
    /// Extra room kept open above the content while a refresh is running.
    hold: f32,
    refreshing: bool,
    /// Pulled past the refresh threshold and not released yet.
    armed: bool,
    /// Last time the offset changed (drives the auto-hiding scroll bar).
    active_at: Option<Instant>,
    /// Last trackpad delta.
    wheel_at: Option<Instant>,
    /// Where the trackpad gesture is: fingers, lift, OS momentum.
    pad: Pad,
    /// Speed of the trackpad deltas (pt/s), carried into the bounce when momentum meets an end.
    pad_v: f32,
}

/// A trackpad gesture as macOS reports it: fingers move (`Started`…`Ended`), then the OS may
/// send its own momentum as a second `Started`…`Ended` run right after the lift.
#[derive(Clone, Copy, Default, PartialEq)]
enum Pad {
    #[default]
    Idle,
    /// Fingers on the pad: the rubber band holds its stretch even while they rest.
    Fingers,
    /// Fingers just lifted; a `Started` now is the OS momentum.
    Lifted(Instant),
    Momentum,
    /// Momentum that reached an end: the bounce owns the offset, the rest of the run is
    /// ignored (NSScrollView does the same). Feeding it back in restarts the bounce on every
    /// tail event, which is the stutter of repeated hits at the edge.
    Spent,
}

/// Rubber band: finger travel `r` past the end → stretch. The slope is 1 / (1 + A·stretch)²,
/// so it follows the finger at first and stiffens; [`unstretch`] is its exact inverse, which
/// makes pulling out and pushing back retrace the same path.
const RUBBER_A: f32 = 0.03;
fn stretch(r: f32) -> f32 {
    ((1.0 + 3.0 * RUBBER_A * r).cbrt() - 1.0) / RUBBER_A
}
fn unstretch(o: f32) -> f32 {
    ((1.0 + RUBBER_A * o).powi(3) - 1.0) / (3.0 * RUBBER_A)
}

/// Pull distance that triggers a refresh, and the gap kept open while it runs.
const REFRESH_AT: f32 = 72.0;
const REFRESH_HOLD: f32 = 56.0;

impl ScrollState {
    /// Resting range of the offset.
    fn bounds(&self) -> (f32, f32) {
        let lo = -self.hold;
        (lo, self.max.max(lo))
    }
    /// How far past the resting range the content is stretched.
    fn over(&self) -> f32 {
        let (lo, hi) = self.bounds();
        (lo - self.offset).max(self.offset - hi).max(0.0)
    }
    /// How long after the last trackpad delta the offset stays where the fingers put it.
    /// Fingers resting on the pad send nothing, so they hold it until the lift; the time
    /// limit only guards against a lost `Ended`.
    fn wheel_hold(&self) -> f32 {
        if self.pad == Pad::Fingers { 1.0 } else { 0.08 }
    }
    /// Moves the offset by a finger delta; past the ends it stretches like a rubber band.
    fn pull(&mut self, delta: f32) {
        let (lo, hi) = self.bounds();
        // Work in finger travel, where the motion is linear, and map back.
        let at = if self.offset > hi {
            hi + unstretch(self.offset - hi)
        } else if self.offset < lo {
            lo - unstretch(lo - self.offset)
        } else {
            self.offset
        };
        let t = at + delta;
        self.offset = if t > hi {
            hi + stretch(t - hi)
        } else if t < lo {
            lo - stretch(lo - t)
        } else {
            t
        };
    }
}

/// How a wheel / trackpad delta should be treated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelPhase {
    /// A classic mouse wheel: no rubber band, stops at the ends.
    Line,
    /// Trackpad gesture phases (fingers down / moving / lifted, then OS momentum).
    Started,
    Moved,
    Ended,
}

thread_local! {
    static PENDING: RefCell<Vec<(f32, Box<dyn Any>)>> = const { RefCell::new(Vec::new()) };
    static COPIES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Puts `text` on the system clipboard. Call it from any event handler, e.g. a "copy"
/// button under a message: `button("Copy", move |_: &mut App| ruru::copy_text(msg.clone()))`.
/// It reaches the runner as [`WindowRequest::SetClipboard`] right after the handler (a tap)
/// or with the next frame.
pub fn copy_text(text: impl Into<String>) {
    let text = text.into();
    COPIES.with(|c| c.borrow_mut().push(text));
}

/// Runs `f` on the app state after `seconds`, then redraws. Call it from any event
/// handler, e.g. to hide a snackbar or finish a refresh:
/// `ruru::after(2.0, |s: &mut App| s.toast = false)`.
pub fn after<S: 'static>(seconds: f32, f: impl FnOnce(&mut S) + 'static) {
    let f: Box<dyn FnOnce(&mut S)> = Box::new(f);
    PENDING.with(|p| p.borrow_mut().push((seconds.max(0.0), Box::new(f))));
}

// ---------------------------------------------------------------- input

enum Action<S> {
    Tap(Option<Box<dyn Fn(&mut S)>>, Option<Box<dyn Fn(&mut S)>>),
    Toggle(bool, Box<dyn Fn(&mut S, bool)>),
    /// x of value 0, width of the value range (screen space), number of stops (0 = smooth)
    Slider(f32, f32, u32, Box<dyn Fn(&mut S, f32)>),
    Scroll { refresh: Option<Box<dyn Fn(&mut S)>> },
    /// Covers everything below an overlay; a tap on it dismisses the overlay.
    Block(Option<Box<dyn Fn(&mut S)>>),
    /// The body of an overlay: taps on it do nothing (and don't dismiss).
    Swallow,
    /// Text field (see `edit.rs`).
    Field(Box<edit::FieldHit<S>>),
    /// Tab bar: right edges in screen coordinates; hit testing follows measured widths.
    Tabs { ends: Vec<f32>, disabled: Vec<usize>, on_select: Box<dyn Fn(&mut S, usize)> },
    OpenMenu,
    SelectAnchor {open:bool},
    Dismiss,
    Pick(usize, Rc<dyn Fn(&mut S, usize)>),
    /// `.window_drag()` area: a press moves the window.
    DragWindow,
    /// A `.selectable()` area, under the views inside it: a press starts a selection.
    Select { area: u64 },
    /// A link in rich text (`Span::link`).
    Link(Box<select::LinkHit<S>>),
    /// Everything outside an open popover: a press closes it and goes no further.
    Outside(Dismiss<S>),
    /// A menu row: close its source popover, then run the action.
    MenuPick(Box<dyn Fn(&mut S)>, Option<Dismiss<S>>),
    /// A submenu row: (menu id, row) opens it.
    SubMenu(u64, usize),
    /// Secondary click: run the handler, or (`None`) open the context menu of view `.1`.
    /// Ignored by the primary button.
    Secondary(Option<Box<dyn Fn(&mut S)>>, u64),
    /// The scroll bar of scroll view `scroll`: screen y and length of the thumb's travel,
    /// thumb length (screen), the offset range and one page (content units).
    ScrollBar { scroll: u64, y0: f32, track: f32, len: f32, lo: f32, hi: f32, page: f32 },
    /// A split's divider.
    Split(Rc<SplitHit<S>>),
}

/// Scroll bar: hover strip along the right edge (thumb widths are in the theme).
const BAR_STRIP: f32 = 14.0;
const BAR_SALT: u64 = 0x5C_B0A2;

/// Something the UI asks of the window it lives in. The core knows nothing about windows:
/// the runner drains [`Ui::requests`] after each event and carries them out (or ignores them
/// where they make no sense, like on the web).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowRequest {
    /// Start moving the window with the pressed pointer (a press on `.window_drag()`).
    DragWindow,
    /// Zoom / maximize or restore the window (a double click on `.window_drag()`).
    Zoom,
    /// Put this text on the system clipboard (copy / cut in a text field, copy of selected text).
    SetClipboard(String),
    /// Open this http(s) or mailto URL in the browser (a click on a link without `.on_link()`).
    OpenUrl(String),
}

/// Keys the UI understands besides typed text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyInput {
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    /// Start / end of the visual row; with `command` held, of the whole text.
    Home,
    End,
    Enter,
    Escape,
    /// Shortcuts the runner maps from Cmd/Ctrl+A, C, X, Z, Shift+Z. Pasting goes through
    /// [`Ui::paste`], since only the runner can read the clipboard.
    SelectAll,
    Copy,
    Cut,
    Undo,
    Redo,
    /// Tab and Shift+Tab: focus the next / previous `text_field` in paint order, wrapping;
    /// inside an open modal or popover only its own fields take part.
    Tab,
    BackTab,
}

/// An open picker menu, painted above everything else.
struct Menu<S> {
    id: u64,
    anchor: Rect,
    options: Vec<String>,
    selected: usize,
    on_select: Rc<dyn Fn(&mut S, usize)>,
    fg: Color,
    text_style: crate::theme::TextStyle,
}

/// The view with a tooltip under the mouse.
struct TipState {
    id: u64,
    since: Instant,
    /// Seconds to wait (0 right after another tooltip was shown).
    delay: f32,
    /// Hidden by a click until the mouse moves to another view.
    hidden: bool,
}

/// Seconds between two presses that make a double click.
const DOUBLE_CLICK: f32 = 0.4;
/// Width of the strip that grabs a split's divider, centered on its line.
const SPLIT_GRAB: f32 = 8.0;
const SPLIT_SALT: u64 = 0x5B_17D1;

/// What a press on a split's divider needs to resize it.
pub(crate) struct SplitHit<S> {
    /// The split view.
    id: u64,
    /// Screen x of the panel's outer edge, and which way from it the panel grows (+1 right).
    edge: f32,
    dir: f32,
    /// Screen points per point.
    s: f32,
    min: f32,
    max: f32,
    /// The panel's width now (while open).
    open: f32,
    collapsed: bool,
    /// Width under which a drag collapses it, with the handler.
    collapse: Option<(f32, Rc<dyn Fn(&mut S, bool)>)>,
    on_resize: Rc<dyn Fn(&mut S, f32)>,
    reset: Option<f32>,
}

/// A divider being dragged.
struct SplitDrag<S> {
    hit: Rc<SplitHit<S>>,
    /// Pointer distance from the panel's width at the press, kept so the line stays under it.
    grab: f32,
    collapsed: bool,
}

struct Hit<S> {
    id: u64,
    rect: Rect,
    /// The rounded clip the hit area was cut by (`.clip()`), and its radius.
    round: (Rect, f32),
    action: Action<S>,
}

const NO_ROUND: (Rect, f32) = (Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, 0.0);

/// Inside `rect` and inside the rounded corners of the clip `round`.
fn inside(rect: &Rect, round: &(Rect, f32), (x, y): (f32, f32)) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let (c, r) = *round;
    if r <= 0.0 {
        return true;
    }
    let r = r.min(c.w * 0.5).min(c.h * 0.5);
    let dx = (x - (c.x + c.w * 0.5)).abs() - (c.w * 0.5 - r);
    let dy = (y - (c.y + c.h * 0.5)).abs() - (c.h * 0.5 - r);
    dx <= 0.0 || dy <= 0.0 || dx * dx + dy * dy <= r * r
}

impl<S> Hit<S> {
    fn contains(&self, pos: (f32, f32)) -> bool {
        inside(&self.rect, &self.round, pos)
    }
}

/// Where the pointer counts as being over a view: for `.hover()` and the pointer shape.
struct Zone {
    id: u64,
    rect: Rect,
    round: (Rect, f32),
    cursor: Option<Cursor>,
    /// An overlay's backdrop: nothing underneath is hovered.
    blocks: bool,
    /// The view has a tooltip.
    tip: bool,
}

/// The views under `pos`, topmost first, the pointer shape to show there and the topmost
/// view with a tooltip.
fn hover_at(zones: &[Zone], pos: Option<(f32, f32)>) -> (Vec<u64>, Cursor, Option<u64>) {
    let (mut ids, mut cursor, mut tip) = (Vec::new(), None, None);
    let Some(pos) = pos else { return (ids, Cursor::Default, None) };
    for z in zones.iter().rev() {
        if !inside(&z.rect, &z.round, pos) {
            continue;
        }
        if z.blocks {
            break;
        }
        ids.push(z.id);
        cursor = cursor.or(z.cursor);
        if z.tip && tip.is_none() {
            tip = Some(z.id);
        }
    }
    (ids, cursor.unwrap_or_default(), tip)
}

#[derive(Default)]
struct Pointer {
    pressed: Option<u64>,
    /// A button or finger is down.
    down: bool,
    touch: bool,
    last: (f32, f32),
    down_at: (f32, f32),
    drag_scroll: Option<u64>,
    dragging: bool,
    last_move: Option<Instant>,
    /// Time and place of the last press on a window-drag area, to spot a double click.
    drag_down: Option<(Instant, (f32, f32))>,
    tap: Option<(Instant, u64, (f32, f32))>,
    /// A scroll bar thumb held by the mouse: the scroll view, and where on the thumb.
    bar: Option<(u64, f32)>,
}

// ---------------------------------------------------------------- painter

#[derive(Clone, Copy)]
struct Xf {
    s: f32,
    tx: f32,
    ty: f32,
    opacity: f32,
    text_reveal: Option<crate::view::TextReveal>,
    blur: f32,
    hover_scope: Option<bool>,
    clip: Rect,
    clip_r: f32,
    /// The clip's corners are continuous (`.smooth()`), not circular.
    clip_smooth: bool,
    /// Corners of the clip that are square although `clip_r` > 0 (bit 0 top left, then
    /// clockwise): a side panel cut from a rounded one at a divider keeps its outer corners
    /// round and its inner ones sharp.
    clip_sq: u8,
    shape_sq: u8,
    border_hidden: u8,
    fg: Color,
    /// Device pixels per point, for snapping to the pixel grid; 0 turns snapping off for
    /// Turn of pictures, radians clockwise about each picture's own center (`.rotation()`).
    rot: f32,
    /// what is drawn with it (a knob or lens that swells on a spring: steps would show).
    px: f32,
}

/// `v` (screen points) moved to the nearest edge between device pixels. Half rounds up,
/// so a shift by whole pixels (scrolling) shifts the result by exactly as much.
pub(crate) fn snap(v: f32, px: f32) -> f32 {
    (v * px + 0.5).floor() / px
}

/// Both edges of `r` on the pixel grid. A hairline (half a pixel or more) that would
/// round away stays one pixel thick, centered where it was; anything thinner (a panel
/// collapsing to nothing) is let go, so it meets its neighbour.
pub(crate) fn snap_rect(r: Rect, px: f32) -> Rect {
    let axis = |a: f32, len: f32| {
        let (mut lo, mut hi) = (snap(a, px), snap(a + len, px));
        if len * px >= 0.5 && hi - lo < 0.5 / px {
            lo = snap(a + len * 0.5 - 0.5 / px, px);
            hi = lo + 1.0 / px;
        }
        (lo, hi - lo)
    };
    let (x, w) = axis(r.x, r.w);
    let (y, h) = axis(r.y, r.h);
    Rect::new(x, y, w, h)
}

impl Xf {
    /// No transform: screen points, clipped to `clip`, text in `fg`, snapped at `px`.
    fn screen(clip: Rect, fg: Color, px: f32) -> Xf {
        Xf { s: 1.0, tx: 0.0, ty: 0.0, opacity: 1.0, text_reveal: None, blur: 0.0, hover_scope: None, clip, clip_r: 0.0, clip_smooth: false, clip_sq: 0, shape_sq: 0, border_hidden: 0, fg, px, rot: 0.0 }
    }
    /// Scales what follows by `k` about the local point `(cx, cy)`, which stays put on
    /// screen: press squeeze, reveal, popovers growing out of their anchor.
    fn scale_about(&mut self, cx: f32, cy: f32, k: f32) {
        self.tx += cx * (1.0 - k) * self.s;
        self.ty += cy * (1.0 - k) * self.s;
        self.s *= k;
    }
    fn rect(&self, r: Rect) -> Rect {
        Rect::new(r.x * self.s + self.tx, r.y * self.s + self.ty, r.w * self.s, r.h * self.s)
    }
    // Snapping happens in local points, before the transform. The translation only ever comes
    // from scaling about a point (press, pop-in), so at rest (`s` = 1) local points are screen
    // points: a scaled view is its resting, snapped self, scaled. An animation that ends at
    // `s` = 1 therefore lands exactly on the snapped rect instead of jumping to it.

    /// Screen rect of `r` with its edges on device pixels at rest: crisp edges and hairlines,
    /// and neighbours that share an edge still meet.
    fn rect_px(&self, r: Rect) -> Rect {
        match self.px {
            px if px > 0.0 => self.rect(snap_rect(r, px)),
            _ => self.rect(r),
        }
    }
    /// The local point nearest to `p` that lands on a device pixel at rest. Text and whatever
    /// is drawn relative to it (plates, selection) start here, so they move together.
    fn at_px(&self, p: (f32, f32)) -> (f32, f32) {
        match self.px {
            px if px > 0.0 => (snap(p.0, px), snap(p.1, px)),
            _ => p,
        }
    }
    /// Where text whose box starts at the local point `p` (from [`at_px`](Self::at_px)) and is
    /// `size` big goes on screen, and the scale its glyphs are rasterized at.
    ///
    /// Glyphs follow exactly the same affine transform as their surrounding surface.
    /// Quantizing their scale and centering the difference makes long labels slide
    /// sideways whenever the raster size crosses a step. Only the glyph row is snapped,
    /// because glyphon places rows on whole device pixels.
    fn text_at(&self, p: (f32, f32), _size: (f32, f32)) -> ((f32, f32), f32) {
        let x = p.0 * self.s + self.tx;
        let y = p.1 * self.s + self.ty;
        let y = if self.px > 0.0 { (y * self.px).round() / self.px } else { y };
        ((x, y), self.s)
    }
    /// A border width `w` (points) on screen: never thinner than one device pixel, and a
    /// whole number of them while it is thin, so a hairline is one solid row of pixels.
    fn line(&self, w: f32) -> f32 {
        if self.px <= 0.0 || w <= 0.0 {
            return w * self.s;
        }
        let d = w * self.px;
        let d = if d <= 1.5 { (d - 0.01).round() } else { d };
        d.max(1.0) / self.px * self.s
    }
    /// Narrows the clip to the screen rect `r` with corner radius `radius`. The GPU clips to
    /// one rounded rectangle, so nested clips keep the corners of whichever is inside.
    fn clip_to(&mut self, r: Rect, radius: f32, smooth: bool) {
        self.clip_to_corners(r, radius, smooth, 0);
    }
    fn clip_to_corners(&mut self, r: Rect, radius: f32, smooth: bool, corners: u8) {
        let next = self.clip.intersect(&r);
        let near = |a: &Rect, b: &Rect| {
            (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5 && (a.w - b.w).abs() < 0.5 && (a.h - b.h).abs() < 0.5
        };
        let (src, sq, rad, smooth) = if near(&next, &r) && near(&next, &self.clip) && self.clip_r >= radius {
            let sq = if self.clip_r == radius { self.clip_sq & corners } else { self.clip_sq };
            (self.clip, sq, self.clip_r, self.clip_smooth)
        } else if near(&next, &r) {
            (r, corners, radius, smooth)
        } else if near(&next, &self.clip) {
            (self.clip, self.clip_sq, self.clip_r, self.clip_smooth)
        } else if radius > self.clip_r {
            (r, corners, radius, smooth)
        } else {
            (self.clip, self.clip_sq, self.clip_r, self.clip_smooth)
        };
        // Only the corners the cut shares with the rounded rect it came from are round.
        let corners = |a: &Rect| [(a.x, a.y), (a.x + a.w, a.y), (a.x + a.w, a.y + a.h), (a.x, a.y + a.h)];
        let (a, b) = (corners(&next), corners(&src));
        let mut sq = sq;
        for i in 0..4 {
            if (a[i].0 - b[i].0).abs() >= 0.5 || (a[i].1 - b[i].1).abs() >= 0.5 {
                sq |= 1 << i;
            }
        }
        (self.clip_r, self.clip_smooth, self.clip_sq) = (rad, smooth, sq);
        self.clip = next;
    }
}

/// A scroll bar waiting to be painted over everything else in its layer.
struct Bar<S> {
    /// The scroll view's transform and clip.
    xf: Xf,
    id: u64,
    /// The edge strip (screen) that reveals the bar on hover and takes presses on it.
    strip: Rect,
    /// Hover zone over the strip, with its pointer shape.
    zone: Option<Option<Cursor>>,
    /// Track under the pointer, and the thumb with its radius.
    rail: Option<(Rect, Color)>,
    thumb: Option<(Rect, f32, Color)>,
    hit: Option<Action<S>>,
}

struct PointerObserver<S> {
    rect: Rect,
    scale: f32,
    handler: Box<dyn Fn(&mut S, Option<(f32, f32)>)>,
}

struct IconTimeline {
    value:f32,
    target:f32,
    at:Instant,
    used:u64,
}

struct Painter<'a, S> {
    visible_clip: Option<Rect>,
    visible_events: Vec<(Box<dyn Fn(&mut S, std::ops::Range<usize>)>, std::ops::Range<usize>)>,
    list: &'a mut DrawList,
    look: &'a Look,
    hits: &'a mut Vec<Hit<S>>,
    zones: &'a mut Vec<Zone>,
    pointer_observers: &'a mut Vec<PointerObserver<S>>,
    /// Views under the pointer (from the last frame's zones).
    hovered: &'a [u64],
    /// The scroll view whose bar thumb is being dragged.
    bar_drag: Option<u64>,
    /// The split whose divider is being dragged.
    split_drag: Option<u64>,
    springs: &'a mut HashMap<(u64, u8), Spring>,
    scrolls: &'a mut HashMap<u64, ScrollState>,
    icon_timelines: &'a mut HashMap<u64, IconTimeline>,
    pressed: Option<u64>,
    frame: u64,
    resizing: bool,
    text: &'a mut TextSystem,
    reveals: &'a mut reveal::Reveals,
    marquees: &'a mut marquee::Marquees,
    focus: Option<u64>,
    ed: &'a mut edit::Editor,
    caret_on: bool,
    open_menu: Option<u64>,
    menu: Option<Menu<S>>,
    pointer_x: f32,
    /// Set by a scroll view for its content stack: local y where sticky children pin.
    sticky: Option<f32>,
    /// Earliest moment (seconds from now) something needs a frame without input.
    wake: Option<f32>,
    now: Instant,
    /// Seconds since the UI started, for looping animations.
    time: f32,
    /// A looping animation (spinner) is on screen.
    spin: bool,
    /// The view being painted has `.smooth()` corners (its own quads only).
    smooth: bool,
    /// Sizes changed during paint (lazy rows were measured): lay out again next frame.
    relayout: bool,
    /// Selectable texts and areas on screen, for input.
    texts: &'a mut Vec<select::TextBox>,
    areas: &'a mut Vec<select::Area>,
    sel: Option<select::PaintSel>,
    /// Inherited by text views: selectable area, link handler, highlight color.
    tctx: select::TextCtx<S>,
    /// Number (in tree order) of the next text view; set by the parent before each child.
    tbase: u32,
    /// Lazy-list rows being painted, outermost first.
    path: Vec<(u64, usize)>,
    /// The window, for placing layers.
    screen: Rect,
    /// Popovers, submenus and tooltips to paint after the tree.
    layers: Vec<Layer<S>>,
    /// Scroll bars of the layer being painted, to go over the rest of it (`flush_bars`).
    bars: Vec<Bar<S>>,
    /// The layer being painted (default: the tree).
    layer: LayerCtx<S>,
    menu_state: &'a mut HashMap<u64, MenuState>,
    menu_nav: &'a mut Vec<MenuNav>,
    shortcuts: &'a mut Vec<(Shortcut, Box<dyn Fn(&mut S)>)>,
    /// The view whose `.menu()` / `.context_menu()` is open was painted.
    managed_seen: bool,
    /// Where the open context menu was asked for.
    ctx_at: (f32, f32),
    /// The view whose tooltip shows.
    tip: Option<u64>,
    /// Device pixels per point.
    px: f32,
}

impl<S> Painter<'_, S> {
    /// A split: the panel cut to its region, the main area, the divider line, and the strip
    /// that grabs it (deferred with the scroll bars, so it wins over a bar at the panel's edge).
    #[allow(clippy::too_many_arguments)]
    fn paint_split(&mut self, sp: crate::view::Split<S>, children: Vec<View<S>>, ln: &LNode, (r, cr): (Rect, Rect), xf: Xf, id: u64, tbase: u32) {
        let mut it = children.into_iter();
        let (Some(panel), Some(mut main)) = (it.next(), it.next()) else { return };
        let (pn, mn) = (&ln.children[0], &ln.children[1]);
        let region = ln.split;
        let line = self.look.theme.control.border;
        let (min, max) = (panel.style.min.0, panel.style.max.0.max(panel.style.min.0));
        let (pid, mid) = (child_id(id, 0, panel.key), child_id(id, 1, main.key));
        // The line is the panel region's inner edge.
        let line_x = if sp.trailing { cr.x + cr.w - region } else { cr.x + region - line };
        let inner = (region - line).max(0.0);
        let area = Rect::new(if sp.trailing { line_x + line } else { cr.x }, cr.y, inner, cr.h);
        let mut pxf = xf;
        pxf.clip_to(xf.rect_px(area), 0.0, false);
        self.tbase = tbase;
        self.paint(panel, pn, (r.x, r.y), pxf, pid);
        let div = id ^ SPLIT_SALT;
        let hidden = sp.hide_collapsed_divider && sp.collapse.as_ref().is_some_and(|c| c.0);
        let on = self.split_drag == Some(id) || self.hovered.contains(&div);
        let t = if hidden { 0.0 } else { self.crisp(div, slot::HOVER, if on { 1.0 } else { 0.0 }).clamp(0.0, 1.0) };
        if hidden { main.style.border_hidden |= if sp.trailing { 2 } else { 8 }; }
        if sp.highlight_main_border && !hidden {
            let (width, color) = main.style.border.unwrap_or((line, self.look.theme.color.divider));
            main.style.border = Some((width, color.lerp(self.look.theme.color.accent, t)));
            main.style.border_fade = sp.highlight_fade.map(|fraction| (color, fraction, sp.trailing));
        }
        self.tbase = tbase + pn.tn;
        self.paint(main, mn, (r.x, r.y), xf, mid);
        // An existing drag owns its SplitHit until release; it can reverse without an edge hit.
        if hidden { return; }

        if region > 0.0 && !sp.highlight_main_border {
            let c = self.look.theme.color.divider.lerp(self.look.theme.color.accent, t);
            self.fill(&xf, Rect::new(line_x, cr.y, line.min(region), cr.h), 0.0, Fill::Solid(c), None);
        }
        let center = line_x + line * 0.5;
        if sp.resize_grip && region > 0.0 {
            let (width, resting, hovered) = self.look.theme.control.split_grip;
            let held = self.crisp(div, slot::DIVIDER_HELD, if self.split_drag == Some(id) { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
            let w = width * (1.0 + 0.15 * t);
            let h = (resting + (hovered-resting)*t + hovered*0.15*held).min(cr.h);
            let grip_y = sp.grip_bottom.map_or((cr.h-h)*0.5, |inset| (cr.h-inset-h*0.5).clamp(0.0,(cr.h-h).max(0.0)));
            let grip = Rect::new(center-w*0.5, cr.y+grip_y, w, h);
            let color = self.look.theme.color.text_muted.alpha(0.45).lerp(self.look.theme.color.accent, t);
            self.fill(&xf, grip, w*0.5, Fill::Solid(color), None);
        }
        let grab = Rect::new(center - SPLIT_GRAB * 0.5, cr.y, SPLIT_GRAB, cr.h).intersect(&cr);
        let scr = xf.rect(cr);
        let hit = SplitHit {
            id,
            edge: if sp.trailing { scr.x + scr.w } else { scr.x },
            dir: if sp.trailing { -1.0 } else { 1.0 },
            s: xf.s,
            min,
            max,
            open: sp.width.clamp(min, max),
            collapsed: sp.collapse.as_ref().is_some_and(|c| c.0),
            collapse: sp.collapse.map(|c| (c.1, c.2)),
            on_resize: sp.on_resize,
            reset: sp.reset,
        };
        let mut bxf = xf;
        bxf.clip_r = 0.0;
        self.bars.push(Bar { xf: bxf, id: div, strip: xf.rect(grab), zone: Some(Some(Cursor::ResizeH)), rail: None, thumb: None, hit: Some(Action::Split(Rc::new(hit))) });
    }

    /// Paints the scroll bars collected so far, over everything painted before them: content
    /// laid over a scroll view (a fade, a floating composer) never covers its bar or takes its
    /// presses. Called at the end of the tree and of each layer, and before a modal's scrim,
    /// so popovers, menus and modals still cover bars beneath them.
    fn flush_bars(&mut self) {
        self.smooth = false;
        for b in std::mem::take(&mut self.bars) {
            if let Some(cursor) = b.zone {
                self.zone(&b.xf, b.id, b.strip, cursor);
            }
            if let Some((r, c)) = b.rail {
                self.fill(&b.xf, r, 0.0, Fill::Solid(c), None);
            }
            if let Some((r, radius, c)) = b.thumb {
                self.fill(&b.xf, r, radius, Fill::Solid(c), None);
            }
            if let Some(a) = b.hit {
                self.hit(&b.xf, b.id, b.strip, a);
            }
        }
    }

    fn hit(&mut self, xf: &Xf, id: u64, rect: Rect, action: Action<S>) {
        self.hits.push(Hit { id, rect: rect.intersect(&xf.clip), round: (xf.clip, xf.clip_r), action });
    }

    fn zone(&mut self, xf: &Xf, id: u64, rect: Rect, cursor: Option<Cursor>) {
        self.zones.push(Zone { id, rect: rect.intersect(&xf.clip), round: (xf.clip, xf.clip_r), cursor, blocks: false, tip: false });
    }

    fn label(&mut self, xf: &Xf, key: u64, pos: (f32, f32), size: (f32, f32), color: Color) {
        let (pos, scale) = xf.text_at(xf.at_px(pos), size);
        self.list.cmds.push(Cmd::Text(TextCmd {
            key,
            pos,
            size: (size.0 * xf.s, size.1 * xf.s),
            scale,
            color: color.alpha(color.a * xf.opacity),
            clip: xf.clip,
            fade: None,
            leading_fade: 0.0,
        }));
    }

    /// A picture filling `r`; with `tint` only its shape is used, painted in that color.
    fn picture(&mut self, xf: &Xf, r: Rect, radius: f32, img: &Image, tint: Option<Color>, border: Option<(f32, Color)>) {
        let mut q = self.quad(xf, r, radius, KIND_IMAGE);
        q.n0[0]=xf.blur;
        if let Some(c) = tint {
            q.c1[1] = 1.0;
            q.c1[2] = c.r;
            q.c1[3] = c.g;
            q.p1[0] = c.b;
            q.p1[1] = c.a;
        }
        // Fill the box and crop the overflow, keeping the picture's aspect ratio.
        let (iw, ih) = (img.0.width.max(1) as f32, img.0.height.max(1) as f32);
        let k = (r.w / iw).max(r.h / ih).max(1e-6);
        let (uw, uh) = (r.w / (iw * k) * 0.5, r.h / (ih * k) * 0.5);
        q.c0 = [0.5 - uw, 0.5 - uh, 0.5 + uw, 0.5 + uh];
        q.c1[0] = self.list.images.len() as f32;
        if xf.rot != 0.0 {
            // Images only: the shader turns the picture inside its own (unturned) rect.
            q.n1[0] = xf.rot;
        }
        if let Some((w, c)) = border {
            q.border = c.arr();
            q.p0[1] = xf.line(w);
        }
        self.list.images.push(img.clone());
        self.list.cmds.push(Cmd::Quad(q));
    }

    /// Plates and icons of a rich text, drawn under its glyphs.
    fn span_decor(&mut self, xf: &Xf, key: u64, spans: &[crate::view::Span], at: (f32, f32)) {
        if !spans.iter().any(|s| s.background.is_some() || s.icon.is_some()) {
            return;
        }
        let mut boxes = Vec::new();
        self.text.span_boxes(key, &mut boxes);
        for b in boxes {
            let Some(s) = spans.get(b.span) else { continue };
            let base = at.1 + b.baseline;
            if let Some(img) = &s.icon {
                // One em square, centered on the cap height so it sits with capitals.
                let r = Rect::new(at.0 + b.x, base - 0.36 * b.size - 0.5 * b.size, b.size, b.size);
                self.picture(xf, r, 0.0, img, Some(s.color.unwrap_or(xf.fg)), None);
            } else if let Some(c) = s.background {
                // From the descender to just above the capitals: ~15 pt at 14 pt text.
                let r = Rect::new(at.0 + b.x, base - 0.8 * b.size, b.w, 1.08 * b.size);
                self.fill(xf, r, 0.28 * b.size, Fill::Solid(c), None);
            }
        }
    }

    /// Spring that starts from `initial` the first time it is seen (appear animations).
    fn spring_from(&mut self, id: u64, slot: u8, initial: f32, target: f32) -> f32 {
        let bouncy = self.look.theme.motion.bouncy;
        self.spring_with(id, slot, initial, target, bouncy).x
    }

    /// [`spring_with`] on this frame's springs.
    pub(super) fn spring_with(&mut self, id: u64, slot: u8, initial: f32, target: f32, params: (f32, f32)) -> &mut Spring {
        spring_with(self.springs, (self.frame, self.resizing), (id, slot), initial, target, params)
    }

    /// iOS-style activity indicator; `progress` < 1 shows it filling up while pulling.
    fn spinner(&mut self, xf: &Xf, center: (f32, f32), progress: f32, spinning: bool) {
        const DOTS: usize = 8;
        let step = (self.time * 10.0) as usize;
        for i in 0..DOTS {
            let alpha = if spinning {
                0.2 + 0.8 * ((i + DOTS - step % DOTS) % DOTS) as f32 / (DOTS - 1) as f32
            } else if (i as f32) < progress * DOTS as f32 {
                0.75
            } else {
                continue;
            };
            let a = i as f32 / DOTS as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let dot = Rect::new(center.0 + a.cos() * 9.0 - 2.0, center.1 + a.sin() * 9.0 - 2.0, 4.0, 4.0);
            self.fill(xf, dot, 2.0, Fill::Solid(xf.fg.alpha(xf.fg.a * alpha)), None);
        }
        self.spin |= spinning;
    }

    fn paint_picker_menu(&mut self, m: Menu<S>, size: (f32, f32)) {
        let theme = self.look.theme.clone();
        let row_h = theme.control.picker_row.max((m.text_style.size*m.text_style.line_height).round()+theme.control.picker_padding.1*2.0);
        let screen = Rect::new(0.0, 0.0, size.0, size.1);
        // Anything outside the menu closes it.
        self.hits.push(Hit { id: m.id ^ 0xD15, rect: screen, round: NO_ROUND, action: Action::Dismiss });
        self.zones.push(Zone { id: m.id ^ 0xD15, rect: screen, round: NO_ROUND, cursor: None, blocks: true, tip: false });
        let spec = |text| spec_in(&m.text_style, text);
        let rows: Vec<(u64, (f32, f32))> = m.options.iter().map(|o| self.text.measure(&spec(o), None)).collect();
        let (check, check_size) = self.text.measure(&TextSpec { weight: 600, ..spec("✓") }, None);
        let widest = rows.iter().map(|r| r.1 .0).fold(0.0, f32::max);
        let w = (widest + 46.0 + check_size.0).max(m.anchor.w).min(size.0 - 16.0);
        let h = row_h * rows.len() as f32 + 12.0;
        let x = m.anchor.x.min(size.0 - w - 8.0).max(8.0);
        let below = m.anchor.y + m.anchor.h + 6.0;
        let (y, down) = if below + h <= size.1 - 8.0 { (below, true) } else { ((m.anchor.y - h - 6.0).max(8.0), false) };

        // The menu grows out of the control it belongs to.
        let t = self.spring_with(m.id, slot::PICKER_MENU, 0.0, 1.0, theme.motion.bouncy).x;
        let k = 0.8 + 0.2 * t;
        let (ax, ay) = (m.anchor.x + m.anchor.w * 0.5, if down { y } else { y + h });
        let mut xf = Xf { opacity: t.clamp(0.0, 1.0), ..Xf::screen(screen, m.fg, self.px) };
        xf.scale_about(ax, ay, k);
        let r = Rect::new(x, y, w, h);
        let radius = theme.control.picker_radius;
        self.shadow(&xf, r, radius, theme.shadow.bar, true);
        self.glass(&xf, r, radius, theme.glass_menu, None);
        for (i, (key, ts)) in rows.into_iter().enumerate() {
            let row = Rect::new(x + 6.0, y + 6.0 + row_h * i as f32, w - 12.0, row_h);
            let rid = child_id(m.id, i, Some(0xA000 + i as u64));
            if self.pressed == Some(rid) {
                self.fill(&xf, row, 12.0, Fill::Solid(theme.color.pressed), None);
            }
            self.label(&xf, key, (row.x + 10.0, row.y + (row_h - ts.1) * 0.5), ts, m.fg);
            if i == m.selected {
                let cx = row.x + row.w - 10.0 - check_size.0;
                self.label(&xf, check, (cx, row.y + (row_h - check_size.1) * 0.5), check_size, m.fg);
            }
            self.hits.push(Hit { id: rid, rect: xf.rect(row), round: NO_ROUND, action: Action::Pick(i, m.on_select.clone()) });
            self.zones.push(Zone { id: rid, rect: xf.rect(row), round: NO_ROUND, cursor: Some(Cursor::Pointer), blocks: false, tip: false });
        }
    }

    fn spring(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let bouncy = self.look.theme.motion.bouncy;
        self.spring_with(id, slot, target, target, bouncy).x
    }

    /// A spring without overshoot, for press feedback.
    fn crisp(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let crisp = self.look.theme.motion.crisp;
        self.spring_with(id, slot, target, target, crisp).x
    }

    /// Like `spring`, but also returns the current velocity.
    fn spring_v(&mut self, id: u64, slot: u8, target: f32) -> (f32, f32) {
        let bouncy = self.look.theme.motion.bouncy;
        let s = self.spring_with(id, slot, target, target, bouncy);
        (s.x, s.v)
    }

    fn quad(&mut self, xf: &Xf, r: Rect, radius: f32, kind: f32) -> Quad {
        Quad {
            rect: xf.rect_px(r).arr(),
            clip: xf.clip.arr(),
            c0: [0.0; 4],
            c1: [0.0; 4],
            border: [0.0; 4],
            // A negative radius asks the shader for continuous corners (glass always has them).
            p0: [if self.smooth && kind != KIND_GLASS { -radius * xf.s } else { radius * xf.s }, 0.0, 0.0, kind],
            p1: [0.0, 1.0, if xf.clip_smooth { -xf.clip_r } else { xf.clip_r }, xf.opacity],
            np: [0.0, 0.0, 0.0, (u32::from(xf.clip_sq) | (u32::from(xf.shape_sq) << 4) | (u32::from(xf.border_hidden) << 10)) as f32],
            ..Zeroable::zeroed()
        }
    }

    fn fill(&mut self, xf: &Xf, r: Rect, radius: f32, fill: Fill, border: Option<(f32, Color)>) {
        let mut q = self.quad(xf, r, radius, KIND_FILL);
        match fill {
            Fill::Solid(c) => {
                q.c0 = c.arr();
                q.c1 = c.arr();
            }
            Fill::Linear(a, b, deg) => {
                let rad = deg.to_radians();
                q.c0 = a.arr();
                q.c1 = b.arr();
                q.p1[0] = rad.sin();
                q.p1[1] = -rad.cos();
            }
        }
        if let Some((w, c)) = border {
            q.border = c.arr();
            q.p0[1] = xf.line(w);
        }
        self.list.cmds.push(Cmd::Quad(q));
    }

    fn shadow(&mut self, xf: &Xf, r: Rect, radius: f32, sh: Shadow, knockout: bool) {
        let (off, spread) = ([sh.x * xf.s, sh.y * xf.s], sh.spread * xf.s);
        let mut q = if sh.inset {
            // Drawn on the caster; the shader cuts out the moved, shrunk shape.
            let mut q = self.quad(xf, r, radius, KIND_SHADOW);
            q.border[0] = 1.0;
            q
        } else {
            let at = Rect::new(r.x + sh.x, r.y + sh.y, r.w, r.h).inflate(sh.spread);
            self.quad(xf, at, (radius + sh.spread).max(0.0), KIND_SHADOW)
        };
        q.c0 = sh.color.arr();
        // c1: offset back to the caster, knock-out flag (keeps translucent casters clean), spread
        q.c1 = [off[0], off[1], if knockout && !sh.inset { 1.0 } else { 0.0 }, spread];
        q.p0[2] = sh.blur * xf.s * 0.5;
        self.list.cmds.push(Cmd::Quad(q));
    }

    fn glass(&mut self, xf: &Xf, r: Rect, radius: f32, g: Glass, border: Option<(f32, Color)>) {
        let mut q = self.quad(xf, r, radius, KIND_GLASS);
        q.c0 = g.tint.arr();
        q.c1 = [g.saturation, g.depth * xf.s, g.rim, g.dispersion];
        if g.horizontal_light { q.np[3] = ((q.np[3] as u32) | FLAG_HORIZONTAL_LIGHT) as f32; }
        q.p1[0] = g.frost * xf.s;
        q.p1[1] = g.refraction;
        if let Some((w, c)) = border {
            q.border = c.arr();
            q.p0[1] = xf.line(w);
        }
        self.list.cmds.push(Cmd::Quad(q));
    }

    /// Covers the outside of the rounded corners of `r` in `color`, except the corners in
    /// `square` (bit 0 top left, then clockwise): `.mask_corners()`, `.mask_corners().mask_under()`.
    fn paint_corner_mask(&mut self, mut xf: Xf, r: Rect, st: &crate::view::Style, (color, square): (Color, u8)) {
        xf.shape_sq = square | st.square_corners;
        self.smooth = st.smooth || st.glass.is_some();
        self.fill(&xf, r, st.radius, Fill::Solid(color), None);
        if let Some(Cmd::Quad(q)) = self.list.cmds.last_mut() {
            q.np[3] = ((q.np[3] as u32) | FLAG_CORNER_MASK) as f32;
        }
        self.smooth = false;
    }

    fn report_visible_rows(&mut self,id:u64,handler:Option<Box<dyn Fn(&mut S,std::ops::Range<usize>)>>,range:std::ops::Range<usize>) {
        if let Some(handler)=handler {
            let cache=self.text.lists.entry(id).or_default();
            if cache.reported_visible.as_ref()!=Some(&range) || cache.reported_frame+1<self.frame {
                cache.reported_visible=Some(range.clone());
                self.visible_events.push((handler,range));
                self.relayout=true;
            }
            cache.reported_frame=self.frame;
        }
    }

    fn paint(&mut self, v: View<S>, ln: &LNode, origin: (f32, f32), xf: Xf, id: u64) {
        let hidden = (v.style.reveal == Some(false) || matches!(&v.kind,Kind::Collapse {open:false,..})).then(||
            (self.hits.len(), self.zones.len(), self.layers.len(), self.bars.len(), self.shortcuts.len(), self.pointer_observers.len()));
        let saved_pin = self.sticky;
        let saved = self.enter_text_ctx(&v, id);
        self.paint_node(v, ln, origin, xf, id);
        self.sticky = saved_pin;
        if let Some((hits, zones, layers, bars, shortcuts, pointers)) = hidden {
            self.hits.truncate(hits);
            self.zones.truncate(zones);
            self.layers.truncate(layers);
            self.bars.truncate(bars);
            self.shortcuts.truncate(shortcuts);
            self.pointer_observers.truncate(pointers);
        }
        if let Some(t) = saved {
            self.tctx = t;
        }
    }

    fn paint_node(&mut self, mut v: View<S>, ln: &LNode, origin: (f32, f32), mut xf: Xf, id: u64) {
        let mut extra = v.extra.take().map(|b| *b);
        let mut split = extra.as_mut().and_then(|x| x.split.take());
        // Shortcuts count while their view is in the tree, on screen or not.
        if let Some(x) = &mut extra {
            self.shortcuts.append(&mut x.shortcuts);
        }
        let tap_menu = extra.as_ref().is_some_and(|x| matches!(x.pop, Some(crate::view::Pop { opens: crate::view::Opens::Tap, .. })));
        let pin = self.sticky;
        let tbase = std::mem::take(&mut self.tbase);
        let mut st = v.style;
        // A disabled control keeps its place and tooltip but loses its tap, hover and press.
        let disabled = st.disabled && (v.on_tap.is_some() || v.on_double_tap.is_some());
        if disabled {
            (v.on_tap, v.on_double_tap) = (None, None);
            (st.hover, st.hover_fg) = (None, None);
            let button=matches!(st.role,crate::view::Role::Button|crate::view::Role::SecondaryButton|crate::view::Role::PlainButton);
            if let Some((bg,fg))=self.look.theme.control.disabled_button_colors.filter(|_|button) {
                st.bg=if st.role==crate::view::Role::PlainButton {None}else{Some(bg.into())};
                st.fg=Some(fg);st.glass=None;st.border=None;st.shadow.fill(None);
            }else{xf.opacity *= self.look.theme.control.disabled_opacity;}
        }
        if st.animate_radius { st.radius = self.crisp(id, slot::RADIUS, st.radius.max(0.0)).max(0.0); }
        xf.shape_sq = st.square_corners;
        xf.border_hidden = st.border_hidden;
        let r = Rect::new(origin.0 + ln.pos.0, origin.1 + ln.pos.1, ln.size.0, ln.size.1);
        xf.opacity *= st.opacity;
        xf.text_reveal = st.text_reveal.or(xf.text_reveal);
        if st.hover_scope { xf.hover_scope=Some(self.hovered.contains(&id)); }
        if let Some((x,y))=st.hover_offset {
            let amount=self.crisp(id,slot::HOVER_OFFSET,if xf.hover_scope.unwrap_or(false) {1.0}else{0.0});
            xf.tx+=x*amount*xf.s;xf.ty+=y*amount*xf.s;
        }
        if let Some(factor)=st.hover_scale {
            let amount=self.crisp(id,slot::HOVER_OFFSET,if xf.hover_scope.unwrap_or(false){1.0}else{0.0}).clamp(0.0,1.0);
            xf.scale_about(r.x+r.w*0.5,r.y+r.h*0.5,1.0+(factor-1.0)*amount);
        }
        if let Some(deg) = st.rotation {
            let smooth = self.look.theme.motion.smooth;
            xf.rot += self.spring_with(id, slot::ROTATION, deg, deg, smooth).x.to_radians();
        }
        if let Some(fg) = st.fg {
            xf.fg = fg;
        }

        if let Some(shown) = st.reveal {
            let progress = self.spring(id, slot::REVEAL, if shown { 1.0 } else { 0.0 });
            let from = self.look.theme.motion.reveal_scale;
            xf.scale_about(r.x + r.w * 0.5, r.y + r.h * 0.5, from + (1.0 - from) * progress.max(0.0));
            xf.opacity *= progress.clamp(0.0, 1.0);
        }

        // Attachments follow layout, not the button's transient press squeeze. A moving
        // anchor can cross a half-pixel rounding boundary after the popup itself settles,
        // shifting every label/icon while its frame and separators stay in place.
        let anchor = xf.rect_px(r);
        // Press feedback: a quick, quiet dim with a barely-there squeeze. No bounce.
        if v.on_tap.is_some() || v.on_double_tap.is_some() || tap_menu {
            let p = self.crisp(id, slot::PRESS, if self.pressed == Some(id) { 1.0 } else { 0.0 });
            let motion = &self.look.theme.motion;
            let (scale, opacity) = (st.press_scale.unwrap_or(motion.press_scale), st.press_opacity.unwrap_or(motion.press_opacity));
            xf.scale_about(r.x + r.w * 0.5, r.y + r.h * 0.5, 1.0 - (1.0 - scale) * p);
            xf.opacity *= 1.0 - (1.0 - opacity) * p;
        }

        let sr = xf.rect_px(r);
        let margin = st.shadow.iter().flatten().map(Shadow::reach).fold(0.0, f32::max);
        if !sr.inflate(margin * xf.s).intersects(&xf.clip) || xf.opacity <= 0.001 {
            if let Kind::List {visible,..}=v.kind {self.report_visible_rows(id,visible,0..0);}
            return; // culled
        }
        if let Some(handler) = extra.as_mut().and_then(|x|x.pointer.take()) {
            self.pointer_observers.push(PointerObserver {rect:xf.rect(r),scale:xf.s,handler});
        }

        // Clear only the underlying scene at the corners. Painting this before the
        // surface keeps its shadow continuous across the masked backdrop.
        if let Some(mask) = st.backdrop_corner_mask {
            self.paint_corner_mask(xf, r, &st, mask);
        }
        self.smooth = st.smooth;
        let translucent = st.glass.is_some() || !matches!(st.bg, Some(Fill::Solid(c)) if c.a >= 1.0);
        for sh in st.shadow.into_iter().flatten() {
            self.shadow(&xf, r, st.radius, sh, translucent);
        }
        // A container draws its border last, over its children, its hover and (for glass) its
        // background: a child background running to the edge (a sidebar in a rounded panel)
        // would cover it otherwise.
        let late_border = st
            .border
            .filter(|_| st.clip || ln.clip || st.hover.is_some() || st.glass.is_some() || st.border_fade.is_some() || !ln.children.is_empty())
            .map(|b| (xf, b));
        // Inner shadows lie over the children too, like the border.
        let inset = st.inset.map(|sh| (xf, sh));
        let corner_mask = st.corner_mask.map(|mask| (xf, mask));
        if let Some(g) = st.glass {
            self.glass(&xf, r, st.radius, g, st.border.filter(|_| late_border.is_none()));
            if let Some(Cmd::Quad(q)) = self.list.cmds.last_mut() { q.p0[2] = st.glass_fade * xf.s; }
        }
        match (st.bg, st.border.filter(|_| late_border.is_none()), st.glass) {
            (Some(bg), border, None) => self.fill(&xf, r, st.radius, bg, border),
            (Some(bg), _, Some(_)) => self.fill(&xf, r, st.radius, bg, None),
            (None, Some(b), None) => self.fill(&xf, r, st.radius, Fill::Solid(Color::CLEAR), Some(b)),
            _ => {}
        }

        // Pointer shape and hover: explicit, or what the kind of view suggests.
        let cursor = st.cursor.or(match &v.kind {
            // Not the hand of a tappable view underneath.
            _ if disabled => Some(Cursor::Default),
            Kind::TextField(_) => Some(Cursor::Text),
            Kind::Toggle(..) | Kind::Slider(..) | Kind::Tabs { .. } | Kind::Picker { .. } => Some(Cursor::Pointer),
            _ if v.on_tap.is_some() || v.on_double_tap.is_some() || tap_menu => Some(Cursor::Pointer),
            _ => None,
        });
        if cursor.is_some() || st.hover.is_some() || st.hover_fg.is_some() || st.hover_scope || st.hover_marquee.is_some() {
            self.zone(&xf, id, sr, cursor);
        }
        if st.hover.is_some() || st.hover_fg.is_some() {
            let t = self.crisp(id, slot::HOVER, if self.hovered.contains(&id) { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
            if let Some(c) = st.hover.filter(|_| t > 0.004) {
                self.fill(&xf, r, st.radius, Fill::Solid(c.alpha(c.a * t)), None);
            }
            if let Some(c) = st.hover_fg {
                // Text and template icons inside take the color with the highlight.
                xf.fg = xf.fg.lerp(c, t);
            }
        }
        self.smooth = false;
        if st.clip || ln.clip {
            // Inside the border: children never show through or under it.
            let bw = st.border.map_or(0.0, |b| xf.line(b.0));
            let edge = |bit| if st.border_hidden & bit == 0 { bw } else { 0.0 };
            let (top, right, bottom, left) = (edge(1), edge(2), edge(4), edge(8));
            let inside = Rect::new(sr.x + left, sr.y + top, (sr.w-left-right).max(0.0), (sr.h-top-bottom).max(0.0));
            xf.clip_to_corners(inside, (st.radius * xf.s - bw).max(0.0), st.smooth, st.square_corners);
        }

        self.area_hit(&xf, id, sr);
        // Before the tap: a view that is both draggable and tappable keeps its tap.
        if st.drag {
            self.hit(&xf, id ^ 0xD6A9, sr, Action::DragWindow);
        }
        if v.on_tap.is_some() || v.on_double_tap.is_some() {
            self.hit(&xf, id, sr, Action::Tap(v.on_tap, v.on_double_tap));
        } else if disabled {
            self.hit(&xf, id, sr, Action::Swallow);
        }
        if let Some(x) = extra {
            self.attach(x, ln, &xf, id, sr, anchor);
        }

        let pad = st.padding;
        // Where the content goes: padding works the same on every kind of view, leaves
        // (toggle, slider, menu, tabs, picture) included.
        let cr = Rect::new(r.x + pad[0], r.y + pad[1], (r.w - pad[0] - pad[2]).max(0.0), (r.h - pad[1] - pad[3]).max(0.0));
        let csr = if pad == [0.0; 4] { sr } else { xf.rect_px(cr) };
        match v.kind {
            Kind::Empty | Kind::Spacer => {}
            Kind::Progress { value, ring } => {
                let smooth = self.look.theme.motion.smooth;
                let v = self.spring_with(id, slot::PROGRESS, value, value, smooth).x.clamp(0.0, 1.0);
                let (track, fill, stroke) = {
                    let t = &self.look.theme;
                    (t.color.fill_strong, st.fg.unwrap_or(t.color.accent), t.control.progress_ring.1)
                };
                // The fill grows on a spring: drawn as is, not in pixel steps.
                let fxf = Xf { px: 0.0, ..xf };
                if ring {
                    let d = cr.w.min(cr.h);
                    let stroke = stroke.min(d * 0.5);
                    let o = (cr.x + (cr.w - d) * 0.5, cr.y + (cr.h - d) * 0.5);
                    self.fill(&xf, Rect::new(o.0, o.1, d, d), d * 0.5, Fill::Solid(Color::CLEAR), Some((stroke, track)));
                    if v > 0.0 && stroke > 0.0 {
                        // The arc is a row of round dots a quarter stroke apart (no arc in the
                        // shader): the scallops between them stay far below a pixel.
                        let radius = (d - stroke) * 0.5;
                        let center = (o.0 + d * 0.5, o.1 + d * 0.5);
                        let arc = v * std::f32::consts::TAU;
                        let n = ((arc * radius / (stroke * 0.25)).ceil() as usize).clamp(1, 512);
                        for i in 0..=n {
                            let a = arc * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                            let dot = Rect::new(center.0 + a.cos() * radius - stroke * 0.5, center.1 + a.sin() * radius - stroke * 0.5, stroke, stroke);
                            self.fill(&fxf, dot, stroke * 0.5, Fill::Solid(fill), None);
                        }
                    }
                } else {
                    let h = cr.h;
                    self.fill(&xf, cr, h * 0.5, Fill::Solid(track), None);
                    if v > 0.0 {
                        // Starts as a dot, so the rounded ends never pinch.
                        let w = h + (cr.w - h).max(0.0) * v;
                        self.fill(&fxf, Rect::new(cr.x, cr.y, w, h), h * 0.5, Fill::Solid(fill), None);
                    }
                }
            }
            Kind::Activity {stroke,period} => {
                let diameter=(r.w-pad[0]-pad[2]).min(r.h-pad[1]-pad[3]).max(0.0);
                let stroke=stroke.min(diameter);
                if diameter>0.0 && stroke>0.0 && xf.fg.a>0.001 {
                    // Overlapping subpixel discs form a smooth tapered ring without a texture,
                    // renderer dependency or a per-frame SVG allocation.
                    let radius=(diameter-stroke)*0.5;
                    let center=(r.x+pad[0]+diameter*0.5,r.y+pad[1]+diameter*0.5);
                    let turn=if period>0.0 {self.time/period}else{0.0};
                    let segments=((std::f32::consts::TAU*radius/stroke*2.0).ceil() as usize).clamp(24,128);
                    let smooth=Xf {px:0.0,..xf};
                    for step in 0..segments {
                        let part=step as f32/segments as f32;
                        let angle=(turn+part)*std::f32::consts::TAU;
                        let color=xf.fg.alpha(xf.fg.a*(0.12+0.88*part));
                        self.fill(&smooth,Rect::new(center.0+angle.cos()*radius-stroke*0.5,center.1+angle.sin()*radius-stroke*0.5,stroke,stroke),stroke*0.5,Fill::Solid(color),None);
                    }
                    self.spin|=period>0.0;
                }
            }
            Kind::List { count, row_h, row, visible, .. } => {
                // Only the rows inside the clip are built, laid out and painted. Their real
                // heights are remembered so the scroll range converges on the truth.
                let top = (xf.clip.y - xf.ty) / xf.s;
                let bottom = top + xf.clip.h / xf.s;
                let prev = self.text.lists.get(&id).map_or((0, 0), |c| c.seen);
                let known = |t: &TextSystem, i: usize| t.lists.get(&id).and_then(|c| c.heights.get(i)).copied().unwrap_or(f32::NAN);
                let est = |h: f32| if h.is_nan() { row_h } else { h };
                let scroll = self.tctx.scroll.map(|s| s.0);
                // The anchor: the first row on screen that was on screen last frame too (its
                // height is known), so rows coming in above it at another height than
                // estimated grow upwards instead of pushing what is being read.
                let (mut first, mut y) = (count, r.y);
                let mut anchor = None;
                for i in 0..count {
                    let h = est(known(self.text, i));
                    if y >= bottom {
                        break;
                    }
                    if y + h > top {
                        first = first.min(i);
                        if anchor.is_none() && y >= top && (scroll.is_none() || (prev.0..prev.1).contains(&i)) {
                            anchor = Some((i, y));
                        }
                    }
                    y += h;
                }
                let (j, ay) = match anchor {
                    Some(a) => a,
                    // Nothing seen before is on screen (a jump): plain top-down from the first.
                    None => (first, r.y + (0..first).map(|i| est(known(self.text, i))).sum::<f32>()),
                };
                let measure = |p: &mut Self, i: usize| {
                    let mut rv = row(i);
                    crate::theme::apply(&mut rv, p.look);
                    let cid = child_id(id, i, rv.key);
                    let mut lx = Lx { text: p.text, look: p.look, springs: p.springs, frame: p.frame, resizing: p.resizing, along: None, menu: p.open_menu, tip: p.tip, shift: 0.0, tail_viewport: None };
                    let n = layout(&rv, (Some(r.w), None), &mut lx, cid);
                    (i, rv, n, cid)
                };
                let mut rows = Vec::new();
                // Downwards from the anchor…
                let mut y = ay;
                let mut i = j;
                while i < count && y < bottom {
                    let m = measure(self, i);
                    let h = m.2.size.1;
                    rows.push((y, m));
                    y += h;
                    i += 1;
                }
                // …and upwards from it, keeping the anchor in place.
                let (mut y, mut i, mut shift) = (ay, j, 0.0);
                while i > 0 && y > top && scroll.is_some() {
                    i -= 1;
                    let m = measure(self, i);
                    let h = m.2.size.1;
                    shift += h - est(known(self.text, i));
                    y -= h;
                    rows.push((y, m));
                }
                if shift.abs() > 0.01 {
                    if let Some(s) = scroll.and_then(|sid| self.scrolls.get_mut(&sid)) {
                        s.offset += shift;
                        s.max += shift;
                    }
                }
                rows.sort_by_key(|r| r.1 .0);
                let clip=self.visible_clip.map_or(xf.clip,|clip|clip.intersect(&xf.clip));
                let visible_top=(clip.y-xf.ty)/xf.s;
                let visible_bottom=visible_top+clip.h/xf.s;
                let mut visible_range=count..0;
                let mut seen = (usize::MAX, 0, usize::MAX);
                for (y, (i, rv, n, cid)) in rows {
                    let known = known(self.text, i);
                    if known.is_nan() || (n.size.1 - known).abs() > 0.5 {
                        if let Some(slot) = self.text.lists.get_mut(&id).and_then(|c| c.heights.get_mut(i)) {
                            *slot = n.size.1;
                            self.relayout = true;
                        }
                    }
                    seen = (seen.0.min(i), seen.1.max(i + 1), if y >= top { seen.2.min(i) } else { seen.2 });
                    if clip.w>0.0 && clip.h>0.0 && y+n.size.1>visible_top && y<visible_bottom {
                        visible_range.start=visible_range.start.min(i);
                        visible_range.end=visible_range.end.max(i+1);
                    }
                    let saved_row = self.tctx.row;
                    self.enter_row(id, i, count);
                    self.path.push((id, i));
                    self.paint(rv, &n, (r.x, y), xf, cid);
                    self.path.pop();
                    self.tctx.row = saved_row;
                }
                if visible_range.is_empty(){visible_range=0..0;}
                self.report_visible_rows(id,visible,visible_range);
                if let Some(c) = self.text.lists.get_mut(&id) {
                    (c.seen, c.anchor) = if seen.0 == usize::MAX { ((0, 0), 0) } else { ((seen.0, seen.1), seen.2.min(seen.1)) };
                }
            }
            Kind::Overlay { open, mode, child, on_dismiss, style } => {
                let target = if open { 1.0 } else { 0.0 };
                let t = if mode == OverlayMode::Modal { self.fade(id, target) }
                    else { self.spring_from(id, slot::OVERLAY, 0.0, target) };
                if t <= 0.004 && !open {
                    return;
                }
                let tc = t.clamp(0.0, 1.0);
                let cn = &ln.children[0];
                let (w, h) = cn.size;
                let cx = r.x + (r.w - w) * 0.5;
                let mut cxf = xf;
                let pos = match mode {
                    OverlayMode::Modal => {
                        // Dialogs fade and rise at their final size.
                        cxf.opacity *= tc;
                        (cx, r.y + (r.h - h) * 0.5 + style.rise * (1.0 - tc))
                    }
                    OverlayMode::Sheet => (cx, r.y + r.h - h + (1.0 - t) * (h + 24.0)),
                    OverlayMode::Drawer => (r.x - (1.0 - t) * (w + 24.0), r.y),
                    OverlayMode::Toast => {
                        cxf.opacity *= tc;
                        (cx, r.y + r.h - h - 100.0 + (1.0 - t) * 40.0)
                    }
                };
                // Slides as one piece in whole pixels: its parts keep their sizes on the way.
                let pos = cxf.at_px(pos);
                // Bars of what lies underneath go under the scrim with it.
                self.flush_bars();
                if mode != OverlayMode::Toast {
                    if let Some(mut glass) = style.backdrop {
                        // Fade both the blur radius and its contribution; the dialog is
                        // painted afterwards and remains crisp throughout the transition.
                        glass.frost *= tc;
                        let mut backdrop_xf = xf;
                        backdrop_xf.opacity *= tc;
                        self.glass(&backdrop_xf, r, 0.0, glass, None);
                    }
                    let scrim = self.look.theme.color.scrim;
                    self.fill(&xf, r, 0.0, Fill::Solid(scrim.alpha(scrim.a * tc)), None);
                    self.hit(&xf, id, sr, Action::Block(on_dismiss));
                    self.zones.push(Zone {
                        id, rect: sr.intersect(&xf.clip), round: (xf.clip, xf.clip_r),
                        cursor: None, blocks: true, tip: false,
                    });
                }
                let body = cxf.rect(Rect::new(pos.0, pos.1, w, h));
                self.hits.push(Hit { id: child_id(id, 1, None), rect: body, round: NO_ROUND, action: Action::Swallow });
                let cid = child_id(id, 0, child.key);
                self.tbase = tbase;
                self.paint(*child, cn, pos, cxf, cid);
            }
            Kind::Collapse {child,..} => {
                let p=ln.collapse_progress;
                if p>0.001 {
                    let mut child_xf=xf;
                    child_xf.clip_to(xf.rect(cr),0.0,false);
                    child_xf.opacity*=p;
                    child_xf.ty-=8.0*(1.0-p)*xf.s;
                    child_xf.blur=1.6*(1.0-p);
                    let cid=child_id(id,0,child.key);
                    self.paint(*child,&ln.children[0],(cr.x,cr.y),child_xf,cid);
                }
            }
            Kind::AnimatedIcon { progress, hover, cycle, duration, frame } => {
                let target=if xf.hover_scope.unwrap_or_else(||self.hovered.contains(&id)){hover.unwrap_or(progress)}else{progress};
                let p=if let Some(duration)=duration {
                    let target=target.clamp(0.0,1.0);
                    let timeline=self.icon_timelines.entry(id).or_insert(IconTimeline{value:target,target,at:self.now,used:self.frame});
                    if timeline.target!=target {timeline.target=target;timeline.at=self.now;}
                    let step=self.now.duration_since(timeline.at).as_secs_f32().min(0.05)/duration;
                    timeline.value+=(target-timeline.value).clamp(-step,step);
                    timeline.at=self.now;timeline.used=self.frame;
                    if (target-timeline.value).abs()>0.0001 {self.spin=true;}else{timeline.value=target;}
                    timeline.value
                }else if let Some(period)=cycle {
                    let phase=target.rem_euclid(period);
                    let target=self.springs.get(&(id,slot::ICON_PROGRESS)).map_or(phase,|s| {
                        let delta=(phase-s.target.rem_euclid(period)).rem_euclid(period);
                        s.target+if delta<0.0001||period-delta<0.0001{0.0}else{delta}
                    });
                    self.crisp(id,slot::ICON_PROGRESS,target)
                }else{self.crisp(id, slot::ICON_PROGRESS, target).clamp(0.0, 1.0)};
                let img = frame(p);
                self.picture(&xf, cr, st.radius, &img, Some(xf.fg), None);
            }
            Kind::Image(img, template) => {
                // Padded, the frame (background, border) is the node's and the picture sits inside.
                if pad == [0.0; 4] {
                    self.picture(&xf, r, st.radius, &img, template.then_some(xf.fg), st.border);
                } else {
                    let k = (st.radius - pad[0].max(pad[1])).max(0.0);
                    self.picture(&xf, cr, k, &img, template.then_some(xf.fg), None);
                }
            }
            Kind::TextField(f) => self.paint_field(f, &st, r, sr, xf, id),
            Kind::Tabs { labels, icons, icon_frames, disabled, selected, on_select, .. } => {
                let (tabs_pad, drop_tint, lift) = (self.look.theme.control.tab_padding, self.look.theme.control.tab_drop, self.look.theme.shadow.lift);
                let n=labels.len();
                if n==0 {return;}
                let selected=selected.min(n-1);
                let r=cr;
                let widths=&ln.tab_widths;
                let mut edge=r.x+tabs_pad;
                let mut starts=Vec::with_capacity(n);
                let mut ends=Vec::with_capacity(n);
                for &width in widths {starts.push(edge);edge+=width;ends.push(edge);}
                let center=|i:usize|starts[i]+widths[i]/2.0;
                let held = self.pressed == Some(id);
                // The drop rests on the selected tab; while held it follows the pointer.
                let target = if held {
                    ((self.pointer_x - xf.tx) / xf.s).clamp(center(0), center(n - 1))
                } else {
                    center(selected)
                };
                let target_width=if held {
                    let right=(0..n).find(|&i|center(i)>=target).unwrap_or(n-1);
                    let left=right.saturating_sub(1);
                    let fraction=if left==right{0.0}else{((target-center(left))/(center(right)-center(left))).clamp(0.0,1.0)};
                    widths[left]+(widths[right]-widths[left])*fraction
                }else{widths[selected]};
                let tab_w=self.spring(id,slot::TAB_WIDTH,target_width).max(1.0);
                let (cx, vel) = self.spring_v(id, slot::TAB_DROP, target);
                let p = self.spring(id, slot::TAB_HELD, if held { 1.0 } else { 0.0 });
                // Liquid: it swells when grabbed and stretches along its motion.
                let stretch = (vel.abs() * 0.035).min(tab_w * 0.6);
                let lw = tab_w * (1.0 + 0.14 * p) + stretch;
                let lh = (r.h - 2.0 * tabs_pad) * (1.0 + 0.42 * p) * (1.0 - 0.18 * stretch / (tab_w * 0.6));
                let lens = Rect::new(cx - lw * 0.5, r.y + (r.h - lh) * 0.5, lw, lh);
                if p > 0.02 {
                    let sh = Shadow { color: lift.color.alpha(lift.color.a * p), ..lift };
                    self.shadow(&Xf { px: 0.0, ..xf }, lens, lh * 0.5, sh, true);
                }
                // At the top/bottom rim the refracted ray must not reach the glyphs:
                // otherwise separate RGB samples pull isolated coloured dots into the rim.
                // Size the optical bevel from the actual vertical text/icon clearance.
                let label_height=ln.texts.iter().map(|(_,size)|size.1).fold(0.0f32,f32::max)
                    .max(if icons.is_empty(){0.0}else{self.look.theme.control.tab_icon});
                let optical_depth=((lh-label_height).max(0.0)*0.3).clamp(1.0,6.0);
                let drop = Glass {
                    tint: drop_tint.alpha(drop_tint.a * (1.0 - 0.7 * p.clamp(0.0, 1.0))),
                    saturation: 1.25,
                    frost: 0.0,
                    refraction: 1.0,
                    depth: optical_depth,
                    dispersion: 0.6,
                    rim: 0.55,
                    horizontal_light: false,
                };
                // Labels form part of the lens backdrop: its rim refracts the letters,
                // while zero frost keeps the center readable. Never draw a sharp duplicate.
                for (i, (key, ts)) in ln.texts.iter().enumerate() {
                    let near = (1.0 - (center(i) - cx).abs() / widths[i]).clamp(0.0, 1.0);
                    let unavailable=disabled.contains(&i);
                    let color=if unavailable {
                        self.look.theme.control.disabled_button_colors.map(|(_,fg)|fg).unwrap_or(xf.fg.alpha(self.look.theme.control.disabled_opacity))
                    }else{xf.fg.alpha(xf.fg.a*(0.6+0.4*near))};
                    let cell=Rect::new(starts[i],r.y+tabs_pad,widths[i],r.h-2.0*tabs_pad);
                    if unavailable {
                        if let Some((bg,_))=self.look.theme.control.disabled_button_colors {
                            self.fill(&xf,cell,cell.h/2.0,Fill::Solid(bg),None);
                        }
                        self.zone(&xf,id,xf.rect(cell),Some(Cursor::Default));
                    }
                    let mut label_xf=xf;
                    label_xf.clip_to(xf.rect(cell),0.0,false);
                    let mut x = center(i) - ts.0 * 0.5;
                    if let Some(img) = icons.get(i) {
                        let g = self.look.theme.control.tab_icon;
                        x = center(i) - (g * 1.5 + ts.0) * 0.5;
                        let animated=icon_frames.get(i).map(|frame| {
                            let progress=self.crisp(child_id(id,i,None),slot::ICON_PROGRESS,if i==selected{1.0}else{0.0});
                            frame(progress.clamp(0.0,1.0))
                        });
                        self.picture(&label_xf, Rect::new(x, r.y + (r.h - g) * 0.5, g, g), 0.0, animated.as_ref().unwrap_or(img), Some(color), None);
                        x += g * 1.5;
                    }
                    self.label(&label_xf, *key, (x, r.y + (r.h - ts.1) * 0.5), *ts, color);
                }
                if !disabled.contains(&selected) {
                    self.glass(&Xf {px:0.0,..xf},lens,lh*0.5,drop,None);
                }
                self.hit(&xf,id,csr.inflate(4.0),Action::Tabs {
                    ends:ends.iter().map(|x|x*xf.s+xf.tx).collect(),disabled,on_select,
                });
            }
            Kind::Picker { options, selected, on_select, label } => {
                let text_style=label.text_style();
                let inner=cr;
                let ts=ln.texts[0].1;
                let mut pxf=xf;
                pxf.clip_to(xf.rect(inner),0.0,false);
                self.label(&pxf,ln.text,(inner.x,inner.y+(inner.h-ts.1)*0.5),ts,xf.fg);
                let spec=TextSpec {text:"⌄",weight:700,..spec_in(&text_style,"")};
                let (chev,cs)=self.text.measure(&spec,None);
                // The down-chevron glyph sits below the baseline; align its ink, not the
                // font's full line box, to the control's vertical center.
                let at=(inner.x+inner.w-cs.0,inner.y+(inner.h-cs.1)*0.5-text_style.size*0.18);
                self.label(&pxf,chev,at,cs,xf.fg.alpha(xf.fg.a*0.6));
                self.hit(&xf,id,sr,Action::OpenMenu);
                if self.open_menu==Some(id) {
                    self.menu=Some(Menu {id,anchor,options,selected,on_select,fg:xf.fg,text_style});
                }
            }
            Kind::Text(t) => {
                let mut at = xf.at_px((r.x + pad[0], r.y + pad[1]));
                let mut inner = Rect::new(at.0, at.1, r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                let box_ = xf.rect(inner);
                // Text limited to some lines is cut to its box; the last line may fade out.
                let mut clip = if t.lines > 0 { xf.clip.intersect(&box_.inflate(1.0)) } else { xf.clip };
                let mut fade = (ln.fade && t.truncate == Truncate::Fade).then(|| {
                    let line_h = (t.size * t.line_height).round() * xf.s;
                    let w = (self.look.theme.control.fade_width * xf.s).min(box_.w * 0.5);
                    Rect::new(box_.x + box_.w - w, box_.y + box_.h - line_h, w, line_h)
                });
                let mut leading_fade = 0.0;
                if let Some((speed,inset,fade_width))=st.hover_marquee {
                    let fade_width=fade_width.unwrap_or(self.look.theme.control.fade_width);
                    let hovered=xf.hover_scope.unwrap_or_else(||self.hovered.contains(&id));
                    let width=(inner.w-if hovered {inset}else{0.0}).max(0.0);
                    let distance=(ln.marquee_width-width).max(0.0);
                    let (offset,running)=self.marquees.offset(id,ln.text,distance,speed,hovered,self.time,self.frame,self.look.theme.motion.marquee_return);
                    self.spin|=running;
                    let leading=st.hover_marquee_leading_outset;
                    let visible=xf.rect(Rect::new(inner.x-leading,inner.y,width+leading,inner.h));
                    clip=xf.clip.intersect(&visible);
                    leading_fade = (leading.min(fade_width) * xf.s).min(clip.w);
                    fade=(offset<distance-0.5 && fade_width>0.0).then(|| {
                        let width=(fade_width*xf.s).min(visible.w);
                        Rect::new(visible.x+visible.w-width,visible.y,width,visible.h)
                    });
                    at.0-=offset;
                    inner.x-=offset;
                    inner.w=ln.marquee_width;
                }
                let mut dxf = xf;
                dxf.clip = clip;
                self.text_extras(&dxf, id, ln.text, &t.spans, &t.content, (inner.x, inner.y), (inner.w, inner.h), tbase);
                self.span_decor(&dxf, ln.text, &t.spans, (inner.x, inner.y));
                let (pos, scale) = xf.text_at(at, (inner.w, inner.h));
                let revealed = (xf.blur>0.001 || (xf.text_reveal.is_some_and(|reveal| reveal.duration > 0.0) && fade.is_none()))
                    && self.paint_reveal(&dxf, id, ln.text, &t.spans, &t.content, pos, scale);
                if !revealed {
                    self.list.cmds.push(Cmd::Text(TextCmd {
                        key: ln.text,
                        pos,
                        size: (inner.w*xf.s, box_.h),
                        scale,
                        color: xf.fg.alpha(xf.fg.a * xf.opacity),
                        clip,
                        fade,
                        leading_fade,
                    }));
                }
                if let Some(cursor)=st.text_cursor {
                    let shaped=crate::text::shaped_text(&t.spans,&t.content);
                    let index=shaped.char_indices().last().map_or(0,|(index,_)|index);
                    let (x,y,height)=self.text.caret(ln.text,index);
                    let target=cursor.size*scale;
                    let at=(pos.0+(x+cursor.gap)*scale,pos.1+y*scale);
                    let area=Rect::new(at.0,at.1,cursor.reserve*scale,height*scale);
                    if area.intersects(&clip) && xf.opacity>0.0 {
                        let size=self.crisp(id,slot::CURSOR,target).max(0.0);
                        let pose=cursor::pose(self.time,cursor.cycle_period,cursor.transition_duration);
                        self.spin|=pose.animate;
                        if let Some(delay)=pose.wake {self.wake=Some(self.wake.map_or(delay,|wake|wake.min(delay)));}
                        let draw_size=size*pose.scale;
                        let marker=Rect::new(at.0+(size-draw_size)*0.5,at.1+(height*scale-draw_size)*0.5,draw_size,draw_size);
                        let cxf=Xf {s:1.0,tx:0.0,ty:0.0,px:0.0,clip,opacity:xf.opacity,..xf};
                        if cursor.cycle_period>0.0 && cursor.transition_duration>0.0 {
                            let mut quad=self.quad(&cxf,marker,cursor.radius*scale,KIND_MORPH);
                            quad.c0=cursor.color.arr();quad.p1[0]=pose.shape;quad.p1[1]=pose.rotation;
                            self.list.cmds.push(Cmd::Quad(quad));
                        } else {self.fill(&cxf,marker,cursor.radius*scale,Fill::Solid(cursor.color),None);}
                    }
                }
                if let Some((period, highlight)) = st.text_shimmer.filter(|_| box_.intersects(&clip) && xf.opacity > 0.0 && !crate::text::shaped_text(&t.spans, &t.content).is_empty()) {
                    // Reuse the shaped run and atlas. Narrow clips tint only the glyphs under
                    // the moving band; neither hit testing nor text selection is duplicated.
                    self.spin = true;
                    let band = (box_.w * 0.6).max(t.size * xf.s);
                    let center = box_.x - band + (self.time / period).fract() * (box_.w + 2.0 * band);
                    let stripe = (2.0 * band / 32.0).max(1.0 / self.px.max(1.0));
                    for index in 0..32 {
                        let x = center - band + index as f32 * stripe;
                        let mask = Rect::new(x, box_.y, stripe, box_.h).intersect(&clip).intersect(&box_);
                        if mask.w <= 0.0 || mask.h <= 0.0 { continue; }
                        let distance = ((x + stripe * 0.5 - center) / band).abs();
                        let alpha = (1.0 - distance * distance).max(0.0).powi(3);
                        self.list.cmds.push(Cmd::Text(TextCmd {
                            key: ln.text, pos, size: (box_.w, box_.h), scale,
                            color: highlight.alpha(highlight.a * xf.opacity * alpha), clip: mask, fade, leading_fade,
                        }));
                    }
                }
            }
            Kind::Stack { children, .. } if split.is_some() && children.len() == 2 => {
                let sp = split.take().expect("guarded");
                self.paint_split(sp, children, ln, (r, cr), xf, id, tbase);
            }
            Kind::Stack { children, axis, .. } => {
                let pin = pin.filter(|_| axis == Axis::V && children.iter().any(|c| c.style.sticky));
                let mut stuck = Vec::new();
                // Text numbers follow the tree, not the paint order.
                let bases: Vec<u32> = ln.children.iter().scan(tbase, |b, n| Some(std::mem::replace(b, *b + n.tn))).collect();
                for (i, (c, n)) in children.into_iter().zip(&ln.children).enumerate() {
                    if pin.is_some() && c.style.sticky {
                        stuck.push((i, c)); // painted last, above the rows that slide under it
                        continue;
                    }
                    let cid = child_id(id, i, c.key);
                    self.tbase = bases[i];
                    self.paint(c, n, (r.x, r.y), xf, cid);
                }
                let tops: Vec<f32> = stuck.iter().map(|(i, _)| r.y + ln.children[*i].pos.1).collect();
                for (k, (i, c)) in stuck.into_iter().enumerate() {
                    let n = &ln.children[i];
                    // Pinned to the top of the viewport, pushed out by the next sticky one.
                    let mut y = tops[k].max(pin.unwrap_or(tops[k]));
                    y = y.min((r.y + r.h - pad[3] - n.size.1).max(tops[k]));
                    if let Some(next) = tops.get(k + 1) {
                        y = y.min(next - n.size.1);
                    }
                    let cid = child_id(id, i, c.key);
                    self.tbase = bases[i];
                    self.paint(c, n, (r.x, r.y + y - tops[k]), xf, cid);
                }
            }
            Kind::Scroll { child, refresh, indicator, stick, bar_insets, bottom_overlay } => {
                let content = ln.content_h + pad[1] + pad[3];
                let (frame, now) = (self.frame, self.now);
                let s = self.scrolls.entry(id).or_default();
                let max = (content - r.h).max(0.0);
                let parked = s.offset >= s.max - 1.0 && s.vel == 0.0;
                if ln.shift != 0.0 && s.used != 0 && s.navigation.is_none() {
                    // A lazy list inside re-wrapped for a new width: same rows on screen.
                    let (lo, hi) = s.bounds();
                    let inside = s.offset >= lo && s.offset <= hi;
                    s.offset += ln.shift;
                    if inside {
                        s.offset = s.offset.clamp(lo, max.max(lo));
                    }
                }
                // Chat-style: start at the end and follow new content while parked there.
                // Shrinking too: a lazy row measured smaller than its estimate must not
                // leave the view hanging past the end and bouncing back.
                if stick && s.navigation.is_none() && (s.used == 0 || (parked && max != s.max)) {
                    s.offset = max;
                }
                s.max = max;
                if self.resizing {
                    let (lo, hi) = s.bounds();
                    s.offset = s.offset.clamp(lo, hi);
                    s.vel = 0.0;
                }
                fn row_target(n:&LNode,y:f32)->Option<(u64,f32)> {
                    let y=y+n.pos.1;
                    n.row_target.map(|(request,offset)|(request,y+offset))
                        .or_else(||n.children.iter().find_map(|child|row_target(child,y)))
                }
                if let Some((request,target))=row_target(&ln.children[0],0.0) {
                    if s.request!=Some(request) {
                        s.request=Some(request);s.navigation=Some((s.offset,now));
                        s.vel=0.0;s.pad=Pad::Idle;s.wheel_at=None;
                    }
                    if let Some((from,started))=s.navigation {
                        let p=((now-started).as_secs_f32()/0.42).clamp(0.0,1.0);
                        let ease=p*p*(3.0-2.0*p);
                        s.offset=from+(target.clamp(0.0,max)-from)*ease;
                        s.active_at=Some(now);
                        if p>=1.0 {s.navigation=None;}else{self.spin=true;}
                    }
                }else{s.navigation=None;}
                s.used = frame;
                s.refreshing = refresh.as_ref().is_some_and(|r| r.0);
                let (offset, over, refreshing, active_at) = (s.offset, s.over(), s.refreshing, s.active_at);
                let (lo, hi) = s.bounds();
                let has_refresh = refresh.is_some();
                self.hit(&xf, id, sr, Action::Scroll { refresh: refresh.map(|r| r.1) });
                let mut cxf = xf;
                cxf.clip_to_corners(sr, st.radius * xf.s, st.smooth, st.square_corners);
                let cid = child_id(id, 0, child.key);
                self.sticky = Some(r.y + pad[1]);
                self.tbase = tbase;
                let outer = self.tctx.clone();
                self.enter_scroll(id, sr);
                // The physics stays fractional; the content is drawn whole pixels off, so text
                // stays sharp and hairlines don't flicker while it moves.
                let top = cxf.at_px((r.x, r.y - offset)).1;
                let outer_visible=self.visible_clip;
                let overlay_h=ln.children.get(1).map_or(0.0,|overlay|overlay.size.1);
                let visible=xf.rect(Rect::new(r.x,r.y,r.w,(r.h-overlay_h).max(0.0))).intersect(&cxf.clip);
                self.visible_clip=Some(outer_visible.map_or(visible,|outer|outer.intersect(&visible)));
                self.paint(*child, &ln.children[0], (r.x, top), cxf, cid);
                self.visible_clip=outer_visible;
                self.tctx = outer;
                self.sticky = None;

                // Pull-to-refresh: the indicator lives in the gap the pull opens.
                let pull = -offset;
                if has_refresh && pull > 8.0 {
                    let progress = ((pull - 8.0) / (REFRESH_AT - 8.0)).min(1.0);
                    self.spinner(&cxf, (r.x + r.w * 0.5, r.y + pad[1] + pull * 0.5), progress, refreshing);
                }

                // Overlay scroll bar, as on macOS: shows while scrolling or when the pointer
                // comes to the edge, widens with a track under the pointer, and its thumb can
                // be dragged; fades out when idle. Painted after the rest of its layer (see
                // `flush_bars`), so nothing laid over the scroll view hides it.
                let bar_id = id ^ BAR_SALT;
                let grabbed = self.bar_drag == Some(id);
                let near = self.hovered.contains(&bar_id);
                let idle = active_at.map_or(f32::MAX, |t| (now - t).as_secs_f32());
                let scrolls = hi > lo && indicator != Indicator::Never;
                let show = scrolls && (indicator == Indicator::Always || idle < 1.2 || near || grabbed);
                if show && indicator == Indicator::Auto && !near && !grabbed {
                    self.wake = Some(self.wake.map_or(1.22 - idle, |w| w.min(1.22 - idle)));
                }
                let a = self.spring(id, slot::BAR_SHOW, if show { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                let wide = self.crisp(id, slot::BAR_WIDE, if show && (near || grabbed) { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                let (it, ib) = bar_insets;
                let ib=ib+ln.children.get(1).map_or(0.0,|overlay|overlay.size.1);
                let (by, bh) = (r.y + it, (r.h - it - ib).max(0.0));
                let strip = Rect::new(r.x + r.w - BAR_STRIP, by, BAR_STRIP, bh);
                let mut bar = Bar { xf: cxf, id: bar_id, strip: xf.rect(strip), zone: None, rail: None, thumb: None, hit: None };
                bar.xf.clip_r = 0.0;
                if scrolls {
                    // Always there, so hovering the edge reveals the bar; once shown it keeps
                    // the arrow pointer over text that runs under it.
                    bar.zone = Some((a > 0.5).then_some(Cursor::Default));
                }
                if a > 0.01 && bh > 8.0 {
                    let track = bh - 8.0;
                    // Its length mirrors how much of the content is visible; it squashes
                    // against the end while the content is stretched.
                    let len = ((track * r.h / content.max(1.0)).max(34.0) - over).clamp(10.0, track.max(10.0));
                    let at = ((offset - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
                    let (thin, wide_w) = self.look.theme.control.scrollbar;
                    let w = thin + (wide_w - thin) * wide;
                    let track_opacity = self.look.theme.control.scrollbar_track_opacity;
                    if wide > 0.01 && track_opacity > 0.0 {
                        let rail = Rect::new(r.x + r.w - wide_w - 4.0, by, wide_w + 4.0, bh);
                        bar.rail = Some((rail, xf.fg.alpha(track_opacity * wide * a)));
                    }
                    let thumb = Rect::new(r.x + r.w - 2.0 - w, by + 4.0 + (track - len) * at, w, len);
                    bar.thumb = Some((thumb, w * 0.5, xf.fg.alpha((0.45 + 0.1 * wide) * a)));
                    if a > 0.5 {
                        let y0 = (by + 4.0) * xf.s + xf.ty;
                        bar.hit = Some(Action::ScrollBar { scroll: id, y0, track: track * xf.s, len: len * xf.s, lo, hi, page: (r.h * 0.9).max(1.0) });
                    }
                }
                self.bars.push(bar);
                if let Some(overlay)=bottom_overlay {
                    let cid=child_id(id,1,overlay.key);
                    self.tbase=tbase+ln.children[0].tn;
                    self.paint(*overlay,&ln.children[1],(r.x,r.y),cxf,cid);
                }
            }
            Kind::Toggle(on, f) => {
                let r = cr;
                let t = self.spring(id, slot::TOGGLE, if on { 1.0 } else { 0.0 });
                let p = self.spring(id, slot::CONTROL_PRESS, if self.pressed == Some(id) { 1.0 } else { 0.0 });
                let theme = &*self.look.theme;
                let (knob_c, knob_sh) = (theme.control.knob, theme.shadow.control);
                let track = theme.color.fill_strong.lerp(theme.toggle_on(), t.clamp(0.0, 1.0));
                self.fill(&xf, r, r.h * 0.5, Fill::Solid(track), None);
                // The knob stretches a little while pressed, like on iOS.
                let kw = r.h - 4.0 + 6.0 * p;
                let kx = r.x + 2.0 + (r.w - 4.0 - kw) * t;
                let knob = Rect::new(kx, r.y + 2.0, kw, r.h - 4.0);
                // It slides and stretches on springs: drawn as is, not in pixel steps.
                let kxf = Xf { px: 0.0, ..xf };
                self.shadow(&kxf, knob, knob.h * 0.5, knob_sh, false);
                self.fill(&kxf, knob, knob.h * 0.5, Fill::Solid(knob_c), None);
                self.hit(&xf, id, csr.inflate(6.0), Action::Toggle(on, f));
            }
            Kind::Menu { items, style } => {
                let style = style.unwrap_or_else(|| self.look.menu.clone());
                self.paint_menu(items, style, ln, cr, xf, id)
            }
            Kind::Slider(value, steps, f, style) => {
                let st = style.map_or(self.look.slider, |b| *b);
                let r = cr;
                let p = self.spring(id, slot::CONTROL_PRESS, if self.pressed == Some(id) { 1.0 } else { 0.0 });
                let knob_d = st.knob_size;
                let range = (r.w - knob_d).max(1.0);
                let cy = r.y + r.h * 0.5;
                let kx = r.x + range * value;
                let (th, tr) = (st.track_height, st.track_height * 0.5);
                let track = Rect::new(r.x, cy - tr, r.w, th);
                self.fill(&xf, track, tr, Fill::Solid(st.track), st.track_border);
                // Zero is an empty track, including the half-knob cap at its start.
                if value > 0.0 {
                    let filled = Rect::new(r.x, cy - tr, kx - r.x + knob_d * 0.5, th);
                    self.fill(&xf, filled, tr, Fill::Solid(st.fill), None);
                }
                let (ts, tk) = (st.tick_size, st.tick_size * 0.5);
                for k in 0..steps {
                    let x = r.x + knob_d * 0.5 + range * k as f32 / (steps - 1) as f32;
                    let on = value > 0.0 && x <= kx + knob_d * 0.5;
                    let c = if on { st.tick_on } else { st.tick_off.unwrap_or(xf.fg.alpha(0.35)) };
                    self.fill(&xf, Rect::new(x - tk, cy - tk, ts, ts), tk, Fill::Solid(c), None);
                }
                let d = knob_d * (1.0 + 0.12 * p);
                let knob = Rect::new(kx + (knob_d - d) * 0.5, cy - d * 0.5, d, d);
                if let Some(sh) = st.knob_shadow {
                    self.shadow(&xf, knob, d * 0.5, sh, false);
                }
                self.fill(&xf, knob, d * 0.5, Fill::Solid(st.knob), None);
                let x0 = (r.x + knob_d * 0.5) * xf.s + xf.tx;
                self.hit(&xf, id, csr.inflate(8.0), Action::Slider(x0, range * xf.s, steps, f));
            }
        }
        if let Some((mxf, mask)) = corner_mask {
            self.paint_corner_mask(mxf, r, &st, mask);
        }
        if let Some((ixf, sh)) = inset {
            self.smooth = st.smooth;
            self.shadow(&ixf, r, st.radius, sh, false);
            self.smooth = false;
        }
        if let Some((bxf, b)) = late_border {
            self.smooth = st.smooth;
            self.fill(&bxf, r, st.radius, Fill::Solid(Color::CLEAR), Some(b));
            if let Some((color, fraction, trailing)) = st.border_fade {
                if let Some(Cmd::Quad(q)) = self.list.cmds.last_mut() {
                    q.n0 = color.arr();
                    q.n1 = [fraction, if trailing { 1.0 } else { 0.0 }, 0.0, 0.0];
                    q.np[3] = ((q.np[3] as u32) | FLAG_BORDER_FADE) as f32;
                }
            }
            self.smooth = false;
        }
    }
}

// ---------------------------------------------------------------- Ui

pub struct Ui<S> {
    pub state: S,
    view_fn: Box<dyn Fn(&S, &Env) -> View<S>>,
    pub text: TextSystem,
    pub list: DrawList,
    /// Window background; `None` = the theme's `color.bg`. See [`Ui::clear_color`].
    pub background: Option<Color>,
    look: Rc<Look>,
    theme_fn: Option<Box<dyn Fn(&S,bool) -> Theme>>,
    /// Latest platform appearance. The runner updates this only on OS events; embedders
    /// and headless tests may set it explicitly. It is independent of the current theme.
    pub system_dark:bool,
    /// True while springs or scroll inertia are still moving: keep drawing frames.
    pub animating: bool,
    /// Native live-resize gesture. Size changes also bypass animations automatically.
    /// Embedders can hold this true between resize events; the app runner sets it on macOS.
    pub resizing: bool,
    hits: Vec<Hit<S>>,
    zones: Vec<Zone>,
    pointer_observers: Vec<PointerObserver<S>>,
    /// Views under the mouse, topmost first.
    hovered: Vec<u64>,
    /// A split divider held by the pointer, and the last press on one (for a double click).
    split_drag: Option<SplitDrag<S>>,
    split_down: Option<(Instant, u64)>,
    /// Mouse position while it is over the window (touch has none).
    mouse: Option<(f32, f32)>,
    /// Pointer shape the runner should show (the topmost `.cursor()` under the mouse).
    pub cursor: Cursor,
    springs: HashMap<(u64, u8), Spring>,
    scrolls: HashMap<u64, ScrollState>,
    icon_timelines: HashMap<u64, IconTimeline>,
    reveals: reveal::Reveals,
    marquees: marquee::Marquees,
    ptr: Pointer,
    last_frame: Option<Instant>,
    frame: u64,
    focus: Option<u64>,
    /// Caret, selection, undo history and IME state of the focused text field.
    ed: edit::Editor,
    /// Selection of read-only text (`.selectable()`).
    sel: select::SelState,
    /// Window size of the last frame, for building the view outside a frame (copying).
    size: (f32, f32),
    open_menu: Option<u64>,
    /// Where the open context menu was asked for.
    ctx_at: (f32, f32),
    menu_state: HashMap<u64, MenuState>,
    /// Menus on screen in the last frame, innermost last.
    menu_nav: Vec<MenuNav>,
    menu_keyboard: Option<u64>,
    /// `.shortcut()`s of the last frame.
    shortcuts: Vec<(Shortcut, Box<dyn Fn(&mut S)>)>,
    /// The view with a tooltip the mouse rests on.
    tip: Option<TipState>,
    /// Last time a tooltip was on screen: the next one shows without delay.
    tip_seen: Option<Instant>,
    started: Instant,
    timers: Vec<(Instant, Box<dyn FnOnce(&mut S)>)>,
    /// Seconds until the UI wants another frame even without input (caret blink).
    pub wake_in: Option<f32>,
    /// Draw a frames-per-second counter in the top-right corner. While it is on the UI
    /// redraws continuously (otherwise there would be nothing to count when idle).
    pub show_fps: bool,
    /// CPU time of the last frame in milliseconds (build + layout + paint + GPU encode),
    /// reported by the runner; shown next to the FPS.
    pub frame_ms: f32,
    /// Window requests raised by input (`.window_drag()`); the runner drains them.
    pub requests: Vec<WindowRequest>,
    /// Passed to the view as [`Env::titlebar`]; set by the runner.
    pub titlebar: f32,
    /// Passed to the view as [`Env::window_buttons`]; set by the runner.
    pub window_buttons: f32,
    /// Device pixels per point (2 on a Retina screen); set by the runner every frame, so a
    /// window moved to another monitor follows. Edges, hairlines and text are snapped to
    /// this pixel grid. Default 1.
    pub scale: f32,
    fps: f32,
    fps_label: String,
    fps_label_at: Option<Instant>,
}

impl<S: 'static> Ui<S> {
    /// `view` is `Fn(&S) -> View<S>`, or `Fn(&S, &Env) -> View<S>` to adapt to the screen size.
    pub fn new<M>(state: S, view: impl ViewFn<S, M>) -> Self {
        Ui {
            state,
            view_fn: Box::new(move |s, env| view.build(s, env)),
            text: TextSystem::new(),
            list: DrawList::default(),
            background: None,
            look: Rc::new(Look::new(Rc::new(Theme::default()))),
            theme_fn: None,
            system_dark:true,
            animating: false,
            resizing: false,
            hits: Vec::new(),
            zones: Vec::new(),
            pointer_observers: Vec::new(),
            hovered: Vec::new(),
            split_drag: None,
            split_down: None,
            mouse: None,
            cursor: Cursor::Default,
            springs: HashMap::new(),
            scrolls: HashMap::new(),
            icon_timelines: HashMap::new(),
            reveals: reveal::Reveals::default(),
            marquees: marquee::Marquees::default(),
            ptr: Pointer::default(),
            last_frame: None,
            frame: 0,
            focus: None,
            ed: edit::Editor::default(),
            sel: select::SelState::default(),
            size: (0.0, 0.0),
            open_menu: None,
            ctx_at: (0.0, 0.0),
            menu_state: HashMap::new(),
            menu_nav: Vec::new(),
            menu_keyboard: None,
            shortcuts: Vec::new(),
            tip: None,
            tip_seen: None,
            started: Instant::now(),
            timers: Vec::new(),
            wake_in: None,
            show_fps: false,
            frame_ms: 0.0,
            requests: Vec::new(),
            titlebar: 0.0,
            window_buttons: 0.0,
            scale: 1.0,
            fps: 0.0,
            fps_label: String::new(),
            fps_label_at: None,
        }
    }

    /// Replaces the theme; the next frame repaints everything in it. Built-in widgets take
    /// their colors and sizes from it; modifiers on a view still win.
    pub fn set_theme(&mut self, theme: Theme) {
        if *self.look.theme != theme {
            self.text.set_fonts(&theme.font, &theme.mono);
            self.look = Rc::new(Look::new(Rc::new(theme)));
        }
    }
    /// The theme in use.
    pub fn theme(&self) -> &Theme {
        &self.look.theme
    }
    /// Derives the theme from the state before every frame, so a handler can switch it:
    /// `ui.theme_with(|s: &State| if s.dark { Theme::dark() } else { Theme::light() })`.
    /// Overrides [`set_theme`](Self::set_theme).
    pub fn theme_with(&mut self, f: impl Fn(&S) -> Theme + 'static) {
        self.theme_fn = Some(Box::new(move |state,_|f(state)));
    }
    /// Derive a theme from state and the latest OS appearance. This supports a persistent
    /// System/Light/Dark preference without polling or mutating the OS appearance.
    pub fn theme_with_system(&mut self,f:impl Fn(&S,bool)->Theme+'static) {
        self.theme_fn=Some(Box::new(f));
    }
    /// What to clear the window with: [`background`](Self::background), or the theme's.
    pub fn clear_color(&self) -> Color {
        self.background.unwrap_or(self.look.theme.color.bg)
    }

    /// Builds, lays out and paints one frame for a window of `size` points.
    pub fn frame(&mut self, size: (f32, f32)) {
        // Dependent animated layout follows direct split drags just like window resizing.
        let dragging_split = self.split_drag.as_ref().is_some_and(|d| !d.collapsed
            && self.springs.get(&(d.hit.id, slot::SPLIT)).is_some_and(|s| s.target > 0.0 && (s.x - s.target).abs() < 0.5));
        let resizing = self.resizing || dragging_split || (self.frame > 0 && self.size != size);
        // Updates sent through `Link`s from background work.
        self.pump();
        self.text.set_scale(self.scale);
        let now = Instant::now();
        let dt = match self.last_frame {
            Some(t) if self.animating || self.wake_in.is_some() => (now - t).as_secs_f32().min(1.0 / 20.0),
            _ => 1.0 / 120.0,
        };
        if let Some(t) = self.last_frame {
            let real = (now - t).as_secs_f32();
            if real > 0.0 && real < 0.5 {
                let inst = 1.0 / real;
                self.fps = if self.fps == 0.0 { inst } else { self.fps + (inst - self.fps) * 0.1 };
            }
        }
        self.last_frame = Some(now);
        self.frame += 1;

        for s in self.springs.values_mut() {
            if resizing { s.snap_to(s.target); } else { s.step(dt); }
        }
        // Timers queued with `after()`.
        PENDING.with(|p| {
            for (delay, f) in p.borrow_mut().drain(..) {
                if let Ok(f) = f.downcast::<Box<dyn FnOnce(&mut S)>>() {
                    self.timers.push((now + std::time::Duration::from_secs_f32(delay), *f));
                }
            }
        });
        let mut i = 0;
        while i < self.timers.len() {
            if self.timers[i].0 <= now {
                (self.timers.swap_remove(i).1)(&mut self.state);
            } else {
                i += 1;
            }
        }
        // A timer can schedule its next bounded animation tick. Collect those before
        // computing wake_in, otherwise the UI could sleep with callbacks still pending.
        PENDING.with(|p| {
            for (delay,f) in p.borrow_mut().drain(..) {
                if let Ok(f)=f.downcast::<Box<dyn FnOnce(&mut S)>>() {
                    self.timers.push((now+std::time::Duration::from_secs_f32(delay),*f));
                }
            }
        });
        // From handlers that ran outside a tap (keys, timers, background updates).
        if let Some(f)=&self.theme_fn {
            let theme=f(&self.state,self.system_dark);
            self.set_theme(theme);
        }
        self.take_copies();

        // Scroll physics: inertia inside the content, a spring back from beyond its ends.
        let drag = self.ptr.drag_scroll.filter(|_| self.ptr.dragging);
        let mut refresh = Vec::new();
        let mut scroll_wake: Option<f32> = None;
        for (id, s) in self.scrolls.iter_mut() {
            if s.navigation.is_some() {continue;}
            let hold = if s.refreshing { REFRESH_HOLD } else { 0.0 };
            s.hold = if (s.hold - hold).abs() > 0.2 { s.hold + (hold - s.hold) * (1.0 - (-dt * 10.0).exp()) } else { hold };
            let since_wheel = s.wheel_at.map_or(f32::MAX, |t| (now - t).as_secs_f32());
            let hold_for = s.wheel_hold();
            let held = drag == Some(*id) || since_wheel < hold_for;
            if held {
                s.armed = !s.refreshing && s.offset < -REFRESH_AT;
                if since_wheel < hold_for {
                    scroll_wake = Some(hold_for + 0.01 - since_wheel); // notice when the fingers lift
                }
                continue;
            }
            if std::mem::take(&mut s.armed) {
                refresh.push(*id);
            }
            let (lo, hi) = s.bounds();
            let before = s.offset;
            if s.offset < lo || s.offset > hi {
                let end = if s.offset < lo { lo } else { hi };
                for _ in 0..4 {
                    let h = dt / 4.0;
                    s.vel += (-220.0 * (s.offset - end) - 30.0 * s.vel) * h;
                    s.offset += s.vel * h;
                }
                // Crossing the edge ends the bounce: with long frames the spring overshoots,
                // and the leftover speed would coast the list away from the edge.
                let crossed = (s.offset - end).signum() != (before - end).signum();
                if crossed || (s.offset - end).abs() < 0.25 && s.vel.abs() < 6.0 {
                    s.offset = end;
                    s.vel = 0.0;
                }
            } else if s.vel != 0.0 {
                s.offset += s.vel * dt;
                s.vel *= (-dt * 2.2).exp(); // iOS-like deceleration
                if s.vel.abs() < 4.0 {
                    s.vel = 0.0;
                }
            }
            if s.offset != before {
                s.active_at = Some(now);
            }
        }
        for id in refresh {
            if let Some(Action::Scroll { refresh: Some(f) }) = self.hits.iter().find(|h| h.id == id).map(|h| &h.action) {
                f(&mut self.state);
            }
        }
        // A selection dragged to the edge of its scroll view scrolls it.
        let autoscroll = self.sel_autoscroll(dt);
        if let Some((sid, d)) = autoscroll {
            if let Some(s) = self.scrolls.get_mut(&sid) {
                let (lo, hi) = s.bounds();
                s.offset = (s.offset + d).clamp(lo.min(s.offset), hi.max(s.offset));
                s.vel = 0.0;
                s.active_at = Some(now);
            }
        }
        self.size = size;

        let mut view = (self.view_fn)(&self.state, &self.env());
        let look = self.look.clone();
        crate::theme::apply(&mut view, &look);
        // A tooltip shows once the mouse has rested long enough; until then a timer, not frames.
        let tip_left = self.tip.as_ref().filter(|t| !t.hidden).map(|t| t.delay - (now - t.since).as_secs_f32());
        let tip_show = self.tip.as_ref().filter(|_| tip_left.is_some_and(|l| l <= 0.0)).map(|t| t.id);
        let tip_wake = tip_left.filter(|&l| l > 0.0);
        if tip_show.is_some() {
            self.tip_seen = Some(now);
        }
        let mut lx = Lx { text: &mut self.text, look: &look, springs: &mut self.springs, frame: self.frame, resizing, along: None, menu: self.open_menu, tip: tip_show, shift: 0.0, tail_viewport: None };
        let root = layout(&view, (Some(size.0), Some(size.1)), &mut lx, 1);
        self.list.cmds.clear();
        self.list.images.clear();
        let previous_fields = self.field_ids();
        self.hits.clear();
        self.zones.clear();
        self.pointer_observers.clear();
        self.sel.texts.clear();
        self.sel.areas.clear();
        self.menu_nav.clear();
        self.shortcuts.clear();
        let paint_sel = self.sel.for_paint();
        let blink = self.ed.blink_at.map_or(0.0, |t| (now - t).as_secs_f32());
        let fg = look.theme.color.text;
        let xf = Xf::screen(Rect::new(0.0, 0.0, size.0, size.1), fg, self.scale);
        let frame = self.frame;
        let mut p = Painter {
            visible_clip: None,
            visible_events: Vec::new(),
            list: &mut self.list,
            look: &look,
            hits: &mut self.hits,
            zones: &mut self.zones,
            pointer_observers: &mut self.pointer_observers,
            hovered: &self.hovered,
            springs: &mut self.springs,
            scrolls: &mut self.scrolls,
            icon_timelines: &mut self.icon_timelines,
            pressed: self.ptr.pressed,
            bar_drag: self.ptr.bar.map(|b| b.0),
            split_drag: self.split_drag.as_ref().map(|d| d.hit.id),
            frame,
            resizing,
            text: &mut self.text,
            reveals: &mut self.reveals,
            marquees: &mut self.marquees,
            focus: self.focus,
            ed: &mut self.ed,
            caret_on: blink % 1.0 < 0.5,
            open_menu: self.open_menu,
            menu: None,
            pointer_x: self.ptr.last.0,
            sticky: None,
            wake: None,
            now,
            time: (now - self.started).as_secs_f32(),
            spin: false,
            smooth: false,
            relayout: false,
            texts: &mut self.sel.texts,
            areas: &mut self.sel.areas,
            sel: paint_sel,
            tctx: select::TextCtx::with_color(look.theme.color.selection),
            tbase: 0,
            path: Vec::new(),
            screen: Rect::new(0.0, 0.0, size.0, size.1),
            layers: Vec::new(),
            bars: Vec::new(),
            layer: LayerCtx::default(),
            menu_state: &mut self.menu_state,
            menu_nav: &mut self.menu_nav,
            shortcuts: &mut self.shortcuts,
            managed_seen: false,
            ctx_at: self.ctx_at,
            tip: tip_show,
            px: self.scale,
        };
        p.paint(view, &root, (0.0, 0.0), xf, 1);
        p.flush_bars();
        p.paint_layers();
        match p.menu.take() {
            Some(menu) => {
                p.paint_picker_menu(menu, size);
                p.flush_bars();
            }
            None if !p.managed_seen => self.open_menu = None, // its picker or anchor is gone
            None => {}
        }
        let (paint_wake, spin, relayout) = (p.wake, p.spin, p.relayout);
        if p.sel.is_some_and(|s| s.stale) {
            self.sel.sel = None; // its rows are gone
        }
        for (handler,range) in std::mem::take(&mut p.visible_events) {handler(&mut self.state,range);}
        if autoscroll.is_some() {
            self.sel_drag(self.ptr.last); // the text under the pointer moved
        }
        if let Some(id) = self.focus {
            if !self.hits.iter().any(|h| h.id == id && matches!(h.action, Action::Field(_))) {
                self.focus = None;
            }
        }
        let autofocus = self.focus_new_field(&previous_fields);
        let timer_wake = self.timers.iter().map(|t| t.0.saturating_duration_since(now).as_secs_f32()).reduce(f32::min);
        self.wake_in = [self.focus.filter(|_| self.ed.blinks()).map(|_| 0.5 - blink % 0.5), paint_wake, scroll_wake, timer_wake, tip_wake]
            .into_iter()
            .flatten()
            .reduce(f32::min)
            .map(|w| w.max(0.0) + 0.01);

        if self.show_fps {
            self.paint_fps(now, size);
        }

        // The layout may have moved under a resting mouse: hover what is there now.
        let rehover = self.update_hover();
        if rehover {
            // A menu that moved or grew in under a resting mouse lights the row now under it.
            self.menu_hover();
        }
        self.menu_state.retain(|_, m| m.used == frame);
        self.springs.retain(|_, s| s.used == frame);
        self.scrolls.retain(|_, s| s.used == frame);
        self.icon_timelines.retain(|_, s| s.used == frame);
        self.reveals.end_frame(frame);
        self.marquees.end_frame(frame);
        self.text.end_frame();
        self.animating = autofocus || self.show_fps
            || rehover
            || spin
            || relayout
            || autoscroll.is_some()
            || self.springs.values().any(Spring::moving)
            || self.scrolls.iter().any(|(id, s)| {
                let resting = s.hold == if s.refreshing { REFRESH_HOLD } else { 0.0 };
                drag != Some(*id) && s.wheel_at.is_none_or(|t| (now - t).as_secs_f32() >= s.wheel_hold())
                    && (s.vel != 0.0 || s.over() > 0.0 || !resting)
            });
    }

    fn paint_fps(&mut self, now: Instant, size: (f32, f32)) {
        // Refresh the numbers a few times a second so they stay readable.
        if self.fps_label_at.is_none_or(|t| (now - t).as_secs_f32() > 0.25) {
            self.fps_label = format!("{:.0} fps  {:.1} ms", self.fps, self.frame_ms);
            self.fps_label_at = Some(now);
        }
        let spec = TextSpec { spans: &[], text: &self.fps_label, size: 12.0, weight: 600, center: false, ..Default::default() };
        let (key, ts) = self.text.measure(&spec, None);
        let clip = Rect::new(0.0, 0.0, size.0, size.1);
        let (w, h) = (ts.0 + 16.0, ts.1 + 8.0);
        let r = Rect::new(size.0 - w - 10.0, 10.0, w, h);
        let bg = Color::BLACK.alpha(0.55).arr();
        self.list.cmds.push(Cmd::Quad(Quad {
            rect: r.arr(),
            clip: clip.arr(),
            c0: bg,
            c1: bg,
            border: [0.0; 4],
            p0: [h * 0.5, 0.0, 0.0, KIND_FILL],
            p1: [0.0, 1.0, 0.0, 1.0],
            ..Zeroable::zeroed()
        }));
        self.list.cmds.push(Cmd::Text(TextCmd {
            key,
            pos: (r.x + 8.0, r.y + 4.0),
            size: ts,
            scale: 1.0,
            color: Color::hex(0x7CFF9B),
            clip,
            fade: None,
            leading_fade: 0.0,
        }));
    }

    /// Whether the topmost input target at `pos` is a window-drag area. Native hosts use this
    /// to gate system title-bar gestures; controls and open overlays block the area underneath.
    pub fn window_drag_at(&self, pos: (f32, f32)) -> bool {
        self.hit(pos, false).is_some_and(|i| matches!(self.hits[i].action, Action::DragWindow))
    }

    fn hit(&self, pos: (f32, f32), scroll: bool) -> Option<usize> {
        self.hits
            .iter()
            .rposition(|h| {
                h.contains(pos) && matches!(h.action, Action::Scroll { .. }) == scroll && !matches!(h.action, Action::Secondary(..))
            })
    }

    /// A press on a scroll bar: on the thumb it grabs it, on the track it pages towards the
    /// press.
    fn bar_press(&mut self, i: usize, y: f32) {
        let Action::ScrollBar { scroll, y0, track, len, lo, hi, page } = self.hits[i].action else { return };
        let Some(s) = self.scrolls.get_mut(&scroll) else { return };
        s.navigation=None;
        let at = ((s.offset - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
        let top = y0 + (track - len) * at;
        if y >= top && y <= top + len {
            self.ptr.bar = Some((scroll, y - top));
            return;
        }
        let dir = if y < top { -1.0 } else { 1.0 };
        s.offset = (s.offset + dir * page).clamp(lo, hi);
        s.vel = 0.0;
        s.active_at = Some(Instant::now());
    }

    /// Thumb dragged: the offset follows proportionally, no rubber band.
    fn bar_drag(&mut self, sid: u64, grab: f32, y: f32) -> bool {
        let Some(&Action::ScrollBar { y0, track, len, lo, hi, .. }) =
            self.hits.iter().map(|h| &h.action).find(|a| matches!(a, Action::ScrollBar { scroll, .. } if *scroll == sid))
        else {
            return false;
        };
        let Some(s) = self.scrolls.get_mut(&sid) else { return false };
        let at = ((y - grab - y0) / (track - len).max(1.0)).clamp(0.0, 1.0);
        let next = lo + at * (hi - lo);
        s.vel = 0.0;
        s.active_at = Some(Instant::now());
        std::mem::replace(&mut s.offset, next) != next
    }

    /// A press on a split's divider: a double click resets the width, otherwise it grabs it.
    fn split_press(&mut self, h: Rc<SplitHit<S>>, x: f32) {
        let now = Instant::now();
        let double = self.split_down.take().is_some_and(|(t, id)| id == h.id && (now - t).as_secs_f32() < DOUBLE_CLICK);
        if double {
            if let Some(w) = h.reset {
                (h.on_resize)(&mut self.state, w.clamp(h.min, h.max));
            }
            if let Some((_, f)) = h.collapse.as_ref().filter(|_| h.collapsed) {
                f(&mut self.state, false);
            }
            return;
        }
        self.split_down = Some((now, h.id));
        let shown = if h.collapsed { 0.0 } else { h.open };
        let grab = h.dir * (x - h.edge) / h.s - shown;
        self.split_drag = Some(SplitDrag { collapsed: h.collapsed, grab, hit: h });
        self.cursor = Cursor::ResizeH;
    }

    /// The divider follows the pointer 1:1 within the panel's limits; past the collapse
    /// threshold the panel closes (and opens again coming back), gliding on its spring.
    fn split_move(&mut self, x: f32) -> bool {
        let Some(d) = &mut self.split_drag else { return false };
        let h = d.hit.clone();
        let want = h.dir * (x - h.edge) / h.s - d.grab;
        if let Some((threshold, f)) = &h.collapse {
            if want < *threshold {
                if d.collapsed {
                    return false;
                }
                d.collapsed = true;
                f(&mut self.state, true);
                return true;
            }
            if d.collapsed {
                d.collapsed = false;
                f(&mut self.state, false);
                (h.on_resize)(&mut self.state, want.clamp(h.min, h.max));
                return true;
            }
        }
        let w = want.clamp(h.min, h.max);
        let line = self.look.theme.control.border;
        (h.on_resize)(&mut self.state, w);
        // Resting (not still gliding open): the region jumps straight to the new width.
        if let Some(s) = self.springs.get_mut(&(h.id, slot::SPLIT)) {
            if (s.x - s.target).abs() < 0.5 {
                (s.x, s.target, s.v) = (w + line, w + line, 0.0);
            }
        }
        true
    }

    fn slide(&mut self, i: usize, x: f32) {
        if let Action::Slider(x0, range, steps, f) = &self.hits[i].action {
            f(&mut self.state, popup::snap(((x - x0) / range).clamp(0.0, 1.0), *steps));
        }
    }

    /// Pointer went down. Returns true if a redraw is needed.
    pub fn pointer_down(&mut self, pos: (f32, f32), touch: bool) -> bool {
        if touch {
            self.mouse = None;
            self.update_hover();
        }
        self.ptr.last = pos;
        self.ptr.down = true;
        self.ptr.down_at = pos;
        self.ptr.touch = touch;
        self.ptr.dragging = false;
        self.ptr.last_move = Some(Instant::now());
        self.ptr.drag_scroll = if touch { self.hit(pos, true).map(|i| self.hits[i].id) } else { None };
        if let Some(id) = self.ptr.drag_scroll {
            if let Some(s) = self.scrolls.get_mut(&id) {
                s.navigation=None;
                s.vel = 0.0; // catch a coasting list
            }
        }
        if let Some(t) = &mut self.tip {
            t.hidden = true; // a click hides the tooltip until the mouse moves on
        }
        let hit = self.hit(pos, false);
        // A consumed click breaks the title-bar gesture, including popover dismissal (which
        // returns early below). Only consecutive mouse presses on the drag area may zoom.
        if touch || !hit.is_some_and(|i| matches!(self.hits[i].action, Action::DragWindow)) {
            self.ptr.drag_down = None;
        }
        if !hit.is_some_and(|i| matches!(self.hits[i].action, Action::Tap(..))) { self.ptr.tap = None; }
        self.ptr.pressed = hit.map(|i| self.hits[i].id);
        if let Some(Action::Tabs {ends,disabled,..})=hit.map(|i|&self.hits[i].action) {
            let index=ends.iter().position(|&end|pos.0<end).unwrap_or(ends.len().saturating_sub(1));
            if disabled.contains(&index){self.ptr.pressed=None;return true;}
        }
        if let Some(Action::Outside(d)) = hit.map(|i| &self.hits[i].action) {
            let d = d.clone();
            // Only an explicitly marked select directly below this overlay may receive
            // the same click. Do not search through another control or modal backdrop.
            let replacement=hit.and_then(|i|self.hits[..i].iter().rposition(|h|h.contains(pos)))
                .filter(|&i|matches!(self.hits[i].action,Action::SelectAnchor {open:false}));
            self.run_dismiss(d);
            self.ptr.pressed=None;
            if let Some(i)=replacement {self.blur();self.activate(i);}
            return true;
        }
        self.ptr.bar = None;
        if let Some(Action::Split(h)) = hit.map(|i| &self.hits[i].action) {
            let h = h.clone();
            self.blur();
            self.ptr.pressed = None;
            self.split_press(h, pos.0);
            return true;
        }
        if let Some(i) = hit.filter(|_| !touch) {
            if let Action::ScrollBar { .. } = self.hits[i].action {
                // The bar is not text: the selection and focus stay as they are.
                self.bar_press(i, pos.1);
                return true;
            }
        }
        if !hit.is_some_and(|i| self.focus == Some(self.hits[i].id)) { self.blur(); }
        if let Some(i) = hit {
            if matches!(self.hits[i].action, Action::DragWindow) && !touch {
                let now = Instant::now();
                let double = self.ptr.drag_down.take().is_some_and(|(t, p)| {
                    (now - t).as_secs_f32() < DOUBLE_CLICK && (p.0 - pos.0).abs() < 4.0 && (p.1 - pos.1).abs() < 4.0
                });
                if double {
                    self.requests.push(WindowRequest::Zoom);
                } else {
                    self.ptr.drag_down = Some((now, pos));
                    self.requests.push(WindowRequest::DragWindow);
                }
            }
            self.slide(i, pos.0);
            if matches!(self.hits[i].action, Action::Field(_)) {
                self.field_press(i, pos);
            }
        }
        // A press anywhere else drops the selection; on an area it starts a new one. Touch
        // only scrolls (no long-press selection yet), a link keeps it until released.
        match hit.map(|i| &self.hits[i].action) {
            Some(&Action::Select { area }) if !touch => self.sel_press(area, pos),
            Some(Action::Link(_)) => {}
            _ => self.sel.sel = None,
        }
        true
    }

    /// Recomputes what is under the mouse; true if that changed (a frame is needed).
    fn update_hover(&mut self) -> bool {
        let (ids, cursor, tip) = hover_at(&self.zones, self.mouse);
        // A dragged scroll bar thumb keeps the arrow wherever the pointer wanders.
        self.cursor = if self.ptr.bar.is_some() {
            Cursor::Default
        } else if self.split_drag.is_some() {
            Cursor::ResizeH
        } else {
            cursor
        };
        match (tip, &self.tip) {
            (Some(id), Some(t)) if t.id == id => {}
            (Some(id), _) => {
                let warm = self.tip_seen.is_some_and(|t| t.elapsed().as_secs_f32() < 0.4);
                self.tip = Some(TipState { id, since: Instant::now(), delay: if warm { 0.0 } else { self.look.theme.motion.tip_delay }, hidden: false });
            }
            (None, _) => self.tip = None,
        }
        if ids == self.hovered {
            return false;
        }
        self.hovered = ids;
        true
    }

    /// The mouse left the window: nothing is hovered any more.
    pub fn pointer_leave(&mut self) -> bool {
        let changed=self.mouse.take().is_some();
        let tracked=changed && self.notify_pointer();
        self.update_hover() || tracked
    }

    fn notify_pointer(&mut self) -> bool {
        for observer in &self.pointer_observers {
            let pos=self.mouse.map(|p|((p.0-observer.rect.x)/observer.scale,(p.1-observer.rect.y)/observer.scale));
            (observer.handler)(&mut self.state,pos);
        }
        !self.pointer_observers.is_empty()
    }

    /// Pointer moved. Returns true if a redraw is needed: dragging something, or the mouse
    /// went onto or off a view that reacts to hover. Plain moves over the same view cost
    /// no frame.
    pub fn pointer_move(&mut self, pos: (f32, f32)) -> bool {
        let dy = pos.1 - self.ptr.last.1;
        self.ptr.last = pos;
        // A finger has no hover; a mouse press (`touch == false`) or plain motion has.
        let hover = if self.ptr.touch && self.ptr.down {
            false
        } else {
            self.ptr.touch = false;
            let moved=self.mouse!=Some(pos);
            self.mouse = Some(pos);
            let tracked=moved && self.notify_pointer();
            let changed = self.update_hover();
            if changed {
                self.menu_hover();
            }
            changed || tracked
        };
        if self.split_drag.is_some() {
            return self.split_move(pos.0) || hover;
        }
        if let Some((sid, grab)) = self.ptr.bar {
            self.cursor = Cursor::Default;
            return self.bar_drag(sid, grab, pos.1) || hover;
        }
        // Touch: a vertical drag inside a scroll view scrolls it and cancels the tap.
        if let Some(sid) = self.ptr.drag_scroll {
            if !self.ptr.dragging && (pos.1 - self.ptr.down_at.1).abs() > 8.0 {
                let on_slider = self
                    .ptr
                    .pressed
                    .and_then(|id| self.hits.iter().find(|h| h.id == id))
                    .is_some_and(|h| matches!(h.action, Action::Slider(..) | Action::Tabs { .. }));
                if !on_slider {
                    self.ptr.dragging = true;
                    self.ptr.pressed = None;
                }
            }
            if self.ptr.dragging {
                let now = Instant::now();
                let dt = self.ptr.last_move.map_or(0.016, |t| (now - t).as_secs_f32()).max(0.001);
                self.ptr.last_move = Some(now);
                if let Some(s) = self.scrolls.get_mut(&sid) {
                    s.pull(-dy);
                    s.vel = s.vel * 0.4 + (-dy / dt) * 0.6;
                    s.active_at = Some(now);
                }
                return true;
            }
        }
        if let Some(id) = self.ptr.pressed {
            if let Some(i) = self.hits.iter().position(|h| h.id == id) {
                if matches!(self.hits[i].action, Action::Slider(..)) {
                    self.slide(i, pos.0);
                    return true;
                }
                if matches!(self.hits[i].action, Action::Tabs { .. }) {
                    return true; // the drop follows the pointer
                }
                if matches!(self.hits[i].action, Action::Field(_)) {
                    return self.field_drag(i, pos);
                }
                if matches!(self.hits[i].action, Action::Select { .. }) {
                    return self.sel_drag(pos) || hover;
                }
                if matches!(self.hits[i].action, Action::Link(_)) {
                    return self.link_to_selection(i, pos) || hover;
                }
            }
        }
        hover
    }

    pub fn pointer_up(&mut self, pos: (f32, f32)) -> bool {
        let pressed = self.ptr.pressed.take();
        self.ptr.down = false;
        if self.ptr.bar.take().is_some() || self.split_drag.take().is_some() {
            self.update_hover();
            return true;
        }
        let was_dragging = std::mem::take(&mut self.ptr.dragging);
        self.ptr.drag_scroll = None;
        self.sel_release();
        if was_dragging {
            return true; // inertia continues in frame()
        }
        // A dragged tab drop selects whatever tab it is released over, even off the bar.
        if let Some(h) = pressed.and_then(|id| self.hits.iter().find(|h| h.id == id)) {
            if let Action::Tabs { ends, disabled, on_select } = &h.action {
                let index=ends.iter().position(|&end|pos.0<end).unwrap_or(ends.len().saturating_sub(1));
                if !disabled.contains(&index){on_select(&mut self.state,index);}
                return true;
            }
        }
        if let (Some(id), Some(i)) = (pressed, self.hit(pos, false)) {
            if self.hits[i].id == id {
                self.activate(i);
            }
        }
        // Still inside the click: the web only lets a page write the clipboard there.
        self.take_copies();
        true
    }

    /// Does what a click on hit `i` does.
    fn activate(&mut self, i: usize) {
        let id = self.hits[i].id;
        if matches!(self.hits[i].action,Action::SelectAnchor {..}) {
            if let Some(anchor)=self.hits[..i].iter().rposition(|h|h.id==id&&matches!(h.action,Action::Tap(..)|Action::OpenMenu)) {self.activate(anchor);}
            return;
        }
        // Dismiss the source menu before its action can open another application layer.
        if let Action::MenuPick(_, Some(d)) = &self.hits[i].action {
            self.run_dismiss(d.clone());
        }
        match &self.hits[i].action {
            Action::Tap(tap, double_tap) => {
                let now = Instant::now();
                let pos = self.ptr.last;
                let double = self.ptr.tap.take().is_some_and(|(t, previous, p)| previous == id
                    && (now-t).as_secs_f32() < DOUBLE_CLICK
                    && (p.0-pos.0).abs() < 4.0 && (p.1-pos.1).abs() < 4.0);
                if double && double_tap.is_some() {
                    if let Some(f) = double_tap { f(&mut self.state); }
                } else {
                    self.ptr.tap = Some((now,id,pos));
                    if let Some(f) = tap { f(&mut self.state); }
                }
            },
            Action::Toggle(on, f) => f(&mut self.state, !on),
            Action::OpenMenu => {
                self.springs.remove(&(id, slot::PICKER_MENU)); // replay the opening animation
                self.springs.remove(&(popup::pop_id(id), slot::FADE));
                self.open_menu = Some(id);
            }
            Action::Dismiss => self.open_menu = None,
            Action::Block(Some(f)) => f(&mut self.state),
            Action::Pick(index, f) => {
                f(&mut self.state, *index);
                self.open_menu = None;
            }
            Action::MenuPick(f, _) => f(&mut self.state),
            &Action::SubMenu(menu, row) => {
                let st = self.menu_state.entry(menu).or_default();
                st.hl = Some(row);
                st.sub = Some(row);
            }
            Action::Link(_) => {
                self.sel.sel = None;
                self.open_link(i);
            }
            _ => {}
        }
    }

    /// Moves texts queued by [`copy_text`] into [`Ui::requests`].
    fn take_copies(&mut self) {
        COPIES.with(|c| self.requests.extend(c.borrow_mut().drain(..).map(WindowRequest::SetClipboard)));
    }

    /// True while a text field has keyboard focus.
    pub fn has_focus(&self) -> bool {
        self.focus.is_some()
    }

    /// Typed text (already translated by the keyboard layout). Returns true if it was used.
    pub fn text_input(&mut self, text: &str) -> bool {
        if text.is_empty() || text.chars().any(char::is_control) {
            return false;
        }
        self.field_type(text)
    }

    /// Editing / navigation key; Shift and the other modifiers come from
    /// [`set_modifiers`](Self::set_modifiers). Returns true if it was used.
    pub fn key(&mut self, key: KeyInput) -> bool {
        if let Some(used) = self.menu_key(key) {
            return used;
        }
        match key {
            KeyInput::Escape if self.dismiss_top() => {
                self.blur();
                true
            }
            KeyInput::Escape if self.field_key(key) => true,
            KeyInput::Escape => self.open_menu.take().is_some() | self.blur() | self.sel_key(key),
            KeyInput::Tab | KeyInput::BackTab => self.focus_step(key == KeyInput::Tab),
            KeyInput::Enter if self.open_menu.is_some() => {
                self.open_menu = None;
                true
            }
            _ if self.focus.is_none() => self.sel_key(key),
            _ => self.field_key(key),
        }
    }

    /// Mouse wheel / trackpad scroll; `dy` in points, positive = content moves down.
    pub fn wheel(&mut self, pos: (f32, f32), dy: f32, phase: WheelPhase) -> bool {
        if self.field_wheel(pos, dy) {
            return true;
        }
        let now = Instant::now();
        // Topmost scroll view under the cursor; an overlay's backdrop stops the search.
        for h in self.hits.iter().rev() {
            if !h.contains(pos) {
                continue;
            }
            match &h.action {
                Action::Block(_) | Action::Outside(_) => return false,
                Action::Scroll { .. } => {}
                _ => continue,
            }
            let Some(s) = self.scrolls.get_mut(&h.id) else { continue };
            s.navigation=None;
            let (lo, hi) = s.bounds();
            if phase == WheelPhase::Line {
                // Mouse wheel: stops dead at the ends, and lets an outer scroll view take over.
                let next = (s.offset - dy).clamp(lo.min(s.offset), hi.max(s.offset));
                if next != s.offset {
                    s.offset = next;
                    s.vel = 0.0;
                    s.active_at = Some(now);
                    return true;
                }
                continue;
            }
            // Speed of the deltas, for the bounce when momentum meets an end.
            let gap = s.wheel_at.map_or(1.0 / 60.0, |t| (now - t).as_secs_f32()).clamp(1.0 / 240.0, 1.0 / 30.0);
            match phase {
                WheelPhase::Started => {
                    // The OS starts its momentum right after the fingers lift; any other
                    // start is fingers (touching the pad also stops a running momentum).
                    let lifted = matches!(s.pad, Pad::Lifted(t) if (now - t).as_secs_f32() < 0.1);
                    s.pad = if lifted { Pad::Momentum } else { Pad::Fingers };
                    s.pad_v = 0.0;
                    if lifted && s.over() > 0.0 {
                        s.pad = Pad::Spent; // flung while stretched: the spring brings it back
                    }
                }
                WheelPhase::Ended => {
                    s.pad = if s.pad == Pad::Fingers { Pad::Lifted(now) } else { Pad::Idle };
                    s.wheel_at = None; // released: the rubber band may snap back
                    return true;
                }
                _ => {}
            }
            match s.pad {
                Pad::Spent => return true,
                Pad::Momentum => {
                    s.pad_v = s.pad_v * 0.5 + (-dy / gap) * 0.5;
                    let (lo, hi) = s.bounds();
                    let next = s.offset - dy;
                    if next < lo || next > hi {
                        // Coasted into an end: stop on it and let the spring turn the speed
                        // into one bounce.
                        s.offset = next.clamp(lo, hi);
                        s.vel = s.pad_v.clamp(-3000.0, 3000.0);
                        s.pad = Pad::Spent;
                        s.wheel_at = None;
                    } else {
                        s.offset = next;
                        s.vel = 0.0;
                        s.wheel_at = Some(now);
                    }
                }
                _ => {
                    s.pull(-dy);
                    s.vel = 0.0;
                    s.wheel_at = Some(now);
                }
            }
            s.active_at = Some(now);
            return true;
        }
        false
    }
}
