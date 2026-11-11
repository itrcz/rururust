//! The window/event-loop runner. Same code path on desktop, mobile and web.

use crate::render::Renderer;
use crate::theme::Theme;
use crate::types::Color;
use crate::ui::{KeyInput, Ui, WheelPhase, WindowRequest};
use crate::view::{Cursor, Mods, ViewFn};
use crate::ShortcutKey;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::dpi::{LogicalPosition, LogicalSize as Size};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorIcon, Window, WindowId};

/// Who draws the window's title bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Titlebar {
    /// The system's title bar above the content.
    #[default]
    Standard,
    /// The content fills the whole window and runs under a transparent title bar; the window
    /// buttons stay where they are (macOS traffic lights). The view learns the zone from
    /// [`Env::titlebar`](crate::Env::titlebar) and [`Env::window_buttons`](crate::Env::window_buttons)
    /// and moves the window with [`View::window_drag`](crate::View::window_drag).
    /// Elsewhere for now: the standard title bar, and both insets are 0.
    Overlay,
}

/// What shows through a window with [`App::blur`]: on macOS the system materials, the same
/// ones native sidebars, HUDs and menus use (an `NSVisualEffectView` under the content, so the
/// blur, the window's rounded corners and its shadow share one shape). Elsewhere every material
/// is the platform's one blur. The app's [`background`](App::background) tints it from above;
/// the material itself is drawn light or dark to match the theme.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Backdrop {
    /// The sidebar of Finder or Mail: a light frost, the desktop's colours come through.
    #[default]
    Sidebar,
    /// The material behind window content in system apps; more opaque than `Sidebar`.
    UnderWindow,
    /// The plain window background, nearly opaque.
    Window,
    /// Behind text and lists; nearly opaque.
    Content,
    Titlebar,
    HeaderView,
    Menu,
    Popover,
    Sheet,
    /// Heads-up windows: dark and strongly blurred.
    Hud,
    FullScreenUi,
    /// The legacy window-server blur with this radius in pt (what `with_blur` in winit sets, at
    /// 80). Its corners are rounder than the window's on macOS 26+, so a sliver of unblurred
    /// desktop shows in them (decision 006); prefer a material.
    Blur(f32),
}

// AppKit's title bar is 28 pt tall; the traffic lights end at about 70 pt, plus a gap.
#[cfg(target_os = "macos")]
const MAC_TITLEBAR: f32 = 28.0;
#[cfg(target_os = "macos")]
const MAC_BUTTONS: f32 = 78.0;

#[cfg(not(target_arch = "wasm32"))]
#[path = "companion.rs"]
mod companion;

pub struct App<S> {
    ui: Ui<S>,
    #[cfg(not(target_arch = "wasm32"))]
    companions: Vec<Box<dyn companion::Pending>>,
    title: String,
    size: (f32, f32),
    min_size: Option<(f32, f32)>,
    max_size: Option<(f32, f32)>,
    transparent: bool,
    backdrop: Option<Backdrop>,
    titlebar: Titlebar,
    /// Height of the title bar strip the traffic lights are centred in (`titlebar_height`).
    titlebar_height: Option<f32>,
    /// Light and dark theme to follow the system appearance with (`theme_auto`).
    auto: Option<(Theme, Theme)>,
    file_drop: Option<Box<dyn Fn(&mut S, std::path::PathBuf)>>,
    file_hover: Option<Box<dyn Fn(&mut S, bool)>>,
}

