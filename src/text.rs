//! Text shaping + measurement with a per-frame cache of shaped buffers.

use crate::view::Span;
use cosmic_text::{
    fontdb, Align as TextAlign, Attrs, Buffer, CacheKeyFlags, Color as TextColor, Ellipsize, EllipsizeHeightLimit,
    Family, FontSystem, Metrics, Shaping, Weight,
};
use crate::types::Rect;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

static INTER: &[u8] = include_bytes!("../assets/Inter.ttf");
#[cfg(feature = "markdown")]
static MONO: &[u8] = include_bytes!("../assets/JetBrainsMono.ttf");

/// Measured row heights of one lazy list.
#[derive(Default)]
pub(crate) struct ListCache {
    pub width: f32,
    pub heights: Vec<f32>,
    /// Rows painted last frame, and the first of them whose top was inside the viewport:
    /// what a new width re-measures at once and the row it keeps in place.
    pub seen: (usize, usize),
    pub anchor: usize,
}

pub(crate) struct Entry {
    pub buffer: Buffer,
    pub size: (f32, f32),
    /// Byte offset of each buffer line (paragraph) in the source text.
    starts: Vec<usize>,
    used: u64,
}

pub struct TextSystem {
    pub(crate) fonts: FontSystem,
    pub(crate) entries: HashMap<u64, Entry>,
    /// Layout cache that has to survive between frames: lazy-list row heights by list id.
    pub(crate) lists: HashMap<u64, ListCache>,
    frame: u64,
}

/// Default line height, as a multiple of the font size.
pub(crate) const LINE_HEIGHT: f32 = 1.3;
/// Room on each side of a span's background plate, in em.
const PLATE_PAD: f32 = 0.3;
/// Gap after an inline icon, in em.
const ICON_GAP: f32 = 0.3;
/// Stands in for padding and icons inside a line: invisible, zero-width once shaped (so its
/// advance is exactly the letter spacing we give it), and it joins the neighbouring word
/// like a letter, so a plate or icon never wraps away from its text.
const SLOT: &str = "\u{2064}";

pub(crate) struct TextSpec<'a> {
    /// Styled runs; when empty, `text` is used.
    pub spans: &'a [Span],
    pub text: &'a str,
    pub size: f32,
    pub weight: u16,
    pub center: bool,
    /// Line height as a multiple of `size`.
    pub line_height: f32,
    /// Letter spacing in em.
    pub tracking: f32,
}

impl Default for TextSpec<'_> {
    fn default() -> Self {
        TextSpec { spans: &[], text: "", size: 17.0, weight: 400, center: false, line_height: LINE_HEIGHT, tracking: 0.0 }
    }
}

impl TextSpec<'_> {
    pub(crate) fn line_h(&self) -> f32 {
        (self.size * self.line_height).round()
    }
}

/// Where a span with a plate or an icon landed: one box per visual row it touches.
pub(crate) struct SpanBox {
    /// Index into the text's spans.
    pub span: usize,
    pub x: f32,
    pub w: f32,
    /// Baseline of the row.
    pub baseline: f32,
    pub size: f32,
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

