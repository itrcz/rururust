//! Platform-independent core: owns the app state, rebuilds the view tree,
//! lays it out, paints it into a `DrawList`, and routes pointer input.

use crate::layout::{child_id, layout, LNode, Lx, FIELD_FONT, SLIDER_H, TABS_PAD};
use crate::text::{TextSpec, TextSystem};
use crate::types::{Color, Rect};
use crate::view::{Axis, Cursor, Env, Fill, Glass, Image, Indicator, Kind, OverlayMode, Shadow, Truncate, View, ViewFn};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use web_time::Instant;

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
    /// Default: lively, with a hint of bounce (toggles, the tab drop, menus).
    const BOUNCY: (f32, f32) = (380.0, 28.0);
    /// Quick and critically damped, no overshoot (press feedback, fades).
    const CRISP: (f32, f32) = (900.0, 60.0);
    /// Critically damped and unhurried (~0.35 s): panels and sizes that change.
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
    /// Last trackpad delta: the rubber band holds its stretch while fingers are down.
    wheel_at: Option<Instant>,
    ended_at: Option<Instant>,
    /// The current wheel deltas are OS momentum after the fingers lifted.
    momentum: bool,
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
    /// The further it is stretched, the less it follows the finger.
    fn rubber(&self, delta: f32) -> f32 {
        let (lo, hi) = self.bounds();
        let outward = (self.offset <= lo && delta < 0.0) || (self.offset >= hi && delta > 0.0);
        if outward {
            delta / (1.0 + self.over() * 0.03).powi(2)
        } else {
            delta
        }
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
    /// x of value 0, width of the value range (screen space)
    Slider(f32, f32, Box<dyn Fn(&mut S, f32)>),
    Scroll { refresh: Option<Box<dyn Fn(&mut S)>> },
    /// Covers everything below an overlay; a tap on it dismisses the overlay.
    Block(Option<Box<dyn Fn(&mut S)>>),
    /// The body of an overlay: taps on it do nothing (and don't dismiss).
    Swallow,
    /// Text field: shaped-text key (0 when empty), screen x of the text origin, scale.
    Field { key: u64, x0: f32, scale: f32, value: String, on_change: Box<dyn Fn(&mut S, String)> },
    /// Tab bar: screen x of the first tab, width of one tab, number of tabs.
    Tabs { x0: f32, tab_w: f32, n: usize, on_select: Box<dyn Fn(&mut S, usize)> },
    OpenMenu,
    Dismiss,
    Pick(usize, Rc<dyn Fn(&mut S, usize)>),
    /// `.window_drag()` area: a press moves the window.
    DragWindow,
}

/// Something the UI asks of the window it lives in. The core knows nothing about windows:
/// the runner drains [`Ui::requests`] after each event and carries them out (or ignores them
/// where they make no sense, like on the web).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRequest {
    /// Start moving the window with the pressed pointer (a press on `.window_drag()`).
    DragWindow,
    /// Zoom / maximize or restore the window (a double click on `.window_drag()`).
    Zoom,
}

/// Keys the UI understands besides typed text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyInput {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Enter,
    Escape,
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

fn prev_char(s: &str, i: usize) -> usize {
    s[..i].char_indices().next_back().map_or(0, |(p, _)| p)
}
fn next_char(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(s.len(), |c| i + c.len_utf8())
}
fn floor_char(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
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
}

/// The views under `pos`, topmost first, and the pointer shape to show there.
fn hover_at(zones: &[Zone], pos: Option<(f32, f32)>) -> (Vec<u64>, Cursor) {
    let (mut ids, mut cursor) = (Vec::new(), None);
    let Some(pos) = pos else { return (ids, Cursor::Default) };
    for z in zones.iter().rev() {
        if !inside(&z.rect, &z.round, pos) {
            continue;
        }
        if z.blocks {
            break;
        }
        ids.push(z.id);
        cursor = cursor.or(z.cursor);
    }
    (ids, cursor.unwrap_or_default())
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
    fg: Color,
}

impl Xf {
    fn rect(&self, r: Rect) -> Rect {
        Rect::new(r.x * self.s + self.tx, r.y * self.s + self.ty, r.w * self.s, r.h * self.s)
    }
    /// Narrows the clip to the screen rect `r` with corner radius `radius`. The GPU clips to
    /// one rounded rectangle, so nested clips keep the corners of whichever is inside.
    fn clip_to(&mut self, r: Rect, radius: f32) {
        let next = self.clip.intersect(&r);
        let near = |a: &Rect, b: &Rect| {
            (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5 && (a.w - b.w).abs() < 0.5 && (a.h - b.h).abs() < 0.5
        };
        self.clip_r = if near(&next, &r) {
            radius
        } else if near(&next, &self.clip) {
            self.clip_r
        } else {
            self.clip_r.max(radius)
        };
        self.clip = next;
    }
}

struct Painter<'a, S> {
    list: &'a mut DrawList,
    hits: &'a mut Vec<Hit<S>>,
    zones: &'a mut Vec<Zone>,
    /// Views under the pointer (from the last frame's zones).
    hovered: &'a [u64],
    springs: &'a mut HashMap<(u64, u8), Spring>,
    scrolls: &'a mut HashMap<u64, ScrollState>,
    pressed: Option<u64>,
    frame: u64,
    text: &'a mut TextSystem,
    focus: Option<u64>,
    caret: usize,
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
    /// Sizes changed during paint (lazy rows were measured): lay out again next frame.
    relayout: bool,
}

