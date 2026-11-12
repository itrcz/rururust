//! Floating layers anchored to a view: popovers, menus (with submenus and keyboard
//! navigation), context menus and tooltips; plus key shortcuts and the secondary click.
//!
//! A child module of `ui`, like `edit.rs`. The anchor lays its popover out right after itself
//! (`LNode::pop`), so layout stays one pass; paint queues the popover as a [`Layer`] and paints
//! all layers after the tree, so they are on top in both pixels and input order.

use super::select::TextCtx;
use super::{slot, Action, Hit, Painter, Ui, Xf, Zone, NO_ROUND};
use crate::layout::{child_id, LNode, Lx};
use crate::text::TextSpec;
use crate::types::{Color, Rect};
use crate::view::{Align, Fill, Glass, Image, Kind, Mods, Shadow, View, Weight};
use crate::KeyInput;
use std::rc::Rc;

/// Id of the popover attached to the view `id`, and of its tooltip.
pub(crate) fn pop_id(id: u64) -> u64 {
    child_id(id, 0, Some(0x504F_5056))
}
pub(crate) fn tip_id(id: u64) -> u64 {
    child_id(id, 0, Some(0x5449_5000))
}
// A key replaces the index in `child_id`, so rows get a keyed parent and their index under it.
fn row_id(menu: u64, i: usize) -> u64 {
    child_id(child_id(menu, 0, Some(0x524F_5700)), i, None)
}
fn sub_id(menu: u64, i: usize) -> u64 {
    child_id(child_id(menu, 0, Some(0x5355_4200)), i, None)
}

/// `v` moved to the nearest of `n` evenly spaced stops.
pub(crate) fn snap(v: f32, n: u32) -> f32 {
    if n < 2 {
        return v;
    }
    let k = (n - 1) as f32;
    (v.clamp(0.0, 1.0) * k).round() / k
}

// ---------------------------------------------------------------- placement

/// Side of the anchor a popover opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// Where a popover sits relative to its anchor: a side, the alignment along that side
/// (`Start` = left / top edges line up) and the gap. It flips to the opposite side when it
/// does not fit and slides along the side to stay `margin` points inside the window.
///
/// ```ignore
/// Placement::top().start()                      // above, left edges aligned
/// Placement::right().gap(4.0)                   // to the right, centered vertically
/// Placement::bottom().end().margin(6.5)         // may come within 6.5 pt of the window edge
/// Placement::right().end().offset(0.0, 1.5)     // then nudged 1.5 pt down
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub side: Side,
    pub align: Align,
    pub gap: f32,
    /// Distance kept from the window edges (default 8).
    pub margin: f32,
    /// Shift (x, y) after placing, before keeping inside the window.
    pub offset: (f32, f32),
}

impl Placement {
    fn on(side: Side) -> Self {
        Placement { side, align: Align::Center, gap: 6.0, margin: 8.0, offset: (0.0, 0.0) }
    }
    pub fn top() -> Self {
        Self::on(Side::Top)
    }
    pub fn bottom() -> Self {
        Self::on(Side::Bottom)
    }
    pub fn left() -> Self {
        Self::on(Side::Left)
    }
    pub fn right() -> Self {
        Self::on(Side::Right)
    }
    pub fn start(mut self) -> Self {
        self.align = Align::Start;
        self
    }
    pub fn center(mut self) -> Self {
        self.align = Align::Center;
        self
    }
    pub fn end(mut self) -> Self {
        self.align = Align::End;
        self
    }
    pub fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self
    }
    pub fn margin(mut self, m: f32) -> Self {
        self.margin = m;
        self
    }
    pub fn offset(mut self, dx: f32, dy: f32) -> Self {
        self.offset = (dx, dy);
        self
    }
}

/// Top-left corner of a `size` box placed at `a` inside `screen`, and the point on the anchor
/// it grows out of.
pub(crate) fn place(a: Rect, size: (f32, f32), p: Placement, screen: Rect) -> ((f32, f32), (f32, f32)) {
    let (w, h) = size;
    let vertical = matches!(p.side, Side::Top | Side::Bottom);
    let m = p.margin;
    let (dmain, dcross) = if vertical { (p.offset.1, p.offset.0) } else { p.offset };
    // Main axis: the preferred side, or the other one if only that one has room.
    let (lo, hi, len, a0, a1) = if vertical {
        (screen.y + m, screen.y + screen.h - m, h, a.y, a.y + a.h)
    } else {
        (screen.x + m, screen.x + screen.w - m, w, a.x, a.x + a.w)
    };
    let before = a0 - p.gap - len; // above / to the left
    let after = a1 + p.gap; // below / to the right
    let fits_before = before >= lo;
    let fits_after = after + len <= hi;
    let want_before = matches!(p.side, Side::Top | Side::Left);
    let use_before = if want_before { fits_before || (!fits_after && a0 - lo > hi - a1) } else { !fits_after && (fits_before || a0 - lo > hi - a1) };
    let main = (if use_before { before } else { after }) + dmain;
    let main = main.min(hi - len).max(lo);
    // Cross axis: aligned with the anchor, then slid back inside the window.
    let (clo, chi, clen, c0, c1) = if vertical {
        (screen.x + m, screen.x + screen.w - m, w, a.x, a.x + a.w)
    } else {
        (screen.y + m, screen.y + screen.h - m, h, a.y, a.y + a.h)
    };
    let cross = dcross
        + match p.align {
            Align::Start => c0,
            Align::Center => (c0 + c1 - clen) * 0.5,
            Align::End => c1 - clen,
        };
    let cross = cross.min(chi - clen).max(clo);
    let edge = if use_before { a0 } else { a1 };
    let mid = ((c0 + c1) * 0.5).clamp(cross, cross + clen);
    if vertical {
        ((cross, main), (mid, edge))
    } else {
        ((main, cross), (edge, mid))
    }
}