impl<S: 'static> App<S> {
    /// `view` is called with the current state whenever the UI needs to be rebuilt.
    pub fn new<M>(state: S, view: impl ViewFn<S, M>) -> Self {
        App {
            ui: Ui::new(state, view),
            #[cfg(not(target_arch = "wasm32"))]
            companions: Vec::new(),
            title: "ruru".into(),
            size: (420.0, 760.0),
            min_size: None,
            max_size: None,
            transparent: false,
            backdrop: None,
            titlebar: Titlebar::Standard,
            titlebar_height: None,
            auto: None,
            file_drop: None,
            file_hover: None,
        }
    }
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    /// Receives each native file dropped onto this window. The application decides
    /// whether to copy, stage, reject, or open the file; the runner never reads it.
    pub fn on_file_drop(mut self, handler:impl Fn(&mut S,std::path::PathBuf)+'static)->Self {
        self.file_drop=Some(Box::new(handler));self
    }
    /// Native file-drag highlight. `false` follows a drop or a cancelled drag.
    pub fn on_file_hover(mut self, handler:impl Fn(&mut S,bool)+'static)->Self {
        self.file_hover=Some(Box::new(handler));self
    }
    fn file_event(&mut self,event:&WindowEvent)->Option<bool> {
        match event {
            WindowEvent::HoveredFile(_) => Some(if let Some(f)=&self.file_hover {f(&mut self.ui.state,true);true}else{false}),
            WindowEvent::HoveredFileCancelled => Some(if let Some(f)=&self.file_hover {f(&mut self.ui.state,false);true}else{false}),
            WindowEvent::DroppedFile(path) => {
                let mut changed=false;
                if let Some(f)=&self.file_hover {f(&mut self.ui.state,false);changed=true;}
                if let Some(f)=&self.file_drop {f(&mut self.ui.state,path.clone());changed=true;}
                Some(changed)
            }
            _=>None,
        }
    }
    /// Initial window size in points (desktop only).
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.size = (w, h);
        self
    }
    /// The smallest the user can make the window, in points (desktop only): below the size the
    /// layout was made for it would only squeeze. The view can still adapt above it, e.g.
    /// collapse a side panel when [`Env::width`](crate::Env::width) gets narrow.
    pub fn min_size(mut self, w: f32, h: f32) -> Self {
        self.min_size = Some((w, h));
        self
    }
    /// The largest the user can make the window, in points (desktop only).
    pub fn max_size(mut self, w: f32, h: f32) -> Self {
        self.max_size = Some((w, h));
        self
    }
    /// Window clear color instead of the theme's `color.bg`. With
    /// [`transparent`](Self::transparent) its alpha tints what is behind the window;
    /// otherwise alpha is ignored.
    pub fn background(mut self, c: Color) -> Self {
        self.ui.background = Some(c);
        self
    }

    /// Colors and metrics of the built-in widgets (default: [`Theme::dark`]).
    /// `App::new(s, view).theme(Theme::light().accent(Color::hex(0x4D81EF)))`
    pub fn theme(mut self, theme: Theme) -> Self {
        self.ui.set_theme(theme);
        self
    }

    /// The theme derived from the state before every frame: a handler switches it by
    /// changing the state. `.theme_with(|s: &State| if s.dark { Theme::dark() } else { Theme::light() })`
    pub fn theme_with(mut self, f: impl Fn(&S) -> Theme + 'static) -> Self {
        self.ui.theme_with(f);
        self
    }
    /// Derives the application's theme from state and live operating-system appearance.
    /// Explicit light/dark choices may ignore the second argument; System follows it.
    pub fn theme_with_system(mut self,f:impl Fn(&S,bool)->Theme+'static)->Self {
        self.ui.theme_with_system(f);self
    }

    /// Follows the system appearance: `light` or `dark`, switched when the user changes it.
    pub fn theme_auto(mut self, light: Theme, dark: Theme) -> Self {
        self.auto = Some((light, dark));
        self
    }

    /// A window with an alpha channel: where the view and the background leave alpha below 1,
    /// the desktop shows through. Desktop only; on the web the canvas stays opaque.
    pub fn transparent(mut self, on: bool) -> Self {
        self.transparent = on;
        self
    }

    /// Blurs the desktop behind the window (implies [`transparent`](Self::transparent)).
    /// macOS: a system material ([`Backdrop::Sidebar`] unless [`backdrop`](Self::backdrop) picks
    /// another); Windows: acrylic / blur-behind where the OS has it; Linux: KDE's blur protocol.
    /// Where the platform has none it is just transparent, so give
    /// [`background`](Self::background) enough alpha to stay readable without it.
    pub fn blur(mut self, on: bool) -> Self {
        self.backdrop = if on { Some(self.backdrop.unwrap_or_default()) } else { None };
        self.transparent |= on;
        self
    }

    /// Blurs the desktop behind the window with this material (implies [`blur`](Self::blur)).
    ///
    /// ```no_run
    /// # use ruru::prelude::*;
    /// App::new((), |_: &(), _: &Env| text("hi")).backdrop(Backdrop::Hud).run();
    /// ```
    pub fn backdrop(mut self, b: Backdrop) -> Self {
        self.backdrop = Some(b);
        self.transparent = true;
        self
    }

    /// Who draws the title bar; see [`Titlebar`].
    pub fn titlebar(mut self, t: Titlebar) -> Self {
        self.titlebar = t;
        self
    }

    /// A taller title bar of your own ([`Titlebar::Overlay`]): `h` pt, the traffic lights centred
    /// in it and moved in from the edge to match, as in apps with a toolbar. [`Env::titlebar`]
    /// is `h` and [`Env::window_buttons`] the width the lights take (both 0 in full screen).
    /// macOS only; elsewhere both insets stay 0.
    ///
    /// [`Env::titlebar`]: crate::Env::titlebar
    /// [`Env::window_buttons`]: crate::Env::window_buttons
    pub fn titlebar_height(mut self, h: f32) -> Self {
        self.titlebar = Titlebar::Overlay;
        self.titlebar_height = Some(h.max(0.0));
        self
    }

    /// Shows an FPS / frame-time counter in the top-right corner (redraws continuously).
    pub fn show_fps(mut self, on: bool) -> Self {
        self.ui.show_fps = on;
        self
    }

    /// Adds an independently sized desktop window with its own state and view.
    /// Use distinct state types and `Link<T>` to exchange updates. Closing the companion
    /// calls `close` and leaves the main application running. Hidden windows do not render.
    /// `visible` returns `None` while closed; changing the `Some` activation number brings
    /// the window forward again without stealing focus for ordinary state updates.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn companion<T: 'static>(mut self, app: App<T>, visible: impl Fn(&T) -> Option<u64> + 'static, close: impl Fn(&mut T) + 'static) -> Self {
        assert!(app.companions.is_empty(), "companion windows must be attached to the main app");
        self.companions.push(Box::new(companion::Setup { app, visible: Box::new(visible), close: Box::new(close) }));
        self
    }

    pub fn run(self) {
        #[cfg(target_arch = "wasm32")]
        {
            console_error_panic_hook::set_once();
            webkit_upload_fix();
        }
        let event_loop = EventLoop::<Wake>::with_user_event().build().expect("event loop");
        // Links wake the loop through the proxy: no polling, idle stays at 0% CPU.
        let proxy = event_loop.create_proxy();
        crate::task::set_waker(move || {
            let _ = proxy.send_event(Wake);
        });
        #[cfg(target_arch = "wasm32")]
        crate::task::set_spawner(wasm_bindgen_futures::spawn_local);
        let runner = Runner::new(self, &event_loop, true);

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut runner = runner;
            event_loop.run_app(&mut runner).expect("run");
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::EventLoopExtWebSys;
            event_loop.spawn_app(runner);
        }
    }
}

