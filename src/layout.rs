//! Single-pass, SwiftUI-flavoured layout: parents propose a size, children pick
//! their own, parents place them. Every node is laid out exactly once.

use crate::text::{TextSpec, TextSystem};
use crate::theme::Look;
use crate::ui::Spring;
use crate::view::{Align, Axis, Dim, Kind, OverlayMode, Truncate, View};
use std::collections::HashMap;

/// What layout needs besides the tree: the text shaper and, for `.animate()`, the springs
/// that carry animated sizes from frame to frame.
pub(crate) struct Lx<'a> {
    pub text: &'a mut TextSystem,
    /// The theme: sizes of the built-in controls, the `.animate()` spring.
    pub look: &'a Look,
    pub springs: &'a mut HashMap<(u64, u8), Spring>,
    pub frame: u64,
    /// Set by a stack for a `.flex()` child just before laying it out: the axis along which
    /// the child takes its share, so it fills it without knowing its parent.
    pub along: Option<Axis>,
    /// The view whose framework-managed menu is open (`Ui::open_menu`).
    pub menu: Option<u64>,
    /// The view whose tooltip shows.
    pub tip: Option<u64>,
    /// How far lazy lists moved their anchor row while re-measuring for a new width; the
    /// enclosing scroll view takes it into its offset (see `LNode::shift`).
    pub shift: f32,
}

impl Lx<'_> {
    fn spring(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(target, target, self.look.theme.motion.smooth));
        s.target = target;
        s.used = self.frame;
        s.x
    }
    /// A value kept between frames (a spring that never moves).
    fn keep(&mut self, id: u64, slot: u8, value: Option<f32>) -> f32 {
        let s = self.springs.entry((id, slot)).or_insert(Spring::new(0.0, 0.0, Spring::SMOOTH));
        if let Some(v) = value {
            (s.x, s.target) = (v, v);
        }
        s.used = self.frame;
        s.x
    }
}

/// A one-line label in the theme's body style (picker, its drop-down).
pub(crate) fn body_spec<'a>(lx: &Lx, text: &'a str) -> TextSpec<'a> {
    spec_in(&lx.look.theme.text.body, text)
}

pub(crate) fn spec_in<'a>(ts: &crate::theme::TextStyle, text: &'a str) -> TextSpec<'a> {
    TextSpec { spans: &[], text, size: ts.size, weight: ts.weight as u16, line_height: ts.line_height, tracking: ts.tracking, ..Default::default() }
}

/// Spring slots used by layout (paint uses 0..=9).
const SLOT_W: u8 = 10;
const SLOT_H: u8 = 11;
const SLOT_LAST_W: u8 = 12;
const SLOT_LAST_H: u8 = 13;

#[derive(Default, Clone)]
pub(crate) struct LNode {
    /// Offset from the parent's top-left corner.
    pub pos: (f32, f32),
    pub size: (f32, f32),
    pub children: Vec<LNode>,
    /// Shaped-text cache key (Text nodes only).
    pub text: u64,
    /// Content height (Scroll nodes only).
    pub content_h: f32,
    /// Scroll nodes: how much the content above the visible rows grew (a lazy list re-wrapped
    /// its rows); the offset follows so the screen stays put.
    pub shift: f32,
    /// Shaped labels with their sizes (Tabs nodes only).
    pub texts: Vec<(u64, (f32, f32))>,
    /// Text that did not fit its lines and fades out (Text nodes only).
    pub fade: bool,
    /// Children are bigger than the node right now (`.animate()` in motion): cut them.
    pub clip: bool,
    /// Number of text views in the subtree, rows of a lazy list not counted. Text selection
    /// numbers them in tree order, which paint (with sticky children last) cannot see.
    pub tn: u32,
    /// The attached popover's content and the tooltip, when they are shown (or fading out).
    pub pop: Option<Box<LNode>>,
    pub tip: Option<Box<LNode>>,
}


