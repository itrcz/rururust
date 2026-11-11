//! Showcase: glass, springs, forms, images, a markdown chat, overlays, and a layout that
//! rearranges itself for phone, tablet and desktop window sizes (resize the window).
//!
//!   cargo run --release --example showcase
//!   cargo run --release --example showcase -- --screenshot shot.png [tab] [frames] [width] [down] [--scale 1] [--light]

use ruru::prelude::*;
use ruru::SizeClass;
use std::rc::Rc;

#[derive(Clone)]
struct Msg {
    me: bool,
    text: String,
}

struct Icons {
    home: Image,
    chat: Image,
    photo: Image,
    form: Image,
    bell: Image,
    bolt: Image,
    heart: Image,
    plus: Image,
    send: Image,
    menu: Image,
}

struct Demo {
    count: i32,
    wifi: bool,
    focus: bool,
    drift: f32,
    /// Glass effect strength slider, 0..=1 (0.5 = the stock material).
    glass: f32,
    /// Light appearance (the theme follows it, see `theme_of`).
    light: bool,
    /// Glass lab: dark or light preset, and its three knobs.
    lab_dark: bool,
    lab_blur: f32,
    lab_distortion: f32,
    lab_opacity: f32,
    tab: usize,
    name: String,
    notes: String,
    city: usize,
    photos: Vec<Image>,
    icons: Rc<Icons>,
    modal: bool,
    sheet: bool,
    drawer: bool,
    toast: Option<String>,
    refreshing: bool,
    messages: Rc<Vec<Msg>>,
    draft: String,
    /// The reply being streamed; replacing it cancels the previous stream.
    reply: Option<Task>,
}

const TABS: [&str; 4] = ["Home", "Chat", "Gallery", "Forms"];
const CITIES: [&str; 5] = ["Lisbon", "Tokyo", "Reykjavik", "Buenos Aires", "Cape Town"];
const PALETTE: [(Color, Color); 6] = [
    (Color::PINK, Color::ORANGE),
    (Color::INDIGO, Color::TEAL),
    (Color::PURPLE, Color::PINK),
    (Color::GREEN, Color::TEAL),
    (Color::ORANGE, Color::YELLOW),
    (Color::BLUE, Color::PURPLE),
];

const REPLY: &str = "Here is what **ruru** renders in a chat:\n\n\
## Markdown\n\
Inline `code`, *emphasis*, **strong** text and [links](https://example.com) wrap together.\n\n\
1. Ordered lists\n2. with several items\n   - and nested bullets\n\n\
> Quotes for system notes or citations.\n\n\
```rust\nfn main() {\n    println!(\"code blocks keep their spacing\");\n}\n```\n\n\
![a generated picture](photo:1)\n\n\
---\nRows are built lazily, so a very long history stays fast.";

fn svg(path: &str) -> Image {
    let src = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill-rule="evenodd" d="{path}"/></svg>"#);
    Image::from_svg(src.as_bytes(), 96).expect("icon")
}

fn icons() -> Icons {
    Icons {
        home: svg("M3 11 12 3l9 8v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z"),
        chat: svg("M4 4h16a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H10l-5 4v-4H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"),
        photo: svg("M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zm0 12 5-5 4 4 3-3 4 4V6H4zM16 8a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z"),
        form: svg("M3 17.2V21h3.8L18 9.8 14.2 6zM20.7 7a1 1 0 0 0 0-1.4l-2.3-2.3a1 1 0 0 0-1.4 0l-1.8 1.8L19 8.8z"),
        bell: svg("M12 22a2 2 0 0 0 2-2h-4a2 2 0 0 0 2 2zm6-6v-5a6 6 0 0 0-4.5-5.8v-.7a1.5 1.5 0 0 0-3 0v.7A6 6 0 0 0 6 11v5l-2 2v1h16v-1z"),
        bolt: svg("M13 2 4 14h6l-1 8 9-12h-6z"),
        heart: svg("M12 21s-8-5.3-8-11a4.5 4.5 0 0 1 8-2.8A4.5 4.5 0 0 1 20 10c0 5.7-8 11-8 11z"),
        plus: svg("M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z"),
        send: svg("M12 4l7 7-1.4 1.4L13 7.8V20h-2V7.8l-4.6 4.6L5 11z"),
        menu: svg("M3 6h18v2H3zm0 5h18v2H3zm0 5h18v2H3z"),
    }
}

