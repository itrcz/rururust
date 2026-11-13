//! AppKit calls winit doesn't expose: the window backdrop (`NSVisualEffectView`) and the place of
//! the traffic lights. The only module with `unsafe` (decision 006): every block says why it holds.
//! Everything here runs on the main thread from the event loop, and only touches views the window
//! owns; when AppKit hands back something unexpected (no button, no superview) it does nothing.

use crate::app::Backdrop;
pub(crate) mod voice;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use std::cell::Cell;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSObjectNSDelayedPerforming};
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

/// Enable native movement only during an explicit, hit-tested drag request.
/// Leaving it enabled between events also lets AppKit handle title-bar double clicks,
/// so one gesture can zoom twice: once natively and once through WindowRequest::Zoom.
pub(crate) fn drag_window(window: &Window) {
    let custom = ns_window(window)
        .filter(|win| win.styleMask().contains(NSWindowStyleMask::FullSizeContentView));
    if let Some(win) = &custom { win.setMovable(true); }
    let _ = window.drag_window();
    if let Some(win) = &custom { win.setMovable(false); }
}

/// Leave winit's input callback before entering AppKit's synchronous zoom animation.
/// Otherwise winit queues every nested resize until the zoom has already finished.
pub(crate) fn request_zoom(window: &Window) {
    if let Some(win) = ns_window(window) {
        // SAFETY: zoom: is NSWindow's standard one-object action. Delayed performing retains
        // its target and runs it on this main run loop after the current handler returns.
        unsafe { win.performSelector_withObject_afterDelay(sel!(zoom:), None, 0.0) };
    }
}

/// Live resize is a direct manipulation, not a UI transition. AppKit stays the owner
/// of the layer bounds; Metal presents the matching frame in the same transaction.
pub(crate) fn sync_resize(window: &Window, surface: &wgpu::Surface<'_>, size_changed: bool) -> bool {
    // SAFETY: content_view retains this window's NSView on the event-loop main thread.
    let live = content_view(window).is_some_and(|view| unsafe { view.inLiveResize() });
    // SAFETY: borrow wgpu's surface without destroying/replacing it or touching its
    // drawable resources. The guard and layer lock stay held for these main-thread setters.
    if let Some(surface) = unsafe { surface.as_hal::<wgpu::hal::api::Metal>() } {
        let layer = surface.render_layer().lock();
        // SAFETY: immutable QuartzCore constant, with the type declared by its bindings.
        let gravity = unsafe { objc2_quartz_core::kCAGravityTopLeft };
        if &*layer.contentsGravity() != gravity {
            // If the compositor gets ahead of a frame, keep it at its native size instead
            // of stretching every fixed-position icon with the window's new bounds.
            layer.setContentsGravity(gravity);
        }
        let transaction = live || size_changed;
        if layer.presentsWithTransaction() != transaction {
            // wgpu waits until scheduling and presents in AppKit's transaction only while
            // resizing; normal frames keep the asynchronous, vsynced presentation path.
            layer.setPresentsWithTransaction(transaction);
        }
    }
    live
}

impl Chrome {
    /// Puts the backdrop under the content. Call before the GPU surface is created, so the Metal
    /// layer (a sublayer of the content view) ends up above the effect view.
    pub(crate) fn new(window: &Window, backdrop: Option<Backdrop>) -> Chrome {
        let mut chrome = Chrome { effect: None, dark: None, keeper: None };
        let (Some(view), Some(mtm)) = (content_view(window), MainThreadMarker::new()) else { return chrome };
        let Some(win) = view.window() else { return chrome };
        if win.styleMask().contains(NSWindowStyleMask::FullSizeContentView) {
            // The custom title bar owns its gestures. AppKit would otherwise intercept a
            // second click on its transparent chrome before ruru can hit-test the button.
            // The runner enables native movement only for an explicit drag request.
            win.setMovable(false);
        }
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
    step: Cell<Option<f64>>,
    placing: Cell<bool>,
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
        let this = mtm.alloc::<Keeper>().set_ivars(Place { height: Cell::new(0.0), system_x: Cell::new(None), step: Cell::new(None), placing: Cell::new(false) });
        // SAFETY: NSObject's `init` on a freshly allocated object with its ivars set.
        unsafe { msg_send_id![super(this), init] }
    }