// ---------------------------------------------------------------- shortcuts

/// A key a [`Shortcut`] is bound to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShortcutKey {
    /// A character key, compared case-insensitively ('k' matches K with Shift too, if Shift
    /// is part of the shortcut).
    Char(char),
    Named(KeyInput),
}

/// A key combination for [`View::shortcut`]. `command` is Cmd on macOS and Ctrl elsewhere.
///
/// ```ignore
/// Shortcut::cmd('k')            // ⌘K / Ctrl+K
/// Shortcut::cmd('n').shift()    // ⇧⌘N
/// Shortcut::key('/')            // plain key: never fires while typing in a field
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shortcut {
    pub key: ShortcutKey,
    pub mods: Mods,
}

impl Shortcut {
    pub fn cmd(c: char) -> Self {
        Shortcut { key: ShortcutKey::Char(c.to_ascii_lowercase()), mods: Mods { command: true, ..Mods::default() } }
    }
    pub fn key(c: char) -> Self {
        Shortcut { key: ShortcutKey::Char(c.to_ascii_lowercase()), mods: Mods::default() }
    }
    pub fn named(k: KeyInput) -> Self {
        Shortcut { key: ShortcutKey::Named(k), mods: Mods::default() }
    }
    pub fn shift(mut self) -> Self {
        self.mods.shift = true;
        self
    }
    pub fn alt(mut self) -> Self {
        self.mods.alt = true;
        self
    }
    pub fn command(mut self) -> Self {
        self.mods.command = true;
        self
    }
    /// How the platform writes it: "⌥⌘R" on Apple systems, "Ctrl+Alt+R" elsewhere. Menu items
    /// take it directly: `.shortcut(Shortcut::cmd('r'))`.
    pub fn label(&self) -> String {
        let apple = cfg!(any(target_os = "macos", target_os = "ios"));
        let mut s = String::new();
        let m = self.mods;
        for (on, mac, other) in [(m.alt, "⌥", "Alt+"), (m.shift, "⇧", "Shift+"), (m.command, "⌘", "Ctrl+")] {
            if on {
                s.push_str(if apple { mac } else { other });
            }
        }
        if !apple && m.command {
            // Ctrl comes first elsewhere.
            s = format!("Ctrl+{}", s.replace("Ctrl+", ""));
        }
        match self.key {
            ShortcutKey::Char(c) => s.extend(c.to_uppercase()),
            ShortcutKey::Named(k) => s.push_str(match k {
                KeyInput::Enter => "↩",
                KeyInput::Escape => "⎋",
                KeyInput::Up => "↑",
                KeyInput::Down => "↓",
                KeyInput::Left => "←",
                KeyInput::Right => "→",
                KeyInput::Backspace => "⌫",
                KeyInput::Delete => "⌦",
                _ => "",
            }),
        }
        s
    }
}

impl From<Shortcut> for String {
    fn from(s: Shortcut) -> String {
        s.label()
    }
}

// ---------------------------------------------------------------- menu model

pub(crate) enum ItemKind<S> {
    Action(Box<dyn Fn(&mut S)>),
    Submenu(Vec<MenuItem<S>>),
    Separator,
    Section,
}

/// A row of a [`menu`](crate::menu).
pub struct MenuItem<S> {
    pub(crate) kind: ItemKind<S>,
    label: String,
    hint: Option<String>,
    /// The picture and whether it is a template (painted in the text color).
    icon: Option<(Image, bool)>,
    shortcut: Option<String>,
    disabled: bool,
    checked: bool,
    keep_open: bool,
    /// Row height instead of the style's (for a section, without the gap above it).
    height: Option<f32>,
}

