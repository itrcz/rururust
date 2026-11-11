//! Markdown → views. Built for chat-style content: paragraphs with inline styles,
//! headings, lists, quotes, code blocks, tables, rules and images, each a normal `View` so it
//! flows with everything else in a scroll or lazy list.

use crate::types::Color;
use crate::view::{hstack, image, rect, rich_text, text, vstack, Align, Image, Span, View, Weight};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[path = "markdown_code.rs"]
mod code;
pub use code::{CodeHeaderStyle, CodeSyntaxStyle};

/// How [`markdown_styled`] looks. `MarkdownStyle::default()` is the look of [`markdown`];
/// change only what you need:
///
/// ```ignore
/// let style = MarkdownStyle { size: 14.0, line_height: 1.625, strong: Weight::Semibold, ..Default::default() };
/// markdown_styled(&answer, &style)
/// ```
#[derive(Clone, Debug)]
pub struct MarkdownStyle {
    /// Body text size in points.
    pub size: f32,
    /// Line height of body text, as a multiple of `size`.
    pub line_height: f32,
    /// Letter spacing of body text in em.
    pub letter_spacing: f32,
    /// Text color; inherited from the parent when `None`.
    pub color: Option<Color>,
    /// Space between blocks (paragraphs, list items, tables…).
    pub gap: f32,
    pub link: Color,
    /// An icon shown before links whose URL starts with the prefix, e.g. a GitHub mark
    /// before `https://github.com/` links. The first matching prefix wins.
    pub link_icons: Vec<(String, Image)>,
    /// Weight of `**strong**` text.
    pub strong: Weight,
    /// Sizes of headings H1, H2, H3 and H4–H6.
    pub headings: [f32; 4],
    pub heading_weight: Weight,
    /// Width of a list marker column.
    pub list_indent: f32,
    /// Extra indent of each nested list level.
    pub list_nest: f32,
    /// `inline code`: its color and the plate under it.
    pub code_color: Option<Color>,
    pub code_background: Option<Color>,
    pub code_block: CodeBlockStyle,
    pub table: TableStyle,
    pub quote: QuoteStyle,
}

/// Fenced and indented code blocks.
#[derive(Clone, Debug)]
pub struct CodeBlockStyle {
    pub size: f32,
    pub line_height: f32,
    pub color: Option<Color>,
    pub background: Color,
    pub border: Option<(f32, Color)>,
    pub radius: f32,
    pub padding: f32,
    pub header: Option<CodeHeaderStyle>,
    pub syntax: Option<CodeSyntaxStyle>,
}

/// Tables: rows of cells with a rule between rows and no outer frame.
#[derive(Clone, Debug)]
pub struct TableStyle {
    /// Text size; the body size when `None`.
    pub size: Option<f32>,
    /// Line height; the body line height when `None`.
    pub line_height: Option<f32>,
    pub letter_spacing: f32,
    /// Space inside a cell: (to the next column, above and below the text).
    pub cell_padding: (f32, f32),
    /// Line between rows (width, color); `None` for no lines.
    pub rule: Option<(f32, Color)>,
    pub header_weight: Weight,
    pub header_color: Option<Color>,
    /// Share of the width of each column, e.g. `vec![0.246, 0.377, 0.377]`; columns not
    /// listed get 1 / (number of columns).
    pub columns: Vec<f32>,
}

/// Block quotes.
#[derive(Clone, Debug)]
pub struct QuoteStyle {
    pub background: Option<Color>,
    /// A bar along the left edge (width, color).
    pub bar: Option<(f32, Color)>,
    pub radius: f32,
    /// left, top, right, bottom
    pub padding: [f32; 4],
    pub opacity: f32,
}

impl Default for MarkdownStyle {
    /// The default (dark) theme's look. In an app with a theme use
    /// `MarkdownStyle::from(&*env.theme)`.
    fn default() -> Self {
        MarkdownStyle::from(&crate::theme::Theme::default())
    }
}

