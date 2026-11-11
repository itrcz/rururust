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

/// One GPU instance. Units are logical points; the shader applies the scale factor.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Quad {
    pub rect: [f32; 4],
    pub clip: [f32; 4],
    pub c0: [f32; 4],
    pub c1: [f32; 4],
    pub border: [f32; 4],
    /// radius, border width, shadow blur, kind
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
}

/// Length of the `Truncate::Fade` ramp in points.
const FADE_W: f32 = 28.0;

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

#[derive(Clone, Copy, Default)]
struct ScrollState {
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
    Tap(Box<dyn Fn(&mut S)>),
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
    /// Tab bar: screen x of the first tab, width of one tab, number of tabs.
    Tabs { x0: f32, tab_w: f32, n: usize, on_select: Box<dyn Fn(&mut S, usize)> },
    OpenMenu,
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
    /// A menu row: run the action, then close the popover it is in.
    MenuPick(Box<dyn Fn(&mut S)>, Option<Dismiss<S>>),
    /// A submenu row: (menu id, row) opens it.
    SubMenu(u64, usize),
    /// Secondary click: run the handler, or (`None`) open the context menu of view `.1`.
    /// Ignored by the primary button.
    Secondary(Option<Box<dyn Fn(&mut S)>>, u64),
    /// The scroll bar of scroll view `scroll`: screen y and length of the thumb's travel,
    /// thumb length (screen), the offset range and one page (content units).
    ScrollBar { scroll: u64, y0: f32, track: f32, len: f32, lo: f32, hi: f32, page: f32 },
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
}

/// An open picker menu, painted above everything else.
struct Menu<S> {
    id: u64,
    anchor: Rect,
    options: Vec<String>,
    selected: usize,
    on_select: Rc<dyn Fn(&mut S, usize)>,
    fg: Color,
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
    clip: Rect,
    clip_r: f32,
    /// The clip's corners are continuous (`.smooth()`), not circular.
    clip_smooth: bool,
    fg: Color,
    /// Device pixels per point, for snapping to the pixel grid; 0 turns snapping off for
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
        let next = self.clip.intersect(&r);
        let near = |a: &Rect, b: &Rect| {
            (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5 && (a.w - b.w).abs() < 0.5 && (a.h - b.h).abs() < 0.5
        };
        (self.clip_r, self.clip_smooth) = if near(&next, &r) {
            (radius, smooth)
        } else if near(&next, &self.clip) {
            (self.clip_r, self.clip_smooth)
        } else if radius > self.clip_r {
            (radius, smooth)
        } else {
            (self.clip_r, self.clip_smooth)
        };
        self.clip = next;
    }
}