impl<S> MenuItem<S> {
    fn new(kind: ItemKind<S>, label: String) -> Self {
        MenuItem { kind, label, hint: None, icon: None, shortcut: None, disabled: false, checked: false, keep_open: false, height: None }
    }
    /// A row that runs `f` and closes the menu.
    pub fn action(label: impl Into<String>, f: impl Fn(&mut S) + 'static) -> Self {
        Self::new(ItemKind::Action(Box::new(f)), label.into())
    }
    /// For independent choices within one panel: invoke the action without dismissing it.
    pub fn keep_open(mut self) -> Self {self.keep_open=true;self}
    /// A row with a chevron that opens `items` beside the menu (hover, click or →).
    pub fn submenu(label: impl Into<String>, items: Vec<MenuItem<S>>) -> Self {
        Self::new(ItemKind::Submenu(items), label.into())
    }
    /// A thin line between groups.
    pub fn separator() -> Self {
        Self::new(ItemKind::Separator, String::new())
    }
    /// A small dimmed title over a group; not selectable.
    pub fn section(title: impl Into<String>) -> Self {
        Self::new(ItemKind::Section, title.into())
    }
    /// A glyph before the label, painted in the text color (like [`icon`](crate::icon)).
    pub fn icon(mut self, img: &Image) -> Self {
        self.icon = Some((img.clone(), true));
        self
    }
    /// A picture before the label, in its own colors (a multicolored SVG, an app icon);
    /// dimmed when the row is disabled. Same place and size as [`icon`](Self::icon).
    pub fn image(mut self, img: &Image) -> Self {
        self.icon = Some((img.clone(), false));
        self
    }
    /// This row's height instead of the style's. For a section it is the title's own part;
    /// [`MenuStyle::section_gap`] still comes above every section but the first row, so
    /// `MenuItem::section("Plugins").height(26.0)` sets one group apart from the others.
    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }
    /// Key hint at the right edge: `"⌘R"` or `Shortcut::cmd('r')`. Only shown; bind the key
    /// itself with [`View::shortcut`].
    pub fn shortcut(mut self, keys: impl Into<String>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }
    /// Dimmed text after the label: "Goal  Set a goal to keep pursuing".
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.hint = Some(text.into());
        self
    }
    /// Shown dimmed, skipped by the keyboard, does nothing when clicked.
    pub fn disabled(self) -> Self {
        self.enabled(false)
    }
    pub fn enabled(mut self, on: bool) -> Self {
        self.disabled = !on;
        self
    }
    /// A check mark before the label.
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
    fn selectable(&self) -> bool {
        !self.disabled && matches!(self.kind, ItemKind::Action(_) | ItemKind::Submenu(_))
    }
}

/// Look of a [`menu`](crate::menu) and its submenus. The theme gives the default
/// ([`Theme::menu`](crate::Theme::menu)); change the fields you need:
/// `MenuStyle { highlight: Color::BLUE, ..MenuStyle::from(&*env.theme) }`.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuStyle {
    pub background: Color,
    /// Glass behind the background (then make the background translucent).
    pub glass: Option<Glass>,
    pub radius: f32,
    pub border: Option<(f32, Color)>,
    /// Key shadow: close and fairly sharp.
    pub shadow: Option<Shadow>,
    /// A second, wide and soft shadow under the first, as macOS menus have.
    pub ambient: Option<Shadow>,
    /// Inner light rim along the edge: `Shadow::new(Color::WHITE.alpha(0.1), 0.0, 0.0).spread(0.5).inset()`.
    pub rim: Option<Shadow>,
    /// Space between the panel edge and the rows.
    pub padding: f32,
    pub font_size: f32,
    pub row_height: f32,
    /// Corner radius of the highlight.
    pub row_radius: f32,
    /// The highlight is this much shorter than the row at the top and at the bottom: a 24 pt
    /// capsule on a 28 pt row step is `row_height: 28.0, highlight_inset: 2.0, row_radius: 12.0`.
    pub highlight_inset: f32,
    /// Left and right padding inside a row.
    pub row_padding: f32,
    pub icon_size: f32,
    /// Between the icon (or check mark) column and the label.
    pub icon_gap: f32,
    pub text: Color,
    /// Shortcuts, hints, section titles, chevrons.
    pub dim: Color,
    pub disabled: Color,
    /// Row under the mouse or the keyboard.
    pub highlight: Color,
    /// Text on the highlighted row (default: unchanged).
    pub highlight_text: Option<Color>,
    pub separator: Color,
    /// Height of a separator row; the line is centered in it.
    pub separator_height: f32,
    /// Size of the shortcut text (default: `font_size`).
    pub shortcut_size: Option<f32>,
    /// Color of the shortcut text (default: `dim`).
    pub shortcut_color: Option<Color>,
    /// Section titles: size (default `font_size - 1.5`), weight, color (default `dim`),
    /// row height (default `row_height - 2`) and extra space above every section but the first row.
    pub section_size: Option<f32>,
    pub section_weight: Weight,
    pub section_color: Option<Color>,
    pub section_height: Option<f32>,
    pub section_gap: f32,
    pub min_width: f32,
    pub max_width: f32,
}

/// Height of row `i`; a section's includes the gap above it.
fn row_h<S>(it: &MenuItem<S>, i: usize, st: &MenuStyle) -> f32 {
    match it.kind {
        ItemKind::Separator => it.height.unwrap_or(st.separator_height),
        ItemKind::Section => section_h(it, st) + if i > 0 { st.section_gap } else { 0.0 },
        _ => it.height.unwrap_or(st.row_height),
    }
}

