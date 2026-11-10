//! The window/event-loop runner. Same code path on desktop, mobile and web.

use crate::render::Renderer;
use crate::types::Color;
use crate::ui::{KeyInput, Ui};
use crate::view::View;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

pub struct App<S> {
    ui: Ui<S>,
    title: String,
    size: (f32, f32),
}

impl<S: 'static> App<S> {
    /// `view` is called with the current state whenever the UI needs to be rebuilt.
    pub fn new(state: S, view: impl Fn(&S) -> View<S> + 'static) -> Self {
        App { ui: Ui::new(state, view), title: "ruru".into(), size: (420.0, 760.0) }
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
    pub fn background(mut self, c: Color) -> Self {
        self.ui.background = c;
        self
    }

    /// Shows an FPS / frame-time counter in the top-right corner (redraws continuously).
    pub fn show_fps(mut self, on: bool) -> Self {
        self.ui.show_fps = on;
        self
    }

    pub fn run(self) {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        let event_loop = EventLoop::new().expect("event loop");
        let runner = Runner {
            app: self,
            window: None,
            gfx: Rc::new(RefCell::new(None)),
            cursor: (0.0, 0.0),
            command: false,
            wake_at: None,
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

struct Gfx {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

async fn init_gfx(window: Arc<Window>) -> Gfx {
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
        gfx.renderer.render(&view, (size.width, size.height), scale, &ui.list, &mut ui.text, ui.background);
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

impl<S: 'static> ApplicationHandler for Runner<S> {
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
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(self.app.title.clone())
            .with_inner_size(LogicalSize::new(self.app.size.0 as f64, self.app.size.1 as f64));
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
                let gfx = init_gfx(win.clone()).await;
                *slot.borrow_mut() = Some(gfx);
                win.request_redraw();
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            *self.gfx.borrow_mut() = Some(pollster::block_on(init_gfx(window.clone())));
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
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 48.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / scale,
                };
                ui.wheel(self.cursor, dy)
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
        if dirty {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}