// WebKit's WebGPU (seen in Playwright WebKit) intermittently drops `GPUQueue.writeTexture` into
// a texture that is sampled every frame: all uploads of an unlucky frame never land, while
// `copyBufferToTexture` from a command encoder is reliable. glyphon uploads every new glyph with
// `write_texture` into the atlas it samples each frame, so glyphs vanished from the atlas. Measured
// by reading the atlas back: lost uploads come in whole frames; via the copy path none were lost.
// The shim keeps the queue-ordering of `writeTexture`: copies of a frame are staged in one
// buffer and submitted right before the next `submit`. Unusual layouts fall back to the original.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function webkit_upload_fix() {
  if (typeof GPUQueue === "undefined" || GPUQueue.prototype.__ruruUpload) return;
  // Only WebKit needs it (Safari, and every iOS browser); Chromium's UA also says AppleWebKit.
  const ua = navigator.userAgent;
  if (!/AppleWebKit/.test(ua) || /Chrome|Chromium|Edg\//.test(ua)) return;
  const Q = GPUQueue.prototype, D = GPUDevice.prototype;
  const writeTexture = Q.writeTexture, submit = Q.submit, createTexture = D.createTexture;
  const texel = { r8unorm: 1, rgba8unorm: 4, "rgba8unorm-srgb": 4, bgra8unorm: 4, "bgra8unorm-srgb": 4 };
  const deviceOf = new WeakMap(); // queue -> device
  const pending = new WeakMap(); // queue -> [{ dst, size, rows, bpr }]
  Q.__ruruUpload = true;
  D.createTexture = function (desc) { deviceOf.set(this.queue, this); return createTexture.call(this, desc); };
  Q.writeTexture = function (dst, data, layout, size) {
    try {
      const bpp = texel[dst.texture.format];
      const w = size.width ?? size[0], h = size.height ?? size[1] ?? 1, d = size.depthOrArrayLayers ?? size[2] ?? 1;
      if (bpp && d === 1 && deviceOf.has(this) && w > 0 && h > 0) {
        const src = ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : new Uint8Array(data);
        const off = layout.offset || 0, row = w * bpp, stride = layout.bytesPerRow ?? row;
        const bpr = Math.ceil(row / 256) * 256, rows = new Uint8Array(bpr * h);
        for (let y = 0; y < h; y++) rows.set(src.subarray(off + y * stride, off + y * stride + row), y * bpr);
        let list = pending.get(this);
        if (!list) pending.set(this, (list = []));
        list.push({ dst: { ...dst }, size: [w, h, 1], rows, bpr });
        return;
      }
    } catch (_) {}
    return writeTexture.call(this, dst, data, layout, size);
  };
  Q.submit = function (buffers) {
    const list = pending.get(this);
    if (list) {
      pending.delete(this);
      try {
        const device = deviceOf.get(this);
        const total = list.reduce((n, c) => n + c.rows.byteLength, 0);
        const staging = device.createBuffer({ size: total, usage: GPUBufferUsage.COPY_SRC, mappedAtCreation: true });
        const view = new Uint8Array(staging.getMappedRange());
        const enc = device.createCommandEncoder();
        let at = 0;
        for (const c of list) {
          view.set(c.rows, at);
          enc.copyBufferToTexture({ buffer: staging, offset: at, bytesPerRow: c.bpr }, c.dst, c.size);
          at += c.rows.byteLength;
        }
        staging.unmap();
        // Separate submit: a copy into a texture destroyed meanwhile must not void the frame.
        submit.call(this, [enc.finish()]);
        this.onSubmittedWorkDone().then(() => staging.destroy(), () => {});
      } catch (_) {
        // Lost device or an allocation failure: hand the uploads to the browser as they were.
        for (const c of list) {
          try { writeTexture.call(this, c.dst, c.rows, { bytesPerRow: c.bpr }, c.size); } catch (_) {}
        }
      }
    }
    return submit.call(this, buffers);
  };
}
"#)]
extern "C" {
    fn webkit_upload_fix();
}
/// The user event a [`Link`](crate::Link) send posts: "the inbox has updates".
struct Wake;

struct Gfx {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    /// The surface could also be a copy source (see `Renderer::wants_copy_src`).
    can_copy: bool,
}

async fn init_gfx(window: Arc<Window>, transparent: bool) -> Gfx {
    // In the browser `navigator.gpu` can exist without a usable adapter (some Safari
    // configurations); this probes it and falls back to WebGL2 instead of failing.
    let instance =
        wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor::new_without_display_handle()).await;
    let surface = instance.create_surface(window.clone()).expect("create surface");
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .expect("no compatible GPU adapter");
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("ruru"),
            // Stay within WebGL2 limits so the same code runs everywhere.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await
        .expect("request device");

    let caps = surface.get_capabilities(&adapter);
    // Blend in gamma space like browsers and CoreAnimation do: prefer a non-sRGB format.
    let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
    let size = window.inner_size();
    let mut config = surface
        .get_default_config(&adapter, size.width.max(1), size.height.max(1))
        .expect("surface not supported by adapter");
    config.format = format;
    config.present_mode = wgpu::PresentMode::AutoVsync;
    if transparent {
        // The renderer writes premultiplied color. Metal lists only `PostMultiplied`, but that just
        // marks the CAMetalLayer non-opaque, and CoreAnimation composites it premultiplied.
        use wgpu::CompositeAlphaMode as M;
        let modes = &caps.alpha_modes;
        if let Some(m) = [M::PreMultiplied, M::PostMultiplied, M::Inherit].into_iter().find(|m| modes.contains(m)) {
            config.alpha_mode = m;
        }
    }
    surface.configure(&device, &config);
    // Native only: in the browser the canvas stays a plain render target.
    let can_copy = !cfg!(target_arch = "wasm32") && caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
    Gfx { surface, config, renderer: Renderer::new(device, queue, format), can_copy }
}