/// A section title's own part of its row (without the gap above).
fn section_h<S>(it: &MenuItem<S>, st: &MenuStyle) -> f32 {
    it.height.or(st.section_height).unwrap_or(st.row_height - 2.0)
}

impl MenuStyle {
    fn section_size(&self) -> f32 {
        self.section_size.unwrap_or(self.font_size - 1.5)
    }
}

fn spec(text: &str, size: f32, weight: u16) -> TextSpec<'_> {
    TextSpec { spans: &[], text, size, weight, center: false, ..Default::default() }
}

/// Width of the icon column (0 when no row has an icon or check).
fn icon_col<S>(items: &[MenuItem<S>], st: &MenuStyle) -> f32 {
    if items.iter().any(|i| i.icon.is_some() || i.checked) {
        st.icon_size + st.icon_gap
    } else {
        0.0
    }
}

/// Measures the rows (three texts per item in `node.texts`: label, hint, right side) and lays
/// out every submenu as a child, so opening one needs no layout outside the pass.
pub(crate) fn layout_menu<S>(items: &[MenuItem<S>], st: &MenuStyle, lx: &mut Lx, id: u64, node: &mut LNode) -> (f32, f32) {
    let col = icon_col(items, st);
    let mut widest = 0.0f32;
    let mut h = 2.0 * st.padding;
    let none = (0u64, (0.0f32, 0.0f32));
    for (i, it) in items.iter().enumerate() {
        h += row_h(it, i, st);
        let (label, hint, right) = match &it.kind {
            ItemKind::Separator => (none, none, none),
            ItemKind::Section => (lx.text.measure(&spec(&it.label, st.section_size(), st.section_weight as u16), None), none, none),
            kind => {
                let label = lx.text.measure(&spec(&it.label, st.font_size, 400), None);
                let hint = it.hint.as_deref().map_or(none, |t| lx.text.measure(&spec(t, st.font_size, 400), None));
                let right = match (kind, &it.shortcut) {
                    (ItemKind::Submenu(_), _) => lx.text.measure(&spec("›", st.font_size + 4.0, 400), None),
                    (_, Some(k)) => lx.text.measure(&spec(k, st.shortcut_size.unwrap_or(st.font_size), 400), None),
                    _ => none,
                };
                (label, hint, right)
            }
        };
        let mut w = 2.0 * st.row_padding + label.1 .0;
        if !matches!(it.kind, ItemKind::Section) {
            w += col;
        }
        if hint.0 != 0 {
            w += 8.0 + hint.1 .0;
        }
        if right.0 != 0 {
            w += 24.0 + right.1 .0;
        }
        widest = widest.max(w);
        node.texts.extend([label, hint, right]);
        if let ItemKind::Submenu(sub) = &it.kind {
            let sid = sub_id(id, i);
            let mut n = LNode::default();
            n.size = layout_menu(sub, st, lx, sid, &mut n);
            node.children.push(n);
        }
    }
    ((widest + 2.0 * st.padding).clamp(st.min_width, st.max_width.max(st.min_width)), h)
}

// ---------------------------------------------------------------- paint

/// How a layer closes.
pub(crate) enum Dismiss<S> {
    /// Opened by the framework (`.menu()`, `.context_menu()`): clear `Ui::open_menu`.
    Managed,
    App(Rc<dyn Fn(&mut S)>),
}

impl<S> Clone for Dismiss<S> {
    fn clone(&self) -> Self {
        match self {
            Dismiss::Managed => Dismiss::Managed,
            Dismiss::App(f) => Dismiss::App(f.clone()),
        }
    }
}

pub(crate) enum LayerKind<S> {
    Popover(Dismiss<S>),
    Submenu { parent: u64, dismiss: Option<Dismiss<S>> },
    Tooltip,
}

/// A view painted above the tree, positioned against a screen rect.
pub(crate) struct Layer<S> {
    pub id: u64,
    pub view: View<S>,
    pub node: LNode,
    pub anchor: Rect,
    pub placement: Placement,
    pub open: bool,
    pub kind: LayerKind<S>,
    pub fg: Color,
}

/// Highlighted row and open submenu of a menu, kept between frames.
#[derive(Default)]
pub(crate) struct MenuState {
    pub hl: Option<usize>,
    pub sub: Option<usize>,
    /// Highlight the first row on the next paint (opened from the keyboard).
    pub first: bool,
    pub used: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Nav {
    Act,
    Sub,
    Skip,
}

/// A menu on screen, for hover and keyboard handling.
pub(crate) struct MenuNav {
    pub id: u64,
    pub parent: Option<u64>,
    pub rows: Vec<(u64, Nav)>,
    /// In a layer (not inline in the tree): it takes the arrow keys.
    pub keys: bool,
}

/// Painter state while painting inside a layer.
pub(crate) struct LayerCtx<S> {
    pub dismiss: Option<Dismiss<S>>,
    pub parent_menu: Option<u64>,
    /// Inside some layer (menus there take the keyboard).
    pub inside: bool,
    /// The layer is open, not fading out.
    pub open: bool,
}

impl<S> Painter<'_, S> {
    pub(super) fn fade(&mut self, id: u64, target: f32) -> f32 {
        let fade = self.look.theme.motion.fade;
        self.spring_with(id, slot::FADE, 0.0, target, fade).x
    }

