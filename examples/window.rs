//! A desktop window like native macOS apps have: the desktop blurred behind the sidebar through a
//! system material, content under a tall title bar with the traffic lights centred in it, an
//! opaque panel on the right.
//!
//! cargo run --release --example window [-- --backdrop sidebar|under|window|content|titlebar|header|menu|popover|sheet|hud|fullscreen|blur]

use ruru::prelude::*;

struct S {
    selected: usize,
    count: u32,
}

const ITEMS: [&str; 5] = ["Inbox", "Today", "Projects", "Archive", "Settings"];
const TITLEBAR: f32 = 44.0;

fn sidebar(s: &S, env: &Env) -> View<S> {
    let rows = ITEMS.iter().enumerate().map(|(i, name)| {
        let row = text(*name).padding_xy(10.0, 6.0).fill_width().radius(6.0);
        let row = if i == s.selected { row.background(Color::WHITE.alpha(0.12)) } else { row };
        row.on_tap(move |s: &mut S| s.selected = i)
    });
    vstack((
        // Room for the traffic lights; pressing here moves the window.
        rect().fill_width().height(env.titlebar.max(8.0)).window_drag(),
        vstack(rows.collect::<Vec<_>>()).spacing(2.0).padding(8.0),
        spacer(),
    ))
    .width(220.0)
    .fill_height()
}

fn content(s: &S, env: &Env) -> View<S> {
    let title = hstack((
        text(ITEMS[s.selected]).weight(Weight::Semibold),
        spacer(),
        button("+1", |s: &mut S| s.count += 1).size(64.0, 26.0),
    ))
    .padding_xy(16.0, 0.0)
    .height(env.titlebar.max(36.0))
    .fill_width()
    .window_drag();
    let panel = vstack((
        text(format!("Pressed {} times", s.count)).font_size(22.0).weight(Weight::Bold),
        text("The title strip above moves the window; double-click it to zoom."),
    ))
    .spacing(8.0)
    .padding(20.0)
    .align(Align::Start)
    .fill()
    .background(Color::hex(0x1C1C1E))
    .radius(12.0)
    .border(1.0, Color::WHITE.alpha(0.08));
    vstack((title, panel)).padding_ltrb(0.0, 0.0, 8.0, 8.0).fill()
}

fn backdrop() -> Backdrop {
    let args: Vec<String> = std::env::args().collect();
    let name = args.iter().position(|a| a == "--backdrop").and_then(|i| args.get(i + 1));
    match name.map(String::as_str) {
        Some("under") => Backdrop::UnderWindow,
        Some("window") => Backdrop::Window,
        Some("content") => Backdrop::Content,
        Some("titlebar") => Backdrop::Titlebar,
        Some("header") => Backdrop::HeaderView,
        Some("menu") => Backdrop::Menu,
        Some("popover") => Backdrop::Popover,
        Some("sheet") => Backdrop::Sheet,
        Some("hud") => Backdrop::Hud,
        Some("fullscreen") => Backdrop::FullScreenUi,
        Some("blur") => Backdrop::Blur(20.0),
        _ => Backdrop::default(),
    }
}

fn main() {
    App::new(S { selected: 0, count: 0 }, |s: &S, env: &Env| hstack((sidebar(s, env), content(s, env))).fill())
        .title("ruru window")
        .size(900.0, 560.0)
        // Dark tint over the blurred desktop; the panel on the right is opaque.
        .background(Color::hex(0x1E1E20).alpha(0.55))
        .backdrop(backdrop())
        .titlebar_height(TITLEBAR)
        .run();
}