impl<S> Painter<'_, S> {
    fn hit(&mut self, xf: &Xf, id: u64, rect: Rect, action: Action<S>) {
        self.hits.push(Hit { id, rect: rect.intersect(&xf.clip), round: (xf.clip, xf.clip_r), action });
    }

    fn zone(&mut self, xf: &Xf, id: u64, rect: Rect, cursor: Option<Cursor>) {
        self.zones.push(Zone { id, rect: rect.intersect(&xf.clip), round: (xf.clip, xf.clip_r), cursor, blocks: false });
    }

    fn label(&mut self, xf: &Xf, key: u64, pos: (f32, f32), size: (f32, f32), color: Color) {
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

    /// Spring that starts from `initial` the first time it is seen (appear animations).
    fn spring_from(&mut self, id: u64, slot: u8, initial: f32, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(initial, target, Spring::BOUNCY));
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

    fn paint_menu(&mut self, m: Menu<S>, size: (f32, f32)) {
        const ROW: f32 = 44.0;
        let screen = Rect::new(0.0, 0.0, size.0, size.1);
        // Anything outside the menu closes it.
        self.hits.push(Hit { id: m.id ^ 0xD15, rect: screen, round: NO_ROUND, action: Action::Dismiss });
        self.zones.push(Zone { id: m.id ^ 0xD15, rect: screen, round: NO_ROUND, cursor: None, blocks: true });
        let spec = |text| TextSpec { spans: &[], text, size: FIELD_FONT, weight: 400, center: false };
        let rows: Vec<(u64, (f32, f32))> = m.options.iter().map(|o| self.text.measure(&spec(o), None)).collect();
        let (check, check_size) = self.text.measure(&TextSpec { weight: 600, ..spec("✓") }, None);
        let widest = rows.iter().map(|r| r.1 .0).fold(0.0, f32::max);
        let w = (widest + 46.0 + check_size.0).max(m.anchor.w).min(size.0 - 16.0);
        let h = ROW * rows.len() as f32 + 12.0;
        let x = m.anchor.x.min(size.0 - w - 8.0).max(8.0);
        let below = m.anchor.y + m.anchor.h + 6.0;
        let (y, down) = if below + h <= size.1 - 8.0 { (below, true) } else { ((m.anchor.y - h - 6.0).max(8.0), false) };

        // The menu grows out of the control it belongs to.
        let frame = self.frame;
        let s = self.springs.entry((m.id, 4)).or_insert(Spring::new(0.0, 1.0, Spring::BOUNCY));
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
            fg: m.fg,
        };
        let r = Rect::new(x, y, w, h);
        self.shadow(&xf, r, 18.0, Shadow { color: Color::BLACK.alpha(0.35), blur: 40.0, y: 12.0 }, true);
        self.glass(&xf, r, 18.0, Glass::frosted(), None);
        for (i, (key, ts)) in rows.into_iter().enumerate() {
            let row = Rect::new(x + 6.0, y + 6.0 + ROW * i as f32, w - 12.0, ROW);
            let rid = child_id(m.id, i, Some(0xA000 + i as u64));
            if self.pressed == Some(rid) {
                self.fill(&xf, row, 12.0, Fill::Solid(Color::WHITE.alpha(0.16)), None);
            }
            self.label(&xf, key, (row.x + 10.0, row.y + (ROW - ts.1) * 0.5), ts, m.fg);
            if i == m.selected {
                let cx = row.x + row.w - 10.0 - check_size.0;
                self.label(&xf, check, (cx, row.y + (ROW - check_size.1) * 0.5), check_size, m.fg);
            }
            self.hits.push(Hit { id: rid, rect: xf.rect(row), round: NO_ROUND, action: Action::Pick(i, m.on_select.clone()) });
            self.zones.push(Zone { id: rid, rect: xf.rect(row), round: NO_ROUND, cursor: Some(Cursor::Pointer), blocks: false });
        }
    }

    fn spring(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(target, target, Spring::BOUNCY));
        s.target = target;
        s.used = self.frame;
        s.x
    }

    /// A spring without overshoot, for press feedback.
    fn crisp(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(target, target, Spring::CRISP));
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
            rect: xf.rect(r).arr(),
            clip: xf.clip.arr(),
            c0: [0.0; 4],
            c1: [0.0; 4],
            border: [0.0; 4],
            p0: [radius * xf.s, 0.0, 0.0, kind],
            p1: [0.0, 1.0, xf.clip_r, xf.opacity],
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
            q.p0[1] = w * xf.s;
        }
        self.list.cmds.push(Cmd::Quad(q));
    }

    fn shadow(&mut self, xf: &Xf, r: Rect, radius: f32, sh: Shadow, knockout: bool) {
        let mut q = self.quad(xf, Rect::new(r.x, r.y + sh.y, r.w, r.h), radius, KIND_SHADOW);
        q.c0 = sh.color.arr();
        // c1: offset back to the caster + knock-out flag (keeps translucent casters clean)
        q.c1 = [0.0, sh.y * xf.s, if knockout { 1.0 } else { 0.0 }, 0.0];
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
            q.p0[1] = w * xf.s;
        }
        self.list.cmds.push(Cmd::Quad(q));
    }

    fn paint(&mut self, v: View<S>, ln: &LNode, origin: (f32, f32), mut xf: Xf, id: u64) {
        let pin = self.sticky.take();
        let st = v.style;
        let r = Rect::new(origin.0 + ln.pos.0, origin.1 + ln.pos.1, ln.size.0, ln.size.1);
        xf.opacity *= st.opacity;
        if let Some(fg) = st.fg {
            xf.fg = fg;
        }

        // Press feedback: a quick, quiet dim with a barely-there squeeze. No bounce.
        if v.on_tap.is_some() {
            let p = self.crisp(id, 0, if self.pressed == Some(id) { 1.0 } else { 0.0 });
            let k = 1.0 - 0.02 * p;
            let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            xf.tx += cx * (1.0 - k) * xf.s;
            xf.ty += cy * (1.0 - k) * xf.s;
            xf.s *= k;
            xf.opacity *= 1.0 - 0.3 * p;
        }

        let sr = xf.rect(r);
        let margin = st.shadow.map_or(0.0, |s| s.blur * 2.0 + s.y.abs());
        if !sr.inflate(margin * xf.s).intersects(&xf.clip) || xf.opacity <= 0.001 {
            return; // culled
        }

        if let Some(sh) = st.shadow {
            let translucent = st.glass.is_some() || !matches!(st.bg, Some(Fill::Solid(c)) if c.a >= 1.0);
            self.shadow(&xf, r, st.radius, sh, translucent);
        }
        if let Some(g) = st.glass {
            self.glass(&xf, r, st.radius, g, st.border);
        }
        match (st.bg, st.border, st.glass) {
            (Some(bg), border, None) => self.fill(&xf, r, st.radius, bg, border),
            (Some(bg), _, Some(_)) => self.fill(&xf, r, st.radius, bg, None),
            (None, Some(b), None) => self.fill(&xf, r, st.radius, Fill::Solid(Color::CLEAR), Some(b)),
            _ => {}
        }

        // Pointer shape and hover: explicit, or what the kind of view suggests.
        let cursor = st.cursor.or(match &v.kind {
            Kind::TextField { .. } => Some(Cursor::Text),
            Kind::Toggle(..) | Kind::Slider(..) | Kind::Tabs { .. } | Kind::Picker { .. } => Some(Cursor::Pointer),
            _ if v.on_tap.is_some() => Some(Cursor::Pointer),
            _ => None,
        });
        if cursor.is_some() || st.hover.is_some() {
            self.zone(&xf, id, sr, cursor);
        }
        if let Some(c) = st.hover {
            let t = self.crisp(id, 9, if self.hovered.contains(&id) { 1.0 } else { 0.0 });
            if t > 0.004 {
                self.fill(&xf, r, st.radius, Fill::Solid(c.alpha(c.a * t.min(1.0))), None);
            }
        }
        if st.clip || ln.clip {
            xf.clip_to(sr, st.radius * xf.s);
        }

        // Before the tap: a view that is both draggable and tappable keeps its tap.
        if st.drag {
            self.hit(&xf, id ^ 0xD6A9, sr, Action::DragWindow);
        }
        if let Some(f) = v.on_tap {
            self.hit(&xf, id, sr, Action::Tap(f));
        }

        let pad = st.padding;
        match v.kind {
            Kind::Empty | Kind::Spacer => {}
            Kind::List { count, row_h, row } => {
                // Only the rows inside the clip are built, laid out and painted. Their real
                // heights are remembered so the scroll range converges on the truth.
                let top = (xf.clip.y - xf.ty) / xf.s;
                let bottom = top + xf.clip.h / xf.s;
                let mut y = r.y;
                for i in 0..count {
                    let known = self.text.lists.get(&id).and_then(|c| c.heights.get(i)).copied().unwrap_or(f32::NAN);
                    let h = if known.is_nan() { row_h } else { known };
                    if y + h <= top {
                        y += h;
                        continue;
                    }
                    if y >= bottom {
                        break;
                    }
                    let rv = row(i);
                    let cid = child_id(id, i, rv.key);
                    let mut lx = Lx { text: self.text, springs: self.springs, frame: self.frame };
                    let n = layout(&rv, (Some(r.w), None), &mut lx, cid);
                    if known.is_nan() || (n.size.1 - known).abs() > 0.5 {
                        if let Some(slot) = self.text.lists.get_mut(&id).and_then(|c| c.heights.get_mut(i)) {
                            *slot = n.size.1;
                            self.relayout = true;
                        }
                    }
                    let h = n.size.1;
                    self.paint(rv, &n, (r.x, y), xf, cid);
                    y += h;
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
                if mode != OverlayMode::Toast {
                    self.fill(&xf, r, 0.0, Fill::Solid(Color::BLACK.alpha(0.45 * tc)), None);
                    self.hit(&xf, id, sr, Action::Block(on_dismiss));
                }
                let body = cxf.rect(Rect::new(pos.0, pos.1, w, h));
                self.hits.push(Hit { id: child_id(id, 1, None), rect: body, round: NO_ROUND, action: Action::Swallow });
                let cid = child_id(id, 0, child.key);
                self.paint(*child, cn, pos, cxf, cid);
            }
            Kind::Image(img, template) => {
                let mut q = self.quad(&xf, r, st.radius, KIND_IMAGE);
                if template {
                    // A glyph: keep only the shape, paint it in the text color.
                    q.c1[1] = 1.0;
                    q.c1[2] = xf.fg.r;
                    q.c1[3] = xf.fg.g;
                    q.p1[0] = xf.fg.b;
                    q.p1[1] = xf.fg.a;
                }
                // Fill the box and crop the overflow, keeping the picture's aspect ratio.
                let (iw, ih) = (img.0.width.max(1) as f32, img.0.height.max(1) as f32);
                let k = (r.w / iw).max(r.h / ih).max(1e-6);
                let (uw, uh) = (r.w / (iw * k) * 0.5, r.h / (ih * k) * 0.5);
                q.c0 = [0.5 - uw, 0.5 - uh, 0.5 + uw, 0.5 + uh];
                q.c1[0] = self.list.images.len() as f32;
                if let Some((w, c)) = st.border {
                    q.border = c.arr();
                    q.p0[1] = w * xf.s;
                }
                self.list.images.push(img);
                self.list.cmds.push(Cmd::Quad(q));
            }
            Kind::TextField { value, on_change, .. } => {
                let focused = self.focus == Some(id);
                let f = self.spring(id, 3, if focused { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                if f > 0.01 {
                    self.fill(&xf, r, st.radius, Fill::Solid(Color::CLEAR), Some((2.0, Color::BLUE.alpha(f))));
                }
                let inner = Rect::new(r.x + pad[0], r.y + pad[1], r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                let caret = if focused { floor_char(&value, self.caret) } else { 0 };
                let cx = if value.is_empty() { 0.0 } else { self.text.caret_x(ln.text, caret) };
                // Keep the caret in view when the text is longer than the field.
                let shift = if focused { (cx - inner.w + 2.0).max(0.0) } else { 0.0 };
                let mut cxf = xf;
                cxf.clip = xf.clip.intersect(&xf.rect(inner.inflate(1.0)));
                cxf.clip_r = 0.0;
                let color = if value.is_empty() { xf.fg.alpha(xf.fg.a * 0.4) } else { xf.fg };
                self.label(&cxf, ln.text, (inner.x - shift, inner.y), (inner.w, inner.h), color);
                if focused && self.caret_on {
                    let bar = Rect::new(inner.x + cx - shift, inner.y + 1.0, 2.0, inner.h - 2.0);
                    self.fill(&cxf, bar, 1.0, Fill::Solid(Color::BLUE), None);
                }
                self.hit(&xf, id, sr, Action::Field {
                        key: if value.is_empty() { 0 } else { ln.text },
                        x0: (inner.x - shift) * xf.s + xf.tx,
                        scale: xf.s,
                        value,
                        on_change,
                    });
            }
            Kind::Tabs { labels, selected, on_select } => {
                let n = labels.len().max(1);
                let tab_w = (r.w - 2.0 * TABS_PAD) / n as f32;
                let center = |i: usize| r.x + TABS_PAD + tab_w * (i as f32 + 0.5);
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
                let lh = (r.h - 2.0 * TABS_PAD) * (1.0 + 0.42 * p) * (1.0 - 0.18 * stretch / (tab_w * 0.6));
                let lens = Rect::new(cx - lw * 0.5, r.y + (r.h - lh) * 0.5, lw, lh);
                if p > 0.02 {
                    let sh = Shadow { color: Color::BLACK.alpha(0.22 * p), blur: 18.0, y: 5.0 };
                    self.shadow(&xf, lens, lh * 0.5, sh, true);
                }
                let drop = Glass {
                    tint: Color::WHITE.alpha(0.13 - 0.09 * p.clamp(0.0, 1.0)),
                    saturation: 1.25,
                    frost: 1.5 * (1.0 - p.clamp(0.0, 1.0)),
                    refraction: 1.0,
                    depth: 16.0,
                    dispersion: 0.6,
                    rim: 0.55,
                };
                self.glass(&xf, lens, lh * 0.5, drop, None);
                self.hit(&xf, id, sr.inflate(4.0), Action::Tabs {
                        x0: (r.x + TABS_PAD) * xf.s + xf.tx,
                        tab_w: tab_w * xf.s,
                        n,
                        on_select,
                    });
            }
            Kind::Picker { options, selected, on_select } => {
                let inner = Rect::new(r.x + pad[0], r.y + pad[1], r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                self.label(&xf, ln.text, (inner.x, inner.y), (inner.w - 26.0, inner.h), xf.fg);
                let spec = TextSpec { spans: &[], text: "⌄", size: 15.0, weight: 700, center: false };
                let (chev, cs) = self.text.measure(&spec, None);
                let at = (inner.x + inner.w - cs.0 - 2.0, inner.y + (inner.h - cs.1) * 0.5 - 3.0);
                self.label(&xf, chev, at, cs, xf.fg.alpha(xf.fg.a * 0.6));
                self.hit(&xf, id, sr, Action::OpenMenu);
                if self.open_menu == Some(id) {
                    self.menu = Some(Menu { id, anchor: sr, options, selected, on_select, fg: xf.fg });
                }
            }
            Kind::Text(t) => {
                let inner = Rect::new(r.x + pad[0], r.y + pad[1], r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                let box_ = xf.rect(inner);
                // Text limited to some lines is cut to its box; the last line may fade out.
                let clip = if t.lines > 0 { xf.clip.intersect(&box_.inflate(1.0)) } else { xf.clip };
                let fade = (ln.fade && t.truncate == Truncate::Fade).then(|| {
                    let line_h = (t.size * 1.3).round() * xf.s;
                    let w = (FADE_W * xf.s).min(box_.w * 0.5);
                    Rect::new(box_.x + box_.w - w, box_.y + box_.h - line_h, w, line_h)
                });
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
                for (i, (c, n)) in children.into_iter().zip(&ln.children).enumerate() {
                    if pin.is_some() && c.style.sticky {
                        stuck.push((i, c)); // painted last, above the rows that slide under it
                        continue;
                    }
                    let cid = child_id(id, i, c.key);
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
                    self.paint(c, n, (r.x, r.y + y - tops[k]), xf, cid);
                }
            }
            Kind::Scroll { child, refresh, indicator, stick } => {
                let content = ln.content_h + pad[1] + pad[3];
                let (frame, now) = (self.frame, self.now);
                let s = self.scrolls.entry(id).or_default();
                let max = (content - r.h).max(0.0);
                // Chat-style: start at the end and follow new content while parked there.
                // Shrinking too: a lazy row measured smaller than its estimate must not
                // leave the view hanging past the end and bouncing back.
                if stick && (s.used == 0 || (s.offset >= s.max - 1.0 && s.vel == 0.0 && max != s.max)) {
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
                cxf.clip_to(sr, st.radius * xf.s);
                let cid = child_id(id, 0, child.key);
                self.sticky = Some(r.y + pad[1]);
                self.paint(*child, &ln.children[0], (r.x, r.y - offset), cxf, cid);
                self.sticky = None;

                // Pull-to-refresh: the indicator lives in the gap the pull opens.
                let pull = -offset;
                if has_refresh && pull > 8.0 {
                    let progress = ((pull - 8.0) / (REFRESH_AT - 8.0)).min(1.0);
                    self.spinner(&cxf, (r.x + r.w * 0.5, r.y + pad[1] + pull * 0.5), progress, refreshing);
                }

                // Overlay scroll bar: shows while scrolling, fades out when idle.
                let idle = active_at.map_or(f32::MAX, |t| (now - t).as_secs_f32());
                let show = hi > lo && match indicator {
                    Indicator::Never => false,
                    Indicator::Always => true,
                    Indicator::Auto => idle < 1.2,
                };
                if show && indicator == Indicator::Auto {
                    self.wake = Some(self.wake.map_or(1.22 - idle, |w| w.min(1.22 - idle)));
                }
                let a = self.spring(id, 8, if show { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
                if a > 0.01 {
                    let track = r.h - 8.0;
                    // Its length mirrors how much of the content is visible; it squashes
                    // against the end while the content is stretched.
                    let len = ((track * r.h / content.max(1.0)).max(34.0) - over).clamp(10.0, track.max(10.0));
                    let at = ((offset - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
                    let bar = Rect::new(r.x + r.w - 7.0, r.y + 4.0 + (track - len) * at, 4.0, len);
                    let mut bxf = cxf;
                    bxf.clip_r = 0.0;
                    self.fill(&bxf, bar, 2.0, Fill::Solid(xf.fg.alpha(0.45 * a)), None);
                }
            }
            Kind::Toggle(on, f) => {
                let t = self.spring(id, 1, if on { 1.0 } else { 0.0 });
                let p = self.spring(id, 2, if self.pressed == Some(id) { 1.0 } else { 0.0 });
                let off = Color::rgba(0.47, 0.47, 0.5, 0.36);
                let track = off.lerp(Color::GREEN, t.clamp(0.0, 1.0));
                self.fill(&xf, r, r.h * 0.5, Fill::Solid(track), None);
                // The knob stretches a little while pressed, like on iOS.
                let kw = r.h - 4.0 + 6.0 * p;
                let kx = r.x + 2.0 + (r.w - 4.0 - kw) * t;
                let knob = Rect::new(kx, r.y + 2.0, kw, r.h - 4.0);
                self.shadow(&xf, knob, knob.h * 0.5, Shadow { color: Color::BLACK.alpha(0.25), blur: 8.0, y: 3.0 }, false);
                self.fill(&xf, knob, knob.h * 0.5, Fill::Solid(Color::WHITE), None);
                self.hit(&xf, id, sr.inflate(6.0), Action::Toggle(on, f));
            }
            Kind::Slider(value, f) => {
                let p = self.spring(id, 2, if self.pressed == Some(id) { 1.0 } else { 0.0 });
                let knob_d = SLIDER_H;
                let range = (r.w - knob_d).max(1.0);
                let cy = r.y + r.h * 0.5;
                let kx = r.x + range * value;
                let track = Rect::new(r.x, cy - 3.0, r.w, 6.0);
                self.fill(&xf, track, 3.0, Fill::Solid(Color::rgba(0.47, 0.47, 0.5, 0.36)), None);
                let filled = Rect::new(r.x, cy - 3.0, kx - r.x + knob_d * 0.5, 6.0);
                self.fill(&xf, filled, 3.0, Fill::Solid(Color::BLUE), None);
                let d = knob_d * (1.0 + 0.12 * p);
                let knob = Rect::new(kx + (knob_d - d) * 0.5, cy - d * 0.5, d, d);
                self.shadow(&xf, knob, d * 0.5, Shadow { color: Color::BLACK.alpha(0.28), blur: 10.0, y: 3.0 }, false);
                self.fill(&xf, knob, d * 0.5, Fill::Solid(Color::WHITE), None);
                let x0 = (r.x + knob_d * 0.5) * xf.s + xf.tx;
                self.hit(&xf, id, sr.inflate(8.0), Action::Slider(x0, range * xf.s, f));
            }
        }
    }
}

// ---------------------------------------------------------------- Ui

pub struct Ui<S> {
    pub state: S,
    view_fn: Box<dyn Fn(&S, &Env) -> View<S>>,
    pub text: TextSystem,
    pub list: DrawList,
    pub background: Color,
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
    caret: usize,
    caret_at: Option<Instant>,
    open_menu: Option<u64>,
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
            background: Color::BLACK,
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
            caret: 0,
            caret_at: None,
            open_menu: None,
            started: Instant::now(),
            timers: Vec::new(),
            wake_in: None,
            show_fps: false,
            frame_ms: 0.0,
            requests: Vec::new(),
            titlebar: 0.0,
            window_buttons: 0.0,
            fps: 0.0,
            fps_label: String::new(),
            fps_label_at: None,
        }
    }

    /// Builds, lays out and paints one frame for a window of `size` points.
    pub fn frame(&mut self, size: (f32, f32)) {
        // Updates sent through `Link`s from background work.
        self.pump();
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

        // Scroll physics: inertia inside the content, a spring back from beyond its ends.
        let drag = self.ptr.drag_scroll.filter(|_| self.ptr.dragging);
        let mut refresh = Vec::new();
        let mut scroll_wake: Option<f32> = None;
        for (id, s) in self.scrolls.iter_mut() {
            let hold = if s.refreshing { REFRESH_HOLD } else { 0.0 };
            s.hold = if (s.hold - hold).abs() > 0.2 { s.hold + (hold - s.hold) * (1.0 - (-dt * 10.0).exp()) } else { hold };
            let since_wheel = s.wheel_at.map_or(f32::MAX, |t| (now - t).as_secs_f32());
            let held = drag == Some(*id) || since_wheel < 0.08;
            if held {
                s.armed = !s.refreshing && s.offset < -REFRESH_AT;
                if since_wheel < 0.08 {
                    scroll_wake = Some(0.09 - since_wheel); // notice when the fingers lift
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

        let view = (self.view_fn)(&self.state, &Env {
            width: size.0,
            height: size.1,
            titlebar: self.titlebar,
            window_buttons: self.window_buttons,
        });
        let mut lx = Lx { text: &mut self.text, springs: &mut self.springs, frame: self.frame };
        let root = layout(&view, (Some(size.0), Some(size.1)), &mut lx, 1);
        self.list.cmds.clear();
        self.list.images.clear();
        self.hits.clear();
        self.zones.clear();
        let blink = self.caret_at.map_or(0.0, |t| (now - t).as_secs_f32());
        let fg = if self.background.luminance() < 0.5 { Color::WHITE } else { Color::BLACK };
        let xf = Xf {
            s: 1.0,
            tx: 0.0,
            ty: 0.0,
            opacity: 1.0,
            clip: Rect::new(0.0, 0.0, size.0, size.1),
            clip_r: 0.0,
            fg,
        };
        let frame = self.frame;
        let mut p = Painter {
            list: &mut self.list,
            hits: &mut self.hits,
            zones: &mut self.zones,
            hovered: &self.hovered,
            springs: &mut self.springs,
            scrolls: &mut self.scrolls,
            pressed: self.ptr.pressed,
            frame,
            text: &mut self.text,
            focus: self.focus,
            caret: self.caret,
            caret_on: blink % 1.0 < 0.5,
            open_menu: self.open_menu,
            menu: None,
            pointer_x: self.ptr.last.0,
            sticky: None,
            wake: None,
            now,
            time: (now - self.started).as_secs_f32(),
            spin: false,
            relayout: false,
        };
        p.paint(view, &root, (0.0, 0.0), xf, 1);
        match p.menu.take() {
            Some(menu) => p.paint_menu(menu, size),
            None => self.open_menu = None, // its picker is gone
        }
        let (paint_wake, spin, relayout) = (p.wake, p.spin, p.relayout);
        if let Some(id) = self.focus {
            if !self.hits.iter().any(|h| h.id == id) {
                self.focus = None;
            }
        }
        let timer_wake = self.timers.iter().map(|t| t.0.saturating_duration_since(now).as_secs_f32()).reduce(f32::min);
        self.wake_in = [self.focus.map(|_| 0.5 - blink % 0.5), paint_wake, scroll_wake, timer_wake]
            .into_iter()
            .flatten()
            .reduce(f32::min)
            .map(|w| w.max(0.0) + 0.01);

        if self.show_fps {
            self.paint_fps(now, size);
        }

        // The layout may have moved under a resting mouse: hover what is there now.
        let rehover = self.update_hover();
        self.springs.retain(|_, s| s.used == frame);
        self.scrolls.retain(|_, s| s.used == frame);
        self.text.end_frame();
        self.animating = self.show_fps
            || rehover
            || spin
            || relayout
            || self.springs.values().any(Spring::moving)
            || self.scrolls.iter().any(|(id, s)| {
                let resting = s.hold == if s.refreshing { REFRESH_HOLD } else { 0.0 };
                drag != Some(*id) && s.wheel_at.is_none_or(|t| (now - t).as_secs_f32() >= 0.08)
                    && (s.vel != 0.0 || s.over() > 0.0 || !resting)
            });
    }

    fn paint_fps(&mut self, now: Instant, size: (f32, f32)) {
        // Refresh the numbers a few times a second so they stay readable.
        if self.fps_label_at.is_none_or(|t| (now - t).as_secs_f32() > 0.25) {
            self.fps_label = format!("{:.0} fps  {:.1} ms", self.fps, self.frame_ms);
            self.fps_label_at = Some(now);
        }
        let spec = TextSpec { spans: &[], text: &self.fps_label, size: 12.0, weight: 600, center: false };
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
            .rposition(|h| h.contains(pos) && matches!(h.action, Action::Scroll { .. }) == scroll)
    }

    fn slide(&mut self, i: usize, x: f32) {
        if let Action::Slider(x0, range, f) = &self.hits[i].action {
            f(&mut self.state, ((x - x0) / range).clamp(0.0, 1.0));
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
        let hit = self.hit(pos, false);
        self.ptr.pressed = hit.map(|i| self.hits[i].id);
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
            if let Action::Field { key, x0, scale, .. } = &self.hits[i].action {
                self.focus = Some(self.hits[i].id);
                self.caret = if *key == 0 { 0 } else { self.text.index_at(*key, (pos.0 - x0) / scale) };
                self.caret_at = Some(Instant::now());
            }
        }
        true
    }

    /// Recomputes what is under the mouse; true if that changed (a frame is needed).
    fn update_hover(&mut self) -> bool {
        let (ids, cursor) = hover_at(&self.zones, self.mouse);
        self.cursor = cursor;
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
            self.update_hover()
        };
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
                    s.offset += s.rubber(-dy);
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
            }
        }
        hover
    }

    pub fn pointer_up(&mut self, pos: (f32, f32)) -> bool {
        let pressed = self.ptr.pressed.take();
        self.ptr.down = false;
        let was_dragging = std::mem::take(&mut self.ptr.dragging);
        self.ptr.drag_scroll = None;
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
                match &self.hits[i].action {
                    Action::Tap(f) => f(&mut self.state),
                    Action::Toggle(on, f) => f(&mut self.state, !on),
                    Action::OpenMenu => {
                        self.springs.remove(&(id, 4)); // replay the opening animation
                        self.open_menu = Some(id);
                    }
                    Action::Dismiss => self.open_menu = None,
                    Action::Block(Some(f)) => f(&mut self.state),
                    Action::Pick(index, f) => {
                        f(&mut self.state, *index);
                        self.open_menu = None;
                    }
                    _ => {}
                }
            }
        }
        true
    }

    /// True while a text field has keyboard focus.
    pub fn has_focus(&self) -> bool {
        self.focus.is_some()
    }

    fn edit(&mut self, f: impl FnOnce(&mut String, &mut usize)) -> bool {
        let Some(i) = self.focus.and_then(|id| self.hits.iter().position(|h| h.id == id)) else { return false };
        let Action::Field { value, on_change, .. } = &mut self.hits[i].action else { return false };
        let mut next = value.clone();
        let mut caret = floor_char(&next, self.caret);
        f(&mut next, &mut caret);
        self.caret = caret;
        self.caret_at = Some(Instant::now());
        if next != *value {
            // Remember it right away: several keys can arrive before the next frame.
            *value = next.clone();
            on_change(&mut self.state, next);
        }
        true
    }

    /// Typed text (already translated by the keyboard layout). Returns true if it was used.
    pub fn text_input(&mut self, text: &str) -> bool {
        if text.is_empty() || text.chars().any(char::is_control) {
            return false;
        }
        self.edit(|v, c| {
            v.insert_str(*c, text);
            *c += text.len();
        })
    }

    /// Editing / navigation key. Returns true if it was used.
    pub fn key(&mut self, key: KeyInput) -> bool {
        match key {
            KeyInput::Escape | KeyInput::Enter => {
                self.open_menu.take().is_some() | self.focus.take().is_some()
            }
            _ => self.edit(|v, c| match key {
                KeyInput::Backspace if *c > 0 => {
                    let p = prev_char(v, *c);
                    v.replace_range(p..*c, "");
                    *c = p;
                }
                KeyInput::Delete if *c < v.len() => {
                    let n = next_char(v, *c);
                    v.replace_range(*c..n, "");
                }
                KeyInput::Left => *c = prev_char(v, *c),
                KeyInput::Right => *c = next_char(v, *c),
                KeyInput::Home => *c = 0,
                KeyInput::End => *c = v.len(),
                _ => {}
            }),
        }
    }

    /// Mouse wheel / trackpad scroll; `dy` in points, positive = content moves down.
    pub fn wheel(&mut self, pos: (f32, f32), dy: f32, phase: WheelPhase) -> bool {
        let now = Instant::now();
        // Topmost scroll view under the cursor; an overlay's backdrop stops the search.
        for h in self.hits.iter().rev() {
            if !h.contains(pos) {
                continue;
            }
            match &h.action {
                Action::Block(_) => return false,
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
            match phase {
                // The OS starts its momentum right after the fingers lift.
                WheelPhase::Started => s.momentum = s.ended_at.is_some_and(|t| (now - t).as_secs_f32() < 0.1),
                WheelPhase::Ended => {
                    s.ended_at = Some(now);
                    s.wheel_at = None; // released: the rubber band may snap back
                    return true;
                }
                _ => {}
            }
            if s.momentum && s.over() > 0.0 {
                return true; // coasted past the end: ignore the rest, the spring takes over
            }
            s.offset += s.rubber(-dy);
            s.active_at = Some(now);
            if s.momentum && s.over() > 0.0 {
                s.vel = (-dy * 60.0).clamp(-3000.0, 3000.0); // carry the speed into the bounce
                s.wheel_at = None;
            } else {
                s.vel = 0.0;
                s.wheel_at = Some(now);
            }
            return true;
        }
        false
    }
}
