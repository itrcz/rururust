//! Text editing for `text_field`: caret and selection, multi-line with auto-grow,
//! clipboard (through [`WindowRequest::SetClipboard`] and [`Ui::paste`]), undo, IME preedit.
//!
//! A child module of `ui`, so it reaches `Ui` and `Painter` internals without widening them.
//! All geometry comes from the shared primitives in `text.rs` (`hit`, `caret`, `range_rects`…),
//! which work in byte offsets of the whole text.

use super::{Action, Painter, Ui, WindowRequest, Xf, DOUBLE_CLICK};
use crate::layout::LNode;
use crate::text::{TextSpec, TextSystem};
use crate::types::{Color, Rect};
use crate::view::{Fill, Kind, Mods, Style, View};
use crate::KeyInput;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use web_time::Instant;

/// Typing within this many seconds of the previous keystroke joins its undo step.
const UNDO_GROUP: f32 = 1.0;
const UNDO_DEPTH: usize = 200;

/// The data of a `text_field` view.
pub(crate) struct Field<S> {
    pub value: String,
    pub placeholder: String,
    pub on_change: Box<dyn Fn(&mut S, String)>,
    pub on_submit: Option<Box<dyn Fn(&mut S)>>,
    pub on_blur: Option<Box<dyn Fn(&mut S)>>,
    pub on_cancel: Option<Box<dyn Fn(&mut S)>>,
    pub autofocus: bool,
    pub select_on_focus: bool,
    pub size: f32,
    pub weight: u16,
    pub weighted: bool,
    pub intrinsic: bool,
    /// Set by `.font_size()`: the theme's body size does not replace it.
    pub sized: bool,
    pub line_height: f32,
    /// Letter spacing in em.
    pub tracking: f32,
    /// Set by `.line_height()` / `.letter_spacing()`: the theme does not replace them.
    pub lh_set: bool,
    pub track_set: bool,
    /// 1 = single-line; more = grows with its text up to this many rows, then scrolls.
    pub max_lines: usize,
    /// No ring of its own: the app draws the container.
    pub plain: bool,
    /// Placeholder color; the text color at 40 % when unset.
    pub placeholder_color: Option<Color>,
}

impl<S> Field<S> {
    pub(crate) fn new(value: String, placeholder: String, on_change: Box<dyn Fn(&mut S, String)>) -> Self {
        let body = crate::theme::Theme::default().text.body;
        Field {
            value,
            placeholder,
            on_change,
            on_submit: None,
            on_blur: None, on_cancel: None, autofocus: false, select_on_focus: false,
            size: body.size,
            weight: body.weight as u16, weighted: false, intrinsic: false,
            sized: false,
            line_height: body.line_height,
            tracking: body.tracking,
            lh_set: false,
            track_set: false,
            max_lines: 1,
            plain: false,
            placeholder_color: None,
        }
    }

    pub(crate) fn theme(&mut self, body: &crate::theme::TextStyle) {
        if !self.weighted { self.weight = body.weight as u16; }
        if !self.sized {
            self.size = body.size;
        }
        if !self.lh_set {
            self.line_height = body.line_height;
        }
        if !self.track_set {
            self.tracking = body.tracking;
        }
    }
}

impl<S> View<S> {
    fn field(&mut self) -> Option<&mut Field<S>> {
        match &mut self.kind {
            Kind::TextField(f) => Some(f),
            _ => None,
        }
    }

