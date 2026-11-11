//! Text shaping + measurement with a per-frame cache of shaped buffers.

use crate::view::Span;
use cosmic_text::{
    fontdb, Align as TextAlign, Attrs, AttrsList, Buffer, BufferLine, CacheKeyFlags, Color as TextColor, Ellipsize,
    EllipsizeHeightLimit, Family, FeatureTag, FontFeatures, FontSystem, LayoutLine, LineEnding, Metrics, ShapeBuffer, Shaping, Weight,
};
use crate::types::Rect;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

static INTER: &[u8] = include_bytes!("../assets/Inter.ttf");
#[cfg(feature = "markdown")]
static MONO: &[u8] = include_bytes!("../assets/JetBrainsMono.ttf");

const BUNDLED_SANS: &str = "Inter";
#[cfg(feature = "markdown")]
const BUNDLED_MONO: Option<&str> = Some("JetBrains Mono");
#[cfg(not(feature = "markdown"))]
const BUNDLED_MONO: Option<&str> = None;

/// The platform's UI font, best first. macOS lists its variable San Francisco as "System
/// Font" (SFNS.ttf), with ".SF NS" as another name; an installed SF Pro comes first.
#[cfg(target_os = "macos")]
const SYSTEM_SANS: &[&str] = &["SF Pro Text", "SF Pro", "System Font", ".SF NS", "Helvetica Neue"];
#[cfg(target_os = "macos")]
const SYSTEM_MONO: &[&str] = &["SF Mono", ".SF NS Mono", "Menlo"];
#[cfg(target_os = "windows")]
const SYSTEM_SANS: &[&str] = &["Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI"];
#[cfg(target_os = "windows")]
const SYSTEM_MONO: &[&str] = &["Cascadia Mono", "Consolas"];
#[cfg(target_os = "ios")]
const SYSTEM_SANS: &[&str] = &["System Font", ".SF UI Text", ".SF NS"];
#[cfg(target_os = "ios")]
const SYSTEM_MONO: &[&str] = &["SF Mono", "Menlo", "Courier"];
#[cfg(target_os = "android")]
const SYSTEM_SANS: &[&str] = &["Roboto", "Google Sans Text", "Noto Sans"];
#[cfg(target_os = "android")]
const SYSTEM_MONO: &[&str] = &["Droid Sans Mono", "Roboto Mono", "Noto Sans Mono"];
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios", target_os = "android")))]
const SYSTEM_SANS: &[&str] = &["Cantarell", "Ubuntu", "Noto Sans", "DejaVu Sans", "Liberation Sans"];
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios", target_os = "android")))]
const SYSTEM_MONO: &[&str] = &["Ubuntu Mono", "Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono"];

/// Names of the variable San Francisco (SFNS.ttf) that macOS ships.
const SF_VARIABLE: &[&str] = &["System Font", ".SF NS"];

/// Letter spacing (em) that brings the variable San Francisco, drawn at its default 28 pt
/// optical size (cosmic-text varies only the weight), to the advance widths AppKit gets at
/// `size` (optical size = point size, plus Apple's tracking table). Measured with CoreText
/// on a mixed Latin/Cyrillic sentence, macOS 26: regular weight, semibold needs ~0.006 less.
/// What it cannot fix: the 28 pt design has a lower x-height (0.508 em against 0.526 at
/// text sizes) and slightly lighter stems.
fn sf_tracking(size: f32, weight: u16) -> f32 {
    const T: [(f32, f32); 13] = [
        (11.0, 0.066), (12.0, 0.060), (13.0, 0.054), (14.0, 0.049), (15.0, 0.044), (16.0, 0.040), (17.0, 0.035),
        (18.0, 0.033), (20.0, 0.023), (22.0, 0.016), (24.0, 0.0134), (28.0, 0.0132), (34.0, 0.0107),
    ];
    let at = match T.iter().position(|&(s, _)| s >= size) {
        Some(0) => T[0].1,
        None => T[T.len() - 1].1,
        Some(i) => {
            let ((s0, t0), (s1, t1)) = (T[i - 1], T[i]);
            t0 + (t1 - t0) * (size - s0) / (s1 - s0)
        }
    };
    at - 0.006 * (weight as f32 - 400.0) / 200.0
}

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

/// How a shaped text is laid out: wrap width, line limit (with "…") and centering.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Variant {
    width: Option<f32>,
    lines: usize,
    center: bool,
}

const NATURAL: Variant = Variant { width: None, lines: 0, center: false };

