//! Showcase: glass cards over a live background, springs, toggles, sliders, scrolling.
//!
//!   cargo run --release --example showcase
//!   cargo run --release --example showcase -- --screenshot shot.png

use ruru::prelude::*;

struct Demo {
    count: i32,
    wifi: bool,
    focus: bool,
    drift: f32,
    /// Glass effect strength slider, 0..=1 (0.5 = the stock material).
    glass: f32,
    tab: usize,
    name: String,
    city: usize,
    photos: Vec<Image>,
}

const CITIES: [&str; 5] = ["Lisbon", "Tokyo", "Reykjavik", "Buenos Aires", "Cape Town"];

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

const TABS: [&str; 3] = ["Home", "Gallery", "Forms"];
const PALETTE: [(Color, Color); 6] = [
    (Color::PINK, Color::ORANGE),
    (Color::INDIGO, Color::TEAL),
    (Color::PURPLE, Color::PINK),
    (Color::GREEN, Color::TEAL),
    (Color::ORANGE, Color::YELLOW),
    (Color::BLUE, Color::PURPLE),
];

fn blob(color: Color, d: f32, x: f32, y: f32) -> View<Demo> {
    hstack(rect().size(d, d).radius(d / 2.0).background(color))
        .padding_ltrb(x, y, 0.0, 0.0)
        .place(Align::Start, Align::Start)
}

fn backdrop(s: &Demo) -> View<Demo> {
    let t = s.drift;
    zstack((
        blob(Color::PINK, 280.0, -80.0 + 220.0 * t, 40.0),
        blob(Color::BLUE, 320.0, 200.0 - 240.0 * t, 260.0),
        blob(Color::ORANGE, 200.0, 40.0 + 160.0 * t, 520.0),
        blob(Color::PURPLE, 180.0, 260.0 - 60.0 * t, -40.0),
    ))
    .fill()
    .gradient(Color::hex(0x1A1040), Color::hex(0x05060F), 180.0)
}

fn glass(s: &Demo) -> Glass {
    Glass::regular().strength(s.glass * 2.0)
}

fn card(s: &Demo, children: impl ruru::Children<Demo>) -> View<Demo> {
    vstack(children).fill_width().glass_with(glass(s)).radius(28.0).shadow(Color::BLACK.alpha(0.30), 40.0, 14.0)
}

fn row(tint: Color, title: &str, trailing: View<Demo>) -> View<Demo> {
    hstack((rect().size(30.0, 30.0).radius(8.0).background(tint), text(title), spacer(), trailing))
        .spacing(12.0)
        .fill_width()
        .padding_xy(16.0, 11.0)
}

fn divider() -> View<Demo> {
    rect().fill_width().height(0.5).background(Color::WHITE.alpha(0.16))
}

fn round_button(label: &str, f: impl Fn(&mut Demo) + 'static) -> View<Demo> {
    zstack(text(label).font_size(26.0).weight(Weight::Medium))
        .size(56.0, 56.0)
        .radius(28.0)
        .background(Color::WHITE.alpha(0.16))
        .border(1.0, Color::WHITE.alpha(0.22))
        .on_tap(f)
}

