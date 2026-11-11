//! AppKit calls winit doesn't expose: the window backdrop (`NSVisualEffectView`) and the place of
//! the traffic lights. The only module with `unsafe` (decision 006): every block says why it holds.
//! Everything here runs on the main thread from the event loop, and only touches views the window
//! owns; when AppKit hands back something unexpected (no button, no superview) it does nothing.

use crate::app::Backdrop;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use std::cell::Cell;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSNotification, NSNotificationCenter, NSPoint, NSRect};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    // Private, but stable for a decade and what winit's `with_blur` calls too; only it lets the
    // blur radius be chosen.
    fn CGSMainConnectionID() -> *mut std::ffi::c_void;
    fn CGSSetWindowBackgroundBlurRadius(connection: *mut std::ffi::c_void, window: isize, radius: i64) -> i32;
}

/// The window's own chrome: the backdrop view and where the traffic lights go.
pub(crate) struct Chrome {
    effect: Option<Retained<NSView>>,
    /// The appearance last given to the backdrop (dark?), so it is set only on a change.
    dark: Option<bool>,
    /// Keeps the traffic lights in place (`titlebar_height`).
    keeper: Option<Retained<Keeper>>,
}

/// winit's content view: the one its window handle points at.
fn content_view(window: &Window) -> Option<Retained<NSView>> {
    let RawWindowHandle::AppKit(h) = window.window_handle().ok()?.as_raw() else { return None };
    // SAFETY: winit documents `ns_view` as a valid `NSView*` that lives as long as the window; we
    // are on the main thread (event loop callbacks) and retain it, so the reference is not kept
    // past this borrow.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    Some(view.retain())
}

fn ns_window(window: &Window) -> Option<Retained<NSWindow>> {
    content_view(window)?.window()
}

impl Chrome {
    /// Puts the backdrop under the content. Call before the GPU surface is created, so the Metal
    /// layer (a sublayer of the content view) ends up above the effect view.
    pub(crate) fn new(window: &Window, backdrop: Option<Backdrop>) -> Chrome {
        let mut chrome = Chrome { effect: None, dark: None, keeper: None };
        let (Some(view), Some(mtm)) = (content_view(window), MainThreadMarker::new()) else { return chrome };
        let Some(win) = view.window() else { return chrome };
        match backdrop {
            None => {}
            Some(Backdrop::Blur(radius)) => {
                // SAFETY: plain C call with the window's own number; a bad number is ignored.
                unsafe { CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), win.windowNumber(), radius as i64) };
            }
            Some(b) => {
                let material = match b {
                    Backdrop::Sidebar => NSVisualEffectMaterial::Sidebar,
                    Backdrop::UnderWindow => NSVisualEffectMaterial::UnderWindowBackground,
                    Backdrop::Window => NSVisualEffectMaterial::WindowBackground,
                    Backdrop::Content => NSVisualEffectMaterial::ContentBackground,
                    Backdrop::Titlebar => NSVisualEffectMaterial::Titlebar,
                    Backdrop::HeaderView => NSVisualEffectMaterial::HeaderView,
                    Backdrop::Menu => NSVisualEffectMaterial::Menu,
                    Backdrop::Popover => NSVisualEffectMaterial::Popover,
                    Backdrop::Sheet => NSVisualEffectMaterial::Sheet,
                    Backdrop::Hud => NSVisualEffectMaterial::HUDWindow,
                    Backdrop::FullScreenUi => NSVisualEffectMaterial::FullScreenUI,
                    Backdrop::Blur(_) => unreachable!(),
                };
                // SAFETY: a plain `new` with the main-thread marker, then setters on that view;
                // the arguments are AppKit's own constants and the content view's bounds.
                let effect = unsafe { objc2_app_kit::NSVisualEffectView::new(mtm) };
                // SAFETY: as above, our own view on the main thread.
                unsafe {
                    effect.setMaterial(material);
                    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                    // Like a sidebar: dims with the window when it is not key.
                    effect.setState(NSVisualEffectState::FollowsWindowActiveState);
                    effect.setAutoresizingMask(
                        NSAutoresizingMaskOptions::NSViewWidthSizable | NSAutoresizingMaskOptions::NSViewHeightSizable,
                    );
                    // A sibling under the content view, not its child: AppKit keeps subviews'
                    // layers above the view's own sublayers, and the Metal layer is one of those.
                    match view.superview() {
                        Some(frame) => {
                            effect.setFrame(view.frame());
                            frame.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::NSWindowBelow, Some(&view));
                        }
                        None => return chrome,
                    }
                }
                chrome.effect = Some(Retained::into_super(effect));
            }
        }
        // The shadow follows the window's new shape and opacity.
        // SAFETY: a main-thread call on the window that owns the view.
        unsafe { win.invalidateShadow() };
        chrome
    }

    /// The backdrop's material is drawn light or dark to match the app's theme rather than the
    /// system appearance (an app can be dark on a light system).
    pub(crate) fn set_dark(&mut self, dark: bool) {
        let Some(effect) = &self.effect else { return };
        if self.dark == Some(dark) {
            return;
        }
        self.dark = Some(dark);
        // SAFETY: reading AppKit's appearance-name constants (immutable statics it exports) and
        // setting the appearance of our own view on the main thread.
        unsafe {
            let name = if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua };
            let appearance = NSAppearance::appearanceNamed(name);
            effect.setAppearance(appearance.as_deref());
        }
    }

    /// Centers the traffic lights in a title bar strip `height` pt tall and moves them in from the
    /// leading edge by as much as they sit below the top. Returns the width of their zone (to the
    /// right edge of the last button plus a gap), or `None` when they can't be found. AppKit lays
    /// them out again on resize, full screen, key and appearance changes, sometimes after winit's
    /// event for it: a frame-change observer on the title bar puts them back whenever it moves.
    pub(crate) fn place_buttons(&mut self, window: &Window, height: f32) -> Option<f32> {
        let win = ns_window(window)?;
        let keeper = match &self.keeper {
            Some(k) => k,
            None => {
                let k = Keeper::new(MainThreadMarker::new()?);
                k.watch(&win);
                self.keeper.insert(k)
            }
        };
        keeper.ivars().height.set(height as f64);
        place(&win, keeper.ivars())
    }
}

