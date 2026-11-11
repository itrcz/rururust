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
mod theme;
mod types;
mod ui;
mod view;

#[cfg(feature = "app")]
pub use app::{App, Titlebar};
#[cfg(feature = "renderer")]
pub use render::Renderer;
pub use task::{link, set_spawner, set_waker, spawn, Link, Task};
#[cfg(any(not(target_arch = "wasm32"), feature = "app"))]
pub use task::sleep;
pub use text::TextSystem;
pub use theme::{inter_tracking, Controls, Font, Motion, Palette, Radii, Shadows, TextStyle, Theme, TypeScale};
pub use types::{Color, Rect};
pub use ui::{after, copy_text, Cmd, DrawList, KeyInput, Quad, TextCmd, Ui, WheelPhase, WindowRequest};
pub use view::{
    button, hstack, icon, image, list, modal, picker, rect, scroll, sheet, slider, snackbar, spacer, tab_bar, text,
    text_field, toggle, vstack, zstack, Align, Children, Cursor, Dim, Fill, Glass, Image, Indicator, Shadow, SliderStyle,
    Truncate, View, Weight,
};
pub use view::{drawer, menu, rich_text, Env, Mods, SizeClass, Span, ViewFn};
pub use ui::popup::{MenuItem, MenuStyle, Placement, Shortcut, ShortcutKey, Side};
#[cfg(feature = "markdown")]
mod markdown;
#[cfg(feature = "markdown")]
pub use markdown::{markdown, markdown_styled, markdown_styled_with, markdown_with, CodeBlockStyle, MarkdownStyle, QuoteStyle, TableStyle};

pub mod prelude {
    #[cfg(feature = "app")]
    pub use crate::{App, Titlebar};
    pub use crate::{
        after, button, copy_text, hstack, icon, image, list, modal, picker, rect, scroll, sheet, slider, snackbar, spacer,
        tab_bar, text, text_field, toggle, vstack, zstack, Align, Color, Cursor, Fill, Glass, Image, Indicator, Shadow,
        SliderStyle, Truncate, View, Weight,
    };
    #[cfg(feature = "markdown")]
    pub use crate::{markdown, markdown_styled, markdown_with, MarkdownStyle};
    pub use crate::{drawer, rich_text, Env, Font, SizeClass, Span, TextStyle, Theme};
    pub use crate::{menu, MenuItem, MenuStyle, Placement, Shortcut};
    pub use crate::{link, spawn, Link, Task};
    #[cfg(any(not(target_arch = "wasm32"), feature = "app"))]
    pub use crate::sleep;
}
