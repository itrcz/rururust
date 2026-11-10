//! # ruru
//!
//! A tiny, fast, GPU-accelerated UI framework with real glass materials.
//! One codebase runs natively (macOS, Windows, Linux, iOS, Android) and in the
//! browser on a `<canvas>` via WebGPU/WebGL2.
//!
//! ```no_run
//! use ruru::prelude::*;
//!
//! struct Counter { n: i32 }
//!
//! fn view(s: &Counter) -> View<Counter> {
//!     vstack((
//!         text(format!("{}", s.n)).font_size(64.0).weight(Weight::Bold),
//!         button("Add one", |s: &mut Counter| s.n += 1),
//!     ))
//!     .spacing(16.0)
//!     .fill()
//! }
//!
//! fn main() {
//!     App::new(Counter { n: 0 }, view).title("Counter").run();
//! }
//! ```

mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
mod layout;
mod render;
mod text;
mod types;
mod ui;
mod view;

pub use app::App;
pub use render::Renderer;
pub use text::TextSystem;
pub use types::{Color, Rect};
pub use ui::{DrawList, KeyInput, Ui};
pub use view::{
    button, hstack, image, picker, rect, scroll, slider, spacer, tab_bar, text, text_field, toggle, vstack, zstack,
    Align, Children, Dim, Fill, Glass, Image, Shadow, View, Weight,
};

pub mod prelude {
    pub use crate::{
        button, hstack, image, picker, rect, scroll, slider, spacer, tab_bar, text, text_field, toggle, vstack,
        zstack, Align, App, Color, Fill, Glass, Image, View, Weight,
    };
}