struct Runner<S> {
    app: App<S>,
    window: Option<Arc<Window>>,
    visible: bool,
    #[cfg(not(target_arch = "wasm32"))]
    companions: Vec<Box<dyn companion::Running>>,
    gfx: Rc<RefCell<Option<Gfx>>>,
    cursor: (f32, f32),
    /// Pointer shape last given to the window.
    shown_cursor: Cursor,
    mods: winit::keyboard::ModifiersState,
    /// When the UI asked to be redrawn without input (caret blink).
    wake_at: Option<web_time::Instant>,
    /// The surface was set up with alpha (`App::transparent` on a desktop).
    transparent: bool,
    clipboard: Clipboard,
    /// IME allowed, and the caret area last reported to it.
    ime: (bool, Option<crate::Rect>),
    /// The backdrop view and the traffic lights' place.
    #[cfg(target_os = "macos")]
    chrome: Option<crate::macos::Chrome>,
    /// Width of the traffic lights' zone as last placed (`titlebar_height`).
    #[cfg(target_os = "macos")]
    buttons: Option<f32>,
}

/// The system clipboard: `arboard` on desktops (feature `clipboard`), the browser's clipboard
/// events on the web, otherwise a buffer private to the app.
struct Clipboard {
    local: String,
    #[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
    sys: Option<arboard::Clipboard>,
    /// Web: paste / cut arriving from the page's clipboard events.
    #[cfg(target_arch = "wasm32")]
    inbox: Rc<RefCell<Vec<WebClip>>>,
    #[cfg(target_arch = "wasm32")]
    shared: String,
    /// The `.selectable()` selection `shared` was taken from: its text is only rebuilt when
    /// the selection changes, since that builds the view.
    #[cfg(target_arch = "wasm32")]
    mirrored: Option<Option<crate::ui::select::Selection>>,
}

#[cfg(target_arch = "wasm32")]
enum WebClip {
    Paste(String),
    Cut,
}

impl Clipboard {
    #[allow(unused_variables)]
    fn new(event_loop: &EventLoop<Wake>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let inbox = {
            use wasm_bindgen::closure::Closure;
            let inbox: Rc<RefCell<Vec<WebClip>>> = Rc::default();
            let (a, b) = (inbox.clone(), inbox.clone());
            let (pa, pb) = (event_loop.create_proxy(), event_loop.create_proxy());
            let on_paste = Closure::<dyn FnMut(String)>::new(move |t: String| {
                a.borrow_mut().push(WebClip::Paste(t));
                let _ = pa.send_event(Wake);
            });
            let on_cut = Closure::<dyn FnMut()>::new(move || {
                b.borrow_mut().push(WebClip::Cut);
                let _ = pb.send_event(Wake);
            });
            clip_install(&on_paste, &on_cut);
            (on_paste.forget(), on_cut.forget());
            inbox
        };
        Clipboard {
            local: String::new(),
            #[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
            sys: None,
            #[cfg(target_arch = "wasm32")]
            inbox,
            #[cfg(target_arch = "wasm32")]
            shared: String::new(),
            #[cfg(target_arch = "wasm32")]
            mirrored: None,
        }
    }

    #[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
    fn sys(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.sys.is_none() {
            self.sys = arboard::Clipboard::new().ok(); // lazily: no X11 round-trips at startup
        }
        self.sys.as_mut()
    }

    fn set(&mut self, text: String) {
        #[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
        if let Some(c) = self.sys() {
            let _ = c.set_text(text.clone());
        }
        #[cfg(target_arch = "wasm32")]
        clip_write(&text);
        self.local = text;
    }

    fn get(&mut self) -> String {
        #[cfg(all(feature = "clipboard", not(target_arch = "wasm32")))]
        if let Some(t) = self.sys().and_then(|c| c.get_text().ok()) {
            return t;
        }
        self.local.clone()
    }
}

// The browser only lets a page touch the clipboard inside `copy` / `cut` / `paste` events,
// and only fires them if the key press keeps its default action. winit cancels every key
// press on the canvas, so Cmd/Ctrl+C/X/V are taken out of its way first (capture phase).
// The selection is mirrored here so the synchronous `copy` handler can hand it over.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
let sel = "";
export function clip_selection(s) { sel = s; }
export function clip_write(s) {
  try { navigator.clipboard.writeText(s).catch(() => {}); } catch (_) {}
}
export function clip_install(onPaste, onCut) {
  window.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && !e.altKey && ["KeyC", "KeyX", "KeyV"].includes(e.code)) e.stopPropagation();
  }, true);
  const give = (e) => {
    if (!sel || !e.clipboardData) return false;
    e.clipboardData.setData("text/plain", sel);
    e.preventDefault();
    return true;
  };
  document.addEventListener("copy", (e) => { give(e); });
  document.addEventListener("cut", (e) => { if (give(e)) onCut(); });
  document.addEventListener("paste", (e) => {
    const t = e.clipboardData && e.clipboardData.getData("text/plain");
    if (t) { e.preventDefault(); onPaste(t); }
  });
}
"#)]
extern "C" {
    fn clip_selection(s: &str);
    fn clip_write(s: &str);
    fn clip_install(on_paste: &wasm_bindgen::closure::Closure<dyn FnMut(String)>, on_cut: &wasm_bindgen::closure::Closure<dyn FnMut()>);
}