    /// Observes frame changes of the title bar container and the close button.
    fn watch(&self, win: &NSWindow) {
        let Some(close) = win.standardWindowButton(NSWindowButton::NSWindowCloseButton) else { return };
        // SAFETY: main-thread AppKit getters; the observer is removed in `Drop for Chrome` before
        // it is released, and the selector is the one `Keeper` implements.
        unsafe {
            let Some(bar) = close.superview() else { return };
            let Some(container) = bar.superview() else { return };
            let center = NSNotificationCenter::defaultCenter();
            let mut views = vec![container, bar, Retained::into_super(Retained::into_super(close))];
            for button in [NSWindowButton::NSWindowMiniaturizeButton, NSWindowButton::NSWindowZoomButton] {
                if let Some(button) = win.standardWindowButton(button) {
                    views.push(Retained::into_super(Retained::into_super(button)));
                }
            }
            if let Some(content) = win.contentView() { views.push(content); }
            for v in &views {
                v.setPostsFrameChangedNotifications(true);
                center.addObserver_selector_name_object(self, sel!(frameChanged:), Some(NSViewFrameDidChangeNotification), Some(&**v));
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
    if p.placing.replace(true) { return None; }
    struct Placing<'a>(&'a Cell<bool>);
    impl Drop for Placing<'_> {
        fn drop(&mut self) { self.0.set(false); }
    }
    let _placing = Placing(&p.placing);
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
        let step = p.step.get().unwrap_or(mini.frame().origin.x - close.frame().origin.x);
        p.step.set(Some(step));
        // The parent, not the container's previous frame, owns the current window top.
        let parent = container.superview()?;
        let bounds = parent.bounds();
        objc2_quartz_core::CATransaction::begin();
        objc2_quartz_core::CATransaction::setDisableActions(true);
        let mut frame: NSRect = container.frame();
        frame.size.height = h;
        frame.origin.y = if parent.isFlipped() { bounds.origin.y } else { bounds.origin.y + bounds.size.height - h };
        if container.frame() != frame { container.setFrame(frame); }
        let y = (bar.frame().size.height - bh) / 2.0;
        let x = system_x.max((h - bh) / 2.0 + 1.0);
        for (i, b) in [&close, &mini, &zoom].into_iter().enumerate() {
            let at = NSPoint::new(x + step * i as f64, y);
            if b.frame().origin != at {
                b.setFrameOrigin(at);
            }
        }
        objc2_quartz_core::CATransaction::commit();
        Some((zoom.frame().origin.x + zoom.frame().size.width + 8.0) as f32)
    }
}


/// Native directory chooser. Called only by an event handler on the AppKit main thread.
pub(crate) fn pick_folder() -> Option<std::path::PathBuf> {
    let mtm = MainThreadMarker::new()?;
    // SAFETY: AppKit panel creation and its modal loop run on the main thread. The retained
    // panel and URL live through conversion to an owned Rust path; no delegate is installed.
    unsafe {
        let panel = objc2_app_kit::NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        if panel.runModal() != objc2_app_kit::NSModalResponseOK { return None; }
        panel.URL()?.path().map(|path| std::path::PathBuf::from(path.to_string()))
    }
}

/// Native multi-file chooser; this function never reads file contents.
pub(crate) fn pick_files()->Result<Vec<std::path::PathBuf>,String> {
    let mtm=MainThreadMarker::new().ok_or("File selection must run on the main thread")?;
    // SAFETY: panel creation and its nested modal loop run on AppKit's main thread.
    // Retained panel/URLs outlive conversion to owned paths; no raw reference escapes.
    unsafe {
        let panel=objc2_app_kit::NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(true);
        if panel.runModal()!=objc2_app_kit::NSModalResponseOK {return Ok(Vec::new());}
        let urls=panel.URLs();
        urls.iter().map(|url| {
            if !url.isFileURL() {return Err("Selected item is not a local file".into());}
            url.path().map(|path|std::path::PathBuf::from(path.to_string())).ok_or_else(||"Selected file has no path".into())
        }).collect()
    }
}

// Native menu callbacks enqueue commands instead of re-entering a borrowed Runner.
use crate::native_menu::{MenuCommand, NativeMenu, NativeMenuAction};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem, NSEventModifierFlags};
use objc2_foundation::NSString;
use std::{cell::RefCell, rc::Rc};

