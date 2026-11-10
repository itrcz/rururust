//! Selecting and copying read-only text (`.selectable()`), and links in rich text.
//!
//! A child module of `ui`, like `edit.rs`. A position in a selectable area is
//! `(row, node, byte)`: the row of the lazy list (0 without one), the number of the text view
//! in tree order inside the row (or the area), and a byte offset into its shaped text. Node
//! ids can't serve: they have no order, and rows off screen have none at all. The same
//! numbering is walked three times — `layout` counts (`LNode::tn`), paint records what is on
//! screen (`TextBox`), copying walks the `View` tree (`collect`) — and the three must agree.

use super::{Action, Painter, Ui, WindowRequest, Xf, DOUBLE_CLICK};
use crate::layout::child_id;
use crate::text::{shaped_text, strip_slots};
use crate::types::{Color, Rect};
use crate::view::{Axis, Cursor, Env, Fill, Kind, Span, View};
use crate::KeyInput;
use std::rc::Rc;
use web_time::Instant;

/// Default highlight under selected text (and in text fields).
pub(crate) const SEL_COLOR: Color = Color::BLUE.alpha(0.3);
/// Mixed into an area's id for its hit, so it never equals a tappable view's id.
const SEL_SALT: u64 = 0x5E1E_C7AB_1E00_0001;
/// A press on a link that moves further than this (points) becomes a selection.
const LINK_SLOP: f32 = 4.0;
/// Distance from the edge of the scroll view where a selection drag starts scrolling.
const EDGE: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub(crate) struct TextPos {
    pub row: u32,
    pub node: u32,
    pub byte: u32,
}

impl TextPos {
    const MAX: TextPos = TextPos { row: u32::MAX, node: u32::MAX, byte: u32::MAX };
    fn new(row: u32, node: u32, byte: usize) -> Self {
        TextPos { row, node, byte: byte.min(u32::MAX as usize) as u32 }
    }
    fn at(&self) -> (u32, u32) {
        (self.row, self.node)
    }
}

/// Which lazy-list rows lead from the root to an area: `(list id, row)` pairs. Copying builds
/// only these rows to find an area that sits inside a list row (a `markdown()` in a bubble).
pub(crate) type Path = Rc<[(u64, usize)]>;

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Selection {
    pub area: u64,
    pub path: Path,
    pub anchor: TextPos,
    pub focus: TextPos,
}

impl Selection {
    fn range(&self) -> (TextPos, TextPos) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }
}

/// A selectable text painted this frame.
pub(crate) struct TextBox {
    area: u64,
    row: u32,
    node: u32,
    key: u64,
    /// Screen position of the text's origin, and the view scale.
    origin: (f32, f32),
    scale: f32,
    /// On screen, cut to the clip.
    rect: Rect,
}

/// A selectable area painted this frame.
pub(crate) struct Area {
    id: u64,
    path: Path,
    /// The scroll view it scrolls in, with its screen rect (for autoscroll).
    scroll: Option<(u64, Rect)>,
}

/// What text views inherit from above while painting.
pub(crate) struct TextCtx<S> {
    area: Option<(u64, Path)>,
    pub row: u32,
    link: Option<Rc<dyn Fn(&mut S, &str)>>,
    pub color: Color,
    pub scroll: Option<(u64, Rect)>,
}

impl<S> Clone for TextCtx<S> {
    fn clone(&self) -> Self {
        TextCtx { area: self.area.clone(), row: self.row, link: self.link.clone(), color: self.color, scroll: self.scroll }
    }
}

impl<S> Default for TextCtx<S> {
    fn default() -> Self {
        TextCtx { area: None, row: 0, link: None, color: SEL_COLOR, scroll: None }
    }
}

/// A link under a press: where it goes, who handles it, and the area it is in.
pub(crate) struct LinkHit<S> {
    url: String,
    on_link: Option<Rc<dyn Fn(&mut S, &str)>>,
    area: Option<u64>,
}

/// Selection state of a `Ui`.
#[derive(Default)]
pub(crate) struct SelState {
    pub sel: Option<Selection>,
    pub texts: Vec<TextBox>,
    pub areas: Vec<Area>,
    /// Last press: time, place, click count (1..=3).
    click: Option<(Instant, (f32, f32), u8)>,
    /// Word / paragraph picked by a double / triple click: a drag keeps it selected.
    unit: Option<(TextPos, TextPos)>,
    /// A mouse press is extending the selection.
    pub dragging: bool,
}

