//! Regression check for text rendering under glyph-atlas pressure: opens and closes a
//! picker menu (its opening animation rasterizes every label at a new size each frame).
//!   cargo run --release --example stress -- out.png
use ruru::prelude::*;

struct S {
    city: usize,
}
const CITIES: [&str; 5] = ["Lisbon", "Tokyo", "Reykjavik", "Buenos Aires", "Cape Town"];

fn view(s: &S) -> View<S> {
    vstack((
        text(format!("Hello from {}!", CITIES[s.city])).font_size(20.0).weight(Weight::Semibold),
        picker(&CITIES, s.city, |s: &mut S, i| s.city = i).width(300.0),
        button("Clear", |_: &mut S| {}).width(300.0),
        text("The quick brown fox jumps over the lazy dog 0123456789").font_size(13.0),
        text("Sphinx of black quartz, judge my vow").font_size(28.0).weight(Weight::Bold),
    ))
    .spacing(14.0)
    .padding(20.0)
    .fill()
}

fn main() {
    let out = std::env::args().nth(1).expect("output path");
    let mut ui = ruru::Ui::new(S { city: 0 }, view);
    ui.show_fps = true;
    let picker = (210.0, 80.0);
    ruru::headless::render_png_after(
        &mut ui,
        420,
        600,
        2.0,
        140,
        |ui, i| {
            std::thread::sleep(std::time::Duration::from_millis(8));
            match i % 45 {
                2 => {
                    ui.pointer_down(picker, false);
                    ui.pointer_up(picker);
                }
                30 => {
                    let row = (120.0, 114.0 + 44.0 * (1.5 + (i / 45) as f32));
                    ui.pointer_down(row, false);
                    ui.pointer_up(row);
                }
                _ => {}
            }
        },
        out,
    );
    println!("city = {}", ui.state.city);
}
