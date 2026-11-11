//! The look of the built-in widgets in one plain struct: colors, type, control metrics,
//! shadows and motion. Every color and size a widget uses without being told lives here.
//!
//! ```ignore
//! App::new(state, view).theme(Theme::light().accent(Color::hex(0x4D81EF)));
//! fn view(s: &State, env: &Env) -> View<State> {
//!     let t = &env.theme;
//!     text("Title").style(t.text.title).color(t.color.text_muted)
//! }
//! ```

use crate::types::Color;
use crate::ui::popup::MenuStyle;
use crate::view::{hstack, text, Glass, Kind, Role, Shadow, SliderStyle, TextRole, View, Weight};
use std::rc::Rc;

/// Colors and metrics of the built-in widgets. Start from [`Theme::dark`] or [`Theme::light`]
/// and change the fields you need; a modifier on a view (`.background()`, `.color()`,
/// `.padding()`…) always wins over the theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub color: Palette,
    /// Spacing scale for the app's own layouts: 2 4 8 12 16 24 32.
    pub space: [f32; 7],
    /// Corner radius scale for the app's own views.
    pub radius: Radii,
    pub text: TypeScale,
    pub control: Controls,
    pub shadow: Shadows,
    pub motion: Motion,
    /// Material of the floating tab bar.
    pub glass: Glass,
    /// Material of the `picker` drop-down.
    pub glass_menu: Glass,
    /// The look of every `menu` that has no `.menu_style()`; `None` derives it from the
    /// palette ([`MenuStyle::from`]).
    pub menu: Option<MenuStyle>,
    /// The look of every `slider` that has no `.slider_style()`; `None` derives it.
    pub slider: Option<SliderStyle>,
    /// Typeface of all text; set it with [`Theme::font`], which also fits the headings' tracking.
    pub font: Font,
    /// Typeface of code (`Span::code`, code blocks).
    pub mono: Font,
}

/// Which typeface the text uses ([`Theme::font`], [`Theme::mono`]).
///
/// A font that is not there (any system font on the web, a name the machine lacks) quietly
/// falls back to the bundled one, so the layout never depends on it being found.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Font {
    /// The fonts compiled in: Inter (text) and JetBrains Mono (code, with the `markdown`
    /// feature). The same pixels on every platform and the web.
    #[default]
    Bundled,
    /// The platform's UI font: San Francisco on macOS (and SF Mono for code), Segoe UI
    /// Variable / Segoe UI on Windows (Cascadia Mono / Consolas), a common sans on Linux
    /// (Cantarell, Ubuntu, Noto Sans, DejaVu Sans…).
    ///
    /// On macOS the system font is a variable font with an optical-size axis that the
    /// shaper does not drive: every size is drawn with the 28 pt (display) design. Letter
    /// spacing is opened up to AppKit's line widths (within ~0.5 %), but the x-height stays
    /// ~3.5 % lower and stems a little lighter than in a native app.
    System,
    /// A family by name, among the system fonts: `Font::Named("Helvetica Neue".into())`.
    Named(String),
}

/// Semantic colors. Built-in widgets use them; read them in your own views through
/// `env.theme.color`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Window background (the clear color unless `App::background` is set).
    pub bg: Color,
    /// Grouped content, panels and cards one step above `bg`.
    pub surface: Color,
    /// One more step: cards on a surface.
    pub raised: Color,
    /// Menus and tooltips.
    pub popover: Color,
    /// Default text and icon color.
    pub text: Color,
    /// Secondary text: captions, hints.
    pub text_muted: Color,
    /// Hairlines around popovers and panels.
    pub border: Color,
    /// Separators between rows.
    pub divider: Color,
    pub accent: Color,
    /// Text and icons on an `accent` fill.
    pub on_accent: Color,
    /// Background of text fields and pickers.
    pub fill: Color,
    /// Stronger fill: the empty track of a toggle or slider.
    pub fill_strong: Color,
    /// Highlight of a row under the pointer.
    pub hover: Color,
    /// Highlight of a pressed row.
    pub pressed: Color,
    /// Selected text.
    pub selection: Color,
    /// Focus ring of a text field.
    pub focus: Color,
    /// Dims the screen under a modal, sheet or drawer.
    pub scrim: Color,
    pub link: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
}

