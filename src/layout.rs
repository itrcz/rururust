//! Single-pass, SwiftUI-flavoured layout: parents propose a size, children pick
//! their own, parents place them. Every node is laid out exactly once.

use crate::text::{TextSpec, TextSystem};
use crate::view::{Align, Axis, Dim, Kind, OverlayMode, View};

#[derive(Default)]
pub(crate) struct LNode {
    /// Offset from the parent's top-left corner.
    pub pos: (f32, f32),
    pub size: (f32, f32),
    pub children: Vec<LNode>,
    /// Shaped-text cache key (Text nodes only).
    pub text: u64,
    /// Content height (Scroll nodes only).
    pub content_h: f32,
    /// Shaped labels with their sizes (Tabs nodes only).
    pub texts: Vec<(u64, (f32, f32))>,
}

pub(crate) const TOGGLE: (f32, f32) = (51.0, 31.0);
pub(crate) const SLIDER_H: f32 = 28.0;
pub(crate) const FIELD_FONT: f32 = 17.0;
pub(crate) const TABS_PAD: f32 = 6.0;

fn fills_main<S>(v: &View<S>, axis: Axis) -> bool {
    if matches!(v.kind, Kind::Spacer) {
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

pub(crate) fn layout<S>(v: &View<S>, prop: (Option<f32>, Option<f32>), ts: &mut TextSystem, id: u64) -> LNode {
    let st = &v.style;
    let pad = st.padding;
    let (pad_w, pad_h) = (pad[0] + pad[2], pad[1] + pad[3]);
    let ow = if let Dim::Fixed(w) = st.width { Some(w) } else { prop.0 };
    let oh = if let Dim::Fixed(h) = st.height { Some(h) } else { prop.1 };
    let inner = (ow.map(|w| (w - pad_w).max(0.0)), oh.map(|h| (h - pad_h).max(0.0)));

    let mut node = LNode::default();
    let content: (f32, f32) = match &v.kind {
        Kind::Empty | Kind::Spacer => (0.0, 0.0),
        Kind::Text(t) => {
            let spec =
                TextSpec { spans: &t.spans, text: &t.content, size: t.size, weight: t.weight as u16, center: t.center };
            let (key, size) = ts.measure(&spec, inner.0);
            node.text = key;
            size
        }
        Kind::Image(img, _) => {
            let (nw, nh) = (img.0.width as f32, img.0.height.max(1) as f32);
            let w = match st.width {
                Dim::Auto => inner.0.map_or(nw, |p| p.min(nw)),
                _ => inner.0.unwrap_or(nw),
            };
            let h = match st.height {
                Dim::Auto => w * nh / nw.max(1.0),
                _ => inner.1.unwrap_or(w * nh / nw.max(1.0)),
            };
            (w, h)
        }
        Kind::TextField { value, placeholder, .. } => {
            let shown = if value.is_empty() { placeholder } else { value };
            let (key, size) = ts.measure(&TextSpec { spans: &[], text: shown, size: FIELD_FONT, weight: 400, center: false }, None);
            node.text = key;
            (inner.0.unwrap_or(220.0), size.1)
        }
        Kind::Picker { options, selected, .. } => {
            let label = options.get(*selected).map_or("", |s| s.as_str());
            let (key, size) = ts.measure(&TextSpec { spans: &[], text: label, size: FIELD_FONT, weight: 400, center: false }, None);
            node.text = key;
            (size.0 + 26.0, size.1)
        }
        Kind::Tabs { labels, .. } => {
            let mut widest = 0.0f32;
            for l in labels {
                let m = ts.measure(&TextSpec { spans: &[], text: l, size: 15.0, weight: 600, center: false }, None);
                widest = widest.max(m.1 .0);
                node.texts.push(m);
            }
            ((widest + 40.0) * labels.len() as f32 + 2.0 * TABS_PAD, 56.0)
        }
        Kind::Toggle(..) => TOGGLE,
        Kind::Slider(..) => (inner.0.unwrap_or(200.0), SLIDER_H),
        Kind::List { count, row_h, .. } => {
            // Rows are built during paint (only the visible ones); here we just need the
            // total height: measured rows as measured, the rest at the estimate.
            let w = inner.0.unwrap_or(0.0);
            let cache = ts.lists.entry(id).or_default();
            if cache.width != w {
                cache.width = w;
                cache.heights.clear(); // rows wrap differently at another width
            }
            cache.heights.resize(*count, f32::NAN);
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
            let mut c = layout(child, (Some(max_w.max(1.0)), full_h), ts, cid);
            if *mode == OverlayMode::Toast {
                // A toast hugs its content.
                let natural = layout(child, (None, None), ts, cid);
                if natural.size.0 <= max_w {
                    c = natural;
                }
            }
            node.children.push(c);
            (w, h)
        }
        Kind::Scroll { child, .. } => {
            let c = layout(child, (inner.0, None), ts, child_id(id, 0, child.key));
            node.content_h = c.size.1;
            let s = (inner.0.unwrap_or(c.size.0), inner.1.unwrap_or(c.size.1));
            node.children.push(c);
            s
        }
        Kind::Stack { axis: Axis::Z, children, .. } => {
            let mut s = (0.0f32, 0.0f32);
            for (i, c) in children.iter().enumerate() {
                let n = layout(c, inner, ts, child_id(id, i, c.key));
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
            let mut flex = 0usize;
            node.children = children.iter().map(|_| LNode::default()).collect();
            // Pass 1: fixed children take what they need.
            for (i, c) in children.iter().enumerate() {
                if fills_main(c, *axis) {
                    flex += 1;
                    continue;
                }
                let n = layout(c, prop_for(main_prop.map(|m| (m - used).max(0.0))), ts, child_id(id, i, c.key));
                used += if h { n.size.0 } else { n.size.1 };
                cross = cross.max(if h { n.size.1 } else { n.size.0 });
                node.children[i] = n;
            }
            // Pass 2: flexible children share the rest.
            if flex > 0 {
                let share = main_prop.map(|m| ((m - used).max(0.0)) / flex as f32);
                for (i, c) in children.iter().enumerate() {
                    if !fills_main(c, *axis) {
                        continue;
                    }
                    let mut n = layout(c, prop_for(share), ts, child_id(id, i, c.key));
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
    node.size = (resolve(st.width, prop.0, content.0 + pad_w), resolve(st.height, prop.1, content.1 + pad_h));
    let (iw, ih) = ((node.size.0 - pad_w).max(0.0), (node.size.1 - pad_h).max(0.0));

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
    node
}