    fn key(spec: &TextSpec, width: Option<f32>, ellipsis: usize) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        spec.text.hash(&mut h);
        for s in spec.spans {
            s.text.hash(&mut h);
            (s.bold, s.italic, s.mono, s.weight, s.size.map(f32::to_bits)).hash(&mut h);
            s.color.map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]).hash(&mut h);
            (s.background.is_some(), s.icon.is_some(), s.link.is_some()).hash(&mut h);
        }
        spec.size.to_bits().hash(&mut h);
        (spec.line_height.to_bits(), spec.tracking.to_bits()).hash(&mut h);
        spec.weight.hash(&mut h);
        spec.center.hash(&mut h);
        width.map(f32::to_bits).hash(&mut h);
        ellipsis.hash(&mut h);
        h.finish()
    }

    /// Shapes (or finds) the text wrapped at `width`; `ellipsis` > 0 cuts it to that many
    /// lines ending with "…".
    fn entry(&mut self, spec: &TextSpec, width: Option<f32>, ellipsis: usize) -> (u64, (f32, f32)) {
        let key = Self::key(spec, width, ellipsis);
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&key) {
            e.used = frame;
            return (key, e.size);
        }
        let line_h = spec.line_h();
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(spec.size, line_h));
        buffer.set_size(width, None);
        if ellipsis > 0 && width.is_some() {
            buffer.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(ellipsis)));
        }
        let mut attrs = Attrs::new().family(Family::SansSerif).weight(Weight(spec.weight));
        if spec.tracking != 0.0 {
            attrs = attrs.letter_spacing(spec.tracking);
        }
        let align = (spec.center && width.is_some()).then_some(TextAlign::Center);
        let starts = if spec.spans.is_empty() {
            buffer.set_text(spec.text, &attrs, Shaping::Advanced, align);
            line_starts(spec.text)
        } else {
            let mut runs: Vec<(&str, Attrs)> = Vec::with_capacity(spec.spans.len());
            for (i, s) in spec.spans.iter().enumerate() {
                let mut a = attrs.clone();
                if s.bold {
                    a = a.weight(Weight(700));
                }
                if let Some(w) = s.weight {
                    a = a.weight(Weight(w as u16));
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
                let size = s.size.unwrap_or(spec.size);
                if s.size.is_some() {
                    a = a.metrics(Metrics::new(size, (size * spec.line_height).round()));
                }
                // Plates, icons and links find their glyphs again by this tag.
                if s.background.is_some() || s.icon.is_some() || s.link.is_some() {
                    a = a.metadata(i + 1);
                }
                // Same runs as `shaped_text`, which byte offsets into the text refer to.
                let slot = |em: f32| a.clone().letter_spacing(em);
                if s.icon.is_some() {
                    runs.push((SLOT, slot(1.0 + ICON_GAP)));
                    continue;
                }
                if s.background.is_some() {
                    runs.push((SLOT, slot(PLATE_PAD)));
                    runs.push((s.text.as_str(), a.clone()));
                    runs.push((SLOT, slot(PLATE_PAD)));
                    continue;
                }
                runs.push((s.text.as_str(), a));
            }
            let starts = line_starts(&shaped_text(spec.spans, ""));
            buffer.set_rich_text(runs, &attrs, Shaping::Advanced, align);
            starts
        };
        buffer.shape_until_scroll(&mut self.fonts, false);
        let (mut w, mut h) = (0.0f32, 0.0f32);
        for run in buffer.layout_runs() {
            w = w.max(run.line_w);
            h = h.max(run.line_top + run.line_height);
        }
        // A centered, wrapped block occupies its full wrap width.
        let w = if align.is_some() { width.unwrap() } else { w.ceil() };
        let size = (w, if h > 0.0 { h } else { line_h });
        self.entries.insert(key, Entry { buffer, size, starts, used: frame });
        (key, size)
    }

    /// Boxes of the spans that have a plate or an icon, row by row.
    pub(crate) fn span_boxes(&self, key: u64, out: &mut Vec<SpanBox>) {
        let Some(e) = self.entries.get(&key) else { return };
        for run in e.buffer.layout_runs() {
            for g in run.glyphs {
                if g.metadata == 0 {
                    continue;
                }
                match out.last_mut() {
                    Some(b) if b.span + 1 == g.metadata && b.baseline == run.line_y && g.x <= b.x + b.w + 0.5 => {
                        b.w = (g.x + g.w - b.x).max(b.w);
                    }
                    _ => out.push(SpanBox { span: g.metadata - 1, x: g.x, w: g.w, baseline: run.line_y, size: g.font_size }),
                }
            }
        }
    }

    /// Returns a cache key for the shaped text and its size, wrapping at `max_w` if needed.
    pub(crate) fn measure(&mut self, spec: &TextSpec, max_w: Option<f32>) -> (u64, (f32, f32)) {
        let natural = self.entry(spec, None, 0);
        match max_w {
            Some(w) if natural.1 .0 > w + 0.5 => self.entry(spec, Some(w.floor().max(1.0)), 0),
            _ => natural,
        }
    }

    /// Like `measure`, but at most `lines` lines tall. Returns the key, the size it takes
    /// and whether something was cut. With `fade` the text is not shortened: the caller
    /// clips it to the returned size and fades the last line out.
    pub(crate) fn measure_lines(
        &mut self,
        spec: &TextSpec,
        max_w: Option<f32>,
        lines: usize,
        fade: bool,
    ) -> (u64, (f32, f32), bool) {
        let line_h = spec.line_h();
        let max_h = lines as f32 * line_h;
        let natural = self.entry(spec, None, 0);
        let w = match max_w {
            Some(w) if natural.1 .0 > w + 0.5 => w.floor().max(1.0),
            // Fits on its lines (an explicit newline can still make too many of them).
            _ => {
                let cut = natural.1 .1 > max_h + 0.5;
                if cut && !fade {
                    let e = self.entry(spec, Some(natural.1 .0.ceil().max(1.0)), lines);
                    return (e.0, e.1, true);
                }
                return (natural.0, (natural.1 .0, natural.1 .1.min(max_h)), cut);
            }
        };
        if !fade {
            let (key, size) = self.entry(spec, Some(w), lines);
            return (key, (size.0.min(w), size.1.min(max_h)), true);
        }
        // One line: keep it unwrapped and let it run under the fade. More lines: wrap, and
        // the last one shown fades if more text follows.
        let (key, size) = if lines == 1 && natural.1 .1 <= line_h + 0.5 { natural } else { self.entry(spec, Some(w), 0) };
        let cut = size.0 > w + 0.5 || size.1 > max_h + 0.5;
        (key, (size.0.min(w), size.1.min(max_h)), cut)
    }

    /// Byte index (in the whole text) closest to the point `(x, y)` of the shaped text `key`.
    /// Points above the text land at its start, below it at its end.
    pub(crate) fn hit(&self, key: u64, x: f32, y: f32) -> usize {
        let Some(e) = self.entries.get(&key) else { return 0 };
        let y = y.clamp(0.0, (e.size.1 - 0.01).max(0.0));
        match e.buffer.hit(x.max(0.0), y) {
            Some(c) => e.starts.get(c.line).copied().unwrap_or(0) + c.index,
            None => 0,
        }
    }

    /// Caret before byte `i`: x, top of its visual row, and the row height.
    /// At a soft wrap the caret goes to the start of the next row.
    pub(crate) fn caret(&self, key: u64, i: usize) -> (f32, f32, f32) {
        let Some(e) = self.entries.get(&key) else { return (0.0, 0.0, 0.0) };
        let (line, local) = locate(&e.starts, i);
        let mut end_match = None;
        let mut last = None;
        for run in e.buffer.layout_runs().filter(|r| r.line_i == line) {
            last = Some((run.line_w, run.line_top, run.line_height));
            for g in run.glyphs {
                if local >= g.start && local < g.end {
                    // Inside a cluster (ligature, emoji sequence): split it by graphemes.
                    let cluster = run.text.get(g.start..g.end).unwrap_or("");
                    let total = cluster.grapheme_indices(true).count().max(1);
                    let before = cluster.grapheme_indices(true).filter(|(k, _)| g.start + k < local).count();
                    return (g.x + g.w * before as f32 / total as f32, run.line_top, run.line_height);
                }
                if local == g.end {
                    end_match = Some((g.x + g.w, run.line_top, run.line_height));
                }
            }
            if run.glyphs.is_empty() {
                return (0.0, run.line_top, run.line_height);
            }
        }
        end_match.or(last).unwrap_or((0.0, 0.0, e.size.1))
    }

    /// Selection highlight of the byte range `r`: one rectangle per visual row it touches.
    /// A row whose line break is selected gets a little extra width, so selected empty
    /// lines stay visible.
    pub(crate) fn range_rects(&self, key: u64, r: Range<usize>, out: &mut Vec<Rect>) {
        let Some(e) = self.entries.get(&key) else { return };
        if r.start >= r.end {
            return;
        }
        let nl = 5.0;
        for run in e.buffer.layout_runs() {
            let base = e.starts.get(run.line_i).copied().unwrap_or(0);
            let lo = run.glyphs.iter().map(|g| g.start).min().unwrap_or(0) + base;
            let hi = run.glyphs.iter().map(|g| g.end).max().unwrap_or(0) + base;
            let line_end = base + run.text.len();
            if r.end < lo || r.start > hi || (r.end == lo && lo != hi) {
                continue;
            }
            let (mut x0, mut x1) = (f32::MAX, f32::MIN);
            for g in run.glyphs {
                let (gs, ge) = (base + g.start, base + g.end);
                if ge <= r.start || gs >= r.end {
                    continue;
                }
                x0 = x0.min(g.x);
                x1 = x1.max(g.x + g.w);
            }
            if hi == line_end && r.end > line_end && r.start <= line_end {
                // The line break itself is selected.
                if x0 > x1 {
                    x0 = run.line_w;
                    x1 = run.line_w;
                }
                x1 += nl;
            }
            if x0 < x1 {
                out.push(Rect::new(x0, run.line_top, x1 - x0, run.line_height));
            }
        }
    }

    /// The word (or run of spaces / punctuation) around byte `i`, within its paragraph.
    pub(crate) fn word_at(&self, key: u64, i: usize) -> Range<usize> {
        let Some(e) = self.entries.get(&key) else { return i..i };
        let (line, local) = locate(&e.starts, i);
        let Some(l) = e.buffer.lines.get(line) else { return i..i };
        let base = e.starts[line];
        let text = l.text();
        let mut prev: Option<Range<usize>> = None;
        for (k, w) in text.split_word_bound_indices() {
            let r = k..k + w.len();
            let wordy = w.chars().any(char::is_alphanumeric);
            if local < r.end || (local == r.end && wordy && r.end == text.len()) {
                // Right after a word (at its end) prefer that word over the space that follows.
                let pick = match prev {
                    Some(p) if local == r.start && !wordy && p.end == local => p,
                    _ => r,
                };
                return base + pick.start..base + pick.end;
            }
            prev = Some(r).filter(|_| wordy);
        }
        match prev {
            Some(p) if p.end == local => base + p.start..base + p.end,
            _ => i..i,
        }
    }

    /// The paragraph (source line, without its line break) around byte `i`.
    pub(crate) fn para_at(&self, key: u64, i: usize) -> Range<usize> {
        let Some(e) = self.entries.get(&key) else { return i..i };
        let (line, _) = locate(&e.starts, i);
        let base = e.starts[line];
        base..base + e.buffer.lines.get(line).map_or(0, |l| l.text().len())
    }

    /// Start and end of the visual row holding byte `i` (soft wraps count as row ends;
    /// a trailing space at a wrap is left out of the end).
    pub(crate) fn row_at(&self, key: u64, i: usize) -> Range<usize> {
        let Some(e) = self.entries.get(&key) else { return i..i };
        let (_, top, _) = self.caret(key, i);
        for run in e.buffer.layout_runs() {
            if (run.line_top - top).abs() > 0.5 {
                continue;
            }
            let base = e.starts.get(run.line_i).copied().unwrap_or(0);
            let lo = run.glyphs.iter().map(|g| g.start).min().unwrap_or(0);
            let mut hi = run.glyphs.iter().map(|g| g.end).max().unwrap_or(0);
            if hi < run.text.len() && run.text[..hi].ends_with(char::is_whitespace) {
                hi = run.text[..hi].char_indices().next_back().map_or(hi, |(k, _)| k);
            }
            return base + lo..base + hi;
        }
        i..i
    }

    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        if self.entries.len() > 512 {
            let frame = self.frame;
            self.entries.retain(|_, e| e.used + 2 >= frame);
        }
    }
}