/// Painting side: the selection as paint needs it.
pub(crate) struct PaintSel {
    pub area: u64,
    pub lo: TextPos,
    pub hi: TextPos,
    /// The selection reaches past the end of its list (rows were removed): drop it.
    pub stale: bool,
}

impl SelState {
    pub(super) fn for_paint(&self) -> Option<PaintSel> {
        let s = self.sel.as_ref()?;
        let (lo, hi) = s.range();
        (lo != hi).then_some(PaintSel { area: s.area, lo, hi, stale: false })
    }
}

fn floor_char(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Only these open from a click without an `on_link` handler.
fn openable(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")
}

// ---------------------------------------------------------------- paint

impl<S> Painter<'_, S> {
    /// Before a view paints: picks up `.selectable()`, `.selection_color()`, `.on_link()`.
    /// Returns what to restore afterwards.
    pub(super) fn enter_text_ctx(&mut self, v: &View<S>, id: u64) -> Option<TextCtx<S>> {
        let st = &v.style;
        let new_area = st.select && self.tctx.area.is_none();
        if !new_area && st.sel_color.is_none() && v.on_link.is_none() {
            return None;
        }
        let saved = self.tctx.clone();
        if new_area {
            let path: Path = Rc::from(self.path.as_slice());
            self.tctx.area = Some((id, path.clone()));
            self.tctx.row = 0;
            self.tbase = 0; // the area numbers its texts from zero
            self.areas.push(Area { id, path, scroll: self.tctx.scroll });
        }
        if let Some(c) = st.sel_color {
            self.tctx.color = c;
        }
        if let Some(f) = &v.on_link {
            self.tctx.link = Some(f.clone());
        }
        Some(saved)
    }

    pub(super) fn area_hit(&mut self, xf: &Xf, id: u64, sr: Rect) {
        if self.tctx.area.as_ref().is_some_and(|a| a.0 == id) {
            self.hit(xf, id ^ SEL_SALT, sr, Action::Select { area: id });
        }
    }

    pub(super) fn enter_scroll(&mut self, id: u64, sr: Rect) {
        self.tctx.scroll = Some((id, sr));
        // `scroll(..).selectable()`: the area scrolls in itself.
        if let Some(a) = self.areas.last_mut().filter(|a| a.id == id) {
            a.scroll = Some((id, sr));
        }
    }

    /// A lazy list starts row `i`: its texts are numbered from zero within the row.
    pub(super) fn enter_row(&mut self, list: u64, i: usize, count: usize) {
        self.tctx.row = i as u32;
        self.tbase = 0;
        if let Some(s) = self.sel.as_mut() {
            if s.area == list && s.hi.row != u32::MAX && s.hi.row as usize >= count {
                s.stale = true;
            }
        }
    }

    /// Selection highlight, I-beam and link areas of a text view; called before its glyphs.
    /// `at` is the local origin of the text, `node` its number.
    pub(super) fn text_extras(&mut self, xf: &Xf, id: u64, key: u64, spans: &[Span], content: &str, at: (f32, f32), size: (f32, f32), node: u32) {
        if let Some((area, _)) = self.tctx.area.clone() {
            let row = self.tctx.row;
            let origin = (at.0 * xf.s + xf.tx, at.1 * xf.s + xf.ty);
            let rect = xf.rect(Rect::new(at.0, at.1, size.0, size.1)).intersect(&xf.clip);
            self.texts.push(TextBox { area, row, node, key, origin, scale: xf.s, rect });
            if let Some(s) = self.sel.as_ref().filter(|s| s.area == area) {
                let me = (row, node);
                if s.lo.at() <= me && me <= s.hi.at() {
                    let len = shaped_text(spans, content).len();
                    let a = if s.lo.at() == me { s.lo.byte as usize } else { 0 };
                    let b = if s.hi.at() == me { (s.hi.byte as usize).min(len) } else { len };
                    let mut rects = Vec::new();
                    self.text.range_rects(key, a..b, &mut rects);
                    let c = self.tctx.color;
                    for q in rects {
                        self.fill(xf, Rect::new(at.0 + q.x, at.1 + q.y, q.w, q.h), 2.0, Fill::Solid(c), None);
                    }
                }
            }
            // One id for all texts of an area: moving between them costs no frame.
            self.zone(xf, area ^ SEL_SALT, Rect::new(rect.x, rect.y, rect.w, rect.h), Some(Cursor::Text));
        }
        if !spans.iter().any(|s| s.link.is_some()) {
            return;
        }
        let mut boxes = Vec::new();
        self.text.span_boxes(key, &mut boxes);
        for b in boxes {
            let Some(span) = spans.get(b.span) else { continue };
            let Some(url) = &span.link else { continue };
            let zid = child_id(id, b.span, Some(0x4C14_4B00));
            let r = Rect::new(at.0 + b.x, at.1 + b.baseline - 0.95 * b.size, b.w, 1.3 * b.size);
            self.zone(xf, zid, xf.rect(r), Some(Cursor::Pointer));
            let hit = LinkHit { url: url.clone(), on_link: self.tctx.link.clone(), area: self.tctx.area.as_ref().map(|a| a.0) };
            self.hit(xf, zid, xf.rect(r), Action::Link(Box::new(hit)));
            if self.hovered.contains(&zid) && span.icon.is_none() {
                let color = span.color.unwrap_or(xf.fg);
                let line = Rect::new(at.0 + b.x, at.1 + b.baseline + 0.12 * b.size, b.w, (1.0 / xf.s).max(1.0));
                self.fill(xf, line, 0.0, Fill::Solid(color.alpha(color.a * xf.opacity)), None);
            }
        }
    }
}

