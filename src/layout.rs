//! Single-pass, SwiftUI-flavoured layout: parents propose a size, children pick
//! their own, parents place them. Every node is laid out exactly once.

use crate::text::{TextSpec, TextSystem};
use crate::theme::Look;
use crate::ui::{slot, Spring};
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
    /// Window geometry is changing: layout follows it immediately, without springs.
    pub resizing: bool,
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
    /// A bottom-following scroll measures its lazy tail before selecting the first offset.
    pub tail_viewport: Option<f32>,
}

impl Lx<'_> {
    fn spring(&mut self, id: u64, slot: u8, target: f32) -> f32 {
        crate::ui::spring_with(self.springs, (self.frame, self.resizing), (id, slot), target, target, self.look.theme.motion.smooth).x
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

pub(crate) fn spec_in<'a>(ts: &crate::theme::TextStyle, text: &'a str) -> TextSpec<'a> {
    TextSpec { spans: &[], text, size: ts.size, weight: ts.weight as u16, line_height: ts.line_height, tracking: ts.tracking, ..Default::default() }
}

#[derive(Default, Clone)]
pub(crate) struct LNode {
    /// Offset from the parent's top-left corner.
    pub pos: (f32, f32),
    pub size: (f32, f32),
    pub children: Vec<LNode>,
    /// Shaped-text cache key (Text nodes only).
    pub text: u64,
    /// Natural width of a one-line hover marquee, shaped once during layout.
    pub marquee_width: f32,
    /// Content height (Scroll nodes only).
    pub content_h: f32,
    pub collapse_progress: f32,
    /// Scroll nodes: how much the content above the visible rows grew (a lazy list re-wrapped
    /// its rows); the offset follows so the screen stays put.
    pub shift: f32,
    pub row_target: Option<(u64,f32)>,
    /// Shaped labels with their sizes (Tabs nodes only).
    pub texts: Vec<(u64, (f32, f32))>,
    /// Measured or explicitly supplied width of each tab.
    pub tab_widths: Vec<f32>,
    /// Text that did not fit its lines and fades out (Text nodes only).
    pub fade: bool,
    /// Children are bigger than the node right now (`.animate()` in motion): cut them.
    pub clip: bool,
    /// Number of text views in the subtree, rows of a lazy list not counted. Text selection
    /// numbers them in tree order, which paint (with sticky children last) cannot see.
    pub tn: u32,
    /// Split nodes: width of the side region on screen (panel and divider) this frame.
    pub split: f32,
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
        last = (lx.keep(id, slot::LAST_W, None), lx.keep(id, slot::LAST_H, None));
        if ow == Some(0.0) && last.0 > 0.0 {
            ow = Some(last.0);
        }
        if oh == Some(0.0) && last.1 > 0.0 {
            oh = Some(last.1);
        }
    }
    let inner = (ow.map(|w| (w - pad_w).max(0.0)), oh.map(|h| (h - pad_h).max(0.0)));
    let split = v.extra.as_ref().and_then(|x| x.split.as_ref());

    let mut node = LNode::default();
    let mut hug_h = false;
    let content: (f32, f32) = match &v.kind {
        Kind::Empty | Kind::Spacer | Kind::Activity {..} => (0.0, 0.0),
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
                features: &t.features,
                owner: id,
            };
            if v.style.hover_marquee.is_some() {
                let (key,size,_) = lx.text.measure_lines(&spec,None,1,true);
                node.text = key;
                node.marquee_width = size.0;
                (size.0.min(inner.0.unwrap_or(size.0)),size.1)
            } else if t.lines > 0 {
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
        Kind::Collapse {open, child} => {
            let child_node=layout(child,(inner.0,None),lx,child_id(id,0,child.key));
            let target=if *open{1.0}else{0.0};
            let p=crate::ui::spring_with(lx.springs,(lx.frame,lx.resizing),(id,slot::COLLAPSE),target,target,lx.look.theme.motion.crisp).x.clamp(0.0,1.0);
            let size=(child_node.size.0,child_node.size.1*p);
            node.collapse_progress=p;node.children.push(child_node);size
        }
        Kind::AnimatedIcon { .. } => (22.0, 22.0),
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
        Kind::Picker { label, .. } => {
            let style=label.text_style();
            let reserved=style.size*1.5;
            let (key,size,_)=lx.text.measure_lines(&spec_in(&style,&label.content),inner.0.map(|w|(w-reserved).max(1.0)),1,false);
            node.text=key;
            node.texts.push((key,size));
            (size.0+reserved,size.1)
        }
        Kind::Tabs { labels, icons, widths, .. } => {
            let t = &lx.look.theme;
            let (font, k) = (t.text.subhead, t.control);
            for (index,l) in labels.iter().enumerate() {
                let spec = TextSpec { spans: &[], text: l, size: font.size, weight: 600, line_height: font.line_height, ..Default::default() };
                let m = lx.text.measure(&spec, None);
                let glyph=if icons.get(index).is_some(){k.tab_icon*1.5}else{0.0};
                node.tab_widths.push(widths.get(index).copied().flatten().unwrap_or(m.1.0+glyph+k.tab_extra).max(1.0));
                node.texts.push(m);
            }
            (node.tab_widths.iter().sum::<f32>() + 2.0*k.tab_padding, k.tab_height)
        }
        Kind::Toggle(..) => lx.look.theme.control.toggle,
        Kind::Progress { ring: false, .. } => (inner.0.unwrap_or(200.0), lx.look.theme.control.progress_bar),
        Kind::Progress { ring: true, .. } => {
            let d = lx.look.theme.control.progress_ring.0;
            let side = |dim: Dim, offer: Option<f32>| if dim == Dim::Auto { d } else { offer.unwrap_or(d) };
            (side(dim_w, inner.0), side(dim_h, inner.1))
        }
        Kind::Slider(.., st) => {
            let s = st.as_deref().unwrap_or(&lx.look.slider);
            (inner.0.unwrap_or(200.0), s.knob_size.max(s.track_height))
        }
        Kind::Menu { items, style } => {
            let st = style.clone().unwrap_or_else(|| lx.look.menu.clone());
            crate::ui::popup::layout_menu(items, &st, lx, id, &mut node)
        }
        Kind::List { count, row_h, row, target, .. } => {
            // Rows are built during paint (only the visible ones); here we just need the
            // total height: measured rows as measured, the rest at the estimate.
            let w = inner.0.unwrap_or(0.0);
            // Do not pass the outer tail budget into a nested list inside one row.
            let tail_viewport = lx.tail_viewport.take();
            let cache = lx.text.lists.entry(id).or_default();
            cache.heights.resize(*count, f32::NAN);
            {
                let was = std::mem::replace(&mut cache.width, w);
                let seeking=target.filter(|(_,request)|cache.target_request!=Some(*request) || (was-w).abs()>0.5);
                if let Some((_,request))=target {cache.target_request=Some(*request);}
                // Measure visible rows BEFORE the enclosing scroll chooses its offset.
                // Content can change at the same width (streamed markdown), not just on
                // resize. Measuring only in paint left the scroll range one frame behind,
                // making the bottom bounce on every added line. Offscreen rows stay lazy.
                let (seen, anchor) = (cache.seen, cache.anchor);
                let est = |h: f32| if h.is_nan() { *row_h } else { h };
                let old: Vec<f32> = cache.heights.iter().map(|&h| est(h)).collect();
                let mut measure = |i: usize| {
                    let mut rv = row(i);
                    crate::theme::apply(&mut rv, lx.look);
                    layout(&rv, (Some(w + pad_w), None), lx, child_id(id, i, rv.key)).size.1
                };
                let mut fresh = HashMap::new();
                if let Some((index,_))=seeking {
                    for i in 0..=index.min(count.saturating_sub(1)) {
                        if i<*count {fresh.insert(i,measure(i));}
                    }
                }
                if was > 0.0 {
                    let (a, b) = (seen.0.min(*count), (seen.1 + 2).min(*count));
                    let on_screen: f32 = old[a..seen.1.min(*count)].iter().sum();
                    for i in a..b { fresh.insert(i, measure(i)); }
                    // Narrower rows grow: cover the old screen height upwards too.
                    let mut covered: f32 = fresh.values().sum();
                    let mut i = a;
                    while i > 0 && covered < on_screen + 2.0 * row_h {
                        i -= 1;
                        let h = measure(i);
                        covered += h;
                        fresh.insert(i, h);
                    }
                }
                // The first visible frame must already use the real tail height. Estimates
                // can be much smaller than a single markdown message, so painting and then
                // correcting the offset on the following frame causes a visible top→bottom
                // jump. Only measure enough final rows to cover one viewport.
                if let Some(viewport) = tail_viewport {
                    let mut covered = 0.0;
                    for i in (0..*count).rev() {
                        let h = *fresh.entry(i).or_insert_with(|| measure(i));
                        covered += h;
                        if covered >= viewport { break; }
                    }
                }
                let cache = lx.text.lists.entry(id).or_default();
                for (i, h) in fresh {
                    if was > 0.0 && i < anchor { lx.shift += h - old[i]; }
                    cache.heights[i] = h;
                }
            }
            lx.tail_viewport = tail_viewport;
            let cache = lx.text.lists.entry(id).or_default();
            if let Some((usize::MAX,request))=target {
                node.row_target=Some((*request,f32::MAX));
            }
            if let Some((index,request))=target.filter(|(index,_)|*index<*count) {
                node.row_target=Some((request,cache.heights[..index].iter().map(|h|if h.is_nan(){*row_h}else{*h}).sum::<f32>()+pad[1]));
            }
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
        Kind::Scroll { child, stick, bottom_overlay, .. } => {
            let outer = std::mem::take(&mut lx.shift);
            // Measure the floating layer before content: its true wrapped height defines
            // both the visible tail budget and the empty space at the content's end.
            let outer_tail=lx.tail_viewport.take();
            let overlay=bottom_overlay.as_ref().map(|view|layout(view,inner,lx,child_id(id,1,view.key)));
            let overlay_h=overlay.as_ref().map_or(0.0,|node|node.size.1);
            lx.shift=0.0;
            lx.tail_viewport=if *stick {inner.1.map(|h|(h-overlay_h).max(0.0))} else {None};
            let c = layout(child, (inner.0, None), lx, child_id(id, 0, child.key));
            lx.tail_viewport = outer_tail;
            node.shift = std::mem::replace(&mut lx.shift, outer);
            node.content_h = c.size.1+overlay_h;
            // A scroll fills what it is offered, except in height under a max limit: there it
            // hugs the content up to the limit, then scrolls (a menu list "as tall as its
            // rows, at most 320"). `.height()` still fixes it.
            hug_h = !matches!(dim_h, Dim::Fixed(_)) && st.max.1.is_finite();
            let h = match inner.1 {
                Some(o) if hug_h => node.content_h.min(o),
                Some(o) => o,
                None => node.content_h,
            };
            let s = (inner.0.unwrap_or(c.size.0), h);
            node.children.push(c);
            if let Some(overlay)=overlay {node.children.push(overlay);}
            s
        }
        Kind::Stack { children, .. } if split.is_some() && children.len() == 2 => {
            let sp = split.expect("guarded");
            let line = lx.look.theme.control.border;
            let w = inner.0.unwrap_or(0.0);
            let (panel, main) = (&children[0], &children[1]);
            let (min, max) = (panel.style.min.0, panel.style.max.0.max(panel.style.min.0));
            let open = sp.width.clamp(min, max);
            let collapsed = sp.collapse.as_ref().is_some_and(|c| c.0);
            let region = lx.spring(id, slot::SPLIT, if collapsed { 0.0 } else { open + line }).clamp(0.0, w);
            // The panel fills its region, but never narrower than its open width: closing,
            // it slides out whole instead of being squeezed.
            let pw = open.max(region - line);
            let pn = layout(panel, (Some(pw), inner.1), lx, child_id(id, 0, panel.key));
            let mn = layout(main, (Some((w - region).max(0.0)), inner.1), lx, child_id(id, 1, main.key));
            let (px, mx) = if sp.trailing { (w - region + line, 0.0) } else { ((region - line - pn.size.0).min(0.0), region) };
            let h = inner.1.unwrap_or(pn.size.1.max(mn.size.1));
            node.split = region;
            node.children = vec![pn, mn];
            node.children[0].pos = (pad[0] + px, pad[1]);
            node.children[1].pos = (pad[0] + mx, pad[1]);
            (w, h)
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
            lx.keep(id, slot::LAST_W, Some(target.0));
        } else if last.0 > 0.0 {
            laid.0 = last.0;
        }
        if target.1 > 0.0 {
            lx.keep(id, slot::LAST_H, Some(target.1));
        } else if last.1 > 0.0 {
            laid.1 = last.1;
        }
        // A size that follows the parent (fill) tracks window resizes directly.
        if dim_w != Dim::Fill {
            node.size.0 = lx.spring(id, slot::W, target.0).max(0.0);
        }
        if dim_h != Dim::Fill {
            node.size.1 = lx.spring(id, slot::H, target.1).max(0.0);
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
        Kind::Stack { .. } if split.is_some() && node.children.len() == 2 => {} // placed above
        Kind::Scroll { .. } => {
            node.children[0].pos = (pad[0], pad[1]);
            if let Some(overlay)=node.children.get_mut(1) {
                overlay.pos=(pad[0]+(laid.0-pad_w-overlay.size.0)*0.5,laid.1-pad[3]-overlay.size.1);
            }
        }
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
    use crate::ui::popup::{pop_id, tip_id};
    use crate::view::Opens;
    let fading = |lx: &Lx, lid: u64| lx.springs.get(&(lid, slot::FADE)).is_some_and(|s| s.x > 0.004);
    if let Some(p) = &x.pop {
        let open = match &p.opens {
            Opens::State(open, _) => *open,
            _ => lx.menu == Some(id),
        };
        let pid = pop_id(id);
        if open || fading(lx, pid) {
            let width=if p.match_anchor_width {Some(node.size.0)} else {None};
            node.pop = Some(Box::new(layout(&p.content, (width, None), lx, pid)));
        }
    }
    if let Some(t) = &x.tip {
        let tid = tip_id(id);
        if lx.tip == Some(id) || fading(lx, tid) {
            node.tip = Some(Box::new(layout(t, (None, None), lx, tid)));
        }
    }
}