    /// Text field: several lines with word wrap. It starts one row tall and grows with its
    /// text up to [`max_lines`](Self::max_lines) rows (6 unless set), then scrolls inside.
    /// Enter inserts a line break unless there is an [`on_submit`](Self::on_submit).
    pub fn multiline(mut self) -> Self {
        if let Some(f) = self.field() {
            f.max_lines = f.max_lines.max(6);
        }
        self
    }
    /// Text field: the most rows it grows to before scrolling (implies
    /// [`multiline`](Self::multiline) when above 1).
    pub fn max_lines(mut self, n: usize) -> Self {
        if let Some(f) = self.field() {
            f.max_lines = n.max(1);
        }
        self
    }
    /// Text field: Enter calls `f` instead of inserting a line break; Shift+Enter or
    /// Alt+Enter still breaks the line in a multi-line field. The field keeps focus, so a chat
    /// composer clears its draft in `f` and the user types on.
    ///
    /// ```ignore
    /// text_field(&s.draft, "Message", |s, v| s.draft = v)
    ///     .multiline()
    ///     .on_submit(|s| s.messages.push(std::mem::take(&mut s.draft)))
    /// ```
    pub fn on_submit(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        if let Some(field) = self.field() {
            field.on_submit = Some(Box::new(f));
        }
        self
    }
    /// Text field: color of the placeholder (the text color at 40 % unless set).
    pub fn placeholder_color(mut self, c: Color) -> Self {
        if let Some(f) = self.field() {
            f.placeholder_color = Some(c);
        }
        self
    }
    /// Size a single-line field to its shaped text, growing up to the parent's available width.
    /// Existing padding is included, with room for the caret at the end.
    pub fn fit_content(mut self) -> Self {
        if let Some(f) = self.field() { f.intrinsic = true; }
        self.style.width = crate::view::Dim::Auto;
        self
    }
    /// Focus this field once when it appears in the view tree.
    pub fn autofocus(mut self) -> Self {
        if let Some(f) = self.field() { f.autofocus = true; }
        self
    }
    /// Select the whole value when focus enters this field.
    pub fn select_on_focus(mut self) -> Self {
        if let Some(f) = self.field() { f.select_on_focus = true; }
        self
    }
    /// Run when focus leaves the field, for example to commit an inline edit.
    pub fn on_blur(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        if let Some(field) = self.field() { field.on_blur = Some(Box::new(f)); }
        self
    }
    /// Handle Escape without committing an inline edit.
    pub fn on_cancel(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        if let Some(field) = self.field() { field.on_cancel = Some(Box::new(f)); }
        self
    }
    /// Text field without its own look: no background, padding, corners or focus ring.
    /// Put it in a container styled the way the app needs (e.g. a chat composer).
    pub fn plain(mut self) -> Self {
        if let Some(f) = self.field() {
            f.plain = true;
            self.style.role = crate::view::Role::None;
            self.style.bg = None;
            self.style.border = None;
            self.style.radius = 0.0;
            self.style.padding = [0.0; 4];
        }
        self
    }
}

fn line_h(size: f32, k: f32) -> f32 {
    (size * k).round() // as text.rs lays out lines
}

fn spec(text: &str, size: f32, line_height: f32, tracking: f32, weight: u16) -> TextSpec<'_> {
    TextSpec { spans: &[], text, size, weight, center: false, line_height, tracking, ..Default::default() }
}

fn wrap_at(multi: bool, w: f32) -> Option<f32> {
    multi.then_some(w.max(1.0))
}

/// Content size of a field: the offered width, and one row or as many as its text
/// needs, up to `max_lines`.
pub(crate) fn layout_field<S>(f: &Field<S>, width: Option<f32>, ts: &mut TextSystem, node: &mut LNode) -> (f32, f32) {
    let w = if f.intrinsic && f.max_lines == 1 {
        let label = if f.value.is_empty() { &f.placeholder } else { &f.value };
        let natural = ts.measure(&spec(label, f.size, f.line_height, f.tracking, f.weight), None).1.0 + 2.0;
        width.map_or(natural, |offered| natural.min(offered))
    } else { width.unwrap_or(220.0) };
    let lh = line_h(f.size, f.line_height);
    let (key, size) = ts.measure(&spec(&f.value, f.size, f.line_height, f.tracking, f.weight), wrap_at(f.max_lines > 1, w));
    node.text = key;
    let rows = ((size.1 / lh).round() as usize).clamp(1, f.max_lines.max(1));
    (w, rows as f32 * lh)
}