    /// Paints queued layers, then the layers they open (submenus, popovers inside popovers),
    /// so later ones are on top; tooltips go last of all.
    pub(super) fn paint_layers(&mut self) {
        let mut tips = Vec::new();
        while !self.layers.is_empty() {
            for l in std::mem::take(&mut self.layers) {
                match l.kind {
                    LayerKind::Tooltip => tips.push(l),
                    _ => self.paint_layer(l),
                }
            }
        }
        for l in tips {
            self.paint_layer(l);
        }
    }

    fn paint_layer(&mut self, l: Layer<S>) {
        let t = self.fade(l.id, if l.open { 1.0 } else { 0.0 });
        if !l.open && t <= 0.004 {
            return;
        }
        // Never fully transparent while open: a transparent view is skipped, and the layer's
        // rows must take clicks and keys from its first frame.
        let tc = t.clamp(if l.open { 0.01 } else { 0.0 }, 1.0);
        let size = l.node.size;
        let ((x, y), from) = place(l.anchor, size, l.placement, self.screen);
        // Grows a little out of the anchor while it fades in.
        let k0 = self.look.theme.motion.popup_scale;
        let mut xf = Xf { opacity: tc, ..Xf::screen(self.screen, l.fg, self.px) };
        xf.scale_about(from.0, from.1, k0 + (1.0 - k0) * tc);
        let body = xf.rect(Rect::new(x, y, size.0, size.1));
        let tip = matches!(l.kind, LayerKind::Tooltip);
        if let (LayerKind::Popover(d), true) = (&l.kind, l.open) {
            // A click anywhere else only closes the popover; nothing underneath is hovered.
            let oid = l.id ^ 0x0D75;
            self.hits.push(Hit { id: oid, rect: self.screen, round: NO_ROUND, action: Action::Outside(d.clone()) });
            self.zones.push(Zone { id: oid, rect: self.screen, round: NO_ROUND, cursor: None, blocks: true, tip: false });
        }
        if !tip && l.open {
            self.hits.push(Hit { id: l.id ^ 0xB0D7, rect: body, round: NO_ROUND, action: Action::Swallow });
        }
        let ctx = LayerCtx {
            dismiss: match &l.kind {
                LayerKind::Popover(d) => Some(d.clone()),
                LayerKind::Submenu { dismiss, .. } => dismiss.clone(),
                LayerKind::Tooltip => None,
            },
            parent_menu: match l.kind {
                LayerKind::Submenu { parent, .. } => Some(parent),
                _ => None,
            },
            inside: true,
            open: l.open,
        };
        let saved = std::mem::replace(&mut self.layer, ctx);
        let tctx = std::mem::replace(&mut self.tctx, TextCtx::with_color(self.look.theme.color.selection));
        let (nh, nz) = (self.hits.len(), self.zones.len());
        self.tbase = 0;
        self.paint(l.view, &l.node, (x - l.node.pos.0, y - l.node.pos.1), xf, l.id);
        self.flush_bars();
        if tip || !l.open {
            // Tooltips and closing layers never take input.
            self.hits.truncate(nh);
            self.zones.truncate(nz);
        }
        self.tctx = tctx;
        self.layer = saved;
    }

    /// The anchor's attachments: its popover and tooltip go to the layer queue.
    pub(super) fn attach(&mut self, x: crate::view::Extras<S>, ln: &LNode, xf: &Xf, id: u64, sr: Rect, layout_anchor: Rect) {
        use crate::view::Opens;
        if let Some(f) = x.secondary {
            self.hit(xf, id ^ 0x5EC0, sr, Action::Secondary(Some(f), id));
        }
        if let Some(p) = x.pop {
            let managed_open = self.open_menu == Some(id);
            let select_open=match &p.opens {Opens::State(open,_)=>*open,_=>managed_open};
            match p.opens {
                Opens::Tap => self.hit(xf, id, sr, Action::OpenMenu),
                Opens::Context => self.hit(xf, id ^ 0x5EC0, sr, Action::Secondary(None, id)),
                Opens::State(..) => {}
            }
            if !matches!(p.opens, Opens::State(..)) && managed_open {
                self.managed_seen = true;
            }
            if let Some(node) = &ln.pop {
                let (open, dismiss, anchor) = match p.opens {
                    Opens::State(open, f) => (open, Dismiss::App(f), layout_anchor),
                    Opens::Tap => (managed_open, Dismiss::Managed, layout_anchor),
                    Opens::Context => (managed_open, Dismiss::Managed, Rect::new(self.ctx_at.0, self.ctx_at.1, 0.0, 0.0)),
                };
                self.layers.push(Layer {
                    id: pop_id(id),
                    view: *p.content,
                    node: (**node).clone(),
                    anchor,
                    placement: p.placement,
                    open: open && self.layer.open_or_root(),
                    kind: LayerKind::Popover(dismiss),
                    fg: xf.fg,
                });
            }
            if x.select_popup {self.hit(xf,id,sr,Action::SelectAnchor {open:select_open});}
        }
        if let Some(tip) = x.tip {
            let placement = x.tip_at.unwrap_or(Placement::bottom().gap(6.0));
            self.zones.push(Zone { id, rect: sr.intersect(&xf.clip), round: (xf.clip, xf.clip_r), cursor: None, blocks: false, tip: true });
            if let Some(node) = &ln.tip {
                self.layers.push(Layer {
                    id: tip_id(id),
                    view: *tip,
                    node: (**node).clone(),
                    anchor: layout_anchor,
                    placement,
                    open: self.tip == Some(id),
                    kind: LayerKind::Tooltip,
                    fg: xf.fg,
                });
            }
        }
    }