impl From<&crate::theme::Theme> for MarkdownStyle {
    /// Body size and line height from `text.body`, links in `color.link`, code plates and
    /// rules that read on the theme's background.
    fn from(t: &crate::theme::Theme) -> Self {
        let body = t.text.body;
        MarkdownStyle {
            size: body.size,
            line_height: body.line_height,
            letter_spacing: body.tracking,
            color: None,
            gap: 10.0,
            link: t.color.link,
            link_icons: Vec::new(),
            strong: Weight::Bold,
            headings: [26.0, 22.0, 19.0, 17.0],
            heading_weight: Weight::Bold,
            list_indent: 26.0,
            list_nest: 18.0,
            code_color: Some(if t.dark { Color::hex(0xFFB86B) } else { Color::hex(0xB4430B) }),
            code_background: None,
            code_block: CodeBlockStyle {
                line_height: body.line_height,
                background: if t.dark { Color::BLACK.alpha(0.35) } else { Color::BLACK.alpha(0.04) },
                border: Some((1.0, t.color.divider)),
                ..CodeBlockStyle::default()
            },
            table: TableStyle::default(),
            quote: QuoteStyle::default(),
        }
    }
}

impl Default for CodeBlockStyle {
    fn default() -> Self {
        CodeBlockStyle {
            size: 13.5,
            line_height: crate::text::LINE_HEIGHT,
            color: None,
            background: Color::rgba(0.0, 0.0, 0.0, 0.35),
            border: Some((1.0, Color::rgba(1.0, 1.0, 1.0, 0.08))),
            radius: 12.0,
            padding: 14.0,
            header: None,
            syntax: None,
        }
    }
}

impl Default for TableStyle {
    fn default() -> Self {
        TableStyle {
            size: Some(15.0),
            line_height: None,
            letter_spacing: 0.0,
            cell_padding: (12.0, 8.0),
            rule: Some((1.0, Color::rgba(0.5, 0.5, 0.55, 0.3))),
            header_weight: Weight::Semibold,
            header_color: None,
            columns: Vec::new(),
        }
    }
}

impl Default for QuoteStyle {
    fn default() -> Self {
        QuoteStyle {
            background: Some(Color::rgba(0.5, 0.5, 0.55, 0.16)),
            bar: None,
            radius: 10.0,
            padding: [14.0, 8.0, 12.0, 8.0],
            opacity: 0.85,
        }
    }
}

/// Renders markdown. Images are shown as a placeholder; use [`markdown_with`] to supply them.
pub fn markdown<S>(source: &str) -> View<S> {
    markdown_styled_with(source, &MarkdownStyle::default(), |_| None)
}

/// Renders markdown; `images` maps an image URL to an already loaded picture.
pub fn markdown_with<S>(source: &str, images: impl Fn(&str) -> Option<Image>) -> View<S> {
    markdown_styled_with(source, &MarkdownStyle::default(), images)
}

/// Renders markdown in your own type scale and colors; see [`MarkdownStyle`].
pub fn markdown_styled<S>(source: &str, style: &MarkdownStyle) -> View<S> {
    markdown_styled_with(source, style, |_| None)
}

