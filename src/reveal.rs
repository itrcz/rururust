//! Paint-only streaming reveal. Cached glyph masks use a small GPU blur; text layout is
//! never animated or reshaped for the effect, and settled nodes use the normal text atlas.
use super::{Cmd, Painter, Xf};
use crate::{Color, Image, Rect, Span};
use cosmic_text::{CacheKey, SwashCache, SwashContent};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct Reveals {
    nodes: HashMap<u64, Run>,
    masks: HashMap<CacheKey, Mask>,
    swash: SwashCache,
}

impl Default for Reveals {
    fn default() -> Self {
        Self { nodes: HashMap::new(), masks: HashMap::new(), swash: SwashCache::new() }
    }
}

#[derive(Default)]
struct Run {
    source: String,
    births: Vec<(usize, f32)>,
    used: u64,
}

struct Mask {
    image: Image,
    left: i32,
    top: i32,
    template: bool,
    used: u64,
}

impl Run {
    fn update_center(&mut self,source:&str,now:f32,stagger:f32) {
        if self.source==source {return;}
        let graphemes:Vec<_>=source.grapheme_indices(true).collect();
        let last=graphemes.len().saturating_sub(1);
        self.births=graphemes.iter().enumerate().map(|(i,(byte,_))| {
            let rank=(2*i).abs_diff(last)/2;
            (*byte,now+rank as f32*stagger)
        }).collect();
        self.source=source.to_owned();
    }

    fn update(&mut self, source: &str, now: f32) {
        if self.source == source {
            return;
        }
        // Match existing graphemes across small Markdown delimiter changes as well as
        // ordinary appends. Styling **bold** must not replay the whole paragraph.
        let old: Vec<_> = self.source.graphemes(true).collect();
        let mut at = 0;
        let mut added = 0;
        let mut births = Vec::new();
        for (byte, g) in source.grapheme_indices(true) {
            let next = (at..(at + 8).min(old.len())).find(|&i| old[i] == g);
            let born = if let Some(i) = next {
                at = i + 1;
                self.births[i].1
            } else {
                // A word arriving in one packet still unfolds over a few display frames.
                let born = now + (added as f32 * 0.008).min(0.064);
                added += 1;
                born
            };
            births.push((byte, born));
        }
        self.births = births;
        self.source = source.to_owned();
    }