/// Corner radius scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub xs: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
    pub xl: f32,
}

/// One text style: `text("…").style(env.theme.text.body)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    /// Multiple of the size.
    pub line_height: f32,
    pub weight: Weight,
    /// Extra space between letters in em.
    pub tracking: f32,
}

/// Text styles, iOS names and sizes. `text()` uses `body`, a button label `headline`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeScale {
    pub large_title: TextStyle,
    pub title: TextStyle,
    pub headline: TextStyle,
    pub body: TextStyle,
    pub subhead: TextStyle,
    pub footnote: TextStyle,
    pub caption: TextStyle,
    pub caption2: TextStyle,
}

/// Metrics (and a few control-only colors) of the built-in controls. Paddings are (x, y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    pub button_padding: (f32, f32),
    pub button_radius: f32,
    /// Laid over a hovered button.
    pub button_hover: Color,
    pub field_padding: (f32, f32),
    /// Text fields and pickers.
    pub field_radius: f32,
    pub picker_padding: (f32, f32),
    /// Rows of the picker drop-down and its corner radius.
    pub picker_row: f32,
    pub picker_radius: f32,
    /// Width and height of a toggle.
    pub toggle: (f32, f32),
    /// Track of a toggle that is on; `None` = `color.accent`.
    pub toggle_on: Option<Color>,
    /// Toggle and slider knob.
    pub knob: Color,
    pub slider_track: f32,
    pub slider_knob: f32,
    pub tab_height: f32,
    /// Inset of the tab drop from the bar edge.
    pub tab_padding: f32,
    /// Added to the widest label to get a tab's width.
    pub tab_extra: f32,
    pub tab_radius: f32,
    /// Tint of the glass drop on the selected tab.
    pub tab_drop: Color,
    /// Hairline around popovers and tooltips.
    pub border: f32,
    /// Width of the text field focus ring.
    pub focus_ring: f32,
    /// Scroll bar thumb: thin, and wide while the pointer is over it.
    pub scrollbar: (f32, f32),
    pub tip_padding: (f32, f32),
    pub tip_radius: f32,
}

/// Shadow presets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadows {
    /// Under a toggle or slider knob.
    pub control: Shadow,
    /// Under a dragged element (the tab drop while held).
    pub lift: Shadow,
    /// Under a menu.
    pub popover: Shadow,
    /// Under a tooltip.
    pub tip: Shadow,
    /// Under floating bars and the picker drop-down.
    pub bar: Shadow,
}

/// Springs as (stiffness, damping), and the tooltip delay in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// Lively, a hint of bounce: toggles, the tab drop, menus.
    pub bouncy: (f32, f32),
    /// Quick, no overshoot: press feedback, fades.
    pub crisp: (f32, f32),
    /// Unhurried, no overshoot: `.animate()` sizes.
    pub smooth: (f32, f32),
    pub tip_delay: f32,
}

const fn ts(size: f32, weight: Weight) -> TextStyle {
    TextStyle { size, line_height: 1.3, weight, tracking: 0.0 }
}

/// A heading style: Inter at large sizes wants tighter tracking (its Display cut, which
/// the bundled font does not have).
fn heading(size: f32, weight: Weight) -> TextStyle {
    TextStyle { tracking: inter_tracking(size), ..ts(size, weight) }
}

/// Letter spacing (em) that Inter's designer recommends for a size ("dynamic metrics":
/// `-0.0223 + 0.185·e^(-0.1745·size)`): about +0.5 % at 11 pt, 0 at 12, -1.3 % at 17,
/// -1.8 % at 22, -2.2 % from 34 up. The theme applies it to titles (20 pt and up); use it
/// for your own large text: `.letter_spacing(inter_tracking(48.0))`.
pub fn inter_tracking(size: f32) -> f32 {
    -0.0223 + 0.185 * (-0.1745 * size).exp()
}

const GRAY_FILL: Color = Color::hex(0x787880);

