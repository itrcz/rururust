//! Platform-independent core: owns the app state, rebuilds the view tree,
//! lays it out, paints it into a `DrawList`, and routes pointer input.

use crate::layout::{layout, LNode, SLIDER_H};
use crate::text::{TextSpec, TextSystem};
use crate::types::{Color, Rect};
use crate::view::{Fill, Glass, Kind, Shadow, View};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use web_time::Instant;

// ---------------------------------------------------------------- draw list

pub(crate) const KIND_FILL: f32 = 0.0;
pub(crate) const KIND_SHADOW: f32 = 1.0;
pub(crate) const KIND_GLASS: f32 = 2.0;

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
}

pub struct TextCmd {
    pub key: u64,
    pub pos: (f32, f32),
    pub size: (f32, f32),
    pub scale: f32,
    pub color: Color,
    pub clip: Rect,
}

pub enum Cmd {
    Quad(Quad),
    Text(TextCmd),
}

#[derive(Default)]
pub struct DrawList {
    pub cmds: Vec<Cmd>,
}

// ---------------------------------------------------------------- animation

#[derive(Clone, Copy)]
struct Spring {
    x: f32,
    v: f32,
    target: f32,
    used: u64,
}

impl Spring {
    const K: f32 = 380.0; // stiffness
    const C: f32 = 26.0; // damping (slightly under-damped: a hint of bounce)

    fn step(&mut self, dt: f32) {
        let n = (dt / (1.0 / 240.0)).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as usize {
            let a = -Self::K * (self.x - self.target) - Self::C * self.v;
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
    used: u64,
}

// ---------------------------------------------------------------- input

enum Action<S> {
    Tap(Box<dyn Fn(&mut S)>),
    Toggle(bool, Box<dyn Fn(&mut S, bool)>),
    /// x of value 0, width of the value range (screen space)
    Slider(f32, f32, Box<dyn Fn(&mut S, f32)>),
    Scroll { max: f32 },
}

struct Hit<S> {
    id: u64,
    rect: Rect,
    action: Action<S>,
}

#[derive(Default)]
struct Pointer {
    pressed: Option<u64>,
    touch: bool,
    last: (f32, f32),
    down_at: (f32, f32),
    drag_scroll: Option<u64>,
    dragging: bool,
    last_move: Option<Instant>,
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
}

fn child_id(parent: u64, index: usize, key: Option<u64>) -> u64 {
    // FxHash-style mix; cheap and good enough for sibling paths.
    let v = key.unwrap_or(index as u64 ^ 0x9E37_79B9_7F4A_7C15);
    (parent.rotate_left(5) ^ v).wrapping_mul(0x517C_C1B7_2722_0A95)
}

struct Painter<'a, S> {
    list: &'a mut DrawList,
    hits: &'a mut Vec<Hit<S>>,
    springs: &'a mut HashMap<(u64, u8), Spring>,
    scrolls: &'a mut HashMap<u64, ScrollState>,
    pressed: Option<u64>,
    frame: u64,
}

impl<S> Painter<'_, S> {
    fn spring(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring { x: target, v: 0.0, target, used: 0 });
        s.target = target;
        s.used = self.frame;
        s.x
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
        q.c1 = [g.saturation, g.refraction * xf.s, g.rim, g.dispersion];
        if let Some((w, c)) = border {
            q.border = c.arr();
            q.p0[1] = w * xf.s;
        }
        self.list.cmds.push(Cmd::Quad(q));
    }

    fn paint(&mut self, v: View<S>, ln: &LNode, origin: (f32, f32), mut xf: Xf, id: u64) {
        let st = v.style;
        let r = Rect::new(origin.0 + ln.pos.0, origin.1 + ln.pos.1, ln.size.0, ln.size.1);
        xf.opacity *= st.opacity;
        if let Some(fg) = st.fg {
            xf.fg = fg;
        }

        // Springy press feedback: scale around the center.
        if v.on_tap.is_some() {
            let p = self.spring(id, 0, if self.pressed == Some(id) { 1.0 } else { 0.0 });
            let k = 1.0 - 0.06 * p;
            let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            xf.tx += cx * (1.0 - k) * xf.s;
            xf.ty += cy * (1.0 - k) * xf.s;
            xf.s *= k;
            xf.opacity *= 1.0 - 0.12 * p;
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

        if let Some(f) = v.on_tap {
            self.hits.push(Hit { id, rect: sr.intersect(&xf.clip), action: Action::Tap(f) });
        }

        let pad = st.padding;
        match v.kind {
            Kind::Empty | Kind::Spacer => {}
            Kind::Text(_) => {
                let pos = (r.x + pad[0], r.y + pad[1]);
                self.list.cmds.push(Cmd::Text(TextCmd {
                    key: ln.text,
                    pos: (pos.0 * xf.s + xf.tx, pos.1 * xf.s + xf.ty),
                    size: ((r.w - pad[0] - pad[2]) * xf.s, (r.h - pad[1] - pad[3]) * xf.s),
                    scale: xf.s,
                    color: xf.fg.alpha(xf.fg.a * xf.opacity),
                    clip: xf.clip,
                }));
            }
            Kind::Stack { children, .. } => {
                for (i, (c, n)) in children.into_iter().zip(&ln.children).enumerate() {
                    let cid = child_id(id, i, c.key);
                    self.paint(c, n, (r.x, r.y), xf, cid);
                }
            }
            Kind::Scroll(child) => {
                let max = (ln.content_h + pad[1] + pad[3] - r.h).max(0.0);
                let frame = self.frame;
                let s = self.scrolls.entry(id).or_default();
                s.offset = s.offset.clamp(0.0, max);
                if s.offset <= 0.0 || s.offset >= max {
                    s.vel = 0.0;
                }
                s.used = frame;
                let offset = s.offset;
                self.hits.push(Hit { id, rect: sr.intersect(&xf.clip), action: Action::Scroll { max } });
                let mut cxf = xf;
                cxf.clip = xf.clip.intersect(&sr);
                cxf.clip_r = st.radius * xf.s;
                let cid = child_id(id, 0, child.key);
                self.paint(*child, &ln.children[0], (r.x, r.y - offset), cxf, cid);
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
                self.hits.push(Hit { id, rect: sr.inflate(6.0).intersect(&xf.clip), action: Action::Toggle(on, f) });
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
                self.hits.push(Hit {
                    id,
                    rect: sr.inflate(8.0).intersect(&xf.clip),
                    action: Action::Slider(x0, range * xf.s, f),
                });
            }
        }
    }
}