    fn progress(&self, byte: usize, now: f32, duration: f32) -> f32 {
        let i = self.births.partition_point(|(b, _)| *b <= byte).saturating_sub(1);
        let born = self.births.get(i).map_or(now - duration, |b| b.1);
        let t = ((now - born) / duration).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

impl Reveals {
    pub(super) fn end_frame(&mut self, frame: u64) {
        self.nodes.retain(|_, r| r.used == frame);
        // Glyph images are shared between letters/paragraphs and survive short pauses.
        // Keep both CPU masks and swash's raster cache bounded for long conversations.
        if frame % 120 == 0 {
            self.masks.retain(|_, m| m.used + 120 >= frame);
            self.swash.image_cache.retain(|k, _| self.masks.contains_key(k));
        }
    }
}

impl<S> Painter<'_, S> {
    pub(super) fn paint_reveal(&mut self, xf: &Xf, id: u64, key: u64, spans: &[Span], content: &str, pos: (f32, f32), scale: f32) -> bool {
        let effect = xf.text_reveal.unwrap_or_default();
        let duration = effect.duration;
        let source = crate::text::shaped_text(spans, content);
        let run = self.reveals.nodes.entry(id).or_default();
        run.used = self.frame;
        if let Some(stagger)=effect.center_stagger {run.update_center(&source,self.time,stagger);} else {run.update(&source, self.time);}
        if self.resizing {
            for (_, born) in &mut run.births { *born = self.time - duration; }
            return false;
        }
        if run.births.iter().all(|b| self.time - b.1 >= duration) {
            return false;
        }
        self.spin = true; // redraw only until the last new glyph has become sharp
        let px = self.px.max(1.0);
        let physical_scale = scale * px;
        let glyphs = self.text.reveal_glyphs(key, (pos.0 * px, (pos.1 * px).round()), physical_scale);
        for (glyph, baseline, byte, color) in glyphs {
            let progress = self.reveals.nodes[&id].progress(byte, self.time, duration);
            if progress <= 0.001 {
                continue;
            }
            let mask_key = glyph.cache_key;
            if !self.reveals.masks.contains_key(&mask_key) {
                let Some(src) = self.reveals.swash.get_image(&mut self.text.fonts, mask_key) else { continue };
                let p = &src.placement;
                if p.width == 0 || p.height == 0 {
                    continue;
                }
                // Transparent padding holds the entire blur, including accents/descenders.
                let pad = (5.0 * px).ceil() as u32;
                let (w, h) = (p.width + pad * 2, p.height + pad * 2);
                let template = src.content != SwashContent::Color;
                let mut rgba = vec![0; (w * h * 4) as usize];
                for y in 0..p.height as usize {
                    for x in 0..p.width as usize {
                        let from = y * p.width as usize + x;
                        let to = ((y + pad as usize) * w as usize + x + pad as usize) * 4;
                        let pixel = match src.content {
                            SwashContent::Mask => [255, 255, 255, src.data[from]],
                            SwashContent::Color => src.data[from * 4..from * 4 + 4].try_into().unwrap(),
                            SwashContent::SubpixelMask => {
                                [255, 255, 255, src.data[from * 4..from * 4 + 3].iter().copied().max().unwrap_or(0)]
                            }
                        };
                        rgba[to..to + 4].copy_from_slice(&pixel);
                    }
                }
                self.reveals.masks.insert(
                    mask_key,
                    Mask {
                        image: Image::from_rgba(w, h, rgba),
                        left: p.left - pad as i32,
                        top: p.top + pad as i32,
                        template,
                        used: self.frame,
                    },
                );
            }
            let mask = self.reveals.masks.get_mut(&mask_key).unwrap();
            mask.used = self.frame;
            let image = mask.image.clone();
            let rect = Rect::new(
                (glyph.x + mask.left) as f32 / px,
                ((baseline * physical_scale).round() as i32 + glyph.y - mask.top) as f32 / px,
                image.size().0 as f32 / px,
                image.size().1 as f32 / px,
            );
            if !rect.intersects(&xf.clip) {
                continue;
            }
            let color = color
                .map_or(xf.fg, |c| Color::rgba(c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0));
            let tint = mask.template.then_some(color);
            // Rect is already in screen coordinates. Keep the original clip and opacity.
            let mut image_xf = *xf;
            image_xf.s = 1.0;
            image_xf.tx = 0.0;
            image_xf.ty = 0.0;
            image_xf.opacity *= progress;
            self.picture(&image_xf, rect, 0.0, &image, tint, None);
            if let Some(Cmd::Quad(q)) = self.list.cmds.last_mut() {
                // n0.x is unused by ordinary pictures; shader converts pt to pixels.
                q.n0[0] = 1.4 * (1.0 - progress);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_reveal_pairs_unicode_graphemes_and_preserves_births() {
        let mut run=Run::default();
        let source="Aе\u{301}👨‍👩‍👧БZ";
        run.update_center(source,1.0,0.1);
        assert_eq!(run.births.len(),5,"combining marks and ZWJ emoji stay together");
        let births:Vec<_>=run.births.iter().map(|(_,time)|*time).collect();
        assert_eq!(births,vec![1.2,1.1,1.0,1.1,1.2]);
        let saved=run.births.clone();run.update_center(source,20.0,0.1);assert_eq!(run.births,saved);
        run.update_center("ABCD",21.0,0.1);
        assert_eq!(run.births.iter().map(|(_,time)|*time).collect::<Vec<_>>(),vec![21.1,21.0,21.0,21.1]);
    }

    #[test]
    fn center_reveal_glyphs_never_move_and_completed_text_sleeps() {
        use crate::{Ui,view::{text,zstack}};
        let source="Добрый вечер, e\u{301} 👨‍👩‍👧";
        let mut ui=Ui::new((0u32,false),move |s:&(u32,bool)|zstack(text(source).font_size(24.0).key(s.0).text_reveal_center(0.08,0.015)));
        let size=(600.0,80.0);ui.frame(size);
        std::thread::sleep(std::time::Duration::from_millis(40));ui.frame(size);
        let rects=|ui:&Ui<(u32,bool)>|ui.list.cmds.iter().filter_map(|cmd|match cmd {
            Cmd::Quad(q) if q.p0[3]==3.0=>Some(q.rect),_=>None,
        }).collect::<Vec<_>>();
        let early=rects(&ui);assert!(!early.is_empty());
        std::thread::sleep(std::time::Duration::from_millis(75));ui.frame(size);
        let middle=rects(&ui);assert!(middle.len()>early.len());
        for rect in &early {assert!(middle.contains(rect),"new letters appear without moving existing ones");}
        std::thread::sleep(std::time::Duration::from_millis(350));ui.frame(size);
        assert!(rects(&ui).is_empty());assert!(!ui.animating && ui.wake_in.is_none());
        let settled=ui.list.cmds.iter().find_map(|cmd|if let Cmd::Text(t)=cmd {Some((t.key,t.pos,t.scale))}else{None}).unwrap();
        let glyphs=ui.text.reveal_glyphs(settled.0,settled.1,settled.2);
        let expected:Vec<_>=glyphs.iter().filter_map(|(glyph,baseline,_,_)| {
            let mask=ui.reveals.masks.get(&glyph.cache_key)?;
            Some([(glyph.x+mask.left) as f32,((baseline*settled.2).round() as i32+glyph.y-mask.top) as f32,mask.image.size().0 as f32,mask.image.size().1 as f32])
        }).collect();
        for rect in &middle {assert!(expected.contains(rect),"blur masks use the final shaped glyph positions");}
        ui.state.1=true;ui.pointer_move((10.0,10.0));ui.frame(size);
        assert!(rects(&ui).is_empty());assert!(!ui.animating);
        ui.state.0+=1;ui.frame(size);std::thread::sleep(std::time::Duration::from_millis(40));ui.frame(size);
        assert!(!rects(&ui).is_empty(),"a new stable key gets a new center-out appearance");
    }

    #[test]
    fn appends_preserve_old_graphemes_and_settle() {
        let mut run = Run::default();
        run.update("Привет 👨‍👩‍👧", 1.0);
        let old = run.births.clone();
        run.update("Привет 👨‍👩‍👧 мир", 2.0);
        assert_eq!(&run.births[..old.len()], old.as_slice());
        assert_eq!(run.progress(0, 2.0, 0.22), 1.0);
        assert_eq!(run.progress("Привет 👨‍👩‍👧 ".len(), 2.0, 0.22), 0.0);
        assert_eq!(run.progress(run.source.len(), 2.5, 0.22), 1.0);
    }

    #[test]
    fn closing_markdown_delimiters_does_not_replay_existing_letters() {
        let mut run = Run::default();
        run.update("**Сначала замерить", 1.0);
        run.update("Сначала замерить:", 2.0);
        assert_eq!(run.progress(0, 2.0, 0.22), 1.0);
        assert_eq!(run.progress(run.source.len() - 1, 2.0, 0.22), 0.0);
    }
}
