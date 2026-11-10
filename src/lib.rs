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

#[cfg(feature = "app")]
mod app;
#[cfg(all(feature = "headless", not(target_arch = "wasm32")))]
pub mod headless;
mod layout;
#[cfg(feature = "renderer")]
mod render;
mod task;
mod text;
mod types;
mod ui;
mod view;

#[cfg(feature = "app")]
pub use app::App;
#[cfg(feature = "renderer")]
pub use render::Renderer;
pub use task::{link, set_spawner, set_waker, spawn, Link, Task};
#[cfg(any(not(target_arch = "wasm32"), feature = "app"))]
pub use task::sleep;
pub use text::TextSystem;
pub use types::{Color, Rect};
pub use ui::{after, DrawList, KeyInput, Ui, WheelPhase};
pub use view::{
    button, hstack, icon, image, list, modal, picker, rect, scroll, sheet, slider, snackbar, spacer, tab_bar, text,
    text_field, toggle, vstack, zstack, Align, Children, Dim, Fill, Glass, Image, Indicator, Shadow, View, Weight,
};
pub use view::{drawer, rich_text, Env, SizeClass, Span, ViewFn};
#[cfg(feature = "markdown")]
mod markdown;
#[cfg(feature = "markdown")]
pub use markdown::{markdown, markdown_with};

pub mod prelude {
    #[cfg(feature = "app")]
    pub use crate::App;
    pub use crate::{
        after, button, hstack, icon, image, list, modal, picker, rect, scroll, sheet, slider, snackbar, spacer,
        tab_bar, text, text_field, toggle, vstack, zstack, Align, Color, Fill, Glass, Image, Indicator, View, Weight,
    };
    #[cfg(feature = "markdown")]
    pub use crate::{markdown, markdown_with};
    pub use crate::{drawer, rich_text, Env, SizeClass, Span};
    pub use crate::{link, spawn, Link, Task};
    #[cfg(any(not(target_arch = "wasm32"), feature = "app"))]
    pub use crate::sleep;
}