impl Theme {
    /// iOS dark: black background, system blue accent, green toggles.
    pub fn dark() -> Theme {
        Theme {
            dark: true,
            color: Palette {
                bg: Color::BLACK,
                surface: Color::hex(0x1C1C1E),
                raised: Color::hex(0x2C2C2E),
                popover: Color::hex(0x2C2C2E),
                text: Color::WHITE,
                text_muted: Color::hex(0xEBEBF5).alpha(0.6),
                border: Color::WHITE.alpha(0.12),
                divider: Color::WHITE.alpha(0.09),
                accent: Color::hex(0x0A84FF),
                on_accent: Color::WHITE,
                fill: GRAY_FILL.alpha(0.24),
                fill_strong: GRAY_FILL.alpha(0.36),
                hover: Color::WHITE.alpha(0.06),
                pressed: Color::WHITE.alpha(0.16),
                selection: Color::hex(0x0A84FF).alpha(0.3),
                focus: Color::hex(0x0A84FF).alpha(0.55),
                scrim: Color::BLACK.alpha(0.45),
                link: Color::hex(0x4DA3FF),
                danger: Color::hex(0xFF453A),
                success: Color::hex(0x30D158),
                warning: Color::hex(0xFF9F0A),
            },
            space: [2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0],
            radius: Radii { xs: 4.0, s: 6.0, m: 8.0, l: 12.0, xl: 22.0 },
            text: TypeScale {
                large_title: heading(34.0, Weight::Bold),
                title: heading(22.0, Weight::Bold),
                headline: ts(17.0, Weight::Semibold),
                body: ts(17.0, Weight::Regular),
                subhead: ts(15.0, Weight::Regular),
                footnote: ts(13.0, Weight::Regular),
                caption: ts(12.0, Weight::Regular),
                caption2: ts(11.0, Weight::Regular),
            },
            control: Controls {
                button_padding: (20.0, 13.0),
                button_radius: 14.0,
                button_hover: Color::WHITE.alpha(0.14),
                field_padding: (14.0, 12.0),
                field_radius: 12.0,
                picker_padding: (14.0, 10.0),
                picker_row: 44.0,
                picker_radius: 18.0,
                toggle: (51.0, 31.0),
                toggle_on: Some(Color::hex(0x30D158)),
                knob: Color::WHITE,
                slider_track: 6.0,
                slider_knob: 28.0,
                tab_height: 56.0,
                tab_padding: 6.0,
                tab_extra: 40.0,
                tab_radius: 28.0,
                tab_drop: Color::WHITE.alpha(0.13),
                border: 0.5,
                focus_ring: 1.5,
                scrollbar: (5.0, 9.0),
                tip_padding: (9.0, 5.0),
                tip_radius: 9.0,
            },
            shadow: Shadows {
                control: Shadow::new(Color::BLACK.alpha(0.25), 8.0, 3.0),
                lift: Shadow::new(Color::BLACK.alpha(0.22), 18.0, 5.0),
                popover: Shadow::new(Color::BLACK.alpha(0.4), 28.0, 8.0),
                tip: Shadow::new(Color::BLACK.alpha(0.3), 14.0, 4.0),
                bar: Shadow::new(Color::BLACK.alpha(0.35), 30.0, 10.0),
            },
            motion: Motion { bouncy: (380.0, 28.0), crisp: (900.0, 60.0), smooth: (260.0, 32.25), tip_delay: 0.5 },
            glass: Glass::regular(),
            glass_menu: Glass::frosted(),
            menu: None,
            slider: None,
            font: Font::Bundled,
            mono: Font::Bundled,
        }
    }

