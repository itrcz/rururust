//! An app shell like a desktop chat client: a rounded panel that cuts its children to its
//! corners (`.clip()`), a sidebar that slides shut (`.animate()`), chat titles that fade at
//! the edge (`.truncate()`), hover highlights and pointer shapes, and a reading column that
//! stops growing on a wide window (`.max_width()`).
//!
//!   cargo run --release --example panels
//!   cargo run --release --example panels -- --screenshot out.png [frames] [toggle_at] [mouse_x] [mouse_y] [width]
//!
//! With `toggle_at` < `frames` the sidebar is toggled at that frame (frames are ~16 ms apart),
//! so a small `frames - toggle_at` catches the slide in the middle.

use ruru::prelude::*;

struct S {
    sidebar: bool,
    selected: usize,
}

const CHATS: [&str; 10] = [
    "Оценить Rust для кроссплатформенного интерфейса с GPU-рендерингом",
    "Создать энциклопедическую страницу GPT и сравнить с аналогами",
    "Уточнить гарантии ООО «РТ»",
    "Настроить kubectl и API VK Cloud для нового кластера",
    "Разрешить доступ к vault.shds.io",
    "Убери спрайты не к месту",
    "Сравнить цены VK Cloud и Selectel на GPU-инстансы",
    "Добавить провайдер Grom H3 Ultra",
    "Улучшить анимацию ходьбы персонажа в редакторе",
    "Проверить сбой VPN на сервере",
];

const PANEL: Color = Color::rgba(0.145, 0.145, 0.155, 1.0);
const SIDEBAR: Color = Color::rgba(0.17, 0.17, 0.18, 1.0);
const FAINT: Color = Color::rgba(1.0, 1.0, 1.0, 0.45);

fn rail(s: &S) -> View<S> {
    let item = |label: &str, active: bool| {
        zstack(text(label).font_size(15.0).weight(Weight::Semibold))
            .size(36.0, 36.0)
            .radius(10.0)
            .background(if active { Color::WHITE.alpha(0.10) } else { Color::CLEAR })
            .hover(Color::WHITE.alpha(0.08))
    };
    vstack((
        item(if s.sidebar { "«" } else { "»" }, s.sidebar).on_tap(|s: &mut S| s.sidebar = !s.sidebar),
        item("⌂", false).on_tap(|_: &mut S| {}),
        item("+", false).on_tap(|_: &mut S| {}),
        spacer(),
        zstack(text("IT").font_size(12.0).weight(Weight::Bold)).size(28.0, 28.0).radius(14.0).background(Color::BLUE),
    ))
    .spacing(6.0)
    .padding_v(12.0)
    .width(56.0)
    .fill_height()
}

fn sidebar(s: &S) -> View<S> {
    let rows: Vec<View<S>> = CHATS
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let selected = i == s.selected;
            // The selected chat ends with "…", the others fade out at the edge.
            let label = text(*title).font_size(14.0).fill_width();
            let label = if selected { label.lines(1) } else { label.truncate(Truncate::Fade) };
            let row = zstack(label).padding_h(10.0).padding_v(7.0).fill_width().radius(8.0);
            let row = if selected { row.background(Color::WHITE.alpha(0.10)) } else { row.hover(Color::WHITE.alpha(0.06)) };
            row.on_tap(move |s: &mut S| s.selected = i)
        })
        .collect();
    vstack((
        hstack((text("Codex").font_size(18.0).weight(Weight::Semibold), spacer())).padding_h(10.0).padding_v(12.0),
        text("Recents").font_size(13.0).color(FAINT).fill_width().padding_h(10.0).padding_v(6.0),
        vstack(rows).spacing(2.0),
        spacer(),
    ))
    .align(Align::Start)
    .padding_h(8.0)
    .width(280.0)
    .fill_height()
}

fn chat(s: &S) -> View<S> {
    let header = hstack((text(CHATS[s.selected]).font_size(15.0).weight(Weight::Semibold).lines(1).fill_width(),))
        .padding_h(20.0)
        .height(48.0)
        .fill_width();
    // A card that cuts a picture bigger than itself to its rounded corners.
    let card = zstack((
        rect().size(900.0, 400.0).gradient(Color::hex(0xFF7A59), Color::hex(0x5E5CE6), 120.0),
        text("clip(): the picture and this caption stay inside the corners")
            .font_size(13.0)
            .color(Color::WHITE)
            .padding_h(14.0)
            .padding_v(10.0)
            .fill_width()
            .background(Color::BLACK.alpha(0.35))
            .place(Align::Start, Align::End),
    ))
    .fill_width()
    .height(150.0)
    .radius(22.0)
    .clip();
    let column = vstack((
        text("Это соответствует документации Slint, egui и wgpu.").fill_width(),
        text(
            "Есть один существенный нюанс: «один canvas» удобен для отрисовки, но браузер перестаёт понимать \
             содержимое интерфейса. Колонка не шире 560 pt, сколько бы места ни было вокруг.",
        )
        .fill_width(),
        card,
        hstack((
            button("Свернуть", |s: &mut S| s.sidebar = !s.sidebar),
            text("Курсор над полем — I-beam").font_size(13.0).color(FAINT),
        ))
        .spacing(14.0),
        text_field("", "Спросите что-нибудь…", |_: &mut S, _| {}),
    ))
    .spacing(16.0)
    .align(Align::Start)
    .fill_width()
    .max_width(560.0);
    vstack((header, zstack(column).padding(20.0).place(Align::Center, Align::Start).fill_width(), spacer())).fill()
}

fn view(s: &S, _env: &Env) -> View<S> {
    let panel = hstack((
        // Collapses to 0 and back; its content slides under the moving edge.
        sidebar(s).width(if s.sidebar { 280.0 } else { 0.0 }).background(SIDEBAR).animate(),
        chat(s),
    ))
    .fill()
    .background(PANEL)
    .radius(14.0)
    .border(1.0, Color::WHITE.alpha(0.08))
    .clip();
    hstack((rail(s), panel)).padding_ltrb(0.0, 10.0, 10.0, 10.0).fill()
}

fn main() {
    let state = S { sidebar: true, selected: 0 };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--screenshot") {
            let num = |k: usize, d: f32| args.get(i + k).and_then(|t| t.parse().ok()).unwrap_or(d);
            let (frames, toggle_at) = (num(2, 1.0) as u32, num(3, -1.0));
            let mouse = (num(4, -1.0), num(5, -1.0));
            let width = num(6, 1000.0) as u32;
            let mut ui = ruru::Ui::new(state, view);
            ui.background = Color::hex(0x1C1C1E);
            let before = |ui: &mut ruru::Ui<S>, n: u32| {
                if n > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
                if n == 1 && mouse.0 >= 0.0 {
                    ui.pointer_move(mouse);
                }
                if n as f32 == toggle_at {
                    ui.state.sidebar = !ui.state.sidebar;
                }
            };
            ruru::headless::render_png_after(&mut ui, width, 640, 2.0, frames, before, &args[i + 1]);
            return;
        }
    }
    App::new(state, view).title("ruru panels").size(1000.0, 640.0).background(Color::hex(0x1C1C1E)).run();
}
