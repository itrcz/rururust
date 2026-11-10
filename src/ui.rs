//! Platform-independent core: owns the app state, rebuilds the view tree,
//! lays it out, paints it into a `DrawList`, and routes pointer input.

use crate::layout::{layout, LNode, FIELD_FONT, SLIDER_H, TABS_PAD};
use crate::text::{TextSpec, TextSystem};
use crate::types::{Color, Rect};
use crate::view::{Fill, Glass, Image, Kind, Shadow, View};
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
    /// Pictures used this frame; image quads refer to them by index (in `c1[0]`).
    pub images: Vec<Image>,
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
    /// Text field: shaped-text key (0 when empty), screen x of the text origin, scale.
    Field { key: u64, x0: f32, scale: f32, value: String, on_change: Box<dyn Fn(&mut S, String)> },
    /// Tab bar: screen x of the first tab, width of one tab, number of tabs.
    Tabs { x0: f32, tab_w: f32, n: usize, on_select: Box<dyn Fn(&mut S, usize)> },
    OpenMenu,
    Dismiss,
    Pick(usize, Rc<dyn Fn(&mut S, usize)>),
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
    text: &'a mut TextSystem,
    focus: Option<u64>,
    caret: usize,
    caret_on: bool,
    open_menu: Option<u64>,
    menu: Option<Menu<S>>,
    pointer_x: f32,
}

impl<S> Painter<'_, S> {
    fn label(&mut self, xf: &Xf, key: u64, pos: (f32, f32), size: (f32, f32), color: Color) {
        self.list.cmds.push(Cmd::Text(TextCmd {
            key,
            pos: (pos.0 * xf.s + xf.tx, pos.1 * xf.s + xf.ty),
            size: (size.0 * xf.s, size.1 * xf.s),
            scale: xf.s,
            color: color.alpha(color.a * xf.opacity),
            clip: xf.clip,
        }));
    }

    fn paint_menu(&mut self, m: Menu<S>, size: (f32, f32)) {
        const ROW: f32 = 44.0;
        let screen = Rect::new(0.0, 0.0, size.0, size.1);
        // Anything outside the menu closes it.
        self.hits.push(Hit { id: m.id ^ 0xD15, rect: screen, action: Action::Dismiss });
        let spec = |text| TextSpec { text, size: FIELD_FONT, weight: 400, center: false };
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
        let s = self.springs.entry((m.id, 4)).or_insert(Spring { x: 0.0, v: 0.0, target: 1.0, used: 0 });
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
            self.hits.push(Hit { id: rid, rect: xf.rect(row), action: Action::Pick(i, m.on_select.clone()) });
        }
    }

    fn spring(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring { x: target, v: 0.0, target, used: 0 });
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
            Kind::Image(img) => {
                let mut q = self.quad(&xf, r, st.radius, KIND_IMAGE);
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
                self.hits.push(Hit {
                    id,
                    rect: sr.intersect(&xf.clip),
                    action: Action::Field {
                        key: if value.is_empty() { 0 } else { ln.text },
                        x0: (inner.x - shift) * xf.s + xf.tx,
                        scale: xf.s,
                        value,
                        on_change,
                    },
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
                self.hits.push(Hit {
                    id,
                    rect: sr.inflate(4.0).intersect(&xf.clip),
                    action: Action::Tabs {
                        x0: (r.x + TABS_PAD) * xf.s + xf.tx,
                        tab_w: tab_w * xf.s,
                        n,
                        on_select,
                    },
                });
            }
            Kind::Picker { options, selected, on_select } => {
                let inner = Rect::new(r.x + pad[0], r.y + pad[1], r.w - pad[0] - pad[2], r.h - pad[1] - pad[3]);
                self.label(&xf, ln.text, (inner.x, inner.y), (inner.w - 26.0, inner.h), xf.fg);
                let spec = TextSpec { text: "⌄", size: 15.0, weight: 700, center: false };
                let (chev, cs) = self.text.measure(&spec, None);
                let at = (inner.x + inner.w - cs.0 - 2.0, inner.y + (inner.h - cs.1) * 0.5 - 3.0);
                self.label(&xf, chev, at, cs, xf.fg.alpha(xf.fg.a * 0.6));
                self.hits.push(Hit { id, rect: sr.intersect(&xf.clip), action: Action::OpenMenu });
                if self.open_menu == Some(id) {
                    self.menu = Some(Menu { id, anchor: sr, options, selected, on_select, fg: xf.fg });
                }
            }
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
    focus: Option<u64>,
    caret: usize,
    caret_at: Option<Instant>,
    open_menu: Option<u64>,
    /// Seconds until the UI wants another frame even without input (caret blink).
    pub wake_in: Option<f32>,
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
            focus: None,
            caret: 0,
            caret_at: None,
            open_menu: None,
            wake_in: None,
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
        self.list.images.clear();
        self.hits.clear();
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
        };
        p.paint(view, &root, (0.0, 0.0), xf, 1);
        match p.menu.take() {
            Some(menu) => p.paint_menu(menu, size),
            None => self.open_menu = None, // its picker is gone
        }
        if let Some(id) = self.focus {
            if !self.hits.iter().any(|h| h.id == id) {
                self.focus = None;
            }
        }
        self.wake_in = self.focus.map(|_| 0.5 - blink % 0.5 + 0.01);

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
        self.focus = None;
        if let Some(i) = hit {
            self.slide(i, pos.0);
            if let Action::Field { key, x0, scale, .. } = &self.hits[i].action {
                self.focus = Some(self.hits[i].id);
                self.caret = if *key == 0 { 0 } else { self.text.index_at(*key, (pos.0 - x0) / scale) };
                self.caret_at = Some(Instant::now());
            }
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
                if matches!(self.hits[i].action, Action::Tabs { .. }) {
                    return true; // the drop follows the pointer
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