/// Opens a link in the default browser / mail app. The URL comes from content (markdown), so
/// it goes to the opener as a single argument, never through a shell.
fn open_url(url: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target(url, "_blank");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::process::Command;
        let cmd = if cfg!(target_os = "macos") {
            Some(Command::new("open").arg(url).spawn())
        } else if cfg!(target_os = "windows") {
            // `explorer` hands a URL to the default browser without `cmd`'s parsing of `&`.
            Some(Command::new("explorer").arg(url).spawn())
        } else if cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd")) {
            Some(Command::new("xdg-open").arg(url).spawn())
        } else {
            None // iOS / Android: no opener without platform glue yet
        };
        if let Some(Ok(mut child)) = cmd {
            // Reap it in the background so it does not linger as a zombie.
            std::thread::spawn(move || child.wait());
        }
    }
}

impl<S: 'static> Runner<S> {
    fn new(#[allow(unused_mut)] mut app: App<S>, event_loop: &EventLoop<Wake>, visible: bool) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let companions = std::mem::take(&mut app.companions).into_iter().map(|c| c.start(event_loop)).collect();
        Self {
            app,
            window: None,
            visible,
            #[cfg(not(target_arch = "wasm32"))]
            companions,
            gfx: Rc::new(RefCell::new(None)),
            cursor: (0.0, 0.0),
            shown_cursor: Cursor::Default,
            mods: Default::default(),
            wake_at: None,
            transparent: false,
            clipboard: Clipboard::new(&event_loop),
            ime: (false, None),
            #[cfg(target_os = "macos")]
            chrome: None,
            #[cfg(target_os = "macos")]
            buttons: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn update_companions(&mut self, event_loop: &ActiveEventLoop) -> Option<web_time::Instant> {
        self.companions.iter_mut().filter_map(|c| c.update(event_loop, self.window.as_deref())).min()
    }

    /// Carries out what the UI asked of the window and keeps IME / clipboard in sync with it.
    /// Must run while a press is still being handled: AppKit starts a window move from the
    /// current mouse-down event.
    fn sync(&mut self) {
        let Some(w) = &self.window else { return };
        let cursor = self.app.ui.cursor;
        if cursor != self.shown_cursor {
            self.shown_cursor = cursor;
            w.set_cursor(match cursor {
                Cursor::Default => CursorIcon::Default,
                Cursor::Pointer => CursorIcon::Pointer,
                Cursor::Text => CursorIcon::Text,
                Cursor::ResizeH => CursorIcon::EwResize,
                Cursor::ResizeV => CursorIcon::NsResize,
                Cursor::Grab => CursorIcon::Grab,
                Cursor::Grabbing => CursorIcon::Grabbing,
                Cursor::NotAllowed => CursorIcon::NotAllowed,
            });
        }
        for r in std::mem::take(&mut self.app.ui.requests) {
            match r {
                WindowRequest::DragWindow => {
                    #[cfg(target_os = "macos")]
                    crate::macos::drag_window(w);
                    #[cfg(not(target_os = "macos"))]
                    let _ = w.drag_window();
                }
                WindowRequest::Zoom => {
                    #[cfg(target_os = "macos")]
                    crate::macos::request_zoom(w);
                    #[cfg(not(target_os = "macos"))]
                    w.set_maximized(!w.is_maximized());
                }
                WindowRequest::SetClipboard(t) => self.clipboard.set(t),
                WindowRequest::OpenUrl(url) => open_url(&url),
            }
        }
        let ui = &self.app.ui;
        if ui.has_focus() != self.ime.0 {
            self.ime = (ui.has_focus(), None);
            w.set_ime_allowed(self.ime.0);
        }
        let area = ui.ime_area();
        if self.ime.0 && area != self.ime.1 {
            if let Some(r) = area {
                w.set_ime_cursor_area(LogicalPosition::new(r.x as f64, r.y as f64), Size::new(r.w as f64, r.h as f64));
            }
            self.ime.1 = area;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let sel = if ui.has_focus() {
                self.clipboard.mirrored = None;
                ui.selection_text().unwrap_or_default()
            } else {
                let now = Some(ui.feed_selection().cloned());
                if now != self.clipboard.mirrored {
                    self.clipboard.mirrored = now;
                    ui.feed_text().unwrap_or_default()
                } else {
                    self.clipboard.shared.clone()
                }
            };
            if sel != self.clipboard.shared {
                clip_selection(&sel);
                self.clipboard.shared = sel;
            }
        }
    }

    /// A key press with Cmd/Ctrl held: editing shortcuts (by the letter of the layout, or by
    /// its position for layouts without Latin letters, like Russian).
    fn shortcut(&mut self, logical: &Key, physical: PhysicalKey) -> bool {
        let ch = key_char(logical, physical);
        let ui = &mut self.app.ui;
        match ch {
            Some('a') => ui.key(KeyInput::SelectAll),
            Some('c') => ui.key(KeyInput::Copy),
            Some('x') => ui.key(KeyInput::Cut),
            Some('v') => {
                let t = self.clipboard.get();
                self.app.ui.paste(&t)
            }
            Some('z') if self.mods.shift_key() => ui.key(KeyInput::Redo),
            Some('z') => ui.key(KeyInput::Undo),
            Some('y') if !cfg!(target_os = "macos") => ui.key(KeyInput::Redo),
            _ => false,
        }
    }

    /// Navigation keys. Cmd (Meta) + arrows follow macOS: ←/→ to the row's ends, ↑/↓ to the
    /// text's ends, Backspace to the row start. Ctrl/Alt + arrows go by words.
    fn nav_key(&mut self, k: KeyInput) -> bool {
        let ui = &mut self.app.ui;
        let m = self.mods;
        if !m.super_key() {
            return ui.key(k);
        }
        let plain = Mods { shift: m.shift_key(), command: false, alt: m.alt_key() };
        let row = |ui: &mut Ui<S>, k: KeyInput| {
            ui.set_modifiers(plain);
            let r = ui.key(k);
            ui.set_modifiers(Mods { command: true, ..plain });
            r
        };
        match k {
            KeyInput::Left => row(ui, KeyInput::Home),
            KeyInput::Right => row(ui, KeyInput::End),
            KeyInput::Up => ui.key(KeyInput::Home),
            KeyInput::Down => ui.key(KeyInput::End),
            KeyInput::Backspace => {
                ui.set_modifiers(Mods { shift: true, ..plain });
                let r = ui.key(KeyInput::Home);
                ui.set_modifiers(Mods { command: true, ..plain });
                // Without a selection (caret already at the row start) delete one character.
                r && ui.key(KeyInput::Backspace)
            }
            _ => ui.key(k),
        }
    }

    fn redraw(&mut self) {
        if !self.visible { self.wake_at = None; return; }
        let Some(window) = &self.window else { return };
        let mut slot = self.gfx.borrow_mut();
        let Some(gfx) = slot.as_mut() else { return };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let size_changed = (gfx.config.width, gfx.config.height) != (size.width, size.height);
        if std::env::var_os("RURU_TRACE_WINDOW").is_some() {
            eprintln!("frame {:?} {}x{} changed={size_changed}", std::time::SystemTime::now(), size.width, size.height);
        }
        #[cfg(target_os = "macos")]
        { self.app.ui.resizing = crate::macos::sync_resize(window, &gfx.surface, size_changed); }
        if size_changed {
            gfx.config.width = size.width;
            gfx.config.height = size.height;
            gfx.surface.configure(&gfx.renderer.device, &gfx.config);
            gfx.renderer.invalidate();
        }
        let scale = window.scale_factor() as f32;
        let ui = &mut self.app.ui;
        // In full screen the title bar and its buttons slide away: nothing to make room for.
        #[cfg(target_os = "macos")]
        if self.app.titlebar == Titlebar::Overlay {
            let full = window.fullscreen().is_some();
            ui.titlebar = if full { 0.0 } else { self.app.titlebar_height.unwrap_or(MAC_TITLEBAR) };
            ui.window_buttons = if full { 0.0 } else { self.buttons.unwrap_or(MAC_BUTTONS) };
        }
        let started = web_time::Instant::now();
        ui.scale = scale;
        ui.frame((size.width as f32 / scale, size.height as f32 / scale));
        #[cfg(target_os = "macos")]
        if let Some(chrome)=&mut self.chrome {chrome.set_dark(ui.theme().dark);}
        let bg = ui.clear_color();
        let clear = if self.transparent { bg } else { Color { a: 1.0, ..bg } };

        // Nothing visible changed (hover over plain content, a wake-up for nothing): the
        // window already shows this frame, so the GPU is left alone.
        if gfx.renderer.unchanged((size.width, size.height), scale, &ui.list, clear) {
            self.wake_at = ui.wake_in.map(|s| web_time::Instant::now() + std::time::Duration::from_secs_f32(s));
            if ui.animating {
                // Without a present nothing paces the loop to the display: come back in a
                // frame's time instead of spinning.
                let next = web_time::Instant::now() + std::time::Duration::from_millis(8);
                self.wake_at = Some(self.wake_at.map_or(next, |w| w.min(next)));
            }
            drop(slot);
            self.sync();
            return;
        }

        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            _ => {
                gfx.surface.configure(&gfx.renderer.device, &gfx.config);
                gfx.renderer.invalidate();
                window.request_redraw();
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        gfx.renderer.render(&view, (size.width, size.height), scale, &ui.list, &mut ui.text, clear);
        // Glass draws through an off-screen copy of the window until the window itself can
        // be copied from. Only apps that show glass ask for that (it may cost the compositor
        // a little), once.
        let readable = gfx.can_copy
            && gfx.renderer.wants_copy_src()
            && !gfx.config.usage.contains(wgpu::TextureUsages::COPY_SRC);
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        ui.frame_ms += (ms - ui.frame_ms) * if ui.frame_ms == 0.0 { 1.0 } else { 0.1 };
        window.pre_present_notify();
        gfx.renderer.queue.present(frame);
        if readable {
            gfx.config.usage |= wgpu::TextureUsages::COPY_SRC;
            gfx.surface.configure(&gfx.renderer.device, &gfx.config);
            gfx.renderer.invalidate();
        }
        self.wake_at = ui.wake_in.map(|s| web_time::Instant::now() + std::time::Duration::from_secs_f32(s));
        if ui.animating {
            window.request_redraw();
        }
        drop(slot);
        self.sync(); // the caret may have moved for the IME
    }

    /// With `theme_auto`, takes the theme that matches the system appearance.
    fn follow_system(&mut self, system: Option<winit::window::Theme>) {
        self.app.ui.system_dark=system!=Some(winit::window::Theme::Light);
        if let Some((light, dark)) = &self.app.auto {
            let t = if system == Some(winit::window::Theme::Light) { light } else { dark };
            self.app.ui.set_theme(t.clone());
        }
    }

    /// AppKit lays the traffic lights out again on resize, full screen, key and appearance
    /// changes: put them back after each (no polling).
    #[cfg(target_os = "macos")]
    fn place_buttons(&mut self, window: &Window) {
        let (Some(h), Some(chrome)) = (self.app.titlebar_height, &mut self.chrome) else { return };
        if window.fullscreen().is_none() {
            self.buttons = chrome.place_buttons(window, h).or(self.buttons);
        }
    }

    fn scale(&self) -> f32 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32)
    }
}

