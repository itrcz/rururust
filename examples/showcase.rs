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
    tab: usize,
}

const TABS: [&str; 3] = ["Home", "Gallery", "About"];
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

fn card(children: impl ruru::Children<Demo>) -> View<Demo> {
    vstack(children).fill_width().glass().radius(28.0).shadow(Color::BLACK.alpha(0.30), 40.0, 14.0)
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
        card((
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
        card((
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

fn gallery() -> View<Demo> {
    let tile = |i: usize| {
        let (a, b) = PALETTE[i % PALETTE.len()];
        rect().fill_width().height(150.0).radius(26.0).gradient(a, b, 45.0 + 40.0 * i as f32)
    };
    vstack((
        text("Gallery").font_size(34.0).weight(Weight::Bold).fill_width(),
        vstack((0..5).map(|r| hstack((tile(r * 2), tile(r * 2 + 1))).spacing(12.0).fill_width()).collect::<Vec<_>>())
            .spacing(12.0)
            .fill_width(),
    ))
    .spacing(18.0)
}

fn about() -> View<Demo> {
    vstack((
        text("About").font_size(34.0).weight(Weight::Bold).fill_width(),
        card((
            text("ruru").font_size(28.0).weight(Weight::Bold),
            text(
                "One Rust codebase. Native on macOS, Windows, Linux, iOS and Android, \
                 and in the browser on a canvas. Every pixel is drawn on the GPU.",
            )
            .font_size(16.0)
            .centered()
            .color(Color::WHITE.alpha(0.8)),
        ))
        .spacing(10.0)
        .padding(24.0),
    ))
    .spacing(18.0)
}

fn tab_bar(s: &Demo) -> View<Demo> {
    let tabs: Vec<_> = TABS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let on = s.tab == i;
            hstack(text(*name).font_size(15.0).weight(Weight::Semibold))
                .padding_xy(20.0, 11.0)
                .radius(22.0)
                .background(Color::WHITE.alpha(if on { 0.22 } else { 0.0 }))
                .color(Color::WHITE.alpha(if on { 1.0 } else { 0.65 }))
                .on_tap(move |s: &mut Demo| s.tab = i)
        })
        .collect();
    hstack(hstack(tabs).spacing(2.0).padding(6.0).glass().radius(30.0).shadow(Color::BLACK.alpha(0.4), 30.0, 10.0))
        .padding_ltrb(0.0, 0.0, 0.0, 22.0)
        .place(Align::Center, Align::End)
}

fn view(s: &Demo) -> View<Demo> {
    let page = match s.tab {
        0 => home(s),
        1 => gallery(),
        _ => about(),
    };
    zstack((
        backdrop(s),
        scroll(page.fill_width().padding_ltrb(18.0, 56.0, 18.0, 110.0)).key(s.tab),
        tab_bar(s),
    ))
    .fill()
}

fn main() {
    let state = Demo { count: 0, wifi: true, focus: false, drift: 0.35, tab: 0 };
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
            let wait = |_: &mut ruru::Ui<Demo>, n: u32| {
                if n > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(90));
                }
            };
            ruru::headless::render_png_after(&mut ui, 420, 820, 2.0, frames, wait, &args[i + 1]);
            return;
        }
    }
    App::new(state, view).title("ruru showcase").size(420.0, 820.0).show_fps(true).run();
}