fn fills_main<S>(v: &View<S>, axis: Axis) -> bool {
    if matches!(v.kind, Kind::Spacer) || v.style.flex > 0.0 {
        return true;
    }
    match axis {
        Axis::H => v.style.width == Dim::Fill,
        Axis::V => v.style.height == Dim::Fill,
        Axis::Z => false,
    }
}

fn aligned(a: Align, space: f32, size: f32) -> f32 {
    match a {
        Align::Start => 0.0,
        Align::Center => (space - size) * 0.5,
        Align::End => space - size,
    }
}

/// Identity of a child: its key if it has one, otherwise its position among siblings.
/// Layout and paint derive the same ids, so per-node state can be shared between them.
pub(crate) fn child_id(parent: u64, index: usize, key: Option<u64>) -> u64 {
    // FxHash-style mix; cheap and good enough for sibling paths.
    let v = key.unwrap_or(index as u64 ^ 0x9E37_79B9_7F4A_7C15);
    (parent.rotate_left(5) ^ v).wrapping_mul(0x517C_C1B7_2722_0A95)
}

pub(crate) fn layout<S>(v: &View<S>, prop: (Option<f32>, Option<f32>), lx: &mut Lx, id: u64) -> LNode {
    let st = &v.style;
    // A flex child fills its share along the parent's axis unless it has a size there.
    let (dim_w, dim_h) = match lx.along.take() {
        Some(Axis::H) if st.width == Dim::Auto => (Dim::Fill, st.height),
        Some(Axis::V) if st.height == Dim::Auto => (st.width, Dim::Fill),
        _ => (st.width, st.height),
    };
    let pad = st.padding;
    let (pad_w, pad_h) = (pad[0] + pad[2], pad[1] + pad[3]);
    let clamp_w = |w: f32| w.min(st.max.0).max(st.min.0);
    let clamp_h = |h: f32| h.min(st.max.1).max(st.min.1);
    // The size offered to the content: exact if fixed, else the parent's offer, within the
    // limits. A max limit is an offer even when the parent makes none (text wraps at it).
    let offer = |dim: Dim, prop: Option<f32>, max: f32| match dim {
        Dim::Fixed(x) => Some(x),
        _ => prop.or(max.is_finite().then_some(max)),
    };
    let mut ow = offer(dim_w, prop.0, st.max.0).map(clamp_w);
    let mut oh = offer(dim_h, prop.1, st.max.1).map(clamp_h);
    // An animated view collapsing to 0 keeps laying its content out at the last real size,
    // so the content slides out of view instead of being squeezed.
    let mut last = (0.0f32, 0.0f32);
    if st.animate {
        last = (lx.keep(id, SLOT_LAST_W, None), lx.keep(id, SLOT_LAST_H, None));
        if ow == Some(0.0) && last.0 > 0.0 {
            ow = Some(last.0);
        }
        if oh == Some(0.0) && last.1 > 0.0 {
            oh = Some(last.1);
        }
    }
    let inner = (ow.map(|w| (w - pad_w).max(0.0)), oh.map(|h| (h - pad_h).max(0.0)));

    let mut node = LNode::default();
    let mut hug_h = false;
    let content: (f32, f32) = match &v.kind {
        Kind::Empty | Kind::Spacer => (0.0, 0.0),
        Kind::Text(t) => {
            let spec =
                TextSpec {
                spans: &t.spans,
                text: &t.content,
                size: t.size,
                weight: t.weight as u16,
                center: t.center,
                line_height: t.line_height,
                tracking: t.tracking,
                owner: id,
            };
            if t.lines > 0 {
                let fade = t.truncate == Truncate::Fade;
                let (key, size, cut) = lx.text.measure_lines(&spec, inner.0, t.lines, fade);
                node.text = key;
                node.fade = cut && fade;
                size
            } else {
                let (key, size) = lx.text.measure(&spec, inner.0);
                node.text = key;
                size
            }
        }
        Kind::Image(img, _) => {
            let (nw, nh) = (img.0.width as f32, img.0.height.max(1) as f32);
            let w = match dim_w {
                Dim::Auto => inner.0.map_or(nw, |p| p.min(nw)),
                _ => inner.0.unwrap_or(nw),
            };
            let h = match dim_h {
                Dim::Auto => w * nh / nw.max(1.0),
                _ => inner.1.unwrap_or(w * nh / nw.max(1.0)),
            };
            (w, h)
        }
        Kind::TextField(f) => crate::ui::edit::layout_field(f, inner.0, lx.text, &mut node),
        Kind::Picker { options, selected, .. } => {
            let label = options.get(*selected).map_or("", |s| s.as_str());
            let (key, size) = lx.text.measure(&body_spec(lx, label), None);
            node.text = key;
            (size.0 + 26.0, size.1)
        }
        Kind::Tabs { labels, .. } => {
            let mut widest = 0.0f32;
            let t = &lx.look.theme;
            let (font, k) = (t.text.subhead, t.control);
            for l in labels {
                let spec = TextSpec { spans: &[], text: l, size: font.size, weight: 600, line_height: font.line_height, ..Default::default() };
                let m = lx.text.measure(&spec, None);
                widest = widest.max(m.1 .0);
                node.texts.push(m);
            }
            ((widest + k.tab_extra) * labels.len() as f32 + 2.0 * k.tab_padding, k.tab_height)
        }
        Kind::Toggle(..) => lx.look.theme.control.toggle,
        Kind::Slider(.., st) => {
            let s = st.as_deref().unwrap_or(&lx.look.slider);
            (inner.0.unwrap_or(200.0), s.knob_size.max(s.track_height))
        }
        Kind::Menu { items, style } => {
            let st = style.clone().unwrap_or_else(|| lx.look.menu.clone());
            crate::ui::popup::layout_menu(items, &st, lx, id, &mut node)
        }
        Kind::List { count, row_h, row } => {
            // Rows are built during paint (only the visible ones); here we just need the
            // total height: measured rows as measured, the rest at the estimate.
            let w = inner.0.unwrap_or(0.0);
            let cache = lx.text.lists.entry(id).or_default();
            cache.heights.resize(*count, f32::NAN);
            if cache.width != w {
                let was = std::mem::replace(&mut cache.width, w);
                // Rows wrap differently at another width. Forgetting every height would swap
                // the total for estimates and make a resizing window jump each frame; the old
                // heights stay as estimates, and the rows that were on screen are measured
                // again right here (an extra layout of a few lazy rows, like paint does), so
                // the end of a feed stays on the bottom and the anchor row stays put.
                if was > 0.0 {
                    let (seen, anchor) = (cache.seen, cache.anchor);
                    let est = |h: f32| if h.is_nan() { *row_h } else { h };
                    let old: Vec<f32> = cache.heights.iter().map(|&h| est(h)).collect();
                    let (a, b) = (seen.0.min(*count), (seen.1 + 2).min(*count));
                    let on_screen: f32 = old[a..seen.1.min(*count)].iter().sum();
                    let mut measure = |i: usize| {
                        let mut rv = row(i);
                        crate::theme::apply(&mut rv, lx.look);
                        layout(&rv, (Some(w + pad_w), None), lx, child_id(id, i, rv.key)).size.1
                    };
                    let mut fresh: Vec<(usize, f32)> = (a..b).map(|i| (i, measure(i))).collect();
                    // Narrower rows grow: cover the old screen height upwards too.
                    let mut covered: f32 = fresh.iter().map(|f| f.1).sum();
                    let mut i = a;
                    while i > 0 && covered < on_screen + 2.0 * row_h {
                        i -= 1;
                        let h = measure(i);
                        covered += h;
                        fresh.push((i, h));
                    }
                    let cache = lx.text.lists.entry(id).or_default();
                    for (i, h) in fresh {
                        if i < anchor {
                            lx.shift += h - old[i];
                        }
                        cache.heights[i] = h;
                    }
                }
            }
            let cache = lx.text.lists.entry(id).or_default();
            (w, cache.heights.iter().map(|h| if h.is_nan() { *row_h } else { *h }).sum())
        }
        Kind::Overlay { mode, child, .. } => {
            let (w, h) = (inner.0.unwrap_or(400.0), inner.1.unwrap_or(600.0));
            let max_w = match mode {
                OverlayMode::Modal => (w - 48.0).min(440.0),
                OverlayMode::Sheet => w.min(600.0),
                OverlayMode::Toast => (w - 32.0).min(480.0),
                OverlayMode::Drawer => (w - 56.0).min(320.0),
            };
            let cid = child_id(id, 0, child.key);
            let full_h = (*mode == OverlayMode::Drawer).then_some(h);
            let mut c = layout(child, (Some(max_w.max(1.0)), full_h), lx, cid);
            if *mode == OverlayMode::Toast {
                // A toast hugs its content.
                let natural = layout(child, (None, None), lx, cid);
                if natural.size.0 <= max_w {
                    c = natural;
                }
            }
            node.children.push(c);
            (w, h)
        }
        Kind::Scroll { child, .. } => {
            let outer = std::mem::take(&mut lx.shift);
            let c = layout(child, (inner.0, None), lx, child_id(id, 0, child.key));
            node.shift = std::mem::replace(&mut lx.shift, outer);
            node.content_h = c.size.1;
            // A scroll fills what it is offered, except in height under a max limit: there it
            // hugs the content up to the limit, then scrolls (a menu list "as tall as its
            // rows, at most 320"). `.height()` still fixes it.
            hug_h = !matches!(dim_h, Dim::Fixed(_)) && st.max.1.is_finite();
            let h = match inner.1 {
                Some(o) if hug_h => c.size.1.min(o),
                Some(o) => o,
                None => c.size.1,
            };
            let s = (inner.0.unwrap_or(c.size.0), h);
            node.children.push(c);
            s
        }
        Kind::Stack { axis: Axis::Z, children, .. } => {
            let mut s = (0.0f32, 0.0f32);
            for (i, c) in children.iter().enumerate() {
                let n = layout(c, inner, lx, child_id(id, i, c.key));
                s = (s.0.max(n.size.0), s.1.max(n.size.1));
                node.children.push(n);
            }
            s
        }
        Kind::Stack { axis, spacing, children, .. } => {
            let h = *axis == Axis::H;
            let main_prop = if h { inner.0 } else { inner.1 };
            let cross_prop = if h { inner.1 } else { inner.0 };
            let prop_for = |main: Option<f32>| if h { (main, cross_prop) } else { (cross_prop, main) };
            let gaps = spacing * children.len().saturating_sub(1) as f32;
            let mut used = gaps;
            let mut cross = 0.0f32;
            let weight = |c: &View<_>| if c.style.flex > 0.0 { c.style.flex } else { 1.0 };
            let mut flex = 0.0f32;
            node.children = children.iter().map(|_| LNode::default()).collect();
            // Pass 1: fixed children take what they need.
            for (i, c) in children.iter().enumerate() {
                if fills_main(c, *axis) {
                    flex += weight(c);
                    continue;
                }
                let n = layout(c, prop_for(main_prop.map(|m| (m - used).max(0.0))), lx, child_id(id, i, c.key));
                used += if h { n.size.0 } else { n.size.1 };
                cross = cross.max(if h { n.size.1 } else { n.size.0 });
                node.children[i] = n;
            }
            // Pass 2: flexible children share the rest.
            if flex > 0.0 {
                let free = main_prop.map(|m| (m - used).max(0.0));
                for (i, c) in children.iter().enumerate() {
                    if !fills_main(c, *axis) {
                        continue;
                    }
                    let share = free.map(|f| f * weight(c) / flex);
                    if c.style.flex > 0.0 {
                        lx.along = Some(*axis);
                    }
                    let mut n = layout(c, prop_for(share), lx, child_id(id, i, c.key));
                    if let (Kind::Spacer, Some(sh)) = (&c.kind, share) {
                        if h {
                            n.size.0 = sh
                        } else {
                            n.size.1 = sh
                        }
                    }
                    used += if h { n.size.0 } else { n.size.1 };
                    cross = cross.max(if h { n.size.1 } else { n.size.0 });
                    node.children[i] = n;
                }
            }
            if h {
                (used, cross)
            } else {
                (cross, used)
            }
        }
    };

    let resolve = |dim: Dim, prop: Option<f32>, content: f32| match dim {
        Dim::Fixed(x) => x,
        Dim::Fill => prop.unwrap_or(content),
        Dim::Auto => content,
    };
    node.size = (
        clamp_w(resolve(dim_w, prop.0, content.0 + pad_w)),
        clamp_h(if hug_h { content.1 + pad_h } else { resolve(dim_h, prop.1, content.1 + pad_h) }),
    );
    // Children are placed in the size they were laid out for; an animated node then shows
    // only as much of it as its spring has reached.
    let mut laid = node.size;
    if st.animate {
        let target = node.size;
        if target.0 > 0.0 {
            lx.keep(id, SLOT_LAST_W, Some(target.0));
        } else if last.0 > 0.0 {
            laid.0 = last.0;
        }
        if target.1 > 0.0 {
            lx.keep(id, SLOT_LAST_H, Some(target.1));
        } else if last.1 > 0.0 {
            laid.1 = last.1;
        }
        // A size that follows the parent (fill) tracks window resizes directly.
        if dim_w != Dim::Fill {
            node.size.0 = lx.spring(id, SLOT_W, target.0).max(0.0);
        }
        if dim_h != Dim::Fill {
            node.size.1 = lx.spring(id, SLOT_H, target.1).max(0.0);
        }
        node.clip = (node.size.0 - laid.0).abs() > 0.25 || (node.size.1 - laid.1).abs() > 0.25;
    }
    let (iw, ih) = ((laid.0 - pad_w).max(0.0), (laid.1 - pad_h).max(0.0));

    node.tn = match &v.kind {
        Kind::Text(_) => 1,
        Kind::List { .. } => 0,
        _ => node.children.iter().map(|c| c.tn).sum(),
    };

    // Place children now that the final size is known.
    match &v.kind {
        Kind::Scroll { .. } => node.children[0].pos = (pad[0], pad[1]),
        Kind::Stack { axis: Axis::Z, align, children, .. } => {
            for (n, c) in node.children.iter_mut().zip(children) {
                let (ax, ay) = c.style.place.unwrap_or((*align, *align));
                n.pos = (pad[0] + aligned(ax, iw, n.size.0), pad[1] + aligned(ay, ih, n.size.1));
            }
        }
        Kind::Stack { axis, spacing, align, .. } => {
            let mut cursor = 0.0;
            for n in node.children.iter_mut() {
                if *axis == Axis::H {
                    n.pos = (pad[0] + cursor, pad[1] + aligned(*align, ih, n.size.1));
                    cursor += n.size.0 + spacing;
                } else {
                    n.pos = (pad[0] + aligned(*align, iw, n.size.0), pad[1] + cursor);
                    cursor += n.size.1 + spacing;
                }
            }
        }
        _ => {}
    }
    if let Some(x) = &v.extra {
        layout_attached(x, lx, id, &mut node);
    }
    node
}

/// A popover or tooltip is laid out right after its anchor, at its natural size, but only
/// while it is open or still fading out.
fn layout_attached<S>(x: &crate::view::Extras<S>, lx: &mut Lx, id: u64, node: &mut LNode) {
    use crate::ui::popup::{pop_id, tip_id, SLOT_FADE};
    use crate::view::Opens;
    let fading = |lx: &Lx, lid: u64| lx.springs.get(&(lid, SLOT_FADE)).is_some_and(|s| s.x > 0.004);
    if let Some(p) = &x.pop {
        let open = match &p.opens {
            Opens::State(open, _) => *open,
            _ => lx.menu == Some(id),
        };
        let pid = pop_id(id);
        if open || fading(lx, pid) {
            node.pop = Some(Box::new(layout(&p.content, (None, None), lx, pid)));
        }
    }
    if let Some(t) = &x.tip {
        let tid = tip_id(id);
        if lx.tip == Some(id) || fading(lx, tid) {
            node.tip = Some(Box::new(layout(t, (None, None), lx, tid)));
        }
    }
}