struct Place {
    height: Cell<f64>,
    /// The system's x of the close button, read before it is first moved.
    system_x: Cell<Option<f64>>,
}

declare_class!(
    /// Puts the traffic lights back when AppKit moves them.
    struct Keeper;

    // SAFETY: NSObject has no subclassing requirements; the class is main-thread only (it touches
    // views), and it does not implement Drop.
    unsafe impl ClassType for Keeper {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "RuruTrafficLightKeeper";
    }

    impl DeclaredClass for Keeper {
        type Ivars = Place;
    }

    // SAFETY: the method signature matches the selector a notification observer is called with.
    unsafe impl Keeper {
        #[method(frameChanged:)]
        fn frame_changed(&self, note: &NSNotification) {
            // SAFETY: `watch` registers this selector only with NSViews as the notification
            // objects, so the cast holds.
            let view = unsafe { note.object().map(|o| Retained::cast::<NSView>(o)) };
            if let Some(win) = view.and_then(|v| v.window()) {
                place(&win, self.ivars());
            }
        }
    }
);

impl Keeper {
    fn new(mtm: MainThreadMarker) -> Retained<Keeper> {
        let this = mtm.alloc::<Keeper>().set_ivars(Place { height: Cell::new(0.0), system_x: Cell::new(None) });
        // SAFETY: NSObject's `init` on a freshly allocated object with its ivars set.
        unsafe { msg_send_id![super(this), init] }
    }

    /// Observes frame changes of the title bar container and the close button.
    fn watch(&self, win: &NSWindow) {
        let Some(close) = win.standardWindowButton(NSWindowButton::NSWindowCloseButton) else { return };
        // SAFETY: main-thread AppKit getters; the observer is removed in `Drop for Chrome` before
        // it is released, and the selector is the one `Keeper` implements.
        unsafe {
            let Some(container) = close.superview().and_then(|bar| bar.superview()) else { return };
            let center = NSNotificationCenter::defaultCenter();
            for v in [&*container, &**close] {
                v.setPostsFrameChangedNotifications(true);
                center.addObserver_selector_name_object(self, sel!(frameChanged:), Some(NSViewFrameDidChangeNotification), Some(v));
            }
        }
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        if let Some(k) = &self.keeper {
            // SAFETY: unregistering our own observer object.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(k) };
        }
    }
}

/// Moves the title bar container and the buttons where `p` says, touching only frames that differ
/// (each move posts a frame change back to the keeper).
fn place(win: &NSWindow, p: &Place) -> Option<f32> {
    let buttons = [NSWindowButton::NSWindowCloseButton, NSWindowButton::NSWindowMiniaturizeButton, NSWindowButton::NSWindowZoomButton]
        .map(|b| win.standardWindowButton(b));
    let [Some(close), Some(mini), Some(zoom)] = buttons else { return None };
    // In full screen the buttons live in the menu-bar reveal; leave them to AppKit.
    if win.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return None;
    }
    // SAFETY: walking up from the close button (NSTitlebarView, then its container) and moving
    // views AppKit created for this window; main thread; frames are finite numbers.
    unsafe {
        let bar = close.superview()?;
        let container = bar.superview()?;
        let (h, bh) = (p.height.get(), close.frame().size.height);
        let system_x = p.system_x.get().unwrap_or(close.frame().origin.x);
        p.system_x.set(Some(system_x));
        let step = mini.frame().origin.x - close.frame().origin.x;
        // The container hangs from the top of the window (AppKit's y goes up).
        let mut frame: NSRect = container.frame();
        let top = frame.origin.y + frame.size.height;
        if frame.size.height != h {
            frame.size.height = h;
            frame.origin.y = top - h;
            container.setFrame(frame);
        }
        let y = (bar.frame().size.height - bh) / 2.0;
        let x = system_x.max((h - bh) / 2.0 + 1.0);
        for (i, b) in [&close, &mini, &zoom].into_iter().enumerate() {
            let at = NSPoint::new(x + step * i as f64, y);
            if b.frame().origin != at {
                b.setFrameOrigin(at);
            }
        }
        Some((zoom.frame().origin.x + zoom.frame().size.width + 8.0) as f32)
    }
}
