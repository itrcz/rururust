//! Text shaping + measurement with a per-frame cache of shaped buffers.

use crate::view::Span;
use cosmic_text::{
    fontdb, Align as TextAlign, Attrs, Buffer, CacheKeyFlags, Color as TextColor, Family, FontSystem, Metrics, Shaping,
    Weight,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

static INTER: &[u8] = include_bytes!("../assets/Inter.ttf");
#[cfg(feature = "markdown")]
static MONO: &[u8] = include_bytes!("../assets/JetBrainsMono.ttf");

/// Measured row heights of one lazy list.
#[derive(Default)]
pub(crate) struct ListCache {
    pub width: f32,
    pub heights: Vec<f32>,
}

pub(crate) struct Entry {
    pub buffer: Buffer,
    pub size: (f32, f32),
    used: u64,
}

pub struct TextSystem {
    pub(crate) fonts: FontSystem,
    pub(crate) entries: HashMap<u64, Entry>,
    /// Layout cache that has to survive between frames: lazy-list row heights by list id.
    pub(crate) lists: HashMap<u64, ListCache>,
    frame: u64,
}

pub(crate) struct TextSpec<'a> {
    /// Styled runs; when empty, `text` is used.
    pub spans: &'a [Span],
    pub text: &'a str,
    pub size: f32,
    pub weight: u16,
    pub center: bool,
}

impl TextSystem {
    pub fn new() -> Self {
        // Inter is embedded so text looks identical on every platform (and works
        // in the browser, where there are no system fonts). On native the system
        // fonts are loaded too and serve as fallback for emoji / CJK / etc.
        let source = fontdb::Source::Binary(std::sync::Arc::new(INTER));
        #[cfg(not(target_arch = "wasm32"))]
        let mut fonts = {
            let mut f = FontSystem::new();
            f.db_mut().load_font_source(source);
            f
        };
        #[cfg(target_arch = "wasm32")]
        let mut fonts = FontSystem::new_with_fonts([source]);
        fonts.db_mut().set_sans_serif_family("Inter");
        #[cfg(feature = "markdown")]
        {
            fonts.db_mut().load_font_source(fontdb::Source::Binary(std::sync::Arc::new(MONO)));
            fonts.db_mut().set_monospace_family("JetBrains Mono");
        }
        TextSystem { fonts, entries: HashMap::new(), lists: HashMap::new(), frame: 0 }
    }

    fn key(spec: &TextSpec, width: Option<f32>) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        spec.text.hash(&mut h);
        for s in spec.spans {
            s.text.hash(&mut h);
            (s.bold, s.italic, s.mono).hash(&mut h);
            s.color.map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]).hash(&mut h);
        }
        spec.size.to_bits().hash(&mut h);
        spec.weight.hash(&mut h);
        spec.center.hash(&mut h);
        width.map(f32::to_bits).hash(&mut h);
        h.finish()
    }

    fn entry(&mut self, spec: &TextSpec, width: Option<f32>) -> (u64, (f32, f32)) {
        let key = Self::key(spec, width);
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&key) {
            e.used = frame;
            return (key, e.size);
        }
        let line_h = (spec.size * 1.3).round();
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(spec.size, line_h));
        buffer.set_size(width, None);
        let attrs = Attrs::new().family(Family::SansSerif).weight(Weight(spec.weight));
        let align = (spec.center && width.is_some()).then_some(TextAlign::Center);
        if spec.spans.is_empty() {
            buffer.set_text(spec.text, &attrs, Shaping::Advanced, align);
        } else {
            let runs = spec.spans.iter().map(|s| {
                let mut a = attrs.clone();
                if s.bold {
                    a = a.weight(Weight(700));
                }
                if s.italic {
                    a = a.cache_key_flags(CacheKeyFlags::FAKE_ITALIC); // slanted, no extra font file
                }
                if s.mono {
                    a = a.family(Family::Monospace);
                }
                if let Some(c) = s.color {
                    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    a = a.color(TextColor::rgba(to8(c.r), to8(c.g), to8(c.b), to8(c.a)));
                }
                (s.text.as_str(), a)
            });
            buffer.set_rich_text(runs, &attrs, Shaping::Advanced, align);
        }
        buffer.shape_until_scroll(&mut self.fonts, false);
        let (mut w, mut lines) = (0.0f32, 0u32);
        for run in buffer.layout_runs() {
            w = w.max(run.line_w);
            lines += 1;
        }
        // A centered, wrapped block occupies its full wrap width.
        let w = if align.is_some() { width.unwrap() } else { w.ceil() };
        let size = (w, lines.max(1) as f32 * line_h);
        self.entries.insert(key, Entry { buffer, size, used: frame });
        (key, size)
    }

    /// Returns a cache key for the shaped text and its size, wrapping at `max_w` if needed.
    pub(crate) fn measure(&mut self, spec: &TextSpec, max_w: Option<f32>) -> (u64, (f32, f32)) {
        let natural = self.entry(spec, None);
        match max_w {
            Some(w) if natural.1 .0 > w + 0.5 => self.entry(spec, Some(w.floor().max(1.0))),
            _ => natural,
        }
    }

    /// X offset of the caret placed before byte `index` of a single-line text.
    pub(crate) fn caret_x(&self, key: u64, index: usize) -> f32 {
        let mut x = 0.0;
        if let Some(e) = self.entries.get(&key) {
            for run in e.buffer.layout_runs() {
                for g in run.glyphs.iter() {
                    if g.start >= index {
                        return g.x;
                    }
                    x = g.x + g.w;
                }
            }
        }
        x
    }

    /// Byte index of the caret position closest to `x` in a single-line text.
    pub(crate) fn index_at(&self, key: u64, x: f32) -> usize {
        let mut end = 0;
        if let Some(e) = self.entries.get(&key) {
            for run in e.buffer.layout_runs() {
                for g in run.glyphs.iter() {
                    if x < g.x + g.w * 0.5 {
                        return g.start;
                    }
                    end = g.end;
                }
            }
        }
        end
    }

    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        if self.entries.len() > 512 {
            let frame = self.frame;
            self.entries.retain(|_, e| e.used + 2 >= frame);
        }
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}