fn home(s: &Demo) -> View<Demo> {
    vstack((
        vstack((
            text("Good evening").font_size(15.0).color(Color::WHITE.alpha(0.6)),
            text("Liquid Glass").font_size(34.0).weight(Weight::Bold),
        ))
        .spacing(2.0)
        .align(Align::Start)
        .fill_width()
        .padding_ltrb(4.0, 0.0, 0.0, 4.0),
        card(s, (
            text("TAPS").font_size(12.0).weight(Weight::Semibold).color(Color::WHITE.alpha(0.55)),
            text(s.count.to_string()).font_size(72.0).weight(Weight::Heavy),
            hstack((
                round_button("−", |s: &mut Demo| s.count -= 1),
                button("Tap me", |s: &mut Demo| s.count += 1).radius(28.0).padding_xy(34.0, 17.0),
                round_button("+", |s: &mut Demo| s.count += 10),
            ))
            .spacing(14.0),
        ))
        .spacing(6.0)
        .padding(22.0),
        card(s, (
            row(Color::BLUE, "Wi-Fi", toggle(s.wifi, |s: &mut Demo, on| s.wifi = on)),
            divider(),
            row(Color::INDIGO, "Focus", toggle(s.focus, |s: &mut Demo, on| s.focus = on)),
            divider(),
            vstack((
                hstack((text("Drift"), spacer(), text(format!("{:.0}%", s.drift * 100.0)).color(Color::WHITE.alpha(0.6))))
                    .fill_width(),
                slider(s.drift, |s: &mut Demo, v| s.drift = v),
            ))
            .spacing(8.0)
            .fill_width()
            .padding_xy(16.0, 12.0),
            divider(),
            vstack((
                hstack((
                    text("Glass strength"),
                    spacer(),
                    text(format!("{:.0}%", s.glass * 200.0)).color(Color::WHITE.alpha(0.6)),
                ))
                .fill_width(),
                slider(s.glass, |s: &mut Demo, v| s.glass = v),
            ))
            .spacing(8.0)
            .fill_width()
            .padding_xy(16.0, 12.0),
        ))
        .padding_xy(0.0, 4.0),
        vstack(
            PALETTE
                .iter()
                .enumerate()
                .map(|(i, (a, b))| {
                    hstack((
                        vstack((
                            text(format!("Card {}", i + 1)).weight(Weight::Semibold).font_size(20.0),
                            text("Scrolls under the glass tab bar").font_size(14.0).color(Color::WHITE.alpha(0.8)),
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

fn field(label: &str, control: View<Demo>) -> View<Demo> {
    vstack((text(label).font_size(13.0).weight(Weight::Semibold).color(Color::WHITE.alpha(0.6)), control))
        .spacing(6.0)
        .align(Align::Start)
        .fill_width()
}

fn forms(s: &Demo) -> View<Demo> {
    let hello = if s.name.trim().is_empty() {
        "Type your name below".to_string()
    } else {
        format!("Hello, {} from {}!", s.name.trim(), CITIES[s.city])
    };
    vstack((
        text("Forms").font_size(34.0).weight(Weight::Bold).fill_width(),
        card(s, (
            text(hello).font_size(20.0).weight(Weight::Semibold).centered(),
            field("NAME", text_field(s.name.as_str(), "Your name", |s: &mut Demo, v| s.name = v)),
            field("CITY", picker(&CITIES, s.city, |s: &mut Demo, i| s.city = i).fill_width()),
            button("Clear", |s: &mut Demo| s.name.clear()).fill_width(),
        ))
        .spacing(16.0)
        .padding(20.0),
        image(&s.photos[2]).fill_width().height(180.0).radius(28.0),
    ))
    .spacing(18.0)
}

fn tabs(s: &Demo) -> View<Demo> {
    hstack(tab_bar(&TABS, s.tab, |s: &mut Demo, i| s.tab = i).glass_with(glass(s)))
        .padding_ltrb(0.0, 0.0, 0.0, 22.0)
        .place(Align::Center, Align::End)
}

fn view(s: &Demo) -> View<Demo> {
    let page = match s.tab {
        0 => home(s),
        1 => gallery(s),
        _ => forms(s),
    };
    zstack((
        backdrop(s),
        scroll(page.fill_width().padding_ltrb(18.0, 56.0, 18.0, 110.0)).key(s.tab),
        tabs(s),
    ))
    .fill()
}

fn main() {
    let state = Demo {
        count: 0,
        wifi: true,
        focus: false,
        drift: 0.35,
        glass: 0.5,
        tab: 0,
        name: String::new(),
        city: 0,
        photos: (0..7).map(|i| plasma(i as f32 * 1.3 + 0.4)).collect(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--screenshot") {
            let mut state = state;
            state.tab = args.get(i + 2).and_then(|t| t.parse().ok()).unwrap_or(0);
            let mut ui = ruru::Ui::new(state, view);
            ui.show_fps = true;
            // Optional 3rd value: number of frames to run (90 ms apart) before saving.
            let frames = args.get(i + 3).and_then(|t| t.parse().ok()).unwrap_or(1);
            // Optional 4th value "drag": press the tab bar and drag the drop along it.
            let drag = args.get(i + 4).is_some_and(|a| a == "drag");
            let wait = move |ui: &mut ruru::Ui<Demo>, n: u32| {
                if n > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(if drag { 16 } else { 90 }));
                }
                if drag && n == 2 {
                    ui.pointer_down((130.0, 770.0), false);
                } else if drag && n > 2 {
                    ui.pointer_move((130.0 + (n as f32 * 4.0).min(110.0), 770.0));
                }
            };
            ruru::headless::render_png_after(&mut ui, 420, 820, 2.0, frames, wait, &args[i + 1]);
            return;
        }
    }
    App::new(state, view).title("ruru showcase").size(420.0, 820.0).show_fps(true).run();
}