    /// iOS light: white background, darker system colors, lighter shadows.
    pub fn light() -> Theme {
        let d = Theme::dark();
        let accent = Color::hex(0x007AFF);
        Theme {
            dark: false,
            color: Palette {
                bg: Color::WHITE,
                surface: Color::hex(0xF2F2F7),
                raised: Color::WHITE,
                popover: Color::hex(0xF7F7F7),
                text: Color::BLACK,
                text_muted: Color::hex(0x3C3C43).alpha(0.6),
                border: Color::BLACK.alpha(0.12),
                divider: Color::BLACK.alpha(0.09),
                accent,
                on_accent: Color::WHITE,
                fill: GRAY_FILL.alpha(0.12),
                fill_strong: GRAY_FILL.alpha(0.2),
                hover: Color::BLACK.alpha(0.04),
                pressed: Color::BLACK.alpha(0.08),
                selection: accent.alpha(0.25),
                focus: accent.alpha(0.55),
                scrim: Color::BLACK.alpha(0.3),
                link: Color::hex(0x0066CC),
                danger: Color::hex(0xFF3B30),
                success: Color::hex(0x34C759),
                warning: Color::hex(0xFF9500),
            },
            control: Controls {
                toggle_on: Some(Color::hex(0x34C759)),
                tab_drop: Color::BLACK.alpha(0.06),
                ..d.control
            },
            shadow: Shadows {
                control: Shadow::new(Color::BLACK.alpha(0.2), 8.0, 3.0),
                lift: Shadow::new(Color::BLACK.alpha(0.15), 18.0, 5.0),
                popover: Shadow::new(Color::BLACK.alpha(0.18), 28.0, 8.0),
                tip: Shadow::new(Color::BLACK.alpha(0.15), 14.0, 4.0),
                bar: Shadow::new(Color::BLACK.alpha(0.14), 30.0, 10.0),
            },
            glass: Glass::light(),
            glass_menu: Glass { tint: Color::WHITE.alpha(0.6), ..Glass::frosted() },
            ..d
        }
    }

    /// The same theme with another accent: buttons, sliders, toggles, the focus ring, the
    /// caret and selected text follow it.
    pub fn accent(mut self, c: Color) -> Theme {
        self.color.accent = c;
        self.color.selection = c.alpha(self.color.selection.a);
        self.color.focus = c.alpha(self.color.focus.a);
        self.control.toggle_on = None;
        self
    }

    /// The same theme in another typeface: `Theme::dark().font(Font::System)`. Title tracking
    /// is tuned for Inter, so with any other font it goes back to the font's own (0).
    pub fn font(mut self, f: Font) -> Theme {
        let bundled = f == Font::Bundled;
        for st in [&mut self.text.large_title, &mut self.text.title] {
            st.tracking = if bundled { inter_tracking(st.size) } else { 0.0 };
        }
        self.font = f;
        self
    }

    /// The same theme with another typeface for code.
    pub fn mono(mut self, f: Font) -> Theme {
        self.mono = f;
        self
    }

    /// Toggle track when on.
    pub fn toggle_on(&self) -> Color {
        self.control.toggle_on.unwrap_or(self.color.accent)
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::dark()
    }
}

impl From<&Theme> for MenuStyle {
    /// The menu look the theme gives every `menu` without `.menu_style()`.
    fn from(t: &Theme) -> MenuStyle {
        if let Some(m) = &t.menu {
            return m.clone();
        }
        let c = &t.color;
        MenuStyle {
            background: c.popover,
            glass: None,
            radius: 12.0,
            border: Some((t.control.border, c.border)),
            shadow: Some(t.shadow.popover),
            ambient: None,
            rim: None,
            padding: 5.0,
            font_size: t.text.footnote.size,
            row_height: 28.0,
            row_radius: 7.0,
            highlight_inset: 0.0,
            row_padding: 10.0,
            icon_size: 16.0,
            icon_gap: 8.0,
            text: c.text.alpha(0.92),
            dim: c.text.alpha(0.42),
            disabled: c.text.alpha(0.28),
            highlight: c.text.alpha(0.09),
            highlight_text: None,
            separator: c.divider,
            separator_height: 11.0,
            shortcut_size: None,
            shortcut_color: None,
            section_size: None,
            section_weight: Weight::Semibold,
            section_color: None,
            section_height: None,
            section_gap: 0.0,
            min_width: 180.0,
            max_width: 420.0,
        }
    }
}

impl Default for MenuStyle {
    /// The default (dark) theme's menu. In a light app start from `MenuStyle::from(&env.theme)`.
    fn default() -> Self {
        MenuStyle::from(&Theme::default())
    }
}

impl From<&Theme> for SliderStyle {
    /// The slider look the theme gives every `slider` without `.slider_style()`.
    fn from(t: &Theme) -> SliderStyle {
        if let Some(s) = t.slider {
            return s;
        }
        SliderStyle {
            track_height: t.control.slider_track,
            track: t.color.fill_strong,
            track_border: None,
            fill: t.color.accent,
            knob: t.control.knob,
            knob_size: t.control.slider_knob,
            knob_shadow: Some(t.shadow.control),
            tick_size: 4.0,
            tick_on: t.color.on_accent.alpha(0.6),
            tick_off: None,
        }
    }
}