// ---------------------------------------------------------------- Ui

pub struct Ui<S> {
    pub state: S,
    view_fn: Box<dyn Fn(&S) -> View<S>>,
    pub text: TextSystem,
    pub list: DrawList,
    pub background: Color,
    /// True while springs or scroll inertia are still moving: keep drawing frames.
    pub animating: bool,
    hits: Vec<Hit<S>>,
    springs: HashMap<(u64, u8), Spring>,
    scrolls: HashMap<u64, ScrollState>,
    ptr: Pointer,
    last_frame: Option<Instant>,
    frame: u64,
    /// Draw a frames-per-second counter in the top-right corner. While it is on the UI
    /// redraws continuously (otherwise there would be nothing to count when idle).
    pub show_fps: bool,
    /// CPU time of the last frame in milliseconds (build + layout + paint + GPU encode),
    /// reported by the runner; shown next to the FPS.
    pub frame_ms: f32,
    fps: f32,
    fps_label: String,
    fps_label_at: Option<Instant>,
}

impl<S> Ui<S> {
    pub fn new(state: S, view_fn: impl Fn(&S) -> View<S> + 'static) -> Self {
        Ui {
            state,
            view_fn: Box::new(view_fn),
            text: TextSystem::new(),
            list: DrawList::default(),
            background: Color::BLACK,
            animating: false,
            hits: Vec::new(),
            springs: HashMap::new(),
            scrolls: HashMap::new(),
            ptr: Pointer::default(),
            last_frame: None,
            frame: 0,
            show_fps: false,
            frame_ms: 0.0,
            fps: 0.0,
            fps_label: String::new(),
            fps_label_at: None,
        }
    }

