//! The window/event-loop runner. Same code path on desktop, mobile and web.

use crate::render::Renderer;
use crate::types::Color;
use crate::ui::{KeyInput, Ui, WheelPhase, WindowRequest};
use crate::view::ViewFn;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

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

// AppKit's title bar is 28 pt tall; the traffic lights end at about 70 pt, plus a gap.
#[cfg(target_os = "macos")]
const MAC_TITLEBAR: f32 = 28.0;
#[cfg(target_os = "macos")]
const MAC_BUTTONS: f32 = 78.0;

pub struct App<S> {
    ui: Ui<S>,
    title: String,
    size: (f32, f32),
    transparent: bool,
    blur: bool,
    titlebar: Titlebar,
}

impl<S: 'static> App<S> {
    /// `view` is called with the current state whenever the UI needs to be rebuilt.
    pub fn new<M>(state: S, view: impl ViewFn<S, M>) -> Self {
        App {
            ui: Ui::new(state, view),
            title: "ruru".into(),
            size: (420.0, 760.0),
            transparent: false,
            blur: false,
            titlebar: Titlebar::Standard,
        }
    }
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    /// Initial window size in points (desktop only).
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.size = (w, h);
        self
    }
    /// Window clear color; also picks the default text color (light on dark, dark on light).
    /// With [`transparent`](Self::transparent) its alpha tints what is behind the window;
    /// otherwise alpha is ignored.
    pub fn background(mut self, c: Color) -> Self {
        self.ui.background = c;
        self
    }

    /// A window with an alpha channel: where the view and the background leave alpha below 1,
    /// the desktop shows through. Desktop only; on the web the canvas stays opaque.
    pub fn transparent(mut self, on: bool) -> Self {
        self.transparent = on;
        self
    }

    /// Blurs the desktop behind the window (implies [`transparent`](Self::transparent)).
    /// macOS: the system window-background blur; Windows: acrylic / blur-behind where the OS has
    /// it; Linux: KDE's blur protocol. Where the platform has none it is just transparent, so give
    /// [`background`](Self::background) enough alpha to stay readable without it.
    pub fn blur(mut self, on: bool) -> Self {
        self.blur = on;
        self.transparent |= on;
        self
    }

    /// Who draws the title bar; see [`Titlebar`].
    pub fn titlebar(mut self, t: Titlebar) -> Self {
        self.titlebar = t;
        self
    }

    /// Shows an FPS / frame-time counter in the top-right corner (redraws continuously).
    pub fn show_fps(mut self, on: bool) -> Self {
        self.ui.show_fps = on;
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
        let runner = Runner {
            app: self,
            window: None,
            gfx: Rc::new(RefCell::new(None)),
            cursor: (0.0, 0.0),
            command: false,
            wake_at: None,
            transparent: false,
        };
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
    Gfx { surface, config, renderer: Renderer::new(device, queue, format) }
}

struct Runner<S> {
    app: App<S>,
    window: Option<Arc<Window>>,
    gfx: Rc<RefCell<Option<Gfx>>>,
    cursor: (f32, f32),
    /// Ctrl / Cmd is held: key presses are shortcuts, not text.
    command: bool,
    /// When the UI asked to be redrawn without input (caret blink).
    wake_at: Option<web_time::Instant>,
    /// The surface was set up with alpha (`App::transparent` on a desktop).
    transparent: bool,
}

impl<S: 'static> Runner<S> {
    fn redraw(&mut self) {
        let Some(window) = &self.window else { return };
        let mut slot = self.gfx.borrow_mut();
        let Some(gfx) = slot.as_mut() else { return };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if (gfx.config.width, gfx.config.height) != (size.width, size.height) {
            gfx.config.width = size.width;
            gfx.config.height = size.height;
            gfx.surface.configure(&gfx.renderer.device, &gfx.config);
        }
        let scale = window.scale_factor() as f32;
        let ui = &mut self.app.ui;
        // In full screen the title bar and its buttons slide away: nothing to make room for.
        #[cfg(target_os = "macos")]
        if self.app.titlebar == Titlebar::Overlay {
            let full = window.fullscreen().is_some();
            ui.titlebar = if full { 0.0 } else { MAC_TITLEBAR };
            ui.window_buttons = if full { 0.0 } else { MAC_BUTTONS };
        }
        let started = web_time::Instant::now();
        ui.frame((size.width as f32 / scale, size.height as f32 / scale));

        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            _ => {
                gfx.surface.configure(&gfx.renderer.device, &gfx.config);
                window.request_redraw();
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let clear = if self.transparent { ui.background } else { Color { a: 1.0, ..ui.background } };
        gfx.renderer.render(&view, (size.width, size.height), scale, &ui.list, &mut ui.text, clear);
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        ui.frame_ms += (ms - ui.frame_ms) * if ui.frame_ms == 0.0 { 1.0 } else { 0.1 };
        window.pre_present_notify();
        gfx.renderer.queue.present(frame);
        self.wake_at = ui.wake_in.map(|s| web_time::Instant::now() + std::time::Duration::from_secs_f32(s));
        if ui.animating {
            window.request_redraw();
        }
    }

    fn scale(&self) -> f32 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32)
    }
}

impl<S: 'static> ApplicationHandler<Wake> for Runner<S> {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _: Wake) {
        // Applied here rather than left for the next frame, so a send that changes nothing
        // visible (cancelled task) costs no redraw.
        if self.app.ui.pump() {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.wake_at {
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
            .with_title(self.app.title.clone())
            .with_inner_size(LogicalSize::new(self.app.size.0 as f64, self.app.size.1 as f64))
            .with_transparent(self.transparent)
            .with_blur(self.app.blur && self.transparent);
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            *self.gfx.borrow_mut() = Some(pollster::block_on(init_gfx(window.clone(), self.transparent)));
        }
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let scale = self.scale();
        let ui = &mut self.app.ui;
        let dirty = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                false
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                false
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => true,
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                ui.pointer_move(self.cursor)
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => ui.pointer_down(self.cursor, false),
                ElementState::Released => ui.pointer_up(self.cursor),
            },
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
                self.command = m.state().control_key() || m.state().super_key();
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let named = match &event.logical_key {
                    Key::Named(NamedKey::Backspace) => Some(KeyInput::Backspace),
                    Key::Named(NamedKey::Delete) => Some(KeyInput::Delete),
                    Key::Named(NamedKey::ArrowLeft) => Some(KeyInput::Left),
                    Key::Named(NamedKey::ArrowRight) => Some(KeyInput::Right),
                    Key::Named(NamedKey::Home) => Some(KeyInput::Home),
                    Key::Named(NamedKey::End) => Some(KeyInput::End),
                    Key::Named(NamedKey::Enter) => Some(KeyInput::Enter),
                    Key::Named(NamedKey::Escape) => Some(KeyInput::Escape),
                    _ => None,
                };
                match (named, &event.text) {
                    (Some(k), _) => ui.key(k),
                    (None, Some(text)) if !self.command => ui.text_input(text),
                    _ => false,
                }
            }
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
        // Must run while the press is still being handled: AppKit starts the move from the
        // current mouse-down event.
        if let Some(w) = &self.window {
            for r in std::mem::take(&mut self.app.ui.requests) {
                match r {
                    WindowRequest::DragWindow => {
                        let _ = w.drag_window();
                    }
                    WindowRequest::Zoom => w.set_maximized(!w.is_maximized()),
                }
            }
        }
        if dirty {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}