/// [`markdown_styled`] with pictures: `images` maps an image URL to a loaded picture.
pub fn markdown_styled_with<S>(source: &str, style: &MarkdownStyle, images: impl Fn(&str) -> Option<Image>) -> View<S> {
    let st = style;
    let mut blocks: Vec<View<S>> = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let (mut bold, mut italic) = (0u32, 0u32);
    let mut links: Vec<String> = Vec::new(); // open links, innermost last
    let mut list_from: Option<usize> = None; // first block of the outermost open list
    let mut heading: Option<HeadingLevel> = None;
    let mut code: Option<String> = None;
    let mut code_language = String::new();
    let mut lists: Vec<Option<u64>> = Vec::new(); // open lists: next number, or None for bullets
    let mut bullet: Option<String> = None; // marker waiting for the item's first line
    let mut quote = 0u32;
    let mut alt: Option<String> = None; // inside an image: collecting its alt text
    // Inside a table: its rows, each a header flag and its cells' runs.
    let mut table: Option<Vec<(bool, Vec<Vec<Span>>)>> = None;
    let mut header = false;

    let para = |spans: Vec<Span>| {
        rich_text(spans).font_size(st.size).line_height(st.line_height).letter_spacing(st.letter_spacing)
    };

    // Turns the collected inline runs into one block.
    let flush = |spans: &mut Vec<Span>,
                 blocks: &mut Vec<View<S>>,
                 heading: Option<HeadingLevel>,
                 depth: usize,
                 bullet: &mut Option<String>,
                 quote: u32| {
        if spans.is_empty() {
            return;
        }
        let mut body = para(std::mem::take(spans));
        if let Some(level) = heading {
            let size = match level {
                HeadingLevel::H1 => st.headings[0],
                HeadingLevel::H2 => st.headings[1],
                HeadingLevel::H3 => st.headings[2],
                _ => st.headings[3],
            };
            body = body.font_size(size).weight(st.heading_weight);
        }
        let mut block = if depth > 0 {
            let mark = bullet.take().unwrap_or_default();
            // Copied as plain-text markdown: "- ", "2. ", "- [x] ", nested by two spaces.
            let plain = match mark.as_str() {
                "•" => "-".to_string(),
                "☐" => "- [ ]".to_string(),
                "☑" => "- [x]".to_string(),
                m => m.to_string(),
            };
            let marker = text(mark)
                .font_size(st.size)
                .line_height(st.line_height)
                .width(st.list_indent)
                .opacity(0.7)
                .copy_as(format!("{}{plain} ", "  ".repeat(depth - 1)));
            let indent = st.list_nest * (depth as f32 - 1.0);
            hstack((marker, body)).align(Align::Start).padding_ltrb(indent, 0.0, 0.0, 0.0).copy_sep("")
        } else {
            body
        };
        if quote > 0 {
            let q = &st.quote;
            let [l, t, r, b] = q.padding;
            block = vstack(block).align(Align::Start).fill_width().padding_ltrb(l, t, r, b).radius(q.radius).opacity(q.opacity);
            if let Some(bg) = q.background {
                block = block.background(bg);
            }
            if let Some((w, c)) = q.bar {
                block = hstack((rect().width(w).fill_height().background(c), block)).fill_width();
            }
        }
        blocks.push(block);
    };

    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => heading = Some(level),
                Tag::Strong => bold += 1,
                Tag::Emphasis => italic += 1,
                Tag::Link { dest_url, .. } => {
                    if let Some((_, img)) = st.link_icons.iter().find(|(p, _)| dest_url.starts_with(p.as_str())) {
                        spans.push(Span::icon(img).color(st.link).link(dest_url.to_string()));
                    }
                    links.push(dest_url.into_string());
                }
                Tag::BlockQuote(_) => quote += 1,
                Tag::CodeBlock(kind) => {
                    code_language = match kind { pulldown_cmark::CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_owned(), _ => String::new() };
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    code = Some(String::new());
                }
                Tag::List(start) => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    if lists.is_empty() {
                        list_from = Some(blocks.len());
                    }
                    lists.push(start);
                }
                Tag::Item => {
                    bullet = Some(match lists.last_mut() {
                        Some(Some(n)) => {
                            *n += 1;
                            format!("{}.", *n - 1)
                        }
                        _ => "•".to_string(),
                    });
                }
                Tag::Table(_) => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    table = Some(Vec::new());
                }
                Tag::TableHead | Tag::TableRow => {
                    header = matches!(tag, Tag::TableHead);
                    if let Some(rows) = table.as_mut() {
                        rows.push((header, Vec::new()));
                    }
                }
                Tag::Image { dest_url, .. } => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    match images(&dest_url) {
                        Some(img) => blocks.push(image(&img).fill_width().radius(14.0).copy_as(format!("![]({dest_url})"))),
                        None => alt = Some(String::new()),
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Item => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                }
                TagEnd::Heading(_) => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    heading = None;
                }
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::Link => {
                    links.pop();
                }
                TagEnd::BlockQuote(_) => quote = quote.saturating_sub(1),
                TagEnd::List(_) => {
                    lists.pop();
                    // The items of one list copy as lines, not as paragraphs.
                    if let Some(from) = list_from.filter(|_| lists.is_empty()) {
                        let items = blocks.split_off(from.min(blocks.len()));
                        blocks.push(vstack(items).spacing(st.gap).align(Align::Start).fill_width());
                        list_from = None;
                    }
                }
                TagEnd::TableHead => header = false,
                TagEnd::TableCell => {
                    if let Some((_, cells)) = table.as_mut().and_then(|t| t.last_mut()) {
                        cells.push(std::mem::take(&mut spans));
                    }
                }
                TagEnd::Table => {
                    if let Some(rows) = table.take() {
                        blocks.push(table_view(rows, st));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(src) = code.take() {
                        blocks.push(code::block(src, &code_language, &st.code_block));
                    }
                }
                TagEnd::Image => {
                    if let Some(a) = alt.take() {
                        let label = if a.is_empty() { "image".to_string() } else { a };
                        blocks.push(
                            hstack(text(format!("[image] {label}")).font_size(14.0).opacity(0.7))
                                .fill_width()
                                .padding(22.0)
                                .background(Color::rgba(0.5, 0.5, 0.55, 0.16))
                                .radius(14.0),
                        );
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some(c) = code.as_mut() {
                    c.push_str(&t);
                } else if let Some(a) = alt.as_mut() {
                    a.push_str(&t);
                } else {
                    let mut s = Span::new(t.into_string());
                    if bold > 0 || (header && table.is_some()) {
                        s = s.weight(if bold > 0 { st.strong } else { st.table.header_weight });
                    }
                    s.italic = italic > 0;
                    if let Some(url) = links.last() {
                        s.color = Some(st.link);
                        s.link = Some(url.clone());
                    } else if header && table.is_some() {
                        s.color = st.table.header_color;
                    }
                    spans.push(s);
                }
            }
            Event::Code(t) => {
                let mut s = Span::new(t.into_string()).code();
                s.color = st.code_color;
                s.background = st.code_background;
                s.link = links.last().cloned();
                spans.push(s);
            }
            Event::SoftBreak => spans.push(Span::new(" ")),
            Event::HardBreak => spans.push(Span::new("\n")),
            Event::TaskListMarker(done) => bullet = Some(if done { "☑".into() } else { "☐".into() }),
            Event::Rule => {
                flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                blocks.push(rect().fill_width().height(1.0).background(Color::rgba(0.5, 0.5, 0.55, 0.4)));
            }
            _ => {}
        }
    }
    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
    // Selectable by itself; inside a selectable list the list's selection wins.
    let out = vstack(blocks).spacing(st.gap).align(Align::Start).fill_width().selectable().copy_sep("\n\n");
    match st.color {
        Some(c) => out.color(c),
        None => out,
    }
}