impl<S: 'static> ApplicationHandler<Wake> for Runner<S> {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _: Wake) {
        #[cfg(target_arch = "wasm32")]
        {
            let clips = std::mem::take(&mut *self.clipboard.inbox.borrow_mut());
            let mut used = false;
            for c in clips {
                used |= match c {
                    WebClip::Paste(t) => self.app.ui.paste(&t),
                    WebClip::Cut => self.app.ui.key(KeyInput::Cut),
                };
            }
            if used {
                self.sync();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
        // Applied here rather than left for the next frame, so a send that changes nothing
        // visible (cancelled task) costs no redraw.
        if self.app.ui.pump() {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_arch = "wasm32"))]
        let next = self.wake_at.into_iter().chain(self.update_companions(event_loop)).min();
        #[cfg(target_arch = "wasm32")]
        let next = self.wake_at;
        match next {
            Some(t) if t <= web_time::Instant::now() => {
                self.wake_at = None;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // A transparent canvas would show the page behind it: keep the web opaque.
        self.transparent = self.app.transparent && !cfg!(target_arch = "wasm32");
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_visible(self.visible)
            .with_title(self.app.title.clone())
            .with_inner_size(LogicalSize::new(self.app.size.0 as f64, self.app.size.1 as f64))
            .with_transparent(self.transparent);
        // macOS puts its own backdrop under the content once the window exists.
        #[cfg(not(target_os = "macos"))]
        {
            attrs = attrs.with_blur(self.app.backdrop.is_some() && self.transparent);
        }
        if let Some((w, h)) = self.app.min_size {
            attrs = attrs.with_min_inner_size(LogicalSize::new(w as f64, h as f64));
        }
        if let Some((w, h)) = self.app.max_size {
            attrs = attrs.with_max_inner_size(LogicalSize::new(w as f64, h as f64));
        }
        #[cfg(target_os = "macos")]
        if self.app.titlebar == Titlebar::Overlay {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs.with_titlebar_transparent(true).with_title_hidden(true).with_fullsize_content_view(true);
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs = attrs.with_append(true);
        }
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        #[cfg(target_arch = "wasm32")]
        {
            // The canvas fills the page; winit reports its size through a ResizeObserver.
            use winit::platform::web::WindowExtWebSys;
            if let Some(canvas) = window.canvas() {
                let style = canvas.style();
                for (k, v) in [("width", "100vw"), ("height", "100vh"), ("display", "block"), ("outline", "none")] {
                    let _ = style.set_property(k, v);
                }
            }
            let (slot, win) = (self.gfx.clone(), window.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let gfx = init_gfx(win.clone(), false).await;
                *slot.borrow_mut() = Some(gfx);
                win.request_redraw();
            });
        }
        // Before the surface: its Metal layer then lands above the backdrop view.
        #[cfg(target_os = "macos")]
        {
            let backdrop = self.app.backdrop.filter(|_| self.transparent);
            self.chrome = Some(crate::macos::Chrome::new(&window, backdrop));
            self.place_buttons(&window);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            *self.gfx.borrow_mut() = Some(pollster::block_on(init_gfx(window.clone(), self.transparent)));
        }
        let system = window.theme();
        self.follow_system(system);
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.window.as_ref().is_none_or(|w| w.id() != id) {
            if let Some(child) = self.companions.iter_mut().find(|c| c.id() == Some(id)) {
                child.event(event_loop, id, event);
            }
            return;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = id;
        if let Some(dirty)=self.app.file_event(&event) {
            if dirty {if let Some(window)=&self.window {window.request_redraw();}}
            return;
        }
        #[cfg(target_os = "macos")]
        if matches!(
            event,
            WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(_)
                | WindowEvent::ThemeChanged(_)
                | WindowEvent::Occluded(false)
        ) {
            if let Some(w) = self.window.clone() {
                self.place_buttons(&w);
            }
        }
        let scale = self.scale();
        let ui = &mut self.app.ui;
        let dirty = match event {
            WindowEvent::ThemeChanged(t) => {
                self.follow_system(Some(t));
                true
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
                false
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                false
            }
            #[cfg(target_os = "macos")]
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                // Draw in the resize callback, before AppKit commits the resized layer.
                // Deferring to another redraw can present old pixels in the new bounds.
                self.redraw();
                false
            }
            #[cfg(not(target_os = "macos"))]
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => true,
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                ui.pointer_move(self.cursor)
            }
            WindowEvent::CursorLeft { .. } => ui.pointer_leave(),
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => ui.pointer_down(self.cursor, false),
                ElementState::Released => ui.pointer_up(self.cursor),
            },
            // Context menus open on the press, as on macOS.
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                ui.secondary_click(self.cursor)
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (dy, precise) = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 48.0, false),
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32 / scale, true),
                };
                // Only macOS trackpads report gesture phases; everything else is treated
                // as a plain wheel (no rubber band, since we can't tell when it is released).
                let phase = match phase {
                    _ if !precise || !cfg!(target_os = "macos") => WheelPhase::Line,
                    TouchPhase::Started => WheelPhase::Started,
                    TouchPhase::Moved => WheelPhase::Moved,
                    TouchPhase::Ended | TouchPhase::Cancelled => WheelPhase::Ended,
                };
                ui.wheel(self.cursor, dy, phase)
            }
            WindowEvent::ModifiersChanged(m) => {
                let m = m.state();
                self.mods = m;
                ui.set_modifiers(Mods { shift: m.shift_key(), command: m.control_key() || m.super_key(), alt: m.alt_key() });
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let named = match &event.logical_key {
                    Key::Named(NamedKey::Backspace) => Some(KeyInput::Backspace),
                    Key::Named(NamedKey::Delete) => Some(KeyInput::Delete),
                    Key::Named(NamedKey::ArrowLeft) => Some(KeyInput::Left),
                    Key::Named(NamedKey::ArrowRight) => Some(KeyInput::Right),
                    Key::Named(NamedKey::ArrowUp) => Some(KeyInput::Up),
                    Key::Named(NamedKey::ArrowDown) => Some(KeyInput::Down),
                    Key::Named(NamedKey::Home) => Some(KeyInput::Home),
                    Key::Named(NamedKey::End) => Some(KeyInput::End),
                    Key::Named(NamedKey::Enter) => Some(KeyInput::Enter),
                    Key::Named(NamedKey::Escape) => Some(KeyInput::Escape),
                    _ => None,
                };
                let command = self.mods.control_key() || self.mods.super_key();
                let sk = match named {
                    Some(k) => Some(ShortcutKey::Named(k)),
                    None => key_char(&event.logical_key, event.physical_key).map(ShortcutKey::Char),
                };
                match (named, &event.text) {
                    _ if sk.is_some_and(|k| self.app.ui.key_shortcut(k)) => true,
                    (Some(k), _) => self.nav_key(k),
                    _ if command => self.shortcut(&event.logical_key, event.physical_key),
                    (None, Some(text)) => self.app.ui.text_input(text),
                    _ => false,
                }
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(text, cursor) => ui.ime_preedit(&text, cursor),
                Ime::Commit(text) => {
                    ui.ime_preedit("", None);
                    ui.text_input(&text)
                }
                Ime::Disabled => ui.ime_preedit("", None),
                Ime::Enabled => false,
            },
            WindowEvent::Touch(t) => {
                let pos = (t.location.x as f32 / scale, t.location.y as f32 / scale);
                match t.phase {
                    TouchPhase::Started => ui.pointer_down(pos, true),
                    TouchPhase::Moved => ui.pointer_move(pos),
                    TouchPhase::Ended | TouchPhase::Cancelled => ui.pointer_up(pos),
                }
            }
            _ => false,
        };
        self.sync();
        if dirty {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

/// The character of a key for shortcuts: by the letter of the layout, or by its position for
/// layouts without Latin letters (Russian), so Cmd+K is the same key everywhere.
fn key_char(logical: &Key, physical: PhysicalKey) -> Option<char> {
    if let Key::Character(c) = logical {
        if c.is_ascii() {
            return c.to_ascii_lowercase().chars().next();
        }
    }
    let PhysicalKey::Code(code) = physical else { return None };
    let letters = [
        KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG,
        KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN,
        KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU,
        KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
    ];
    if let Some(i) = letters.iter().position(|k| *k == code) {
        return Some((b'a' + i as u8) as char);
    }
    Some(match code {
        KeyCode::BracketLeft => '[',
        KeyCode::BracketRight => ']',
        KeyCode::Comma => ',',
        KeyCode::Period => '.',
        KeyCode::Slash => '/',
        KeyCode::Semicolon => ';',
        KeyCode::Quote => '\'',
        KeyCode::Backquote => '`',
        KeyCode::Minus => '-',
        KeyCode::Equal => '=',
        KeyCode::Backslash => '\\',
        _ => return None,
    })
}


/// Opens the native directory chooser from a UI event handler. Returns `None` on cancel.
/// Currently available on macOS; other targets return `None` without opening a dialog.
pub fn pick_folder() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")] { crate::macos::pick_folder() }
    #[cfg(not(target_os = "macos"))] { None }
}