// ---------------------------------------------------------------- input

impl<S: 'static> Ui<S> {
    /// The position in `area` nearest to the screen point `pos` among the texts on screen.
    fn sel_pos(&self, area: u64, pos: (f32, f32)) -> Option<(TextPos, usize)> {
        let dist = |r: &Rect| {
            let dx = (r.x - pos.0).max(pos.0 - (r.x + r.w)).max(0.0);
            let dy = (r.y - pos.1).max(pos.1 - (r.y + r.h)).max(0.0);
            // Rows first: a point beside a line belongs to it rather than to a nearer column.
            dy * 1000.0 + dx
        };
        let (i, b) = self
            .sel
            .texts
            .iter()
            .enumerate()
            .filter(|(_, b)| b.area == area && b.rect.w > 0.0 && b.rect.h > 0.0)
            .min_by(|a, b| dist(&a.1.rect).total_cmp(&dist(&b.1.rect)))?;
        let x = (pos.0 - b.origin.0) / b.scale;
        let y = (pos.1 - b.origin.1) / b.scale;
        Some((TextPos::new(b.row, b.node, self.text.hit(b.key, x, y)), i))
    }

    fn area_path(&self, area: u64) -> Path {
        self.sel.areas.iter().find(|a| a.id == area).map_or_else(|| Rc::from(Vec::new()), |a| a.path.clone())
    }

    /// Mouse press on a selectable area (not on a link or a control inside it).
    pub(super) fn sel_press(&mut self, area: u64, pos: (f32, f32)) {
        let now = Instant::now();
        let n = match self.sel.click {
            Some((t, p, n)) if (now - t).as_secs_f32() < DOUBLE_CLICK && (p.0 - pos.0).abs() < 4.0 && (p.1 - pos.1).abs() < 4.0 => n % 3 + 1,
            _ => 1,
        };
        self.sel.click = Some((now, pos, n));
        self.sel.unit = None;
        let Some((p, i)) = self.sel_pos(area, pos) else {
            self.sel.sel = None;
            return;
        };
        self.sel.dragging = true;
        let key = self.sel.texts[i].key;
        let unit = match n {
            2 => Some(self.text.word_at(key, p.byte as usize)),
            3 => Some(self.text.para_at(key, p.byte as usize)),
            _ => None,
        }
        .map(|r| (TextPos { byte: r.start as u32, ..p }, TextPos { byte: r.end as u32, ..p }));
        let path = self.area_path(area);
        match (unit, self.sel.sel.as_mut()) {
            (Some(u), _) => self.sel.sel = Some(Selection { area, path, anchor: u.0, focus: u.1 }),
            (None, Some(s)) if self.ed.mods.shift && s.area == area => s.focus = p,
            (None, _) => self.sel.sel = Some(Selection { area, path, anchor: p, focus: p }),
        }
        self.sel.unit = unit;
    }

    /// Mouse drag after a press on an area: moves the selection's focus. True if it changed.
    pub(super) fn sel_drag(&mut self, pos: (f32, f32)) -> bool {
        if !self.sel.dragging {
            return false;
        }
        let Some(area) = self.sel.sel.as_ref().map(|s| s.area) else { return false };
        let Some((p, _)) = self.sel_pos(area, pos) else { return false };
        let unit = self.sel.unit;
        let Some(s) = self.sel.sel.as_mut() else { return false };
        let before = (s.anchor, s.focus);
        match unit {
            None => s.focus = p,
            // The word / paragraph picked first stays selected whichever way the drag goes.
            Some(u) if p < u.0 => (s.anchor, s.focus) = (u.1, p),
            Some(u) => (s.anchor, s.focus) = (u.0, p.max(u.1)),
        }
        before != (s.anchor, s.focus)
    }

    /// A click without a drag leaves an empty selection: nothing is highlighted or copied,
    /// but Shift+click extends from there.
    pub(super) fn sel_release(&mut self) {
        self.sel.dragging = false;
    }

    /// A press on a link that moved away: select from where it started instead.
    pub(super) fn link_to_selection(&mut self, i: usize, pos: (f32, f32)) -> bool {
        let Action::Link(l) = &self.hits[i].action else { return false };
        let Some(area) = l.area else { return false };
        let d = (pos.0 - self.ptr.down_at.0).hypot(pos.1 - self.ptr.down_at.1);
        if self.ptr.touch || d <= LINK_SLOP {
            return false;
        }
        self.sel.click = None;
        self.sel_press(area, self.ptr.down_at);
        self.ptr.pressed = Some(area ^ SEL_SALT);
        self.sel_drag(pos);
        true
    }

    /// Click released on a link.
    pub(super) fn open_link(&mut self, i: usize) {
        let Action::Link(l) = &self.hits[i].action else { return };
        match &l.on_link {
            Some(f) => f(&mut self.state, &l.url),
            None if openable(&l.url) => self.requests.push(WindowRequest::OpenUrl(l.url.clone())),
            None => {}
        }
    }

    /// While a selection drag is near the top or bottom of its scroll view: how far that
    /// view should scroll this frame. Returns the scroll view and the distance.
    pub(super) fn sel_autoscroll(&self, dt: f32) -> Option<(u64, f32)> {
        if !self.sel.dragging || self.ptr.touch || !self.ptr.down {
            return None;
        }
        let area = self.sel.sel.as_ref()?.area;
        let (sid, r) = self.sel.areas.iter().find(|a| a.id == area)?.scroll?;
        let y = self.ptr.last.1;
        let over = if y < r.y + EDGE {
            y - (r.y + EDGE)
        } else if y > r.y + r.h - EDGE {
            y - (r.y + r.h - EDGE)
        } else {
            return None;
        };
        // Faster the further out, up to ~1800 pt/s.
        let speed = (over / EDGE).clamp(-3.0, 3.0) * 600.0;
        Some((sid, speed * dt))
    }

    /// Cmd+A / Cmd+C when no text field has focus.
    pub(super) fn sel_key(&mut self, key: KeyInput) -> bool {
        match key {
            KeyInput::SelectAll => {
                let mouse = self.mouse.and_then(|m| {
                    self.hits.iter().rev().find_map(|h| match h.action {
                        Action::Select { area } if h.contains(m) => Some(area),
                        _ => None,
                    })
                });
                let current = self.sel.sel.as_ref().map(|s| s.area);
                let Some(area) = current.or(mouse).or(self.sel.areas.first().map(|a| a.id)) else { return false };
                let path = self.area_path(area);
                let path = match &self.sel.sel {
                    Some(s) if s.area == area && path.is_empty() => s.path.clone(),
                    _ => path,
                };
                self.sel.sel = Some(Selection { area, path, anchor: TextPos::default(), focus: TextPos::MAX });
                true
            }
            KeyInput::Copy => match self.feed_text() {
                Some(t) => {
                    self.requests.push(WindowRequest::SetClipboard(t));
                    true
                }
                None => false,
            },
            KeyInput::Escape => self.sel.sel.take().is_some(),
            _ => false,
        }
    }

    /// The current `.selectable()` selection, to notice when it changes.
    #[allow(dead_code)] // used by the web runner only
    pub(crate) fn feed_selection(&self) -> Option<&Selection> {
        self.sel.sel.as_ref()
    }

    /// The selection outside text fields as plain text. Builds the view once and, of a lazy
    /// list, only the selected rows; nothing is laid out.
    pub(crate) fn feed_text(&self) -> Option<String> {
        let s = self.sel.sel.as_ref()?;
        let (lo, hi) = s.range();
        if lo == hi {
            return None;
        }
        let view = (self.view_fn)(&self.state, &self.env());
        let mut c = Collect { lo, hi, row: 0, node: 0 };
        find(view, 1, s.area, &s.path, &mut c).filter(|t| !t.is_empty())
    }

    pub(super) fn env(&self) -> Env {
        Env { width: self.size.0, height: self.size.1, titlebar: self.titlebar, window_buttons: self.window_buttons }
    }
}