    pub(super) fn paint_menu(&mut self, items: Vec<MenuItem<S>>, style: Rc<MenuStyle>, ln: &LNode, r: Rect, xf: Xf, id: u64) {
        let st = &*style;
        let sr = xf.rect(r);
        for sh in [st.shadow, st.ambient].into_iter().flatten() {
            self.shadow(&xf, r, st.radius, sh, true);
        }
        // The border goes on last, over the (often translucent) background and the rows,
        // as a container's does: under a translucent fill it would fade.
        if let Some(g) = st.glass {
            self.glass(&xf, r, st.radius, g, None);
        }
        self.fill(&xf, r, st.radius, Fill::Solid(st.background), None);
        self.hit(&xf, id ^ 0x5A11, sr, Action::Swallow);
        let first = items.iter().position(MenuItem::selectable);
        let frame = self.frame;
        let state = self.menu_state.entry(id).or_default();
        state.used = frame;
        if std::mem::take(&mut state.first) {
            state.hl = first;
        }
        let (hl, sub) = (state.hl, state.sub);
        let col = icon_col(&items, st);
        let mut cxf = xf;
        cxf.clip_to(sr, 0.0, false);
        let (check, check_size) = self.text.measure(&spec("✓", st.font_size, 600), None);
        let mut nav = Vec::with_capacity(items.len());
        let mut y = r.y + st.padding;
        let mut subk = 0;
        for (i, it) in items.into_iter().enumerate() {
            let h = row_h(&it, i, st);
            let title_h = section_h(&it, st);
            let row = Rect::new(r.x + st.padding, y, r.w - 2.0 * st.padding, h);
            let rid = row_id(id, i);
            let (label, hint, right) = (ln.texts[3 * i], ln.texts[3 * i + 1], ln.texts[3 * i + 2]);
            y += h;
            let lx = row.x + st.row_padding;
            let mid = |sz: (f32, f32)| row.y + (h - sz.1) * 0.5;
            match it.kind {
                ItemKind::Separator => {
                    let line = Rect::new(lx, row.y + h * 0.5 - 0.5, row.w - 2.0 * st.row_padding, 1.0);
                    self.fill(&cxf, line, 0.0, Fill::Solid(st.separator), None);
                    nav.push((rid, Nav::Skip));
                }
                ItemKind::Section => {
                    // Centered in the part under the gap.
                    let top = h - title_h;
                    let y = row.y + top + (h - top - label.1 .1) * 0.5 + 1.0;
                    self.label(&cxf, label.0, (lx, y), label.1, st.section_color.unwrap_or(st.dim));
                    nav.push((rid, Nav::Skip));
                }
                kind => {
                    let enabled = !it.disabled;
                    let lit = enabled && hl == Some(i);
                    if lit {
                        let band = Rect::new(row.x, row.y + st.highlight_inset, row.w, h - 2.0 * st.highlight_inset);
                        self.fill(&cxf, band, st.row_radius, Fill::Solid(st.highlight), None);
                    }
                    let color = match (enabled, lit) {
                        (false, _) => st.disabled,
                        (true, true) => st.highlight_text.unwrap_or(st.text),
                        _ => st.text,
                    };
                    let dim = if enabled { st.dim } else { st.disabled };
                    if let Some((img, template)) = &it.icon {
                        let ic = Rect::new(lx, row.y + (h - st.icon_size) * 0.5, st.icon_size, st.icon_size);
                        let mut ixf = cxf;
                        if !template && !enabled {
                            ixf.opacity *= 0.4;
                        }
                        self.picture(&ixf, ic, 0.0, img, template.then_some(color), None);
                    } else if it.checked {
                        let at = (lx + (st.icon_size - check_size.0) * 0.5, mid(check_size));
                        self.label(&cxf, check, at, check_size, color);
                    }
                    let tx = lx + col;
                    self.label(&cxf, label.0, (tx, mid(label.1)), label.1, color);
                    if hint.0 != 0 {
                        self.label(&cxf, hint.0, (tx + label.1 .0 + 8.0, mid(hint.1)), hint.1, dim);
                    }
                    if right.0 != 0 {
                        let rx = row.x + row.w - st.row_padding - right.1 .0;
                        let sub = matches!(kind, ItemKind::Submenu(_));
                        let ry = mid(right.1) - if sub { 1.0 } else { 0.0 };
                        let c = match st.shortcut_color {
                            Some(c) if enabled && !sub => c,
                            _ => dim,
                        };
                        self.label(&cxf, right.0, (rx, ry), right.1, c);
                    }
                    let srow = xf.rect(row);
                    self.zones.push(Zone { id: rid, rect: srow, round: NO_ROUND, cursor: None, blocks: false, tip: false });
                    match kind {
                        ItemKind::Action(f) if enabled => {
                            self.hits.push(Hit { id: rid, rect: srow, round: NO_ROUND, action: Action::MenuPick(f, if it.keep_open {None}else{self.layer.dismiss.clone()}) });
                            nav.push((rid, Nav::Act));
                        }
                        ItemKind::Submenu(sub_items) => {
                            let k = subk;
                            subk += 1;
                            if !enabled {
                                nav.push((rid, Nav::Skip));
                                continue;
                            }
                            self.hits.push(Hit { id: rid, rect: srow, round: NO_ROUND, action: Action::SubMenu(id, i) });
                            nav.push((rid, Nav::Sub));
                            if sub == Some(i) && self.layer.open {
                                // Beside the row, its first row level with this one.
                                let pad = st.padding * xf.s;
                                let anchor = Rect::new(srow.x - pad, srow.y - pad, srow.w + 2.0 * pad, srow.h + 2.0 * pad);
                                self.layers.push(Layer {
                                    id: sub_id(id, i),
                                    view: View::new(Kind::Menu { items: sub_items, style: Some(style.clone()) }),
                                    node: ln.children[k].clone(),
                                    anchor,
                                    placement: Placement::right().start().gap(-2.0),
                                    open: true,
                                    kind: LayerKind::Submenu { parent: id, dismiss: self.layer.dismiss.clone() },
                                    fg: xf.fg,
                                });
                            }
                        }
                        _ => {
                            self.hits.push(Hit { id: rid, rect: srow, round: NO_ROUND, action: Action::Swallow });
                            nav.push((rid, Nav::Skip));
                        }
                    }
                }
            }
        }
        if let Some(rim) = st.rim {
            self.shadow(&xf, r, st.radius, rim, false);
        }
        if let Some(b) = st.border {
            self.fill(&xf, r, st.radius, Fill::Solid(Color::CLEAR), Some(b));
        }
        self.menu_nav.push(MenuNav { id, parent: self.layer.parent_menu, rows: nav, keys: self.layer.inside });
    }
}