struct Painter<'a, S> {
    list: &'a mut DrawList,
    look: &'a Look,
    hits: &'a mut Vec<Hit<S>>,
    zones: &'a mut Vec<Zone>,
    /// Views under the pointer (from the last frame's zones).
    hovered: &'a [u64],
    /// The scroll view whose bar thumb is being dragged.
    bar_drag: Option<u64>,
    springs: &'a mut HashMap<(u64, u8), Spring>,
    scrolls: &'a mut HashMap<u64, ScrollState>,
    pressed: Option<u64>,
    frame: u64,
    text: &'a mut TextSystem,
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
    fn hit(&mut self, xf: &Xf, id: u64, rect: Rect, action: Action<S>) {
        self.hits.push(Hit { id, rect: rect.intersect(&xf.clip), round: (xf.clip, xf.clip_r), action });
    }

    fn zone(&mut self, xf: &Xf, id: u64, rect: Rect, cursor: Option<Cursor>) {
        self.zones.push(Zone { id, rect: rect.intersect(&xf.clip), round: (xf.clip, xf.clip_r), cursor, blocks: false, tip: false });
    }

    fn label(&mut self, xf: &Xf, key: u64, pos: (f32, f32), size: (f32, f32), color: Color) {
        let pos = xf.at_px(pos);
        self.list.cmds.push(Cmd::Text(TextCmd {
            key,
            pos: (pos.0 * xf.s + xf.tx, pos.1 * xf.s + xf.ty),
            size: (size.0 * xf.s, size.1 * xf.s),
            scale: xf.s,
            color: color.alpha(color.a * xf.opacity),
            clip: xf.clip,
            fade: None,
        }));
    }

    /// A picture filling `r`; with `tint` only its shape is used, painted in that color.
    fn picture(&mut self, xf: &Xf, r: Rect, radius: f32, img: &Image, tint: Option<Color>, border: Option<(f32, Color)>) {
        let mut q = self.quad(xf, r, radius, KIND_IMAGE);
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
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(initial, target, self.look.theme.motion.bouncy));
        s.target = target;
        s.used = self.frame;
        s.x
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
        let row_h = theme.control.picker_row;
        let screen = Rect::new(0.0, 0.0, size.0, size.1);
        // Anything outside the menu closes it.
        self.hits.push(Hit { id: m.id ^ 0xD15, rect: screen, round: NO_ROUND, action: Action::Dismiss });
        self.zones.push(Zone { id: m.id ^ 0xD15, rect: screen, round: NO_ROUND, cursor: None, blocks: true, tip: false });
        let spec = |text| spec_in(&theme.text.body, text);
        let rows: Vec<(u64, (f32, f32))> = m.options.iter().map(|o| self.text.measure(&spec(o), None)).collect();
        let (check, check_size) = self.text.measure(&TextSpec { weight: 600, ..spec("✓") }, None);
        let widest = rows.iter().map(|r| r.1 .0).fold(0.0, f32::max);
        let w = (widest + 46.0 + check_size.0).max(m.anchor.w).min(size.0 - 16.0);
        let h = row_h * rows.len() as f32 + 12.0;
        let x = m.anchor.x.min(size.0 - w - 8.0).max(8.0);
        let below = m.anchor.y + m.anchor.h + 6.0;
        let (y, down) = if below + h <= size.1 - 8.0 { (below, true) } else { ((m.anchor.y - h - 6.0).max(8.0), false) };

        // The menu grows out of the control it belongs to.
        let frame = self.frame;
        let s = self.springs.entry((m.id, 4)).or_insert(Spring::new(0.0, 1.0, theme.motion.bouncy));
        s.used = frame;
        let t = s.x;
        let k = 0.8 + 0.2 * t;
        let (ax, ay) = (m.anchor.x + m.anchor.w * 0.5, if down { y } else { y + h });
        let xf = Xf {
            s: k,
            tx: ax * (1.0 - k),
            ty: ay * (1.0 - k),
            opacity: t.clamp(0.0, 1.0),
            clip: screen,
            clip_r: 0.0,
            clip_smooth: false,
            fg: m.fg,
            px: self.px,
        };
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
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(target, target, self.look.theme.motion.bouncy));
        s.target = target;
        s.used = self.frame;
        s.x
    }

    /// A spring without overshoot, for press feedback.
    fn crisp(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(target, target, self.look.theme.motion.crisp));
        s.target = target;
        s.used = self.frame;
        s.x
    }

    /// Like `spring`, but also returns the current velocity.
    fn spring_v(&mut self, id: u64, slot: u8, target: f32) -> (f32, f32) {
        let x = self.spring(id, slot, target);
        (x, self.springs[&(id, slot)].v)
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
        q.p1[0] = g.frost * xf.s;
        q.p1[1] = g.refraction;
        if let Some((w, c)) = border {
            q.border = c.arr();
            q.p0[1] = xf.line(w);
        }
        self.list.cmds.push(Cmd::Quad(q));
    }

    fn paint(&mut self, v: View<S>, ln: &LNode, origin: (f32, f32), xf: Xf, id: u64) {
        let saved = self.enter_text_ctx(&v, id);
        self.paint_node(v, ln, origin, xf, id);
        if let Some(t) = saved {
            self.tctx = t;
        }
    }

    fn paint_node(&mut self, mut v: View<S>, ln: &LNode, origin: (f32, f32), mut xf: Xf, id: u64) {
        let mut extra = v.extra.take().map(|b| *b);
        // Shortcuts count while their view is in the tree, on screen or not.
        if let Some(x) = &mut extra {
            self.shortcuts.append(&mut x.shortcuts);
        }
        let tap_menu = extra.as_ref().is_some_and(|x| matches!(x.pop, Some(crate::view::Pop { opens: crate::view::Opens::Tap, .. })));
        let pin = self.sticky.take();
        let tbase = std::mem::take(&mut self.tbase);
        let st = v.style;
        let r = Rect::new(origin.0 + ln.pos.0, origin.1 + ln.pos.1, ln.size.0, ln.size.1);
        xf.opacity *= st.opacity;
        if let Some(fg) = st.fg {
            xf.fg = fg;
        }

        // Press feedback: a quick, quiet dim with a barely-there squeeze. No bounce.
        if v.on_tap.is_some() || tap_menu {
            let p = self.crisp(id, 0, if self.pressed == Some(id) { 1.0 } else { 0.0 });
            let k = 1.0 - 0.02 * p;
            let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            xf.tx += cx * (1.0 - k) * xf.s;
            xf.ty += cy * (1.0 - k) * xf.s;
            xf.s *= k;
            xf.opacity *= 1.0 - 0.3 * p;
        }

        let sr = xf.rect_px(r);
        let margin = st.shadow.iter().flatten().map(Shadow::reach).fold(0.0, f32::max);
        if !sr.inflate(margin * xf.s).intersects(&xf.clip) || xf.opacity <= 0.001 {
            return; // culled
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
            .filter(|_| st.clip || ln.clip || st.hover.is_some() || st.glass.is_some() || !ln.children.is_empty())
            .map(|b| (xf, b));
        // Inner shadows lie over the children too, like the border.
        let inset = st.inset.map(|sh| (xf, sh));
        if let Some(g) = st.glass {
            self.glass(&xf, r, st.radius, g, st.border.filter(|_| late_border.is_none()));
        }
        match (st.bg, st.border.filter(|_| late_border.is_none()), st.glass) {
            (Some(bg), border, None) => self.fill(&xf, r, st.radius, bg, border),
            (Some(bg), _, Some(_)) => self.fill(&xf, r, st.radius, bg, None),
            (None, Some(b), None) => self.fill(&xf, r, st.radius, Fill::Solid(Color::CLEAR), Some(b)),
            _ => {}
        }

        // Pointer shape and hover: explicit, or what the kind of view suggests.
        let cursor = st.cursor.or(match &v.kind {
            Kind::TextField(_) => Some(Cursor::Text),
            Kind::Toggle(..) | Kind::Slider(..) | Kind::Tabs { .. } | Kind::Picker { .. } => Some(Cursor::Pointer),
            _ if v.on_tap.is_some() || tap_menu => Some(Cursor::Pointer),
            _ => None,
        });
        if cursor.is_some() || st.hover.is_some() || st.hover_fg.is_some() {
            self.zone(&xf, id, sr, cursor);
        }
        if st.hover.is_some() || st.hover_fg.is_some() {
            let t = self.crisp(id, 9, if self.hovered.contains(&id) { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
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
            xf.clip_to(sr.inflate(-bw), (st.radius * xf.s - bw).max(0.0), st.smooth);
        }

        self.area_hit(&xf, id, sr);
        // Before the tap: a view that is both draggable and tappable keeps its tap.
        if st.drag {
            self.hit(&xf, id ^ 0xD6A9, sr, Action::DragWindow);
        }
        if let Some(f) = v.on_tap {
            self.hit(&xf, id, sr, Action::Tap(f));
        }
        if let Some(x) = extra {
            self.attach(x, ln, &xf, id, sr);
        }

        let pad = st.padding;
        // Where the content goes: padding works the same on every kind of view, leaves
        // (toggle, slider, menu, tabs, picture) included.
        let cr = Rect::new(r.x + pad[0], r.y + pad[1], (r.w - pad[0] - pad[2]).max(0.0), (r.h - pad[1] - pad[3]).max(0.0));
        let csr = if pad == [0.0; 4] { sr } else { xf.rect_px(cr) };
        match v.kind {
            Kind::Empty | Kind::Spacer => {}
            Kind::List { count, row_h, row } => {
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
                    let mut lx = Lx { text: p.text, look: p.look, springs: p.springs, frame: p.frame, along: None, menu: p.open_menu, tip: p.tip, shift: 0.0 };
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
                    let saved_row = self.tctx.row;
                    self.enter_row(id, i, count);
                    self.path.push((id, i));
                    self.paint(rv, &n, (r.x, y), xf, cid);
                    self.path.pop();
                    self.tctx.row = saved_row;
                }
                if let Some(c) = self.text.lists.get_mut(&id) {
                    (c.seen, c.anchor) = if seen.0 == usize::MAX { ((0, 0), 0) } else { ((seen.0, seen.1), seen.2.min(seen.1)) };
                }
            }
            Kind::Overlay { open, mode, child, on_dismiss } => {
                let t = self.spring_from(id, 7, 0.0, if open { 1.0 } else { 0.0 });
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
                        // Pops out of the center.
                        let k = 0.88 + 0.12 * t;
                        let (mx, my) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
                        cxf.tx += mx * (1.0 - k) * xf.s;
                        cxf.ty += my * (1.0 - k) * xf.s;
                        cxf.s *= k;
                        cxf.opacity *= tc;
                        (cx, r.y + (r.h - h) * 0.5)
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
                if mode != OverlayMode::Toast {
                    let scrim = self.look.theme.color.scrim;
                    self.fill(&xf, r, 0.0, Fill::Solid(scrim.alpha(scrim.a * tc)), None);
                    self.hit(&xf, id, sr, Action::Block(on_dismiss));
                }
                let body = cxf.rect(Rect::new(pos.0, pos.1, w, h));
                self.hits.push(Hit { id: child_id(id, 1, None), rect: body, round: NO_ROUND, action: Action::Swallow });
                let cid = child_id(id, 0, child.key);
                self.tbase = tbase;
                self.paint(*child, cn, pos, cxf, cid);
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
            Kind::Tabs { labels, selected, on_select } => {
                let (tabs_pad, drop_tint, lift) = (self.look.theme.control.tab_padding, self.look.theme.control.tab_drop, self.look.theme.shadow.lift);
                let n = labels.len().max(1);
                let r = cr;
                let tab_w = (r.w - 2.0 * tabs_pad) / n as f32;
                let center = |i: usize| r.x + tabs_pad + tab_w * (i as f32 + 0.5);
                let held = self.pressed == Some(id);
                // The drop rests on the selected tab; while held it follows the pointer.
                let target = if held {
                    ((self.pointer_x - xf.tx) / xf.s).clamp(center(0), center(n - 1))
                } else {
                    center(selected)
                };
                let (cx, vel) = self.spring_v(id, 5, target);
                let p = self.spring(id, 6, if held { 1.0 } else { 0.0 });
                for (i, (key, ts)) in ln.texts.iter().enumerate() {
                    let near = (1.0 - (center(i) - cx).abs() / tab_w).clamp(0.0, 1.0);
                    let color = xf.fg.alpha(xf.fg.a * (0.6 + 0.4 * near));
                    self.label(&xf, *key, (center(i) - ts.0 * 0.5, r.y + (r.h - ts.1) * 0.5), *ts, color);
                }
                // Liquid: it swells when grabbed and stretches along its motion.
                let stretch = (vel.abs() * 0.035).min(tab_w * 0.6);
                let lw = tab_w * (1.0 + 0.14 * p) + stretch;
                let lh = (r.h - 2.0 * tabs_pad) * (1.0 + 0.42 * p) * (1.0 - 0.18 * stretch / (tab_w * 0.6));
                let lens = Rect::new(cx - lw * 0.5, r.y + (r.h - lh) * 0.5, lw, lh);
                if p > 0.02 {
                    let sh = Shadow { color: lift.color.alpha(lift.color.a * p), ..lift };
                    self.shadow(&Xf { px: 0.0, ..xf }, lens, lh * 0.5, sh, true);
                }
                let drop = Glass {
                    tint: drop_tint.alpha(drop_tint.a * (1.0 - 0.7 * p.clamp(0.0, 1.0))),
                    saturation: 1.25,
                    frost: 1.5 * (1.0 - p.clamp(0.0, 1.0)),
                    refraction: 1.0,
                    depth: 16.0,
                    dispersion: 0.6,
                    rim: 0.55,
                };
                // The drop swells and stretches as it slides: no pixel steps.
                let lxf = Xf { px: 0.0, ..xf };
                self.glass(&lxf, lens, lh * 0.5, drop, None);
                self.hit(&xf, id, csr.inflate(4.0), Action::Tabs {
                        x0: (r.x + tabs_pad) * xf.s + xf.tx,
                        tab_w: tab_w * xf.s,
                        n,
                        on_select,
                    });
            }
            Kind::Picker { options, selected, on_select } => {
                let inner = Rect::new(r.x + pad[0], r.y + pad[1], r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                self.label(&xf, ln.text, (inner.x, inner.y), (inner.w - 26.0, inner.h), xf.fg);
                let spec = TextSpec { spans: &[], text: "⌄", size: 15.0, weight: 700, center: false, ..Default::default() };
                let (chev, cs) = self.text.measure(&spec, None);
                let at = (inner.x + inner.w - cs.0 - 2.0, inner.y + (inner.h - cs.1) * 0.5 - 3.0);
                self.label(&xf, chev, at, cs, xf.fg.alpha(xf.fg.a * 0.6));
                self.hit(&xf, id, sr, Action::OpenMenu);
                if self.open_menu == Some(id) {
                    self.menu = Some(Menu { id, anchor: sr, options, selected, on_select, fg: xf.fg });
                }
            }
            Kind::Text(t) => {
                let at = xf.at_px((r.x + pad[0], r.y + pad[1]));
                let inner = Rect::new(at.0, at.1, r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                let box_ = xf.rect(inner);
                // Text limited to some lines is cut to its box; the last line may fade out.
                let clip = if t.lines > 0 { xf.clip.intersect(&box_.inflate(1.0)) } else { xf.clip };
                let fade = (ln.fade && t.truncate == Truncate::Fade).then(|| {
                    let line_h = (t.size * t.line_height).round() * xf.s;
                    let w = (FADE_W * xf.s).min(box_.w * 0.5);
                    Rect::new(box_.x + box_.w - w, box_.y + box_.h - line_h, w, line_h)
                });
                let mut dxf = xf;
                dxf.clip = clip;
                self.text_extras(&dxf, id, ln.text, &t.spans, &t.content, (inner.x, inner.y), (inner.w, inner.h), tbase);
                self.span_decor(&dxf, ln.text, &t.spans, (inner.x, inner.y));
                self.list.cmds.push(Cmd::Text(TextCmd {
                    key: ln.text,
                    pos: (box_.x, box_.y),
                    size: (box_.w, box_.h),
                    scale: xf.s,
                    color: xf.fg.alpha(xf.fg.a * xf.opacity),
                    clip,
                    fade,
                }));
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
                    if let Some(next) = tops.get(k + 1) {
                        y = y.min(next - n.size.1);
                    }
                    let cid = child_id(id, i, c.key);
                    self.tbase = bases[i];
                    self.paint(c, n, (r.x, r.y + y - tops[k]), xf, cid);
                }
            }
            Kind::Scroll { child, refresh, indicator, stick } => {
                let content = ln.content_h + pad[1] + pad[3];
                let (frame, now) = (self.frame, self.now);
                let s = self.scrolls.entry(id).or_default();
                let max = (content - r.h).max(0.0);
                let parked = s.offset >= s.max - 1.0 && s.vel == 0.0;
                if ln.shift != 0.0 && s.used != 0 {
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
                if stick && (s.used == 0 || (parked && max != s.max)) {
                    s.offset = max;
                }
                s.max = max;
                s.used = frame;
                s.refreshing = refresh.as_ref().is_some_and(|r| r.0);
                let (offset, over, refreshing, active_at) = (s.offset, s.over(), s.refreshing, s.active_at);
                let (lo, hi) = s.bounds();
                let has_refresh = refresh.is_some();
                self.hit(&xf, id, sr, Action::Scroll { refresh: refresh.map(|r| r.1) });
                let mut cxf = xf;
                cxf.clip_to(sr, st.radius * xf.s, st.smooth);
                let cid = child_id(id, 0, child.key);
                self.sticky = Some(r.y + pad[1]);
                self.tbase = tbase;
                let outer = self.tctx.clone();
                self.enter_scroll(id, sr);
                // The physics stays fractional; the content is drawn whole pixels off, so text
                // stays sharp and hairlines don't flicker while it moves.
                let top = cxf.at_px((r.x, r.y - offset)).1;
                self.paint(*child, &ln.children[0], (r.x, top), cxf, cid);
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
                // be dragged; fades out when idle.
                let bar_id = id ^ BAR_SALT;
                let grabbed = self.bar_drag == Some(id);
                let near = self.hovered.contains(&bar_id);
                let idle = active_at.map_or(f32::MAX, |t| (now - t).as_secs_f32());
                let scrolls = hi > lo && indicator != Indicator::Never;
                let show = scrolls && (indicator == Indicator::Always || idle < 1.2 || near || grabbed);
                if show && indicator == Indicator::Auto && !near && !grabbed {
                    self.wake = Some(self.wake.map_or(1.22 - idle, |w| w.min(1.22 - idle)));
                }
                let a = self.spring(id, 8, if show { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                let wide = self.crisp(id, 9, if show && (near || grabbed) { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                let strip = Rect::new(r.x + r.w - BAR_STRIP, r.y, BAR_STRIP, r.h);
                let mut bxf = cxf;
                bxf.clip_r = 0.0;
                if scrolls {
                    // Always there, so hovering the edge reveals the bar; once shown it keeps
                    // the arrow pointer over text that runs under it.
                    let cursor = (a > 0.5).then_some(Cursor::Default);
                    self.zone(&bxf, bar_id, xf.rect(strip), cursor);
                }
                if a > 0.01 {
                    let track = r.h - 8.0;
                    // Its length mirrors how much of the content is visible; it squashes
                    // against the end while the content is stretched.
                    let len = ((track * r.h / content.max(1.0)).max(34.0) - over).clamp(10.0, track.max(10.0));
                    let at = ((offset - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
                    let (thin, wide_w) = self.look.theme.control.scrollbar;
                    let w = thin + (wide_w - thin) * wide;
                    if wide > 0.01 {
                        let rail = Rect::new(r.x + r.w - wide_w - 4.0, r.y, wide_w + 4.0, r.h);
                        self.fill(&bxf, rail, 0.0, Fill::Solid(xf.fg.alpha(0.06 * wide * a)), None);
                    }
                    let bar = Rect::new(r.x + r.w - 2.0 - w, r.y + 4.0 + (track - len) * at, w, len);
                    self.fill(&bxf, bar, w * 0.5, Fill::Solid(xf.fg.alpha((0.45 + 0.1 * wide) * a)), None);
                    if a > 0.5 {
                        let y0 = (r.y + 4.0) * xf.s + xf.ty;
                        let action = Action::ScrollBar { scroll: id, y0, track: track * xf.s, len: len * xf.s, lo, hi, page: (r.h * 0.9).max(1.0) };
                        self.hit(&bxf, bar_id, xf.rect(strip), action);
                    }
                }
            }
            Kind::Toggle(on, f) => {
                let r = cr;
                let t = self.spring(id, 1, if on { 1.0 } else { 0.0 });
                let p = self.spring(id, 2, if self.pressed == Some(id) { 1.0 } else { 0.0 });
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
                let p = self.spring(id, 2, if self.pressed == Some(id) { 1.0 } else { 0.0 });
                let knob_d = st.knob_size;
                let range = (r.w - knob_d).max(1.0);
                let cy = r.y + r.h * 0.5;
                let kx = r.x + range * value;
                let (th, tr) = (st.track_height, st.track_height * 0.5);
                let track = Rect::new(r.x, cy - tr, r.w, th);
                self.fill(&xf, track, tr, Fill::Solid(st.track), st.track_border);
                let filled = Rect::new(r.x, cy - tr, kx - r.x + knob_d * 0.5, th);
                self.fill(&xf, filled, tr, Fill::Solid(st.fill), None);
                let (ts, tk) = (st.tick_size, st.tick_size * 0.5);
                for k in 0..steps {
                    let x = r.x + knob_d * 0.5 + range * k as f32 / (steps - 1) as f32;
                    let on = x <= kx + knob_d * 0.5;
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
        if let Some((ixf, sh)) = inset {
            self.smooth = st.smooth;
            self.shadow(&ixf, r, st.radius, sh, false);
            self.smooth = false;
        }
        if let Some((bxf, b)) = late_border {
            self.smooth = st.smooth;
            self.fill(&bxf, r, st.radius, Fill::Solid(Color::CLEAR), Some(b));
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
    theme_fn: Option<Box<dyn Fn(&S) -> Theme>>,
    /// True while springs or scroll inertia are still moving: keep drawing frames.
    pub animating: bool,
    hits: Vec<Hit<S>>,
    zones: Vec<Zone>,
    /// Views under the mouse, topmost first.
    hovered: Vec<u64>,
    /// Mouse position while it is over the window (touch has none).
    mouse: Option<(f32, f32)>,
    /// Pointer shape the runner should show (the topmost `.cursor()` under the mouse).
    pub cursor: Cursor,
    springs: HashMap<(u64, u8), Spring>,
    scrolls: HashMap<u64, ScrollState>,
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
            animating: false,
            hits: Vec::new(),
            zones: Vec::new(),
            hovered: Vec::new(),
            mouse: None,
            cursor: Cursor::Default,
            springs: HashMap::new(),
            scrolls: HashMap::new(),
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
        self.theme_fn = Some(Box::new(f));
    }
    /// What to clear the window with: [`background`](Self::background), or the theme's.
    pub fn clear_color(&self) -> Color {
        self.background.unwrap_or(self.look.theme.color.bg)
    }

    /// Builds, lays out and paints one frame for a window of `size` points.
    pub fn frame(&mut self, size: (f32, f32)) {
        if let Some(f) = &self.theme_fn {
            let t = f(&self.state);
            self.set_theme(t);
        }
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
            s.step(dt);
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
        // From handlers that ran outside a tap (keys, timers, background updates).
        self.take_copies();

        // Scroll physics: inertia inside the content, a spring back from beyond its ends.
        let drag = self.ptr.drag_scroll.filter(|_| self.ptr.dragging);
        let mut refresh = Vec::new();
        let mut scroll_wake: Option<f32> = None;
        for (id, s) in self.scrolls.iter_mut() {
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
        let mut lx = Lx { text: &mut self.text, look: &look, springs: &mut self.springs, frame: self.frame, along: None, menu: self.open_menu, tip: tip_show, shift: 0.0 };
        let root = layout(&view, (Some(size.0), Some(size.1)), &mut lx, 1);
        self.list.cmds.clear();
        self.list.images.clear();
        self.hits.clear();
        self.zones.clear();
        self.sel.texts.clear();
        self.sel.areas.clear();
        self.menu_nav.clear();
        self.shortcuts.clear();
        let paint_sel = self.sel.for_paint();
        let blink = self.ed.blink_at.map_or(0.0, |t| (now - t).as_secs_f32());
        let fg = look.theme.color.text;
        let xf = Xf {
            s: 1.0,
            tx: 0.0,
            ty: 0.0,
            opacity: 1.0,
            clip: Rect::new(0.0, 0.0, size.0, size.1),
            clip_r: 0.0,
            clip_smooth: false,
            fg,
            px: self.scale,
        };
        let frame = self.frame;
        let mut p = Painter {
            list: &mut self.list,
            look: &look,
            hits: &mut self.hits,
            zones: &mut self.zones,
            hovered: &self.hovered,
            springs: &mut self.springs,
            scrolls: &mut self.scrolls,
            pressed: self.ptr.pressed,
            bar_drag: self.ptr.bar.map(|b| b.0),
            frame,
            text: &mut self.text,
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
        p.paint_layers();
        match p.menu.take() {
            Some(menu) => p.paint_picker_menu(menu, size),
            None if !p.managed_seen => self.open_menu = None, // its picker or anchor is gone
            None => {}
        }
        let (paint_wake, spin, relayout) = (p.wake, p.spin, p.relayout);
        if p.sel.is_some_and(|s| s.stale) {
            self.sel.sel = None; // its rows are gone
        }
        if autoscroll.is_some() {
            self.sel_drag(self.ptr.last); // the text under the pointer moved
        }
        if let Some(id) = self.focus {
            if !self.hits.iter().any(|h| h.id == id) {
                self.focus = None;
            }
        }
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
        self.text.end_frame();
        self.animating = self.show_fps
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
        }));
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
                s.vel = 0.0; // catch a coasting list
            }
        }
        if let Some(t) = &mut self.tip {
            t.hidden = true; // a click hides the tooltip until the mouse moves on
        }
        let hit = self.hit(pos, false);
        self.ptr.pressed = hit.map(|i| self.hits[i].id);
        if let Some(Action::Outside(d)) = hit.map(|i| &self.hits[i].action) {
            // macOS: the click that closes a popover does nothing else.
            let d = d.clone();
            self.run_dismiss(d);
            return true;
        }
        self.ptr.bar = None;
        if let Some(i) = hit.filter(|_| !touch) {
            if let Action::ScrollBar { .. } = self.hits[i].action {
                // The bar is not text: the selection and focus stay as they are.
                self.bar_press(i, pos.1);
                return true;
            }
        }
        self.focus = None;
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
        self.cursor = if self.ptr.bar.is_some() { Cursor::Default } else { cursor };
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
        self.mouse = None;
        self.update_hover()
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
            self.mouse = Some(pos);
            let changed = self.update_hover();
            if changed {
                self.menu_hover();
            }
            changed
        };
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
        if self.ptr.bar.take().is_some() {
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
            if let Action::Tabs { x0, tab_w, n, on_select } = &h.action {
                let index = ((pos.0 - x0) / tab_w).floor().clamp(0.0, (*n - 1) as f32) as usize;
                on_select(&mut self.state, index);
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
        match &self.hits[i].action {
            Action::Tap(f) => f(&mut self.state),
            Action::Toggle(on, f) => f(&mut self.state, !on),
            Action::OpenMenu => {
                self.springs.remove(&(id, 4)); // replay the opening animation
                self.springs.remove(&(popup::pop_id(id), popup::SLOT_FADE));
                self.open_menu = Some(id);
            }
            Action::Dismiss => self.open_menu = None,
            Action::Block(Some(f)) => f(&mut self.state),
            Action::Pick(index, f) => {
                f(&mut self.state, *index);
                self.open_menu = None;
            }
            Action::MenuPick(f, d) => {
                let d = d.clone();
                f(&mut self.state);
                if let Some(d) = d {
                    self.run_dismiss(d);
                }
            }
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
            KeyInput::Escape => self.open_menu.take().is_some() | self.blur() | self.sel_key(key),
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