// ---------------------------------------------------------------- copy

struct Collect {
    lo: TextPos,
    hi: TextPos,
    row: u32,
    node: u32,
}

/// Walks to the area `area` (with the ids layout and paint give) and collects its text.
fn find<S>(v: View<S>, id: u64, area: u64, path: &[(u64, usize)], c: &mut Collect) -> Option<String> {
    if id == area {
        return Some(collect(&v, c).unwrap_or_default());
    }
    match v.kind {
        Kind::Stack { children, .. } => children.into_iter().enumerate().find_map(|(i, ch)| {
            let cid = child_id(id, i, ch.key);
            find(ch, cid, area, path, c)
        }),
        Kind::Scroll { child, .. } | Kind::Overlay { child, .. } => {
            let cid = child_id(id, 0, child.key);
            find(*child, cid, area, path, c)
        }
        Kind::List { count, row, .. } => path.iter().filter(|p| p.0 == id && p.1 < count).find_map(|&(_, i)| {
            let rv = row(i);
            let cid = child_id(id, i, rv.key);
            find(rv, cid, area, path, c)
        }),
        _ => None,
    }
}

/// Number of text views in a subtree; must agree with `LNode::tn`.
fn count_text<S>(v: &View<S>) -> u32 {
    match &v.kind {
        Kind::Text(_) => 1,
        Kind::Stack { children, .. } => children.iter().map(count_text).sum(),
        Kind::Scroll { child, .. } | Kind::Overlay { child, .. } => count_text(child),
        _ => 0,
    }
}