struct MenuTargetData {
    commands: RefCell<Vec<MenuCommand>>,
    pending: Rc<RefCell<Vec<MenuCommand>>>,
    wake: Box<dyn Fn()>,
}
declare_class!(
    struct MenuTarget;
    // SAFETY: NSObject subclass, accessed only from AppKit's main thread.
    unsafe impl ClassType for MenuTarget {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "RuruMenuTarget";
    }
    impl DeclaredClass for MenuTarget { type Ivars = MenuTargetData; }
    // SAFETY: one-object AppKit target/action signature; sender is the menu item we created.
    unsafe impl MenuTarget {
        #[method(invoke:)]
        fn invoke(&self, item: &NSMenuItem) {
            // SAFETY: main-thread getter on the live sender.
            let index = unsafe { item.tag() } as usize;
            if let Some(command) = self.ivars().commands.borrow().get(index).copied() {
                self.ivars().pending.borrow_mut().push(command);
                (self.ivars().wake)();
            }
        }
    }
);
pub(crate) struct ApplicationMenu {
    target: Retained<MenuTarget>,
    pending: Rc<RefCell<Vec<MenuCommand>>>,
    current: Vec<NativeMenu>,
}
impl ApplicationMenu {
    pub(crate) fn new(wake: impl Fn() + 'static) -> Self {
        let mtm = MainThreadMarker::new().expect("event loop runs on main thread");
        let pending = Rc::new(RefCell::new(Vec::new()));
        let object = mtm.alloc::<MenuTarget>().set_ivars(MenuTargetData { commands: RefCell::new(Vec::new()), pending: pending.clone(), wake: Box::new(wake) });
        // SAFETY: freshly allocated NSObject subclass, initialized ivars.
        let target = unsafe { msg_send_id![super(object), init] };
        Self { target, pending, current: Vec::new() }
    }
    pub(crate) fn drain(&self) -> Vec<MenuCommand> { std::mem::take(&mut *self.pending.borrow_mut()) }
    pub(crate) fn update(&mut self, menus: Vec<NativeMenu>) {
        if menus == self.current { return; }
        let Some(mtm) = MainThreadMarker::new() else { return };
        // SAFETY: all AppKit objects are created, retained and configured on the event-loop
        // main thread. The owner retains the action target for the whole Runner lifetime.
        unsafe {
            let app = NSApplication::sharedApplication(mtm);
            let bar = NSMenu::new(mtm);
            let mut commands = self.target.ivars().commands.borrow_mut();
            commands.clear();
            app.setWindowsMenu(None);
            app.setHelpMenu(None);
            for description in &menus {
                let menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(&description.title));
                menu.setAutoenablesItems(false);
                for row in &description.items {
                    let Some(command) = row.action else { menu.addItem(&NSMenuItem::separatorItem(mtm)); continue };
                    let native = match command {
                        MenuCommand::Standard(action) => match action {
                            NativeMenuAction::About => Some(sel!(orderFrontStandardAboutPanel:)),
                            NativeMenuAction::Hide => Some(sel!(hide:)),
                            NativeMenuAction::HideOthers => Some(sel!(hideOtherApplications:)),
                            NativeMenuAction::ShowAll => Some(sel!(unhideAllApplications:)),
                            NativeMenuAction::Quit => Some(sel!(terminate:)),
                            NativeMenuAction::Close => Some(sel!(performClose:)),
                            NativeMenuAction::Minimize => Some(sel!(performMiniaturize:)),
                            NativeMenuAction::Zoom => Some(sel!(zoom:)),
                            NativeMenuAction::Fullscreen => Some(sel!(toggleFullScreen:)),
                            NativeMenuAction::BringAllToFront => Some(sel!(arrangeInFront:)),
                            _ => None,
                        },
                        _ => None,
                    };
                    let fullscreen = row.shortcut.is_none() && command == MenuCommand::Standard(NativeMenuAction::Fullscreen);
                    let key = row.shortcut.and_then(|k| match k.key { crate::ShortcutKey::Char(c) => Some(c.to_string()), _ => None }).unwrap_or_else(|| if fullscreen { "f".into() } else { String::new() });
                    let item = NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(&row.title), Some(native.unwrap_or(sel!(invoke:))), &NSString::from_str(&key));
                    if native.is_none() {
                        item.setTarget(Some(&self.target));
                        item.setTag(commands.len() as isize);
                        commands.push(command);
                    }
                    let mut flags = NSEventModifierFlags::empty();
                    if let Some(key) = row.shortcut {
                        if key.mods.command { flags |= NSEventModifierFlags::NSEventModifierFlagCommand; }
                        if key.mods.shift { flags |= NSEventModifierFlags::NSEventModifierFlagShift; }
                        if key.mods.alt { flags |= NSEventModifierFlags::NSEventModifierFlagOption; }
                    }
                    if fullscreen { flags = NSEventModifierFlags::NSEventModifierFlagCommand | NSEventModifierFlags::NSEventModifierFlagControl; }
                    item.setKeyEquivalentModifierMask(flags);
                    item.setEnabled(row.enabled);
                    item.setState(if row.checked { 1 } else { 0 });
                    menu.addItem(&item);
                }
                let top = NSMenuItem::new(mtm);
                top.setTitle(&NSString::from_str(&description.title));
                top.setSubmenu(Some(&menu));
                bar.addItem(&top);
                if description.role == 1 { app.setWindowsMenu(Some(&menu)); }
                if description.role == 2 { app.setHelpMenu(Some(&menu)); }
            }
            app.setMainMenu(Some(&bar));
        }
        self.current = menus;
    }
}
pub(crate) fn is_key(window: &Window) -> bool { ns_window(window).is_some_and(|win| win.isKeyWindow()) }

/// AppKit's native drag loop may suppress CursorMoved; sample only during file hover.
pub(crate) fn drag_pointer(window:&Window)->Option<(f32,f32)> {
    let view=content_view(window)?;
    let win=view.window()?;
    // SAFETY: both objects belong to the live winit window, on its main event thread.
    let point=unsafe {view.convertPoint_fromView(win.mouseLocationOutsideOfEventStream(),None)};
    let y=if view.isFlipped(){point.y}else{view.bounds().size.height-point.y};
    Some((point.x as f32,y as f32))
}