/// A generated picture, so the example needs no asset files. Real apps use
/// `Image::decode(include_bytes!("photo.jpg"))`.
fn plasma(seed: f32) -> Image {
    let (w, h) = (480u32, 360u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let a = (u * 6.0 + seed).sin() + (v * 5.0 - seed * 1.7).cos() + ((u + v) * 4.0 + seed * 0.6).sin();
            let ring = (((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 18.0 - seed).sin();
            let ch = |k: f32| (((a + ring * 0.6) * 1.3 + k + seed).sin() * 0.5 + 0.5) * 255.0;
            px.extend_from_slice(&[ch(0.0) as u8, ch(2.1) as u8, ch(4.2) as u8, 255]);
        }
    }
    Image::from_rgba(w, h, px)
}

fn toast(s: &mut Demo, text: &str) {
    s.toast = Some(text.to_string());
    after(2.2, |s: &mut Demo| s.toast = None);
}

// ---------------------------------------------------------------- pieces

fn blob(color: Color, d: f32, x: f32, y: f32) -> View<Demo> {
    hstack(rect().size(d, d).radius(d / 2.0).background(color))
        .padding_ltrb(x, y, 0.0, 0.0)
        .place(Align::Start, Align::Start)
}

fn backdrop(s: &Demo, t: &Theme) -> View<Demo> {
    let d = s.drift;
    zstack((
        blob(Color::PINK, 280.0, -80.0 + 220.0 * d, 40.0),
        blob(Color::BLUE, 320.0, 200.0 - 240.0 * d, 260.0),
        blob(Color::ORANGE, 200.0, 40.0 + 160.0 * d, 520.0),
        blob(Color::PURPLE, 180.0, 260.0 - 60.0 * d, -40.0),
    ))
    .fill()
    .gradient(
        if t.dark { Color::hex(0x1A1040) } else { Color::hex(0xE4E0FF) },
        if t.dark { Color::hex(0x05060F) } else { Color::hex(0xF8F8FC) },
        180.0,
    )
}

/// The app's theme: the light switch on the Forms page flips it, and every view follows.
fn theme_of(s: &Demo) -> Theme {
    if s.light {
        Theme::light()
    } else {
        Theme::dark()
    }
}

fn glass(s: &Demo) -> Glass {
    let base = if s.light { Glass::light() } else { Glass::regular() };
    base.strength(s.glass * 2.0)
}

fn card(s: &Demo, children: impl ruru::Children<Demo>) -> View<Demo> {
    let shadow = Color::BLACK.alpha(if s.light { 0.12 } else { 0.30 });
    vstack(children).fill_width().glass_with(glass(s)).radius(28.0).shadow(shadow, 40.0, 14.0)
}

fn row(glyph: &Image, tint: Color, title: &str, trailing: View<Demo>) -> View<Demo> {
    let badge = zstack(icon(glyph).size(18.0, 18.0)).size(30.0, 30.0).radius(8.0).background(tint).color(Color::WHITE);
    hstack((badge, text(title), spacer(), trailing)).spacing(12.0).fill_width().padding_xy(16.0, 11.0)
}

fn divider(t: &Theme) -> View<Demo> {
    rect().fill_width().height(0.5).background(t.color.text.alpha(0.16))
}

fn slider_row(t: &Theme, title: &str, value: String, control: View<Demo>) -> View<Demo> {
    vstack((hstack((text(title), spacer(), text(value).color(t.color.text.alpha(0.6)))).fill_width(), control))
        .spacing(8.0)
        .fill_width()
        .padding_xy(16.0, 12.0)
}

/// Round glass button. Put several close together and they fuse like drops.
fn drop_button(s: &Demo, glyph: &Image, f: impl Fn(&mut Demo) + 'static) -> View<Demo> {
    zstack(icon(glyph)).size(52.0, 52.0).radius(26.0).glass_with(glass(s)).on_tap(f)
}

fn settings(s: &Demo, t: &Theme) -> View<Demo> {
    card(s, (
        row(&s.icons.bolt, Color::BLUE, "Wi-Fi", toggle(s.wifi, |s: &mut Demo, on| s.wifi = on)),
        divider(t),
        row(&s.icons.bell, Color::INDIGO, "Focus", toggle(s.focus, |s: &mut Demo, on| s.focus = on)),
        divider(t),
        slider_row(t, "Drift", format!("{:.0}%", s.drift * 100.0), slider(s.drift, |s: &mut Demo, v| s.drift = v)),
        divider(t),
        slider_row(t, "Glass strength", format!("{:.0}%", s.glass * 200.0), slider(s.glass, |s: &mut Demo, v| s.glass = v)),
    ))
    .padding_xy(0.0, 4.0)
}

/// Glass lab: a dark or light pane and a narrow pill over stripes and text, with the
/// three knobs of `Glass` on sliders.
fn glass_lab(s: &Demo, t: &Theme) -> View<Demo> {
    let preset = if s.lab_dark { Glass::dark() } else { Glass::light() };
    let m = preset.blur(s.lab_blur).distortion(s.lab_distortion).opacity(s.lab_opacity);
    let ink = if s.lab_dark { Color::WHITE } else { Color::BLACK };
    let stripes = hstack(PALETTE.iter().map(|(a, b)| rect().fill().gradient(*a, *b, 160.0)).collect::<Vec<_>>()).fill();
    let words = text("Busy content under the glass: stripes, Handgloves 0123456789, and lines of text that the blur should soften without losing the edge of the pane. ".repeat(3))
        .font_size(17.0)
        .weight(Weight::Bold)
        .color(Color::WHITE)
        .padding(14.0);
    let pane = vstack((
        text(if s.lab_dark { "Dark glass" } else { "Light glass" }).font_size(20.0).weight(Weight::Bold),
        text(format!("blur {:.2} · distortion {:.2} · opacity {:.2}", s.lab_blur, s.lab_distortion, s.lab_opacity)).font_size(12.0),
    ))
    .spacing(2.0)
    .align(Align::Start)
    .padding_xy(18.0, 14.0)
    .glass_with(m)
    .radius(22.0)
    .color(ink);
    let pill = hstack((text("Narrow"), text("pill"), text("bar"))).spacing(18.0).padding_xy(20.0, 9.0).glass_with(m).radius(20.0).color(ink);
    let stage = zstack((
        stripes,
        words.place(Align::Start, Align::Start),
        vstack((pane, pill)).spacing(14.0).place(Align::Center, Align::Center).padding(16.0),
    ))
    .height(250.0)
    .fill_width()
    .radius(24.0)
    .clip();
    let mode = |label: &str, dark: bool| {
        let on = s.lab_dark == dark;
        button(label, move |s: &mut Demo| s.lab_dark = dark)
            .background(t.color.text.alpha(if on { 0.28 } else { 0.08 }))
            .color(t.color.text)
            .fill_width()
    };
    vstack((
        text("Glass lab").font_size(22.0).weight(Weight::Bold).fill_width().padding_ltrb(4.0, 0.0, 0.0, 0.0),
        stage,
        card(s, (
            hstack((mode("Dark", true), mode("Light", false))).spacing(10.0).fill_width().padding_xy(16.0, 12.0),
            divider(t),
            slider_row(t, "Blur", format!("{:.0}%", s.lab_blur * 100.0), slider(s.lab_blur, |s: &mut Demo, v| s.lab_blur = v)),
            divider(t),
            slider_row(t, "Distortion", format!("{:.0}%", s.lab_distortion * 100.0), slider(s.lab_distortion, |s: &mut Demo, v| s.lab_distortion = v)),
            divider(t),
            slider_row(t, "Opacity", format!("{:.0}%", s.lab_opacity * 100.0), slider(s.lab_opacity, |s: &mut Demo, v| s.lab_opacity = v)),
        )),
    ))
    .spacing(12.0)
    .fill_width()
}

// ---------------------------------------------------------------- pages

fn home(s: &Demo, env: &Env) -> View<Demo> {
    let i = &s.icons;
    let t = &*env.theme;
    vstack((
        vstack((
            text("Good evening").font_size(15.0).color(t.color.text.alpha(0.6)),
            text("Liquid Glass").font_size(34.0).weight(Weight::Bold),
        ))
        .spacing(2.0)
        .align(Align::Start)
        .fill_width()
        .padding_ltrb(4.0, 0.0, 0.0, 4.0),
        card(s, (
            text("TAPS").font_size(12.0).weight(Weight::Semibold).color(t.color.text.alpha(0.55)),
            text(s.count.to_string()).font_size(72.0).weight(Weight::Heavy),
            hstack((
                button("−", |s: &mut Demo| s.count -= 1).background(t.color.text.alpha(0.16)).color(t.color.text),
                button("Tap me", |s: &mut Demo| s.count += 1).padding_xy(34.0, 13.0),
                button("+", |s: &mut Demo| s.count += 10).background(t.color.text.alpha(0.16)).color(t.color.text),
            ))
            .spacing(10.0),
        ))
        .spacing(6.0)
        .padding(22.0),
        // Glass that sits close together fuses into one liquid shape.
        hstack((
            drop_button(s, &i.heart, |s: &mut Demo| toast(s, "Liked")),
            drop_button(s, &i.bell, |s: &mut Demo| s.sheet = true),
            drop_button(s, &i.plus, |s: &mut Demo| s.modal = true),
            spacer(),
            drop_button(s, &i.bolt, |s: &mut Demo| toast(s, "Glass fuses when panes nearly touch")),
        ))
        .spacing(7.0)
        .fill_width(),
        glass_lab(s, t),
        // On a desktop the settings live in the right-hand panel instead.
        if env.class() == SizeClass::Desktop { rect() } else { settings(s, t) },
        vstack(
            PALETTE
                .iter()
                .enumerate()
                .map(|(i, (a, b))| {
                    hstack((
                        vstack((
                            text(format!("Card {}", i + 1)).weight(Weight::Semibold).font_size(20.0),
                            text("Scrolls under the glass").font_size(14.0).color(Color::WHITE.alpha(0.8)),
                        ))
                        .align(Align::Start)
                        .spacing(2.0),
                        spacer(),
                    ))
                    .fill_width()
                    .padding(20.0)
                    .gradient(*a, *b, 120.0)
                    .radius(24.0)
                })
                .collect::<Vec<_>>(),
        )
        .spacing(12.0)
        .fill_width(),
    ))
    .spacing(18.0)
}

fn bubble(material: Glass, style: &Bubbles, m: &Msg, photos: &[Image]) -> View<Demo> {
    if m.me {
        let body = text(m.text.as_str()).padding_xy(14.0, 10.0).background(style.mine).color(style.on_mine).radius(18.0);
        hstack((spacer(), body)).fill_width().padding_ltrb(60.0, 5.0, 0.0, 5.0)
    } else {
        let md = ruru::markdown_styled_with(&m.text, &style.markdown, |url| {
            url.strip_prefix("photo:").and_then(|n| n.parse::<usize>().ok()).and_then(|n| photos.get(n).cloned())
        });
        vstack(vstack(md).fill_width().padding(16.0).glass_with(material).radius(22.0)).fill_width().padding_xy(0.0, 5.0)
    }
}

/// What a chat row needs from the theme; built once per frame and shared by the rows.
struct Bubbles {
    mine: Color,
    on_mine: Color,
    markdown: MarkdownStyle,
}

fn chat(s: &Demo, t: &Theme) -> View<Demo> {
    let header = hstack((
        text("Assistant").weight(Weight::Semibold),
        spacer(),
        text(format!("{} messages", s.messages.len())).font_size(13.0).color(t.color.text.alpha(0.6)),
    ))
    .fill_width()
    .padding_xy(16.0, 12.0)
    .glass_with(t.glass_menu)
    .radius(18.0)
    .sticky();
    // The list builds only the rows on screen; each row is ordinary views (markdown,
    // pictures, cards), so system blocks can sit in the flow between messages. The whole
    // history is one selection: drag across messages, Cmd/Ctrl+C copies them as plain text.
    // Links open in the browser (`.on_link()` would route them to the app instead).
    let (messages, photos, material) = (s.messages.clone(), s.photos.clone(), glass(s));
    let style = Bubbles { mine: t.color.accent, on_mine: t.color.on_accent, markdown: MarkdownStyle::from(t) };
    let rows = list(s.messages.len(), 120.0, move |i| bubble(material, &style, &messages[i], &photos))
        .selectable()
        .selection_color(t.color.link.alpha(0.35));
    // The composer draws its own capsule; the field inside is plain. Enter sends,
    // Shift+Enter breaks the line, and the field grows up to six rows.
    let input = hstack((
        text_field(s.draft.as_str(), "Message", |s: &mut Demo, v| s.draft = v)
            .multiline()
            .on_submit(send)
            .plain()
            .padding_ltrb(14.0, 9.0, 4.0, 9.0),
        zstack(icon(&s.icons.send)).size(38.0, 38.0).radius(19.0).background(t.color.accent).color(t.color.on_accent).on_tap(send),
    ))
    .align(Align::End)
    .padding(4.0)
    .glass_with(glass(s))
    .radius(23.0)
    .fill_width();
    vstack((
        scroll(vstack((header, rows)).spacing(8.0).fill_width().padding_ltrb(18.0, 0.0, 18.0, 12.0))
            .padding_ltrb(0.0, 58.0, 0.0, 0.0)
            .stick_to_bottom()
            .on_refresh(s.refreshing, |s: &mut Demo| {
                s.refreshing = true;
                after(1.2, |s: &mut Demo| {
                    s.refreshing = false;
                    let mut all = vec![Msg { me: false, text: "*Earlier messages loaded.*".into() }];
                    all.extend(Rc::make_mut(&mut s.messages).drain(..));
                    *Rc::make_mut(&mut s.messages) = all;
                });
            }),
        vstack(input).fill_width().padding_ltrb(18.0, 6.0, 18.0, 0.0),
    ))
    .fill()
}

fn send(s: &mut Demo) {
    let text = std::mem::take(&mut s.draft);
    if text.trim().is_empty() {
        return;
    }
    Rc::make_mut(&mut s.messages).push(Msg { me: true, text });
    // Stands in for an LLM: the reply arrives word by word from a background task.
    s.reply = Some(spawn(|link: Link<Demo>| async move {
        sleep(0.5).await;
        link.send(|s| Rc::make_mut(&mut s.messages).push(Msg { me: false, text: String::new() }));
        for word in REPLY.split_inclusive(char::is_whitespace) {
            sleep(0.03 + 0.03 * (word.len() as f32 / 8.0).min(1.0)).await;
            link.send(move |s| {
                if let Some(m) = Rc::make_mut(&mut s.messages).last_mut().filter(|m| !m.me) {
                    m.text.push_str(word);
                }
            });
        }
    }));
}

fn gallery(s: &Demo) -> View<Demo> {
    let tile = |i: usize| image(&s.photos[i % s.photos.len()]).fill_width().height(150.0).radius(26.0);
    vstack((
        text("Gallery").font_size(34.0).weight(Weight::Bold).fill_width(),
        image(&s.photos[0]).fill_width().radius(30.0),
        vstack((0..3).map(|r| hstack((tile(r * 2 + 1), tile(r * 2 + 2))).spacing(12.0).fill_width()).collect::<Vec<_>>())
            .spacing(12.0)
            .fill_width(),
    ))
    .spacing(18.0)
}

fn field(t: &Theme, label: &str, control: View<Demo>) -> View<Demo> {
    vstack((text(label).font_size(13.0).weight(Weight::Semibold).color(t.color.text.alpha(0.6)), control))
        .spacing(6.0)
        .align(Align::Start)
        .fill_width()
}

fn forms(s: &Demo, t: &Theme) -> View<Demo> {
    let hello = if s.name.trim().is_empty() {
        "Type your name below".to_string()
    } else {
        format!("Hello, {} from {}!", s.name.trim(), CITIES[s.city])
    };
    vstack((
        text("Forms").font_size(34.0).weight(Weight::Bold).fill_width(),
        card(s, (
            text(hello).font_size(20.0).weight(Weight::Semibold).centered(),
            field(t, "NAME", text_field(s.name.as_str(), "Your name", |s: &mut Demo, v| s.name = v)),
            field(t, "NOTES", text_field(s.notes.as_str(), "Notes (Enter starts a new line)", |s: &mut Demo, v| s.notes = v).max_lines(4)),
            field(t, "CITY", picker(&CITIES, s.city, |s: &mut Demo, i| s.city = i).fill_width()),
            button("Clear", |s: &mut Demo| s.name.clear()).fill_width(),
        ))
        .spacing(16.0)
        .padding(20.0),
        // The whole interface repaints from the theme: built-in controls, and the app's own
        // colors read from `env.theme`.
        card(s, (
            row(&s.icons.bolt, t.color.accent, "Light appearance", toggle(s.light, |s: &mut Demo, on| {
                s.light = on;
                s.lab_dark = !on;
            })),
        ))
        .spacing(16.0)
        .padding(20.0),
        image(&s.photos[2]).fill_width().height(180.0).radius(28.0),
    ))
    .spacing(18.0)
}

// ---------------------------------------------------------------- shell

fn nav(s: &Demo, t: &Theme) -> View<Demo> {
    let glyphs = [&s.icons.home, &s.icons.chat, &s.icons.photo, &s.icons.form];
    let items: Vec<_> = TABS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let on = s.tab == i;
            hstack((icon(glyphs[i]).size(20.0, 20.0), text(*name).weight(Weight::Medium), spacer()))
                .spacing(12.0)
                .fill_width()
                .padding_xy(14.0, 11.0)
                .radius(12.0)
                .background(t.color.text.alpha(if on { 0.18 } else { 0.0 }))
                .color(t.color.text.alpha(if on { 1.0 } else { 0.7 }))
                .on_tap(move |s: &mut Demo| {
                    s.tab = i;
                    s.drawer = false;
                })
        })
        .collect();
    vstack((
        text("ruru").font_size(24.0).weight(Weight::Bold).fill_width().padding_ltrb(14.0, 8.0, 0.0, 12.0),
        vstack(items).spacing(4.0).fill_width(),
        spacer(),
    ))
    .padding(14.0)
    .fill()
}