/// The selected text of a subtree, in tree order; `None` when nothing of it is selected.
/// An empty text inside the selection is `Some("")`, so an empty table cell keeps its tab.
fn collect<S>(v: &View<S>, c: &mut Collect) -> Option<String> {
    if let Some(s) = &v.style.copy_as {
        // Atomic: all of it when any of it is selected.
        let n = count_text(v);
        let first = TextPos { row: c.row, node: c.node, byte: 0 };
        let hit = if n == 0 {
            c.lo < first && first < c.hi
        } else {
            let last = TextPos { row: c.row, node: c.node + n - 1, byte: u32::MAX };
            c.lo <= last && c.hi > first
        };
        c.node += n;
        return (hit && !s.is_empty()).then(|| s.clone());
    }
    let join = |parts: Vec<Option<String>>, sep: &str| {
        let parts: Vec<String> = parts.into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(sep))
    };
    match &v.kind {
        Kind::Text(t) => {
            let me = (c.row, c.node);
            c.node += 1;
            if me < c.lo.at() || me > c.hi.at() {
                return None;
            }
            let s = shaped_text(&t.spans, &t.content);
            let a = if c.lo.at() == me { floor_char(&s, c.lo.byte as usize) } else { 0 };
            let b = if c.hi.at() == me { floor_char(&s, c.hi.byte as usize) } else { s.len() };
            // An end of the selection that takes nothing of this text: as if not reached.
            let inside = c.lo.at() < me && me < c.hi.at();
            (a < b || inside).then(|| strip_slots(&s[a..b.max(a)]))
        }
        Kind::Stack { axis, children, .. } => {
            let parts = children.iter().map(|ch| collect(ch, c)).collect();
            join(parts, v.style.copy_sep.unwrap_or(if *axis == Axis::H { " " } else { "\n" }))
        }
        Kind::Scroll { child, .. } => collect(child, c),
        Kind::Overlay { open, child, .. } => {
            if *open {
                collect(child, c)
            } else {
                c.node += count_text(child);
                None
            }
        }
        Kind::List { count, row, .. } if *count > 0 => {
            let (from, to) = (c.lo.row as usize, (c.hi.row as usize).min(count - 1));
            let mut parts = Vec::new();
            for i in from..=to {
                (c.row, c.node) = (i as u32, 0);
                parts.push(collect(&row(i), c));
            }
            join(parts, v.style.copy_sep.unwrap_or("\n\n"))
        }
        _ => None,
    }
}
