//! Full-height code cards: optional pinned header, copy action and lightweight token coloring.
use super::CodeBlockStyle;
use crate::view::{hstack, icon, rect, rich_text, spacer, text, vstack, Align, Glass, Image, Span, View};
use crate::Color;

#[derive(Clone, Debug)]
pub struct CodeHeaderStyle {
    pub size: f32,
    pub height: f32,
    pub icon_size: f32,
    pub copy_size: f32,
    pub gap: f32,
    pub glass: Option<Glass>,
    /// Height of the material fading to transparent below the header.
    pub fade: f32,
    pub copy_icon: Option<Image>,
    pub language_icon: Option<Image>,
    pub copy_label: String,
}
impl Default for CodeHeaderStyle {
    fn default() -> Self {
        Self { size: 13.0, height: 40.0, icon_size: 18.0, copy_size: 16.0, gap: 8.0, glass: None, fade: 0.0,
            copy_icon: None, language_icon: None, copy_label: "Copy".into() }
    }
}
#[derive(Clone, Debug)]
pub struct CodeSyntaxStyle {
    pub keyword: Color, pub string: Color, pub number: Color, pub comment: Color,
    pub tag: Color, pub attribute: Color, pub function: Color,
}
fn language_label(language: &str) -> &str {
    match language {
        "rs" | "rust" => "Rust", "py" | "python" => "Python", "js" | "javascript" => "JavaScript",
        "ts" | "typescript" => "TypeScript", "html" => "HTML", "xml" | "svg" => "XML",
        "css" => "CSS", "json" => "JSON", "bash" | "sh" | "shell" => "Bash", "" | "text" | "plaintext" => "Text",
        other => other,
    }
}
pub(super) fn block<S>(source: String, language: &str, style: &CodeBlockStyle) -> View<S> {
    let code = source.trim_end_matches('\n');
    let language = language.to_ascii_lowercase();
    let spans = match &style.syntax {
        Some(palette) if !matches!(language.as_str(), "" | "text" | "plaintext") => highlight(code, &language, palette),
        _ => vec![Span::new(code).code()],
    };
    let mut body = rich_text(spans).font_size(style.size).line_height(style.line_height).fill_width();
    if let Some(color) = style.color { body = body.color(color); }
    let mut content = vec![];
    if let Some(h) = &style.header {
        let mut label = vec![];
        if let Some(image) = &h.language_icon { label.push(icon(image).size(h.icon_size, h.icon_size)); }
        label.push(text(language_label(&language)).font_size(h.size));
        let copy = match &h.copy_icon {
            Some(image) => icon(image).size(h.copy_size, h.copy_size),
            None => text(&h.copy_label).font_size(h.size),
        };
        let copy_source = source.clone();
        let copy = hstack((spacer(), copy)).align(Align::Center).height(h.height).width(h.copy_size.max(32.0))
            .on_tap(move |_: &mut S| crate::copy_text(copy_source.clone())).press_scale(1.0);
        let row = hstack((hstack(label).spacing(h.gap).align(Align::Center), spacer(), copy))
            .align(Align::Center).fill_width().height(h.height).padding_h(style.padding);
        let header = vstack((row, rect().height(h.fade.max(0.0)))).fill_width();
        let header = if let Some(glass) = h.glass { header.glass_with(glass).glass_fade(h.fade) }
            else { header.background(style.background) };
        content.push(header.sticky().copy_as(""));
        content.push(body.padding_ltrb(style.padding, 0.0, style.padding, style.padding));
    } else { content.push(body.padding(style.padding)); }
    let mut card = vstack(content).align(Align::Start).fill_width().background(style.background).radius(style.radius).clip();
    if let Some((width, color)) = style.border { card = card.border(width, color); }
    card
}

fn highlight(source: &str, language: &str, palette: &CodeSyntaxStyle) -> Vec<Span> {
    let markup = matches!(language, "html" | "xml" | "svg");
    let hash_comments = matches!(language, "py" | "python" | "bash" | "sh" | "shell" | "ruby" | "yaml" | "yml" | "toml");
    let mut spans: Vec<Span> = vec![];
    let mut i = 0; let mut in_tag = false; let mut tag_name = false;
    while i < source.len() {
        let start = i; let rest = &source[i..]; let ch = rest.chars().next().unwrap();
        let mut color = None;
        if rest.starts_with("<!--") || rest.starts_with("/*") {
            let end = if rest.starts_with("<!--") { "-->" } else { "*/" };
            i += rest.find(end).map_or(rest.len(), |n| n + end.len()); color = Some(palette.comment);
        } else if rest.starts_with("//") || (ch == '#' && hash_comments) {
            i += rest.find('\n').unwrap_or(rest.len()); color = Some(palette.comment);
        } else if matches!(ch, '\'' | '"' | '`') {
            i += ch.len_utf8(); let mut escaped = false;
            while i < source.len() {
                let c = source[i..].chars().next().unwrap(); i += c.len_utf8();
                if escaped { escaped = false; } else if c == '\\' { escaped = true; } else if c == ch { break; }
            }
            color = Some(palette.string);
        } else if ch.is_ascii_digit() {
            i += rest.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_')).map(char::len_utf8).sum::<usize>();
            color = Some(palette.number);
        } else if ch.is_alphabetic() || ch == '_' || ch == '$' {
            i += rest.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '$') || (in_tag && *c == '-')).map(char::len_utf8).sum::<usize>();
            let word = &source[start..i]; let after = source[i..].trim_start();
            color = if in_tag {
                if tag_name { tag_name = false; Some(palette.tag) } else { Some(palette.attribute) }
            } else if matches!(word, "const" | "let" | "var" | "fn" | "function" | "def" | "class" | "struct" | "enum" | "impl" | "pub" | "use" | "import" | "from" | "export" | "return" | "if" | "else" | "elif" | "for" | "while" | "loop" | "in" | "of" | "match" | "switch" | "case" | "break" | "continue" | "true" | "false" | "null" | "None" | "True" | "False" | "self" | "this" | "new" | "async" | "await" | "try" | "catch" | "throw" | "mut" | "move" | "as" | "type" | "interface" | "default") { Some(palette.keyword) }
            else if after.starts_with('(') { Some(palette.function) }
            else if after.starts_with(':') { Some(palette.attribute) }
            else { None };
        } else {
            i += ch.len_utf8();
            if markup && ch == '<' { in_tag = true; tag_name = true; }
            if in_tag { color = Some(palette.tag); }
            if markup && ch == '>' { in_tag = false; }
        }
        if let Some(last) = spans.last_mut().filter(|s| s.color == color) { last.text.push_str(&source[start..i]); }
        else { let mut span = Span::new(&source[start..i]).code(); span.color = color; spans.push(span); }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlighting_preserves_unicode_and_unfinished_streamed_source() {
        let palette = CodeSyntaxStyle { keyword:Color::RED,string:Color::GREEN,number:Color::BLUE,comment:Color::WHITE,tag:Color::RED,attribute:Color::GREEN,function:Color::BLUE };
        for (lang, source) in [("html", "<div lang=\"ru\">Привет 🦀</div>\n<!-- конец -->"), ("python", "def hello():\n    print('Привет 🦀"), ("rust", "let x = 10; // comment\n")] {
            let spans = highlight(source, lang, &palette);
            assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<String>(), source);
            assert!(spans.iter().all(|s| s.mono));
            assert!(spans.iter().any(|s| s.color.is_some()));
        }
    }
}
