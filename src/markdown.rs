//! Markdown → views. Built for chat-style content: paragraphs with inline styles,
//! headings, lists, quotes, code blocks, rules and images, each a normal `View` so it
//! flows with everything else in a scroll or lazy list.

use crate::types::Color;
use crate::view::{hstack, image, rect, rich_text, text, vstack, Align, Image, Span, View, Weight};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Renders markdown. Images are shown as a placeholder; use [`markdown_with`] to supply them.
pub fn markdown<S>(source: &str) -> View<S> {
    markdown_with(source, |_| None)
}

/// Renders markdown; `images` maps an image URL to an already loaded picture.
pub fn markdown_with<S>(source: &str, images: impl Fn(&str) -> Option<Image>) -> View<S> {
    let link = Color::hex(0x4DA3FF);
    let mut blocks: Vec<View<S>> = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let (mut bold, mut italic, mut linked) = (0u32, 0u32, 0u32);
    let mut heading: Option<HeadingLevel> = None;
    let mut code: Option<String> = None;
    let mut lists: Vec<Option<u64>> = Vec::new(); // open lists: next number, or None for bullets
    let mut bullet: Option<String> = None; // marker waiting for the item's first line
    let mut quote = 0u32;
    let mut alt: Option<String> = None; // inside an image: collecting its alt text

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
        let mut body = rich_text(std::mem::take(spans));
        if let Some(level) = heading {
            let size = match level {
                HeadingLevel::H1 => 26.0,
                HeadingLevel::H2 => 22.0,
                HeadingLevel::H3 => 19.0,
                _ => 17.0,
            };
            body = body.font_size(size).weight(Weight::Bold);
        }
        let mut block = if depth > 0 {
            let marker = text(bullet.take().unwrap_or_default()).width(26.0).opacity(0.7);
            hstack((marker, body)).align(Align::Start).padding_ltrb(18.0 * (depth as f32 - 1.0), 0.0, 0.0, 0.0)
        } else {
            body
        };
        if quote > 0 {
            block = vstack(block)
                .align(Align::Start)
                .fill_width()
                .padding_ltrb(14.0, 8.0, 12.0, 8.0)
                .background(Color::rgba(0.5, 0.5, 0.55, 0.16))
                .radius(10.0)
                .opacity(0.85);
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
                Tag::Link { .. } => linked += 1,
                Tag::BlockQuote(_) => quote += 1,
                Tag::CodeBlock(kind) => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    let _lang = matches!(kind, CodeBlockKind::Fenced(_));
                    code = Some(String::new());
                }
                Tag::List(start) => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
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
                Tag::Image { dest_url, .. } => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    match images(&dest_url) {
                        Some(img) => blocks.push(image(&img).fill_width().radius(14.0)),
                        None => alt = Some(String::new()),
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                }
                TagEnd::Heading(_) => {
                    flush(&mut spans, &mut blocks, heading, lists.len(), &mut bullet, quote);
                    heading = None;
                }
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::Link => linked = linked.saturating_sub(1),
                TagEnd::BlockQuote(_) => quote = quote.saturating_sub(1),
                TagEnd::List(_) => {
                    lists.pop();
                }
                TagEnd::TableCell => spans.push(Span::new("   ")),
                TagEnd::CodeBlock => {
                    if let Some(src) = code.take() {
                        let body = rich_text(vec![Span::new(src.trim_end_matches('\n')).code()]).font_size(13.5);
                        blocks.push(
                            vstack(body)
                                .align(Align::Start)
                                .fill_width()
                                .padding(14.0)
                                .background(Color::rgba(0.0, 0.0, 0.0, 0.35))
                                .border(1.0, Color::rgba(1.0, 1.0, 1.0, 0.08))
                                .radius(12.0),
                        );
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
                    s.bold = bold > 0;
                    s.italic = italic > 0;
                    if linked > 0 {
                        s.color = Some(link);
                    }
                    spans.push(s);
                }
            }
            Event::Code(t) => spans.push(Span::new(t.into_string()).code().color(Color::hex(0xFFB86B))),
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
    vstack(blocks).spacing(10.0).align(Align::Start).fill_width()
}