/// What a press or key on a painted field needs (it lives in the hit list until the next frame).
pub(crate) struct FieldHit<S> {
    /// Screen position of the text origin (scroll already applied) and the view scale.
    origin: (f32, f32),
    scale: f32,
    wrap: Option<f32>,
    size: f32,
    weight: u16,
    line_height: f32,
    tracking: f32,
    multi: bool,
    /// Visible text height and full text height (local points).
    view_h: f32,
    content_h: f32,
    value: String,
    on_change: Box<dyn Fn(&mut S, String)>,
    on_submit: Option<Box<dyn Fn(&mut S)>>,
    on_blur: Option<Box<dyn Fn(&mut S)>>,
    on_cancel: Option<Box<dyn Fn(&mut S)>>,
    autofocus: bool,
    select_on_focus: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Type,
    Delete,
    /// Never merged with its neighbours (paste, cut, line break).
    Single,
}

struct Snap {
    value: String,
    caret: usize,
    anchor: usize,
}

/// Editing state of the focused field. There is one per `Ui`: only one field has focus.
#[derive(Default)]
pub(crate) struct Editor {
    field: Option<u64>,
    /// Byte offsets; the selection is `anchor..caret` in either order.
    caret: usize,
    anchor: usize,
    /// Restarts the caret blink (set on every edit and move).
    pub(super) blink_at: Option<Instant>,
    pub(super) mods: Mods,
    /// Offset of the text inside the field: x for a single line, y for several.
    scroll: (f32, f32),
    /// Scroll so the caret is visible on the next paint.
    reveal: bool,
    /// Column kept by ↑/↓ across rows of different length.
    goal_x: Option<f32>,
    preedit: Option<(String, Option<(usize, usize)>)>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    group: Option<(Group, Instant)>,
    /// The value as the editor last saw it; anything else came from the app.
    known: Option<String>,
    /// Last press: time, place, click count (1..=3).
    click: Option<(Instant, (f32, f32), u8)>,
    /// Word / paragraph picked by a double / triple click: a drag extends by whole units.
    unit: Option<Range<usize>>,
    /// The field's value in the hit list is out of date (after a submit) until the next paint.
    stale: bool,
    ime_area: Option<Rect>,
}

impl Editor {
    fn range(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }
    /// The caret blinks only while there is no selection.
    pub(super) fn blinks(&self) -> bool {
        self.caret == self.anchor
    }
    fn touched(&mut self) {
        self.blink_at = Some(Instant::now());
        self.reveal = true;
    }
    fn clamp(&mut self, v: &str) {
        self.caret = floor_char(v, self.caret);
        self.anchor = floor_char(v, self.anchor);
    }
}