/// Columns share the width by `TableStyle::columns`, so they line up across rows without
/// measuring every cell first (layout stays one pass).
fn table_view<S>(rows: Vec<(bool, Vec<Vec<Span>>)>, st: &MarkdownStyle) -> View<S> {
    let t = &st.table;
    let n = rows.iter().map(|r| r.1.len()).max().unwrap_or(0).max(1);
    let share = |c: usize| t.columns.get(c).copied().unwrap_or(1.0 / n as f32).max(0.001);
    let mut out: Vec<View<S>> = Vec::new();
    for (r, (_, cells)) in rows.into_iter().enumerate() {
        if r > 0 {
            if let Some((w, c)) = t.rule {
                out.push(rect().fill_width().height(w).background(c));
            }
        }
        let mut views: Vec<View<S>> = Vec::with_capacity(n);
        for (c, spans) in cells.into_iter().enumerate() {
            let body = rich_text(spans)
                .font_size(t.size.unwrap_or(st.size))
                .line_height(t.line_height.unwrap_or(st.line_height))
                .letter_spacing(t.letter_spacing);
            let (px, py) = t.cell_padding;
            views.push(vstack(body).align(Align::Start).padding_ltrb(0.0, py, px, py).flex(share(c)));
        }
        for c in views.len()..n {
            views.push(rect().flex(share(c))); // a short row keeps the columns in place
        }
        out.push(hstack(views).align(Align::Start).fill_width().copy_sep("\t"));
    }
    vstack(out).align(Align::Start).fill_width()
}