/// The text as it is shaped: what byte offsets from `hit`, `caret` and friends index into.
/// Rich text gets invisible slots around plates and in place of icons; [`strip_slots`]
/// takes them out again for copying.
pub(crate) fn shaped_text<'a>(spans: &'a [Span], text: &'a str) -> std::borrow::Cow<'a, str> {
    if spans.is_empty() {
        return text.into();
    }
    let mut out = String::new();
    for s in spans {
        if s.icon.is_some() {
            out.push_str(SLOT);
        } else if s.background.is_some() {
            out.push_str(SLOT);
            out.push_str(&s.text);
            out.push_str(SLOT);
        } else {
            out.push_str(&s.text);
        }
    }
    out.into()
}

/// `text` without the invisible slots of [`shaped_text`].
pub(crate) fn strip_slots(text: &str) -> String {
    text.replace(SLOT, "")
}

/// Byte offsets where the lines of `text` start, split the way cosmic-text splits them.
fn line_starts(text: &str) -> Vec<usize> {
    let b = text.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < b.len() {
        let pair = matches!((b[i], b.get(i + 1)), (b'\r', Some(b'\n')) | (b'\n', Some(b'\r')));
        if b[i] == b'\n' || b[i] == b'\r' {
            i += if pair { 2 } else { 1 };
            starts.push(i);
        } else {
            i += 1;
        }
    }
    starts
}