fn floor_char(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn prev_grapheme(s: &str, i: usize) -> usize {
    s[..i].grapheme_indices(true).next_back().map_or(0, |(k, _)| k)
}
fn next_grapheme(s: &str, i: usize) -> usize {
    s[i..].graphemes(true).next().map_or(s.len(), |g| i + g.len())
}
fn is_word(w: &str) -> bool {
    w.chars().any(char::is_alphanumeric)
}
/// Start of the word before `i` (skipping spaces and punctuation), as Alt+← does.
fn prev_word(s: &str, i: usize) -> usize {
    s[..i].split_word_bound_indices().rev().find(|(_, w)| is_word(w)).map_or(0, |(k, _)| k)
}
/// End of the word after `i`.
fn next_word(s: &str, i: usize) -> usize {
    s[i..].split_word_bound_indices().find(|(_, w)| is_word(w)).map_or(s.len(), |(k, w)| i + k + w.len())
}

/// Line breaks become `\n`; a single-line field gets spaces instead.
fn clean(text: &str, multi: bool) -> String {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    if multi {
        t
    } else {
        t.replace('\n', " ")
    }
}

impl<S: 'static> Ui<S> {
    /// Modifier keys currently held; the runner reports every change.
    pub fn set_modifiers(&mut self, m: Mods) {
        self.ed.mods = m;
    }

    /// The selected text: in the focused field, or else of a `.selectable()` area as plain
    /// text (what Cmd/Ctrl+C copies). The latter builds the view once, so call it on demand.
    pub fn selection_text(&self) -> Option<String> {
        if self.focus.is_none() {
            return self.feed_text();
        }
        let h = self.field_hit()?;
        let r = self.ed.range();
        h.value.get(r).filter(|t| !t.is_empty()).map(str::to_string)
    }

    /// Pastes `text` into the focused field, replacing the selection. Returns true if used.
    pub fn paste(&mut self, text: &str) -> bool {
        self.insert(text, Group::Single)
    }

    /// IME composition in progress (shown underlined at the caret, not yet in the value);
    /// `cursor` is a byte range inside `text`. An empty `text` ends it. The final text
    /// arrives through [`text_input`](Self::text_input).
    pub fn ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        if self.focus.is_none() {
            return false;
        }
        let next = (!text.is_empty()).then(|| (text.to_string(), cursor));
        let changed = self.ed.preedit != next;
        self.ed.preedit = next;
        self.ed.touched();
        changed
    }

    /// Where the caret of the focused field is on screen, in points: the runner places the
    /// IME candidate window there.
    pub fn ime_area(&self) -> Option<Rect> {
        self.focus.and(self.ed.ime_area)
    }

    fn field_index(&self) -> Option<usize> {
        let id = self.focus?;
        self.hits.iter().position(|h| h.id == id && matches!(h.action, Action::Field(_)))
    }
    fn field_hit(&self) -> Option<&FieldHit<S>> {
        match &self.hits[self.field_index()?].action {
            Action::Field(h) => Some(h),
            _ => None,
        }
    }

    /// Shapes the field's current value (it may have changed since the frame) and returns
    /// the text key plus the press position in text coordinates.
    fn shaped(&mut self, i: usize, pos: (f32, f32)) -> (u64, (f32, f32)) {
        let Action::Field(h) = &self.hits[i].action else { return (0, (0.0, 0.0)) };
        let key = self.text.measure(&spec(&h.value, h.size, h.line_height, h.tracking, h.weight), h.wrap).0;
        (key, ((pos.0 - h.origin.0) / h.scale, (pos.1 - h.origin.1) / h.scale))
    }

    pub(super) fn blur(&mut self) -> bool {
        self.ed.preedit = None;
        if let Some(i) = self.field_index() {
            if let Action::Field(h) = &self.hits[i].action {
                if let Some(f) = &h.on_blur { f(&mut self.state); }
            }
        }
        self.focus.take().is_some()
    }

    pub(super) fn field_ids(&self) -> Vec<u64> {
        self.hits.iter().filter(|h| matches!(h.action, Action::Field(_))).map(|h| h.id).collect()
    }
    pub(super) fn focus_new_field(&mut self, previous: &[u64]) -> bool {
        let target = self.hits.iter().position(|h| !previous.contains(&h.id)
            && matches!(&h.action, Action::Field(f) if f.autofocus));
        if let Some(i) = target {
            self.blur();
            let Action::Field(h) = &self.hits[i].action else { return false };
            self.focus = Some(self.hits[i].id);
            self.ed = Editor { field: self.focus, mods: self.ed.mods, caret: h.value.len(),
                anchor: if h.select_on_focus { 0 } else { h.value.len() }, ..Editor::default() };
            self.ed.touched();
            return true;
        }
        false
    }

    pub(super) fn field_press(&mut self, i: usize, pos: (f32, f32)) {
        let id = self.hits[i].id;
        let entering = self.focus != Some(id);
        let now = Instant::now();
        if self.ed.field != Some(id) {
            self.ed = Editor { field: Some(id), mods: self.ed.mods, ..Editor::default() };
        }
        self.focus = Some(id);
        self.ed.preedit = None;
        let n = match self.ed.click {
            Some((t, p, n)) if (now - t).as_secs_f32() < DOUBLE_CLICK && (p.0 - pos.0).abs() < 4.0 && (p.1 - pos.1).abs() < 4.0 => n % 3 + 1,
            _ => 1,
        };
        let n = if self.ptr.touch { 1 } else { n };
        self.ed.click = Some((now, pos, n));
        let (key, at) = self.shaped(i, pos);
        let Action::Field(h) = &self.hits[i].action else { return };
        let len = h.value.len();
        if self.ed.known.as_deref() != Some(h.value.as_str()) {
            self.ed.known = Some(h.value.clone());
            (self.ed.undo.clear(), self.ed.redo.clear());
        }
        let idx = floor_char(&h.value, self.text.hit(key, at.0, at.1)).min(len);
        let unit = match n {
            2 => Some(self.text.word_at(key, idx)),
            3 => Some(self.text.para_at(key, idx)),
            _ => None,
        };
        match &unit {
            Some(u) => (self.ed.anchor, self.ed.caret) = (u.start, u.end),
            None if self.ed.mods.shift => self.ed.caret = idx,
            None => (self.ed.anchor, self.ed.caret) = (idx, idx),
        }
        if entering && h.select_on_focus { (self.ed.anchor,self.ed.caret) = (0,len); }
        self.ed.unit = unit;
        self.ed.goal_x = None;
        self.ed.group = None;
        self.ed.touched();
    }

    /// Mouse drag after a press on the focused field: extends the selection.
    pub(super) fn field_drag(&mut self, i: usize, pos: (f32, f32)) -> bool {
        if self.ptr.touch || self.focus != Some(self.hits[i].id) {
            return false;
        }
        let (key, at) = self.shaped(i, pos);
        let idx = self.text.hit(key, at.0, at.1);
        let (caret, anchor) = (self.ed.caret, self.ed.anchor);
        match self.ed.unit.clone() {
            None => self.ed.caret = idx,
            Some(u) => {
                // Grow by whole words / paragraphs, always keeping the first one selected.
                let by = if self.ed.click.is_some_and(|c| c.2 == 3) { self.text.para_at(key, idx) } else { self.text.word_at(key, idx) };
                if idx < u.start {
                    (self.ed.anchor, self.ed.caret) = (u.end, by.start);
                } else {
                    (self.ed.anchor, self.ed.caret) = (u.start, by.end.max(u.end));
                }
            }
        }
        self.ed.reveal = true;
        (caret, anchor) != (self.ed.caret, self.ed.anchor)
    }

    pub(super) fn field_wheel(&mut self, pos: (f32, f32), dy: f32) -> bool {
        let Some(i) = self.field_index() else { return false };
        let Action::Field(h) = &self.hits[i].action else { return false };
        let over = h.content_h - h.view_h;
        if !h.multi || over <= 0.5 || !self.hits[i].rect.contains(pos.0, pos.1) {
            return false;
        }
        let next = (self.ed.scroll.1 - dy / h.scale).clamp(0.0, over);
        let moved = next != self.ed.scroll.1;
        self.ed.scroll.1 = next;
        moved
    }

    /// Applies an edit to the focused field's value and reports it to the app.
    fn change(&mut self, group: Group, f: impl FnOnce(&mut String, &mut usize, &mut usize)) -> bool {
        let Some(i) = self.field_index() else { return false };
        if self.ed.stale {
            return true; // swallowed: the value the app just replaced is not known yet
        }
        let Action::Field(h) = &mut self.hits[i].action else { return false };
        let ed = &mut self.ed;
        ed.clamp(&h.value);
        let mut v = h.value.clone();
        let (mut c, mut a) = (ed.caret, ed.anchor);
        f(&mut v, &mut c, &mut a);
        ed.touched();
        ed.goal_x = None;
        if v != h.value {
            let now = Instant::now();
            let merge = group != Group::Single
                && ed.redo.is_empty()
                && ed.group.is_some_and(|(g, t)| g == group && (now - t).as_secs_f32() < UNDO_GROUP);
            if !merge {
                ed.undo.push(Snap { value: h.value.clone(), caret: ed.caret, anchor: ed.anchor });
                if ed.undo.len() > UNDO_DEPTH {
                    ed.undo.remove(0);
                }
            }
            ed.redo.clear();
            ed.group = (group != Group::Single).then_some((group, now));
            // Remember it right away: several keys can arrive before the next frame.
            h.value = v.clone();
            ed.known = Some(v.clone());
            (ed.caret, ed.anchor) = (c, a);
            (h.on_change)(&mut self.state, v);
        } else {
            (ed.caret, ed.anchor) = (c, a);
        }
        true
    }

    fn insert(&mut self, text: &str, group: Group) -> bool {
        let Some(h) = self.field_hit() else { return false };
        let text = clean(text, h.multi);
        self.ed.preedit = None;
        // A space ends a typing step, so undo takes back a word at a time.
        let group = if group == Group::Type && text.chars().any(char::is_whitespace) { Group::Single } else { group };
        self.change(group, |v, c, a| {
            let r = (*c).min(*a)..(*c).max(*a);
            v.replace_range(r.clone(), &text);
            *c = r.start + text.len();
            *a = *c;
        })
    }

    pub(super) fn field_type(&mut self, text: &str) -> bool {
        self.insert(text, Group::Type)
    }

    /// Moves the caret (and with Shift, the selection end) to `to`.
    fn go(&mut self, to: usize) -> bool {
        if self.field_index().is_none() {
            return false;
        }
        self.ed.caret = to;
        if !self.ed.mods.shift {
            self.ed.anchor = to;
        }
        self.ed.group = None;
        self.ed.touched();
        true
    }

    pub(super) fn field_key(&mut self, key: KeyInput) -> bool {
        let Some(i) = self.field_index() else { return false };
        let Action::Field(h) = &self.hits[i].action else { return false };
        let (multi, submit) = (h.multi, h.on_submit.is_some());
        let value = h.value.clone();
        self.ed.clamp(&value);
        let m = self.ed.mods;
        let word = m.alt || m.command;
        let (c, sel) = (self.ed.caret, self.ed.range());
        let key_v = self.text.measure(&spec(&value, h.size, h.line_height, h.tracking, h.weight), h.wrap).0;
        use KeyInput as K;
        if !matches!(key, K::Up | K::Down) {
            self.ed.goal_x = None;
        }
        match key {
            K::Backspace | K::Delete => self.change(Group::Delete, |v, c, a| {
                let mut r = (*c).min(*a)..(*c).max(*a);
                if r.is_empty() {
                    r = match (key, word) {
                        (K::Backspace, false) => prev_grapheme(v, *c)..*c,
                        (K::Backspace, true) => prev_word(v, *c)..*c,
                        (_, false) => *c..next_grapheme(v, *c),
                        (_, true) => *c..next_word(v, *c),
                    };
                }
                v.replace_range(r.clone(), "");
                (*c, *a) = (r.start, r.start);
            }),
            K::Left | K::Right if !sel.is_empty() && !m.shift && !word => {
                self.go(if key == K::Left { sel.start } else { sel.end })
            }
            K::Left => self.go(if word { prev_word(&value, c) } else { prev_grapheme(&value, c) }),
            K::Right => self.go(if word { next_word(&value, c) } else { next_grapheme(&value, c) }),
            K::Up | K::Down if !multi || m.command => self.go(if key == K::Up { 0 } else { value.len() }),
            K::Up | K::Down => {
                let (x, top, h) = self.text.caret(key_v, c);
                let goal = *self.ed.goal_x.get_or_insert(x);
                let y = if key == K::Up { top - h * 0.5 } else { top + h * 1.5 };
                let to = if y < 0.0 {
                    0
                } else if y > self.text.caret(key_v, value.len()).1 + h {
                    value.len()
                } else {
                    self.text.hit(key_v, goal, y)
                };
                let goal = self.ed.goal_x;
                let r = self.go(to);
                self.ed.goal_x = goal;
                r
            }
            K::Home | K::End if m.command || !multi => self.go(if key == K::Home { 0 } else { value.len() }),
            K::Home => self.go(self.text.row_at(key_v, c).start),
            K::End => self.go(self.text.row_at(key_v, c).end),
            K::SelectAll => {
                (self.ed.anchor, self.ed.caret) = (0, value.len());
                self.ed.touched();
                true
            }
            K::Copy | K::Cut => {
                if let Some(t) = value.get(sel.clone()).filter(|t| !t.is_empty()) {
                    self.requests.push(WindowRequest::SetClipboard(t.to_string()));
                    if key == K::Cut {
                        self.insert("", Group::Single);
                    }
                }
                true
            }
            K::Undo | K::Redo => self.undo(key == K::Redo),
            K::Enter if multi && (m.shift || m.alt || !submit) => self.insert("\n", Group::Single),
            K::Enter if submit => {
                if self.ed.stale {
                    return true;
                }
                let Action::Field(h) = &self.hits[i].action else { return false };
                if let Some(f) = &h.on_submit {
                    f(&mut self.state);
                }
                // The app usually clears the draft: wait for the next frame to learn the new
                // value instead of editing the old one.
                self.ed.stale = true;
                self.ed.preedit = None;
                true
            }
            K::Enter => self.blur(),
            K::Escape => {
                if let Some(f) = &h.on_cancel {
                    f(&mut self.state);
                    self.focus = None;
                    self.ed.preedit = None;
                    true
                } else { false }
            },
        }
    }

    fn undo(&mut self, redo: bool) -> bool {
        let Some(i) = self.field_index() else { return false };
        let Action::Field(h) = &mut self.hits[i].action else { return false };
        let ed = &mut self.ed;
        let (from, to) = if redo { (&mut ed.redo, &mut ed.undo) } else { (&mut ed.undo, &mut ed.redo) };
        let Some(snap) = from.pop() else { return true };
        to.push(Snap { value: h.value.clone(), caret: ed.caret, anchor: ed.anchor });
        ed.group = None;
        (ed.caret, ed.anchor) = (snap.caret, snap.anchor);
        ed.touched();
        h.value = snap.value.clone();
        ed.known = Some(snap.value.clone());
        (h.on_change)(&mut self.state, snap.value);
        true
    }
}