    /// Builds, lays out and paints one frame for a window of `size` points.
    pub fn frame(&mut self, size: (f32, f32)) {
        let now = Instant::now();
        let dt = match self.last_frame {
            Some(t) if self.animating => (now - t).as_secs_f32().min(1.0 / 20.0),
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
        let dragging = self.ptr.dragging;
        for s in self.scrolls.values_mut() {
            if !dragging && s.vel != 0.0 {
                s.offset += s.vel * dt;
                s.vel *= (-dt * 2.2).exp(); // iOS-like deceleration
                if s.vel.abs() < 4.0 {
                    s.vel = 0.0;
                }
            }
        }

        let view = (self.view_fn)(&self.state);
        let root = layout(&view, (Some(size.0), Some(size.1)), &mut self.text);
        self.list.cmds.clear();
        self.hits.clear();
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
            springs: &mut self.springs,
            scrolls: &mut self.scrolls,
            pressed: self.ptr.pressed,
            frame,
        };
        p.paint(view, &root, (0.0, 0.0), xf, 1);

        if self.show_fps {
            self.paint_fps(now, size);
        }

        self.springs.retain(|_, s| s.used == frame);
        self.scrolls.retain(|_, s| s.used == frame);
        self.text.end_frame();
        self.animating = self.show_fps
            || self.springs.values().any(Spring::moving)
            || (!dragging && self.scrolls.values().any(|s| s.vel != 0.0));
    }

    fn paint_fps(&mut self, now: Instant, size: (f32, f32)) {
        // Refresh the numbers a few times a second so they stay readable.
        if self.fps_label_at.is_none_or(|t| (now - t).as_secs_f32() > 0.25) {
            self.fps_label = format!("{:.0} fps  {:.1} ms", self.fps, self.frame_ms);
            self.fps_label_at = Some(now);
        }
        let spec = TextSpec { text: &self.fps_label, size: 12.0, weight: 600, center: false };
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
        }));
        self.list.cmds.push(Cmd::Text(TextCmd {
            key,
            pos: (r.x + 8.0, r.y + 4.0),
            size: ts,
            scale: 1.0,
            color: Color::hex(0x7CFF9B),
            clip,
        }));
    }

    fn hit(&self, pos: (f32, f32), scroll: bool) -> Option<usize> {
        self.hits
            .iter()
            .rposition(|h| h.rect.contains(pos.0, pos.1) && matches!(h.action, Action::Scroll { .. }) == scroll)
    }

    fn slide(&mut self, i: usize, x: f32) {
        if let Action::Slider(x0, range, f) = &self.hits[i].action {
            f(&mut self.state, ((x - x0) / range).clamp(0.0, 1.0));
        }
    }

    /// Pointer went down. Returns true if a redraw is needed.
    pub fn pointer_down(&mut self, pos: (f32, f32), touch: bool) -> bool {
        self.ptr.last = pos;
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
        if let Some(i) = hit {
            self.slide(i, pos.0);
        }
        true
    }

    pub fn pointer_move(&mut self, pos: (f32, f32)) -> bool {
        let (dx, dy) = (pos.0 - self.ptr.last.0, pos.1 - self.ptr.last.1);
        let _ = dx;
        self.ptr.last = pos;
        // Touch: a vertical drag inside a scroll view scrolls it and cancels the tap.
        if let Some(sid) = self.ptr.drag_scroll {
            if !self.ptr.dragging && (pos.1 - self.ptr.down_at.1).abs() > 8.0 {
                let on_slider = self
                    .ptr
                    .pressed
                    .and_then(|id| self.hits.iter().find(|h| h.id == id))
                    .is_some_and(|h| matches!(h.action, Action::Slider(..)));
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
                    s.offset -= dy;
                    s.vel = s.vel * 0.4 + (-dy / dt) * 0.6;
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
            }
        }
        false
    }

    pub fn pointer_up(&mut self, pos: (f32, f32)) -> bool {
        let pressed = self.ptr.pressed.take();
        let was_dragging = std::mem::take(&mut self.ptr.dragging);
        self.ptr.drag_scroll = None;
        if was_dragging {
            return true; // inertia continues in frame()
        }
        if let (Some(id), Some(i)) = (pressed, self.hit(pos, false)) {
            if self.hits[i].id == id {
                match &self.hits[i].action {
                    Action::Tap(f) => f(&mut self.state),
                    Action::Toggle(on, f) => f(&mut self.state, !on),
                    _ => {}
                }
            }
        }
        true
    }

    /// Mouse wheel / trackpad scroll, delta in points.
    pub fn wheel(&mut self, pos: (f32, f32), dy: f32) -> bool {
        // Topmost scroll view under the cursor that can still move in that direction.
        for h in self.hits.iter().rev() {
            if let (Action::Scroll { max }, true) = (&h.action, h.rect.contains(pos.0, pos.1)) {
                if let Some(s) = self.scrolls.get_mut(&h.id) {
                    let next = (s.offset - dy).clamp(0.0, *max);
                    if next != s.offset {
                        s.offset = next;
                        s.vel = 0.0;
                        return true;
                    }
                }
            }
        }
        false
    }
}