impl Default for SliderStyle {
    /// The default (dark) theme's slider. In a light app start from `SliderStyle::from(&env.theme)`.
    fn default() -> Self {
        SliderStyle::from(&Theme::default())
    }
}

/// The theme as the frame uses it: the styles derived from it are built once, not per view.
pub(crate) struct Look {
    pub theme: Rc<Theme>,
    pub menu: Rc<MenuStyle>,
    pub slider: SliderStyle,
}

impl Look {
    pub fn new(theme: Rc<Theme>) -> Look {
        Look { menu: Rc::new(MenuStyle::from(&*theme)), slider: SliderStyle::from(&*theme), theme }
    }
}

/// The default tooltip: a plate with the label and an optional key chip.
fn tooltip<S>(t: &Theme, label: String, keys: Option<String>) -> View<S> {
    let c = &t.color;
    let label = text(label).style(TextStyle { weight: Weight::Medium, ..t.text.caption });
    let row = match keys {
        Some(k) => hstack((
            label,
            text(k)
                .style(TextStyle { weight: Weight::Medium, ..t.text.caption2 })
                .padding_xy(5.0, 1.5)
                .background(c.text.alpha(0.14))
                .radius(5.0),
        ))
        .spacing(7.0),
        None => hstack(label),
    };
    row.padding_xy(t.control.tip_padding.0, t.control.tip_padding.1)
        .background(c.popover)
        .border(t.control.border, c.border)
        .radius(t.control.tip_radius)
        .shadow_with(t.shadow.tip)
        .color(c.text.alpha(0.92))
}

/// Fills in what the built-in widgets of a freshly built tree left to the theme: their
/// paddings, fills, radii and text styles, unless a modifier set them. Runs once on the tree
/// from `view()` and on every lazy list row as it is built; layout and paint then read plain
/// values.
pub(crate) fn apply<S>(v: &mut View<S>, look: &Look) {
    use crate::view::{PAD_B, PAD_L, PAD_R, PAD_T, SET_RADIUS};
    let t = &*look.theme;
    let st = &mut v.style;
    if st.role != Role::None {
        let k = &t.control;
        let (pad, radius) = match st.role {
            Role::Button => {
                st.bg.get_or_insert(t.color.accent.into());
                st.fg.get_or_insert(t.color.on_accent);
                st.hover.get_or_insert(k.button_hover);
                (k.button_padding, k.button_radius)
            }
            Role::Field | Role::Picker => {
                st.bg.get_or_insert(t.color.fill.into());
                let pad = if st.role == Role::Field { k.field_padding } else { k.picker_padding };
                (pad, k.field_radius)
            }
            Role::Tabs => {
                st.glass.get_or_insert(t.glass);
                st.shadow[0].get_or_insert(t.shadow.bar);
                ((0.0, 0.0), k.tab_radius)
            }
            Role::None => ((0.0, 0.0), 0.0),
        };
        for (bit, i, p) in [(PAD_L, 0, pad.0), (PAD_T, 1, pad.1), (PAD_R, 2, pad.0), (PAD_B, 3, pad.1)] {
            if st.set & bit == 0 {
                st.padding[i] = p;
            }
        }
        if st.set & SET_RADIUS == 0 {
            st.radius = radius;
        }
    }
    match &mut v.kind {
        Kind::Text(d) => d.theme(match d.role {
            TextRole::Body => &t.text.body,
            TextRole::Headline => &t.text.headline,
        }),
        Kind::TextField(f) => f.theme(&t.text.body),
        Kind::Stack { children, .. } => children.iter_mut().for_each(|c| apply(c, look)),
        Kind::Scroll { child, .. } | Kind::Overlay { child, .. } => apply(child, look),
        _ => {}
    }
    if let Some(x) = &mut v.extra {
        if let Some((label, keys)) = x.tip_text.take() {
            x.tip = Some(Box::new(tooltip(t, label, keys)));
        }
        if let Some(tip) = &mut x.tip {
            apply(tip, look);
        }
        if let Some(p) = &mut x.pop {
            apply(&mut p.content, look);
        }
    }
}