/// Opens a native multi-file chooser on the UI thread. Cancellation returns an empty
/// vector. Platforms without a native implementation return an explicit error.
pub fn pick_files()->Result<Vec<std::path::PathBuf>,String> {
    #[cfg(target_os="macos")] {crate::macos::pick_files()}
    #[cfg(not(target_os="macos"))] {Err("Native file selection is not available on this platform".into())}
}

#[cfg(test)]
mod file_tests {
    use super::*;
    #[test]
    fn native_file_events_are_opt_in_keep_paths_and_clear_hover() {
        #[derive(Default)] struct Files {hovered:bool,paths:Vec<std::path::PathBuf>}
        let mut app=App::new(Files::default(),|_:&Files|crate::rect())
            .on_file_hover(|s,on|s.hovered=on).on_file_drop(|s,path|s.paths.push(path));
        let path=std::path::PathBuf::from("/tmp/quoted name ' image.png");
        assert_eq!(app.file_event(&WindowEvent::HoveredFile(path.clone())),Some(true));assert!(app.ui.state.hovered);
        assert_eq!(app.file_event(&WindowEvent::DroppedFile(path.clone())),Some(true));assert!(!app.ui.state.hovered);
        assert_eq!(app.ui.state.paths,vec![path.clone()]);
        app.file_event(&WindowEvent::HoveredFile(path.clone()));app.file_event(&WindowEvent::HoveredFileCancelled);
        assert!(!app.ui.state.hovered);assert_eq!(app.ui.state.paths.len(),1);
        let mut plain=App::new((),|_:&()|crate::rect());
        assert_eq!(plain.file_event(&WindowEvent::DroppedFile(path)),Some(false));
        assert_eq!(plain.file_event(&WindowEvent::RedrawRequested),None);
    }
}