fn overlays(s: &Demo, t: &Theme) -> Vec<View<Demo>> {
    let panel = |title: &str, body: &str, close: fn(&mut Demo)| {
        vstack((
            text(title).font_size(22.0).weight(Weight::Bold),
            text(body).font_size(15.0).centered().color(t.color.text.alpha(0.75)),
            button("Done", close).fill_width(),
        ))
        .spacing(14.0)
        .padding(24.0)
        .fill_width()
        .glass_with(t.glass_menu)
        .radius(30.0)
    };
    vec![
        drawer(s.drawer, |s: &mut Demo| s.drawer = false, nav(s, t).glass_with(t.glass_menu)),
        sheet(
            s.sheet,
            |s: &mut Demo| s.sheet = false,
            panel("Sheet", "Slides up from the bottom edge. Tap outside to dismiss.", |s| s.sheet = false).padding(10.0),
        ),
        modal(
            s.modal,
            |s: &mut Demo| s.modal = false,
            panel("Modal", "Blocks everything underneath until it is closed.", |s| s.modal = false),
        ),
        snackbar(
            s.toast.is_some(),
            hstack((icon(&s.icons.bell).size(18.0, 18.0), text(s.toast.clone().unwrap_or_default()).font_size(15.0)))
                .spacing(10.0)
                .padding_xy(18.0, 12.0)
                .glass_with(t.glass_menu)
                .radius(24.0)
                .shadow(Color::BLACK.alpha(0.35), 24.0, 8.0),
        ),
    ]
}