impl<S> Painter<'_, S> {
    pub(super) fn paint_field(&mut self, f: Field<S>, st: &Style, r: Rect, sr: Rect, xf: Xf, id: u64) {
        let focused = self.focus == Some(id);
        if !f.plain {
            // A thin ring that fades in without overshoot.
            let k = self.crisp(id, super::slot::FOCUS, if focused { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
            let focus = self.look.theme.color.focus;
            if k > 0.01 {
                self.fill(&xf, r, st.radius, Fill::Solid(Color::CLEAR), Some((self.look.theme.control.focus_ring, focus.alpha(focus.a * k))));
            }
        }
        let pad = st.padding;
        let inner = Rect::new(r.x + pad[0], r.y + pad[1], (r.w - pad[0] - pad[2]).max(0.0), (r.h - pad[1] - pad[3]).max(0.0));
        let multi = f.max_lines > 1;
        let wrap = wrap_at(multi, inner.w);
        let (key_v, size_v) = self.text.measure(&spec(&f.value, f.size, f.line_height, f.tracking, f.weight), wrap);
        let lh = line_h(f.size, f.line_height);
        let content_h = size_v.1.max(lh);

        let ed = &mut *self.ed;
        if focused {
            ed.stale = false;
            if ed.known.as_deref() != Some(f.value.as_str()) {
                // The app changed the value (sent the message, reset the form): old steps
                // would undo into text that no longer exists.
                if ed.known.is_some() {
                    (ed.undo.clear(), ed.redo.clear());
                    ed.caret = f.value.len(); // to the end, as after typing
                    ed.anchor = ed.caret;
                    ed.reveal = true;
                }
                ed.known = Some(f.value.clone());
            }
            ed.clamp(&f.value);
        }
        // With an IME composition the shown text has it inserted at the caret.
        let pre = ed.preedit.clone().filter(|_| focused);
        let (key_d, size_d, caret_d, pre_range) = match &pre {
            Some((p, cur)) => {
                let c = ed.caret;
                let shown = format!("{}{}{}", &f.value[..c], p, &f.value[c..]);
                let (k, s) = self.text.measure(&spec(&shown, f.size, f.line_height, f.tracking, f.weight), wrap);
                (k, s, c + cur.map_or(p.len(), |x| x.0.min(p.len())), Some(c..c + p.len()))
            }
            None => (key_v, size_v, ed.caret, None),
        };
        let (cx, cy, ch) = if focused { self.text.caret(key_d, caret_d) } else { (0.0, 0.0, lh) };
        let ch = if ch <= 0.0 { lh } else { ch };

        // Keep the caret in view (only when it moved, so the wheel can scroll away from it).
        let ed = &mut *self.ed;
        let (mut sx, mut sy) = if focused { ed.scroll } else { (0.0, 0.0) };
        if focused {
            if multi {
                if std::mem::take(&mut ed.reveal) {
                    sy = sy.min(cy).max(cy + ch - inner.h);
                }
                sy = sy.clamp(0.0, (content_h - inner.h).max(0.0));
            } else {
                if std::mem::take(&mut ed.reveal) {
                    sx = sx.min(cx).max(cx - inner.w + 2.0);
                }
                sx = sx.clamp(0.0, (size_d.0 - inner.w + 2.0).max(0.0));
            }
            ed.scroll = (sx, sy);
        }
        let origin = xf.at_px((inner.x - sx, inner.y - sy));
        let mut cxf = xf;
        cxf.clip = xf.clip.intersect(&xf.rect(Rect::new(inner.x - 2.0, inner.y, inner.w + 4.0, inner.h)));
        cxf.clip_r = 0.0;

        let sel = ed.range();
        if focused && !sel.is_empty() && pre.is_none() {
            let mut rects = Vec::new();
            self.text.range_rects(key_v, sel.clone(), &mut rects);
            for q in rects {
                let q = Rect::new(origin.0 + q.x, origin.1 + q.y, q.w, q.h);
                self.fill(&cxf, q, 3.0, Fill::Solid(self.tctx.color), None);
            }
        }
        if f.value.is_empty() && pre.is_none() {
            let (key_p, size_p) = self.text.measure(&spec(&f.placeholder, f.size, f.line_height, f.tracking, f.weight), wrap);
            let color = f.placeholder_color.unwrap_or(xf.fg.alpha(xf.fg.a * 0.4));
            self.label(&cxf, key_p, origin, size_p, color);
        } else {
            self.label(&cxf, key_d, origin, (size_d.0.max(inner.w), size_d.1.max(inner.h)), xf.fg);
        }
        if let Some(pr) = pre_range {
            let mut rects = Vec::new();
            self.text.range_rects(key_d, pr, &mut rects);
            for q in rects {
                let line = Rect::new(origin.0 + q.x, origin.1 + q.y + q.h - 2.5, q.w, 1.5);
                self.fill(&cxf, line, 0.5, Fill::Solid(xf.fg.alpha(xf.fg.a * 0.7)), None);
            }
        }
        let bar = Rect::new(origin.0 + cx - 1.0, origin.1 + cy + 1.0, 2.0, ch - 2.0);
        if focused {
            self.ed.ime_area = Some(xf.rect(bar));
            if self.caret_on && (sel.is_empty() || pre.is_some()) {
                self.fill(&cxf, bar, 1.0, Fill::Solid(self.look.theme.color.accent), None);
            }
        }
        self.hit(
            &xf,
            id,
            sr,
            Action::Field(Box::new(FieldHit {
                origin: (origin.0 * xf.s + xf.tx, origin.1 * xf.s + xf.ty),
                scale: xf.s,
                wrap,
                size: f.size,
                weight: f.weight,
                line_height: f.line_height,
                tracking: f.tracking,
                multi,
                view_h: inner.h,
                content_h,
                value: f.value,
                on_change: f.on_change,
                on_submit: f.on_submit,
                on_blur: f.on_blur, on_cancel: f.on_cancel,
                autofocus: f.autofocus, select_on_focus: f.select_on_focus,
            })),
        );
    }
}