/// One laid-out copy of a text. The first copy of an entry owns the shaping; more copies
/// appear only when the same text is shown at several widths within one frame.
struct Layout {
    buffer: Buffer,
    /// What the buffer is set up for.
    variant: Variant,
    /// Laid out and measured for its current lines.
    ready: bool,
    size: (f32, f32),
    glyphs: usize,
    used: u64,
}

/// A text shaped once. Keyed by content and style, not by width: a new width only re-wraps.
pub(crate) struct Entry {
    layouts: Vec<Layout>,
    /// Byte offset of each buffer line (paragraph) in the source text.
    starts: Vec<usize>,
    /// Unwrapped width and height of each buffer line, filled lazily.
    line_nat: Vec<Option<(f32, f32)>>,
    natural: Option<(f32, f32)>,
    line_h: f32,
    used: u64,
}

impl Entry {
    fn bytes(&self) -> usize {
        let glyphs = self.layouts.iter().map(|l| l.glyphs).max().unwrap_or(0);
        let text = self.starts.last().copied().unwrap_or(0);
        glyphs * (std::mem::size_of::<cosmic_text::ShapeGlyph>() + self.layouts.len() * std::mem::size_of::<cosmic_text::LayoutGlyph>())
            + text * self.layouts.len()
            + self.starts.len() * 256
            + 512
    }
}

/// A view of one laid-out text, what `hit`, `caret` and friends read.
struct Shaped<'a> {
    buffer: &'a Buffer,
    starts: &'a [usize],
    size: (f32, f32),
}

/// Read-only access to laid-out texts while the font system is borrowed for drawing.
#[cfg(feature = "renderer")]
pub(crate) struct Buffers<'a>(&'a HashMap<u64, Entry>);

#[cfg(feature = "renderer")]
impl<'a> Buffers<'a> {
    pub(crate) fn get(&self, key: u64) -> Option<&'a Buffer> {
        let (k, slot) = split_key(key);
        self.0.get(&k)?.layouts.get(slot).map(|l| &l.buffer)
    }
}

/// Up to this many layouts of one text live at once (see [`Entry`]).
const MAX_LAYOUTS: usize = 4;
/// Shaped text kept between frames, by estimated bytes. Over it, the least recently used
/// texts go first, down to 3/4 of it; texts of the last frame are never dropped.
const CACHE_BUDGET: usize = 48 << 20;

pub struct TextSystem {
    pub(crate) fonts: FontSystem,
    entries: HashMap<u64, Entry>,
    /// Which entry a text node showed last time: lets growing text reuse its shaping.
    owners: HashMap<u64, u64>,
    /// Lines of the new text are built here, then only changed ones move into the entry.
    staging: Buffer,
    scratch: ShapeBuffer,
    scratch_lines: Vec<LayoutLine>,
    /// Layout cache that has to survive between frames: lazy-list row heights by list id.
    pub(crate) lists: HashMap<u64, ListCache>,
    frame: u64,
    /// Bytes of shaped text kept between frames, [`CACHE_BUDGET`] unless a test lowers it.
    budget: usize,
    /// Glyph outlines are hinted (only at scale < 2; see `set_scale`).
    hint: bool,
    /// The typefaces asked for (text, code); see `set_fonts`.
    fonts_set: (crate::theme::Font, crate::theme::Font),
    /// What code falls back to when no font for it is bundled.
    default_mono: String,
    /// Text is in macOS's variable San Francisco, whose optical size the shaper leaves at
    /// its 28 pt design: spacing is opened up to what AppKit draws (see `sf_tracking`).
    optical: bool,
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
    /// OpenType features (`tnum`, `ss01`…) with their values.
    pub features: &'a [([u8; 4], u32)],
    /// Id of the node showing the text, 0 if none. When its text changes (streaming,
    /// typing), the previous shaping is reused for the paragraphs that stayed the same.
    pub owner: u64,
}