fn view(s: &Demo, env: &Env) -> View<Demo> {
    let class = env.class();
    let t = &*env.theme;
    let page = match s.tab {
        0 => home(s, env),
        2 => gallery(s),
        3 => forms(s, t),
        _ => rect(),
    };
    let bottom = if class == SizeClass::Phone { 110.0 } else { 24.0 };
    let content = if s.tab == 1 {
        chat(s, t).padding_ltrb(0.0, 0.0, 0.0, bottom - 14.0)
    } else {
        // Keep text lines readable on wide windows.
        let column = vstack(page.width(env.width.min(620.0) - 36.0)).fill_width();
        scroll(column.padding_ltrb(18.0, 56.0, 18.0, bottom)).key(s.tab)
    };

    let mut layers: Vec<View<Demo>> = vec![backdrop(s, t)];
    match class {
        // Phone: one column, floating tab bar, the menu button opens a drawer.
        SizeClass::Phone => {
            layers.push(content);
            layers.push(
                zstack(icon(&s.icons.menu).size(20.0, 20.0))
                    .size(40.0, 40.0)
                    .radius(20.0)
                    .glass_with(glass(s))
                    .on_tap(|s: &mut Demo| s.drawer = true)
                    .place(Align::Start, Align::Start),
            );
            layers.push(
                hstack(tab_bar(&TABS, s.tab, |s: &mut Demo, i| s.tab = i).glass_with(glass(s)))
                    .padding_ltrb(0.0, 0.0, 0.0, 22.0)
                    .place(Align::Center, Align::End),
            );
        }
        // Tablet and desktop: a sidebar, and on a desktop a settings panel as well.
        _ => {
            let sidebar = nav(s, t).width(230.0).glass_with(glass(s)).radius(26.0);
            let mut columns = vec![sidebar, content];
            if class == SizeClass::Desktop {
                columns.push(
                    vstack((text("Inspector").weight(Weight::Semibold).fill_width().padding(6.0), settings(s, t), spacer()))
                        .spacing(10.0)
                        .width(300.0)
                        .fill_height(),
                );
            }
            layers.push(hstack(columns).spacing(14.0).padding(14.0).fill());
        }
    }
    layers.extend(overlays(s, t));
    let menu_inset = if class == SizeClass::Phone { 12.0 } else { 0.0 };
    zstack(layers).fill().padding_ltrb(menu_inset, menu_inset, 0.0, 0.0).padding(0.0)
}

