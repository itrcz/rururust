//! Independent desktop inspector/document windows on the same event loop.
use super::*;

pub(super) trait Pending {
    fn start(self: Box<Self>, event_loop: &EventLoop<Wake>) -> Box<dyn Running>;
}
pub(super) struct Setup<S> {
    pub app: App<S>,
    pub visible: Box<dyn Fn(&S) -> Option<u64>>,
    pub close: Box<dyn Fn(&mut S)>,
}
impl<S: 'static> Pending for Setup<S> {
    fn start(self: Box<Self>, event_loop: &EventLoop<Wake>) -> Box<dyn Running> {
        Box::new(Companion { runner: Runner::new(self.app, event_loop, false), visible: self.visible, close: self.close, activation: None })
    }
}
pub(super) trait Running {
    fn id(&self) -> Option<WindowId>;
    fn update(&mut self, event_loop: &ActiveEventLoop, parent: Option<&Window>) -> Option<web_time::Instant>;
    fn event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent);
}
struct Companion<S> {
    activation: Option<u64>,
    runner: Runner<S>,
    visible: Box<dyn Fn(&S) -> Option<u64>>,
    close: Box<dyn Fn(&mut S)>,
}
impl<S: 'static> Running for Companion<S> {
    fn id(&self) -> Option<WindowId> { self.runner.window.as_ref().map(|w| w.id()) }
    fn update(&mut self, event_loop: &ActiveEventLoop, parent: Option<&Window>) -> Option<web_time::Instant> {
        let dirty = self.runner.app.ui.pump();
        let activation = (self.visible)(&self.runner.app.ui.state);
        let visible = activation.is_some();
        if visible && self.runner.window.is_none() {
            self.runner.resumed(event_loop);
            if let (Some(parent), Some(window)) = (parent, self.runner.window.as_ref()) {
                if let Ok(at) = parent.outer_position() {
                    let mut x = at.x + parent.outer_size().width as i32 + 8;
                    let mut y = at.y;
                    if let Some(monitor) = parent.current_monitor() {
                        let origin = monitor.position(); let size = monitor.size();
                        x = x.min(origin.x + size.width as i32 - window.outer_size().width as i32).max(origin.x);
                        y = y.min(origin.y + size.height as i32 - window.outer_size().height as i32).max(origin.y);
                    }
                    window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
                }
            }
        }
        if activation != self.activation {
            self.activation = activation;
            self.runner.visible = visible;
            if let Some(window) = &self.runner.window {
                window.set_visible(visible);
                if visible { window.set_minimized(false); window.request_redraw(); window.focus_window(); }
            }
        }
        if !visible { self.runner.wake_at = None; return None; }
        let timer_due = self.runner.wake_at.is_some_and(|t| t <= web_time::Instant::now());
        if timer_due { self.runner.wake_at = None; }
        if dirty || timer_due {
            if let Some(window) = &self.runner.window { window.request_redraw(); }
        }
        self.runner.wake_at
    }
    fn event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            (self.close)(&mut self.runner.app.ui.state);
        } else {
            self.runner.window_event(event_loop, id, event);
        }
    }
}