/// Line and offset within it of the byte `i`.
fn locate(starts: &[usize], i: usize) -> (usize, usize) {
    let line = starts.partition_point(|&s| s <= i).saturating_sub(1);
    (line, i - starts.get(line).copied().unwrap_or(0))
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shaped(ts: &mut TextSystem, text: &str, w: Option<f32>) -> u64 {
        ts.measure(&TextSpec { text, ..Default::default() }, w).0
    }

    #[test]
    fn hit_and_caret_are_inverse_across_lines_and_wraps() {
        let mut ts = TextSystem::new();
        let text = "Привет, мир! 👍🏽 wraps here and there\nsecond paragraph";
        let key = shaped(&mut ts, text, Some(120.0));
        let (mut rows, mut first) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
        let nl = text.find('\n').unwrap();
        for (i, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let (x, y, h) = ts.caret(key, i);
            rows.insert(y as i32);
            if i <= nl {
                first.insert(y as i32);
            }
            let back = ts.hit(key, x + 0.01, y + h * 0.5);
            // A soft wrap's end and the next row's start are the same spot for the caret.
            let same = back == i || ts.caret(key, back) == (x, y, h);
            assert!(same || !text.is_char_boundary(i) || text[..i].ends_with(['👍', '🏽']), "{i} -> {back}");
        }
        assert!(rows.len() >= 4, "wrapped into rows: {rows:?}");
        // A selection over a wrap gets one rectangle per row.
        let mut rects = Vec::new();
        ts.range_rects(key, 0..nl + 1, &mut rects);
        assert!(first.len() >= 3);
        assert_eq!(rects.len(), first.len());
    }

    #[test]
    fn words_and_paragraphs() {
        let mut ts = TextSystem::new();
        let text = "раз два-три\nчетыре";
        let key = shaped(&mut ts, text, None);
        let two = text.find("два").unwrap();
        assert_eq!(&text[ts.word_at(key, two + 2)], "два");
        assert_eq!(&text[ts.word_at(key, two)], "два");
        assert_eq!(&text[ts.word_at(key, two - 1)], "раз", "right after a word: that word");
        assert_eq!(&text[ts.para_at(key, 2)], "раз два-три");
        assert_eq!(&text[ts.para_at(key, text.len())], "четыре");
    }
}