impl Demo {
    fn new() -> Demo {
        Demo {
            count: 0,
            wifi: true,
            focus: false,
            drift: 0.35,
            glass: 0.5,
            lab_dark: true,
            light: false,
            lab_blur: 0.3,
            lab_distortion: 0.7,
            lab_opacity: 0.45,
            tab: 0,
            name: String::new(),
            notes: String::new(),
            city: 0,
            photos: (0..7).map(|i| plasma(i as f32 * 1.3 + 0.4)).collect(),
            icons: Rc::new(icons()),
            modal: false,
            sheet: false,
            drawer: false,
            toast: None,
            refreshing: false,
            messages: Rc::new(vec![
                Msg { me: true, text: "What can you render?".into() },
                Msg { me: false, text: REPLY.into() },
            ]),
            draft: String::new(),
            reply: None,
        }
    }
}

fn main() {
    let state = Demo::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--screenshot") {
            let mut state = state;
            let num = |k: usize, d: u32| args.get(i + k).and_then(|t| t.parse().ok()).unwrap_or(d);
            state.tab = num(2, 0) as usize;
            let (frames, width) = (num(3, 1), num(4, 420));
            // Optional: points to scroll down before the shot; `--light` for the light theme;
            // `--scale 1` for a 1x frame (default 2).
            let down = num(5, 0) as f32;
            let scale = args.iter().position(|a| a == "--scale").and_then(|k| args.get(k + 1)).and_then(|t| t.parse().ok()).unwrap_or(2.0);
            state.light = args.iter().any(|a| a == "--light");
            state.lab_dark = !state.light;
            let mut ui = ruru::Ui::new(state, view);
            ui.theme_with(theme_of);
            ui.show_fps = true;
            let wait = |ui: &mut ruru::Ui<Demo>, n: u32| {
                if n == 1 && down > 0.0 {
                    ui.wheel((width as f32 * 0.5, 400.0), -down, ruru::WheelPhase::Line);
                }
                if n > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
            };
            ruru::headless::render_png_after(&mut ui, width, 820, scale, frames, wait, &args[i + 1]);
            return;
        }
    }
    App::new(state, view).title("ruru showcase").size(420.0, 820.0).theme_with(theme_of).show_fps(true).run();
}