impl<S> Default for LayerCtx<S> {
    fn default() -> Self {
        LayerCtx { dismiss: None, parent_menu: None, inside: false, open: false }
    }
}

impl<S> LayerCtx<S> {
    /// In the tree (no layer) everything counts as open.
    fn open_or_root(&self) -> bool {
        !self.inside || self.open
    }
}

// ---------------------------------------------------------------- input

impl<S: 'static> Ui<S> {
    pub(super) fn run_dismiss(&mut self, d: Dismiss<S>) {
        match d {
            Dismiss::Managed => self.open_menu = None,
            Dismiss::App(f) => f(&mut self.state),
        }
    }

    /// Closes the topmost dismissible overlay; false if none is open.
    pub(super) fn dismiss_top(&mut self) -> bool {
        let hit = self.hits.iter().rposition(|h| matches!(h.action, Action::Outside(_) | Action::Block(_)));
        match hit.map(|i| &self.hits[i].action) {
            Some(Action::Outside(d)) => { self.run_dismiss(d.clone()); true }
            Some(Action::Block(Some(f))) => { f(&mut self.state); true }
            _ => false,
        }
    }

    /// A secondary (right) click. Returns true if a redraw is needed.
    pub fn secondary_click(&mut self, pos: (f32, f32)) -> bool {
        if let Some(t) = &mut self.tip {
            t.hidden = true;
        }
        let mut i = self.hits.len();
        while i > 0 {
            i -= 1;
            if !self.hits[i].contains(pos) {
                continue;
            }
            match &self.hits[i].action {
                Action::Outside(d) => {
                    let d = d.clone();
                    self.run_dismiss(d);
                    return true;
                }
                Action::Block(_) | Action::Swallow => return false,
                Action::Secondary(Some(f), _) => {
                    f(&mut self.state);
                    return true;
                }
                &Action::Secondary(None, id) => {
                    self.springs.remove(&(pop_id(id), slot::FADE)); // replay the fade
                    self.open_menu = Some(id);
                    self.ctx_at = pos;
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// A key with the current modifiers ([`set_modifiers`](Self::set_modifiers)): runs the
    /// matching [`View::shortcut`] of the last frame and returns true, or returns false and
    /// the runner handles the key as usual. In a text field the combinations that edit text
    /// are left to it.
    pub fn key_shortcut(&mut self, key: ShortcutKey) -> bool {
        let mods = self.ed.mods;
        let key = match key {
            ShortcutKey::Char(c) => ShortcutKey::Char(c.to_ascii_lowercase()),
            k => k,
        };
        let editing = matches!(key, ShortcutKey::Char('a' | 'c' | 'v' | 'x' | 'z' | 'y'));
        if self.focus.is_some() && ((!mods.command && key != ShortcutKey::Named(KeyInput::Escape)) || editing) {
            return false;
        }
        // Arrows, Enter and Escape belong to an open menu or popover.
        let layer_open = self.hits.iter().any(|h| matches!(h.action, Action::Outside(_)));
        if matches!(key, ShortcutKey::Named(_)) && !mods.command && layer_open {
            return false;
        }
        match self.shortcuts.iter().rev().position(|(s, _)| s.key == key && s.mods == mods) {
            Some(i) => {
                let i = self.shortcuts.len() - 1 - i;
                (self.shortcuts[i].1)(&mut self.state);
                true
            }
            None => false,
        }
    }

    /// Menu rows under the mouse become highlighted; moving onto a submenu row opens it.
    pub(super) fn menu_hover(&mut self) {
        for nav in &self.menu_nav {
            let st = self.menu_state.entry(nav.id).or_default();
            match nav.rows.iter().position(|(rid, k)| *k != Nav::Skip && self.hovered.contains(rid)) {
                Some(i) => {
                    self.menu_keyboard=Some(nav.id);
                    st.hl = Some(i);
                    st.sub = (nav.rows[i].1 == Nav::Sub).then_some(i);
                }
                // Off the rows only the row of an open submenu stays lit.
                None => st.hl = st.sub,
            }
        }
    }

    /// Arrow keys, Enter and Escape in the innermost open menu. `None` = not for a menu.
    pub(super) fn menu_key(&mut self, key: KeyInput) -> Option<bool> {
        // With a field focused (a search box over a list) ← → move its caret.
        if self.focus.is_some() && matches!(key, KeyInput::Left | KeyInput::Right) {
            return None;
        }
        let last=self.menu_nav.iter().rev().find(|n|n.keys)?;
        let peers:Vec<_>=self.menu_nav.iter().filter(|n|n.keys&&n.parent==last.parent).collect();
        let index=peers.iter().position(|n|Some(n.id)==self.menu_keyboard).unwrap_or(0);
        let nav=peers[index];
        let (id, parent) = (nav.id, nav.parent);
        let rows: Vec<(u64, Nav)> = nav.rows.clone();
        let hl = self.menu_state.get(&id).and_then(|s| s.hl).filter(|&i| i < rows.len() && rows[i].1 != Nav::Skip);
        // Peer lists in a compound choice panel share one dismissal boundary. Left/right
        // switch columns, while a highlighted submenu retains the usual Right behavior.
        if peers.len()>1 && matches!(key,KeyInput::Left|KeyInput::Right)
            && !(key==KeyInput::Right&&hl.is_some_and(|i|rows[i].1==Nav::Sub)) {
            let next=if key==KeyInput::Right {(index+1).min(peers.len()-1)}else{index.saturating_sub(1)};
            let peer=peers[next];self.menu_keyboard=Some(peer.id);
            let first=peer.rows.iter().position(|(_,kind)|*kind!=Nav::Skip);
            self.menu_state.entry(peer.id).or_default().hl=first;
            return Some(true);
        }
        self.menu_keyboard=Some(id);
        let st = self.menu_state.entry(id).or_default();
        match key {
            KeyInput::Up | KeyInput::Down => {
                let n = rows.len();
                let mut order: Box<dyn Iterator<Item = usize>> = match (hl, key == KeyInput::Down) {
                    (None, true) => Box::new(0..n),
                    (None, false) => Box::new((0..n).rev()),
                    (Some(i), true) => Box::new(i + 1..n),
                    (Some(i), false) => Box::new((0..i).rev()),
                };
                if let Some(j) = order.find(|&j| rows[j].1 != Nav::Skip) {
                    st.hl = Some(j);
                    st.sub = None;
                }
                Some(true)
            }
            KeyInput::Enter | KeyInput::Right => {
                let i = hl?;
                match rows[i].1 {
                    Nav::Sub => {
                        st.sub = Some(i);
                        self.menu_state.entry(sub_id(id, i)).or_default().first = true;
                        Some(true)
                    }
                    Nav::Act if key == KeyInput::Enter => {
                        let rid = rows[i].0;
                        let h = self.hits.iter().position(|h| h.id == rid)?;
                        self.activate(h);
                        Some(true)
                    }
                    _ => None,
                }
            }
            KeyInput::Left | KeyInput::Escape => {
                let p = parent?;
                self.menu_state.entry(p).or_default().sub = None;
                Some(true)
            }
            _ => None,
        }
    }
}