impl Default for TextSpec<'_> {
    fn default() -> Self {
        TextSpec { spans: &[], text: "", size: 17.0, weight: 400, center: false, line_height: LINE_HEIGHT, tracking: 0.0, features: &[], owner: 0 }
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

/// Low bits of a text key pick the layout within its entry.
const SLOT_BITS: u64 = 3;

fn split_key(key: u64) -> (u64, usize) {
    (key & !SLOT_BITS, (key & SLOT_BITS) as usize)
}

/// The text's features with a run's on top (a later setting of a tag wins in HarfBuzz).
fn features(base: &[([u8; 4], u32)], run: &[([u8; 4], u32)]) -> FontFeatures {
    let mut f = FontFeatures::new();
    for (tag, v) in base.iter().chain(run) {
        f.set(FeatureTag::new(tag), *v);
    }
    f
}

/// Lays `buffer` out as `v` describes. Shaping is kept: only wrapping is redone, and only
/// for lines that changed if the width is the same.
fn apply(fonts: &mut FontSystem, buffer: &mut Buffer, v: Variant) {
    buffer.set_size(v.width, None);
    let lines = if v.lines > 0 && v.width.is_some() { v.lines } else { 0 };
    buffer.set_ellipsize(if lines > 0 { Ellipsize::End(EllipsizeHeightLimit::Lines(lines)) } else { Ellipsize::None });
    let align = (v.center && v.width.is_some()).then_some(TextAlign::Center);
    for l in &mut buffer.lines {
        l.set_align(align);
    }
    buffer.shape_until_scroll(fonts, false);
    // Lines that were never shaped are not "invalidated", so the call above can skip them.
    for i in 0..buffer.lines.len() {
        buffer.line_layout(fonts, i);
    }
}

impl Layout {
    fn measure(&mut self, v: Variant, line_h: f32) {
        let (mut w, mut h, mut n) = (0.0f32, 0.0f32, 0);
        for run in self.buffer.layout_runs() {
            w = w.max(run.line_w);
            h = h.max(run.line_top + run.line_height);
            n += run.glyphs.len();
        }
        // A centered, wrapped block occupies its full wrap width.
        let w = match v.width {
            Some(width) if v.center => width,
            _ => w.ceil(),
        };
        self.size = (w, if h > 0.0 { h } else { line_h });
        self.glyphs = n;
        self.variant = v;
        self.ready = true;
    }
}

impl TextSystem {
    /// Hinting snaps outlines to the pixel grid: crisper stems on a 1x screen, but on 2x and
    /// up it only bends the shapes (most visibly JetBrains Mono, which carries hints; Inter
    /// has next to none) and macOS never hints anyway. The choice lives in the shaped glyphs'
    /// cache keys, so a change (window moved to another screen) reshapes everything once.
    /// Glyph positions do not depend on it, so the layout stays the same.
    pub(crate) fn set_scale(&mut self, scale: f32) {
        let hint = scale < 2.0;
        if hint != self.hint {
            self.hint = hint;
            self.entries.clear();
            self.owners.clear();
        }
    }

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
        fonts.db_mut().set_sans_serif_family(BUNDLED_SANS);
        #[cfg(feature = "markdown")]
        fonts.db_mut().load_font_source(fontdb::Source::Binary(std::sync::Arc::new(MONO)));
        if let Some(m) = BUNDLED_MONO {
            fonts.db_mut().set_monospace_family(m);
        }
        TextSystem {
            default_mono: fonts.db().family_name(&Family::Monospace).to_string(),
            fonts,
            entries: HashMap::new(),
            owners: HashMap::new(),
            staging: Buffer::new_empty(Metrics::new(17.0, 22.0)),
            scratch: ShapeBuffer::default(),
            scratch_lines: Vec::new(),
            lists: HashMap::new(),
            // Starts at 1: a layout with `used == 0` was never handed out.
            frame: 1,
            budget: CACHE_BUDGET,
            hint: true,
            fonts_set: Default::default(),
            optical: false,
        }
    }

    /// Uses these typefaces for text and code from now on; shaped text is dropped when they
    /// change. A font that is not installed (or any system font on the web) leaves the
    /// bundled one.
    pub(crate) fn set_fonts(&mut self, text: &crate::theme::Font, mono: &crate::theme::Font) {
        if (text, mono) == (&self.fonts_set.0, &self.fonts_set.1) {
            return;
        }
        self.fonts_set = (text.clone(), mono.clone());
        let sans = self.family(text, SYSTEM_SANS).unwrap_or(BUNDLED_SANS);
        self.optical = SF_VARIABLE.contains(&sans);
        let sans = sans.to_string();
        self.fonts.db_mut().set_sans_serif_family(sans);
        let mono = self.family(mono, SYSTEM_MONO).or(BUNDLED_MONO).unwrap_or(&self.default_mono).to_string();
        self.fonts.db_mut().set_monospace_family(mono);
        self.entries.clear();
        self.owners.clear();
        // Shaped runs are cached by text and attributes, which name the generic family only.
        self.fonts.shape_run_cache = Default::default();
        // Row heights were measured in the old font; keep them only as estimates.
        for l in self.lists.values_mut() {
            l.heights.iter_mut().for_each(|h| *h = f32::NAN);
            l.width = 0.0;
        }
    }

    /// The family `f` names, if it is installed; `None` for `Bundled`.
    fn family<'a>(&self, f: &'a crate::theme::Font, system: &[&'a str]) -> Option<&'a str> {
        use crate::theme::Font;
        let has = |name: &str| self.fonts.db().faces().any(|face| face.families.iter().any(|(n, _)| n == name));
        match f {
            Font::Bundled => None,
            Font::System => system.iter().copied().find(|n| has(n)),
            Font::Named(n) => has(n).then_some(n.as_str()),
        }
    }

    /// The family plain text is set in right now (after fallback).
    pub fn font_family(&self) -> &str {
        self.fonts.db().family_name(&Family::SansSerif)
    }

    /// Content and style, not width: one entry serves every width the text is shown at.
    fn key(spec: &TextSpec) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        spec.text.hash(&mut h);
        for s in spec.spans {
            s.text.hash(&mut h);
            (s.bold, s.italic, s.mono, s.weight, s.size.map(f32::to_bits)).hash(&mut h);
            s.color.map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]).hash(&mut h);
            (s.background.is_some(), s.icon.is_some(), s.link.is_some()).hash(&mut h);
            s.space.map(f32::to_bits).hash(&mut h);
            s.features.hash(&mut h);
        }
        spec.features.hash(&mut h);
        spec.size.to_bits().hash(&mut h);
        (spec.line_height.to_bits(), spec.tracking.to_bits()).hash(&mut h);
        spec.weight.hash(&mut h);
        h.finish() & !SLOT_BITS
    }

    /// Finds the entry for the text, or shapes it. A text that replaced the one its node
    /// showed before (streaming, typing) takes over that entry and reshapes only the
    /// paragraphs that differ.
    fn shape(&mut self, spec: &TextSpec) -> u64 {
        let key = Self::key(spec);
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&key) {
            e.used = frame;
            return key;
        }
        let line_h = spec.line_h();
        let metrics = Metrics::new(spec.size, line_h);
        // The old entry is reused only if nothing drew from it this frame.
        let prev = match self.owners.get(&spec.owner) {
            Some(&k) if spec.owner != 0 && self.entries.get(&k).is_some_and(|e| e.used < frame) => self.entries.remove(&k),
            _ => None,
        };
        let (mut buffer, mut line_nat, prev_variant) = match prev {
            Some(mut e) => {
                let l = e.layouts.swap_remove(0);
                (l.buffer, std::mem::take(&mut e.line_nat), Some(l.variant))
            }
            None => (Buffer::new_empty(metrics), Vec::new(), None),
        };
        if buffer.metrics() != metrics {
            // Kept lines keep their shaping, but their unwrapped size scales with the font.
            line_nat.clear();
            buffer.set_metrics(metrics);
        }

        let flags = if self.hint { CacheKeyFlags::empty() } else { CacheKeyFlags::DISABLE_HINTING };
        let mut attrs = Attrs::new().family(Family::SansSerif).weight(Weight(spec.weight)).cache_key_flags(flags);
        let optical = self.optical;
        let track = |size: f32, weight: u16| spec.tracking + if optical { sf_tracking(size, weight) } else { 0.0 };
        let base_track = track(spec.size, spec.weight);
        if base_track != 0.0 {
            attrs = attrs.letter_spacing(base_track);
        }
        if !spec.features.is_empty() {
            attrs = attrs.font_features(features(spec.features, &[]));
        }
        let starts = if spec.spans.is_empty() {
            self.staging.set_text(spec.text, &attrs, Shaping::Advanced, None);
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
                    a = a.cache_key_flags(flags | CacheKeyFlags::FAKE_ITALIC); // slanted, no extra font file
                }
                if s.mono {
                    a = a.family(Family::Monospace);
                }
                if optical {
                    let t = if s.mono { spec.tracking } else { track(s.size.unwrap_or(spec.size), a.weight.0) };
                    a = a.letter_spacing(t);
                }
                if !s.features.is_empty() {
                    a = a.font_features(features(spec.features, &s.features));
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
                if let Some(width)=s.space {
                    runs.push((SLOT,slot(width/size.max(1.0))));continue;
                }
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
            self.staging.set_rich_text(runs, &attrs, Shaping::Advanced, None);
            line_starts(&shaped_text(spec.spans, ""))
        };

        // Keep the lines (paragraphs) that did not change, with their shaping; take the
        // rest from staging, whose old lines lend their allocations to the next text.
        let new = &mut self.staging.lines;
        line_nat.resize(new.len(), None);
        for (i, line) in new.iter_mut().enumerate() {
            match buffer.lines.get_mut(i) {
                Some(old) if old.text() == line.text() && old.ending() == line.ending() && old.attrs_list() == line.attrs_list() => {}
                Some(old) => {
                    std::mem::swap(old, line);
                    line_nat[i] = None;
                }
                None => {
                    let fresh = BufferLine::new(String::new(), LineEnding::None, AttrsList::new(&attrs), Shaping::Advanced);
                    buffer.lines.push(std::mem::replace(line, fresh));
                    line_nat[i] = None;
                }
            }
        }
        buffer.lines.truncate(new.len());

        // A reused buffer keeps its wrap width, so unchanged lines keep their layout too.
        let variant = prev_variant.unwrap_or(NATURAL);
        let layout = Layout { buffer, variant, ready: false, size: (0.0, 0.0), glyphs: 0, used: 0 };
        let e = Entry { layouts: vec![layout], starts, line_nat, natural: None, line_h, used: frame };
        self.entries.insert(key, e);
        if spec.owner != 0 {
            self.owners.insert(spec.owner, key);
        }
        key
    }

    /// Size of the text without wrapping. Lines keep their own natural size, so a changed
    /// paragraph does not make the others lay out again.
    fn natural(&mut self, key: u64) -> (f32, f32) {
        let Some(e) = self.entries.get_mut(&key) else { return (0.0, 0.0) };
        if let Some(n) = e.natural {
            return n;
        }
        let main = &mut e.layouts[0];
        // Laid out unwrapped anyway (most short labels): read the lines from there.
        let direct = main.variant == NATURAL;
        if direct && !main.ready {
            apply(&mut self.fonts, &mut main.buffer, NATURAL);
            main.measure(NATURAL, e.line_h);
        }
        let b = &mut main.buffer;
        let line_height = b.metrics().line_height;
        let (mut w, mut h) = (0.0f32, 0.0f32);
        for i in 0..b.lines.len() {
            let (lw, lh) = match e.line_nat[i] {
                Some(n) => n,
                None => {
                    let n = if direct {
                        let ll = b.lines[i].layout_opt().map_or(&[][..], |v| v.as_slice());
                        line_size(ll, line_height)
                    } else {
                        let (size, wrap, mono, hinting) = (b.metrics().font_size, b.wrap(), b.monospace_width(), b.hinting());
                        let Some(shape) = b.line_shape(&mut self.fonts, i) else { continue };
                        shape.layout_to_buffer(&mut self.scratch, size, None, wrap, Ellipsize::None, None, &mut self.scratch_lines, mono, hinting);
                        line_size(&self.scratch_lines, line_height)
                    };
                    e.line_nat[i] = Some(n);
                    n
                }
            };
            w = w.max(lw);
            h += lh;
        }
        let n = (w.ceil(), if h > 0.0 { h } else { e.line_h });
        e.natural = Some(n);
        n
    }

    /// The layout of entry `key` as `v`, made if needed; returns its key and size.
    /// A layout drawn from this frame is never re-wrapped, so two places showing the same
    /// text at different widths each get their own copy.
    fn layout(&mut self, key: u64, v: Variant) -> (u64, (f32, f32)) {
        let frame = self.frame;
        let Some(e) = self.entries.get_mut(&key) else { return (key, (0.0, 0.0)) };
        let ls = &mut e.layouts;
        let slot = match ls.iter().position(|l| l.variant == v) {
            Some(i) if ls[i].ready => i,
            Some(i) => {
                apply(&mut self.fonts, &mut ls[i].buffer, v);
                ls[i].measure(v, e.line_h);
                i
            }
            None => {
                let free = (0..ls.len()).filter(|&i| ls[i].used < frame || !ls[i].ready).min_by_key(|&i| ls[i].used);
                let i = match free {
                    Some(i) => i,
                    None if ls.len() < MAX_LAYOUTS => {
                        let buffer = ls[0].buffer.clone();
                        ls.push(Layout { buffer, variant: ls[0].variant, ready: false, size: (0.0, 0.0), glyphs: 0, used: 0 });
                        ls.len() - 1
                    }
                    // Out of copies: re-wrap the oldest even if it is drawn this frame.
                    None => (0..ls.len()).min_by_key(|&i| ls[i].used).unwrap_or(0),
                };
                apply(&mut self.fonts, &mut ls[i].buffer, v);
                ls[i].measure(v, e.line_h);
                i
            }
        };
        let l = &mut e.layouts[slot];
        l.used = frame;
        (key | slot as u64, l.size)
    }

    fn get(&self, key: u64) -> Option<Shaped<'_>> {
        let (k, slot) = split_key(key);
        let e = self.entries.get(&k)?;
        let l = e.layouts.get(slot)?;
        Some(Shaped { buffer: &l.buffer, starts: &e.starts, size: l.size })
    }

    /// The fonts, and the laid-out buffers behind keys from `measure`, for drawing.
    #[cfg(feature = "renderer")]
    pub(crate) fn for_drawing(&mut self) -> (&mut FontSystem, Buffers<'_>) {
        (&mut self.fonts, Buffers(&self.entries))
    }

    /// Boxes of the spans that have a plate or an icon, row by row.
    pub(crate) fn span_boxes(&self, key: u64, out: &mut Vec<SpanBox>) {
        let Some(e) = self.get(key) else { return };
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
        let key = self.shape(spec);
        let natural = self.natural(key);
        let v = match max_w {
            Some(w) if natural.0 > w + 0.5 => Variant { width: Some(w.floor().max(1.0)), lines: 0, center: spec.center },
            _ => NATURAL,
        };
        self.layout(key, v)
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
        let key = self.shape(spec);
        let natural = self.natural(key);
        let wrap = |width: f32, lines: usize| Variant { width: Some(width), lines, center: spec.center };
        let w = match max_w {
            Some(w) if natural.0 > w + 0.5 => w.floor().max(1.0),
            // Fits on its lines (an explicit newline can still make too many of them).
            _ => {
                let cut = natural.1 > max_h + 0.5;
                if cut && !fade {
                    let e = self.layout(key, wrap(natural.0.ceil().max(1.0), lines));
                    return (e.0, e.1, true);
                }
                let (k, _) = self.layout(key, NATURAL);
                return (k, (natural.0, natural.1.min(max_h)), cut);
            }
        };
        if !fade {
            let (key, size) = self.layout(key, wrap(w, lines));
            return (key, (size.0.min(w), size.1.min(max_h)), true);
        }
        // One line: keep it unwrapped and let it run under the fade. More lines: wrap, and
        // the last one shown fades if more text follows.
        let (key, size) = if lines == 1 && natural.1 <= line_h + 0.5 { self.layout(key, NATURAL) } else { self.layout(key, wrap(w, 0)) };
        let cut = size.0 > w + 0.5 || size.1 > max_h + 0.5;
        (key, (size.0.min(w), size.1.min(max_h)), cut)
    }

    /// Already shaped glyphs for the soft reveal painter; no second layout or shaping.
    pub(crate) fn reveal_glyphs(&self, key: u64, pos: (f32, f32), scale: f32) -> Vec<(cosmic_text::PhysicalGlyph, f32, usize, Option<cosmic_text::Color>)> {
        let Some(e) = self.get(key) else { return Vec::new() };
        e.buffer.layout_runs().flat_map(|run| {
            let base = e.starts.get(run.line_i).copied().unwrap_or(0);
            run.glyphs.iter().map(move |g| (g.physical(pos, scale), run.line_y, base + g.start, g.color_opt))
        }).collect()
    }

    /// Byte index (in the whole text) closest to the point `(x, y)` of the shaped text `key`.
    /// Points above the text land at its start, below it at its end.
    pub(crate) fn hit(&self, key: u64, x: f32, y: f32) -> usize {
        let Some(e) = self.get(key) else { return 0 };
        let y = y.clamp(0.0, (e.size.1 - 0.01).max(0.0));
        match e.buffer.hit(x.max(0.0), y) {
            Some(c) => e.starts.get(c.line).copied().unwrap_or(0) + c.index,
            None => 0,
        }
    }

    /// Caret before byte `i`: x, top of its visual row, and the row height.
    /// At a soft wrap the caret goes to the start of the next row.
    pub(crate) fn caret(&self, key: u64, i: usize) -> (f32, f32, f32) {
        let Some(e) = self.get(key) else { return (0.0, 0.0, 0.0) };
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
        let Some(e) = self.get(key) else { return };
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
        let Some(e) = self.get(key) else { return i..i };
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
        let Some(e) = self.get(key) else { return i..i };
        let (line, _) = locate(&e.starts, i);
        let base = e.starts[line];
        base..base + e.buffer.lines.get(line).map_or(0, |l| l.text().len())
    }

    /// Start and end of the visual row holding byte `i` (soft wraps count as row ends;
    /// a trailing space at a wrap is left out of the end).
    pub(crate) fn row_at(&self, key: u64, i: usize) -> Range<usize> {
        let Some(e) = self.get(key) else { return i..i };
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
        // Shaped words (cosmic-text's run cache) live for 8-16 frames: enough for a streamed
        // paragraph, which is reshaped every frame, to find all but its newest words there.
        if self.frame % 8 == 0 {
            self.fonts.shape_run_cache.trim(1);
        }
        let total: usize = self.entries.values().map(Entry::bytes).sum();
        if total <= self.budget {
            return;
        }
        // Least recently used first; what the frame just drawn still refers to stays.
        let frame = self.frame;
        let mut old: Vec<(u64, u64, usize)> =
            self.entries.iter().filter(|(_, e)| e.used + 1 < frame).map(|(k, e)| (e.used, *k, e.bytes())).collect();
        old.sort_unstable();
        let mut total = total;
        for (_, k, bytes) in old {
            if total <= self.budget / 4 * 3 {
                break;
            }
            self.entries.remove(&k);
            total -= bytes;
        }
        let entries = &self.entries;
        self.owners.retain(|_, k| entries.contains_key(k));
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
        if s.icon.is_some() || s.space.is_some() {
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

/// Unwrapped width and height of one buffer line laid out as `lines`.
fn line_size(lines: &[LayoutLine], line_height: f32) -> (f32, f32) {
    lines.iter().fold((0.0f32, 0.0f32), |(w, h), l| (w.max(l.w), h + l.line_height_opt.unwrap_or(line_height)))
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
    fn hinting_is_off_from_scale_two_and_layout_does_not_move() {
        let mut ts = TextSystem::new();
        let flags = |ts: &TextSystem, key: u64| {
            let (k, slot) = split_key(key);
            let b = &ts.entries[&k].layouts[slot].buffer;
            b.layout_runs().flat_map(|r| r.glyphs.iter().map(|g| g.cache_key_flags)).collect::<Vec<_>>()
        };
        let text = "Hinted at 1x, wgpu";
        ts.set_scale(1.0);
        let (k1, size1) = ts.measure(&TextSpec { text, ..Default::default() }, None);
        assert!(flags(&ts, k1).iter().all(|f| !f.contains(CacheKeyFlags::DISABLE_HINTING)));
        ts.set_scale(2.0);
        let (k2, size2) = ts.measure(&TextSpec { text, ..Default::default() }, None);
        let f2 = flags(&ts, k2);
        assert!(!f2.is_empty() && f2.iter().all(|f| f.contains(CacheKeyFlags::DISABLE_HINTING)));
        assert_eq!(size1, size2);
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

    /// Everything that ends up on screen or in hit tests, for comparing two layouts.
    fn dump(ts: &TextSystem, key: u64) -> (Vec<String>, (f32, f32)) {
        let e = ts.get(key).expect("laid out");
        let mut out = Vec::new();
        for run in e.buffer.layout_runs() {
            out.push(format!("row {} {} {} {}", run.line_i, run.line_y, run.line_top, run.line_w));
            for g in run.glyphs {
                out.push(format!("{} {}..{} {} {} {} {} {}", g.glyph_id, g.start, g.end, g.x, g.y, g.w, g.font_size, g.metadata));
            }
        }
        (out, e.size)
    }

    /// Shapes with nothing cached (font loading is the slow part of a new system, so keep it).
    fn from_scratch(ts: &mut TextSystem, spec: &TextSpec, width: Option<f32>) -> (u64, (f32, f32)) {
        ts.entries.clear();
        ts.fonts.shape_run_cache = Default::default();
        ts.measure(spec, width)
    }

    fn rich(n: usize) -> Vec<Span> {
        let words = ["Привет, ", "мир ", "👍🏽 ", "code\n", "жирный ", "wraps here and there ", "x\n\n", "ещё "];
        (0..n)
            .map(|i| {
                let s = Span::new(words[i % words.len()]);
                match i % 5 {
                    1 => s.bold(),
                    2 => s.code(),
                    3 => s.size(22.0).italic(),
                    4 => s.color(crate::types::Color::rgba(1.0, 0.0, 0.0, 1.0)),
                    _ => s,
                }
            })
            .collect()
    }

    #[test]
    fn growing_text_lays_out_like_shaping_from_scratch() {
        let mut grow = TextSystem::new();
        let mut fresh = TextSystem::new();
        let words = ["Привет, ", "мир! ", "👍🏽 ", "строка\n", "long-long-word ", "x ", "\n", "эмодзи 🇷🇺 ", "end"];
        let mut text = String::new();
        for (step, w) in words.iter().cycle().take(40).enumerate() {
            text.push_str(w);
            for width in [None, Some(90.0), Some(160.0)] {
                let spec = TextSpec { text: &text, owner: 7, ..Default::default() };
                let (k, size) = grow.measure(&spec, width);
                let (k2, size2) = from_scratch(&mut fresh, &TextSpec { owner: 0, ..spec }, width);
                assert_eq!(size, size2, "step {step} width {width:?}");
                assert_eq!(dump(&grow, k), dump(&fresh, k2), "step {step} width {width:?}");
                grow.end_frame();
            }
        }
        // Styled spans: plates, sizes, colors, line breaks inside a span.
        for n in 1..24 {
            let spans = rich(n);
            for width in [Some(120.0), None] {
                let spec = TextSpec { spans: &spans, owner: 9, ..Default::default() };
                let (k, size) = grow.measure(&spec, width);
                let (k2, size2) = from_scratch(&mut fresh, &TextSpec { owner: 0, ..spec }, width);
                assert_eq!(size, size2, "spans {n} width {width:?}");
                assert_eq!(dump(&grow, k), dump(&fresh, k2), "spans {n} width {width:?}");
                let mut a = Vec::new();
                let mut b = Vec::new();
                grow.span_boxes(k, &mut a);
                fresh.span_boxes(k2, &mut b);
                assert_eq!(a.len(), b.len());
                grow.end_frame();
            }
        }
    }

    #[test]
    fn growing_text_reuses_its_entry() {
        let mut ts = TextSystem::new();
        let mut text = String::from("first line\nsecond");
        for _ in 0..5 {
            ts.measure(&TextSpec { text: &text, owner: 3, ..Default::default() }, Some(200.0));
            ts.end_frame();
            text.push_str(" more");
        }
        assert_eq!(ts.entries.len(), 1, "the old text gave its entry to the new one");
        // Same text, bigger font: kept lines must not keep their old unwrapped size.
        let (_, big) = ts.measure(&TextSpec { text: &text, size: 30.0, owner: 3, ..Default::default() }, Some(400.0));
        let mut fresh = TextSystem::new();
        let (_, want) = fresh.measure(&TextSpec { text: &text, size: 30.0, ..Default::default() }, Some(400.0));
        assert_eq!(big, want);
    }

    #[test]
    fn one_text_at_two_widths_in_one_frame() {
        let mut ts = TextSystem::new();
        let text = "the same text shown in a narrow and in a wide place at once";
        let spec = TextSpec { text, ..Default::default() };
        for _ in 0..3 {
            let (narrow, ns) = ts.measure(&spec, Some(100.0));
            let (wide, ws) = ts.measure(&spec, Some(260.0));
            assert_ne!(narrow, wide);
            assert!(ns.1 > ws.1, "{ns:?} {ws:?}");
            // The wide layout did not re-wrap the narrow one.
            assert_eq!(ts.get(narrow).unwrap().size, ns);
            assert_eq!(ts.get(narrow).unwrap().buffer.size().0, Some(100.0));
            let mut fresh = TextSystem::new();
            let k = fresh.measure(&spec, Some(100.0)).0;
            assert_eq!(dump(&ts, narrow), dump(&fresh, k));
            ts.end_frame();
        }
        assert_eq!(ts.entries.len(), 1);
        assert_eq!(ts.entries.values().next().unwrap().layouts.len(), 2, "no new copy every frame");
    }

    #[test]
    fn cache_drops_least_recently_used_first() {
        let mut ts = TextSystem::new();
        let hot = "kept: shown every frame";
        let spec = |t| TextSpec { text: t, ..Default::default() };
        ts.measure(&spec(hot), None);
        ts.end_frame();
        let one = ts.entries.values().next().unwrap().bytes();
        ts.budget = one * 20;
        let cold: Vec<String> = (0..100).map(|i| format!("cold text number {i:03} filler")).collect();
        for (i, t) in cold.iter().enumerate() {
            ts.measure(&spec(hot), None);
            ts.measure(&spec(t), None);
            ts.end_frame();
            let key = TextSystem::key(&spec(hot));
            assert!(ts.entries.contains_key(&key), "hot text evicted at {i}");
            assert!(ts.entries.contains_key(&TextSystem::key(&spec(t))), "text of the last frame evicted");
        }
        let total: usize = ts.entries.values().map(Entry::bytes).sum();
        assert!(total <= ts.budget, "{total} > {}", ts.budget);
        // The most recent cold texts are still there, the oldest are gone.
        assert!(ts.entries.contains_key(&TextSystem::key(&spec(&cold[98]))));
        assert!(!ts.entries.contains_key(&TextSystem::key(&spec(&cold[0]))));
    }
}
