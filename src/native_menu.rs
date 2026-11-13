//! Declarative desktop menu descriptions. AppKit owns presentation on macOS.
use crate::Shortcut;

/// A standard application, window or text-editing command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeMenuAction { About, Hide, HideOthers, ShowAll, Quit, Close, Minimize, Zoom, Fullscreen, BringAllToFront, Undo, Redo, Cut, Copy, Paste, SelectAll }
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MenuCommand { Custom(u32), Key(Shortcut), Standard(NativeMenuAction) }
/// One native menu row. A separator has no command.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMenuItem {
    pub(crate) title: String,
    pub(crate) action: Option<MenuCommand>,
    pub(crate) shortcut: Option<Shortcut>,
    pub(crate) enabled: bool,
    pub(crate) checked: bool,
}
impl NativeMenuItem {
    fn new(title: impl Into<String>, action: Option<MenuCommand>) -> Self { Self { title: title.into(), action, shortcut: None, enabled: true, checked: false } }
    /// Dispatch an application-defined command to `App::native_menu`'s handler.
    pub fn command(title: impl Into<String>, id: u32) -> Self { Self::new(title, Some(MenuCommand::Custom(id))) }
    /// Invoke a shortcut registered by the active window's view.
    pub fn key_action(title: impl Into<String>, key: Shortcut) -> Self { Self::new(title, Some(MenuCommand::Key(key))).shortcut(key) }
    /// Invoke a standard OS or text-editing action.
    pub fn standard(title: impl Into<String>, action: NativeMenuAction) -> Self {
        use NativeMenuAction::*;
        let mut item = Self::new(title, Some(MenuCommand::Standard(action)));
        item.shortcut = match action {
            Hide => Some(Shortcut::cmd('h')), HideOthers => Some(Shortcut::cmd('h').alt()),
            Quit => Some(Shortcut::cmd('q')), Close => Some(Shortcut::cmd('w')), Minimize => Some(Shortcut::cmd('m')),
            Undo => Some(Shortcut::cmd('z')), Redo => Some(Shortcut::cmd('z').shift()),
            Cut => Some(Shortcut::cmd('x')), Copy => Some(Shortcut::cmd('c')), Paste => Some(Shortcut::cmd('v')), SelectAll => Some(Shortcut::cmd('a')),
            _ => None,
        };
        item
    }
    pub fn separator() -> Self { Self::new("", None) }
    pub fn shortcut(mut self, key: Shortcut) -> Self { self.shortcut = Some(key); self }
    pub fn enabled(mut self, enabled: bool) -> Self { self.enabled = enabled; self }
    pub fn checked(mut self, checked: bool) -> Self { self.checked = checked; self }
}
/// A top-level menu. The first menu is the application menu.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMenu { pub(crate) title: String, pub(crate) items: Vec<NativeMenuItem>, pub(crate) role: u8 }
impl NativeMenu {
    pub fn new(title: impl Into<String>, items: Vec<NativeMenuItem>) -> Self { Self { title: title.into(), items, role: 0 } }
    /// Let AppKit manage the window list in this menu.
    pub fn window(mut self) -> Self { self.role = 1; self }
    /// Let AppKit add its menu search field.
    pub fn help(mut self) -> Self { self.role = 2; self }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn dispatch<S: 'static>(ui: &mut crate::Ui<S>, command: MenuCommand, paste: &str) -> bool {
    use NativeMenuAction as A;
    use crate::KeyInput as K;
    match command {
        MenuCommand::Key(key) => { ui.set_modifiers(key.mods); ui.key_shortcut(key.key) }
        MenuCommand::Standard(A::Paste) => ui.paste(paste),
        MenuCommand::Standard(action) => {
            let key = match action { A::Undo => K::Undo, A::Redo => K::Redo, A::Cut => K::Cut, A::Copy => K::Copy, A::SelectAll => K::SelectAll, _ => return false };
            ui.key(key)
        }
        MenuCommand::Custom(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    #[test]
    fn menu_shortcut_uses_its_modifiers_without_a_physical_key_press() {
        let mut ui = crate::Ui::new(0, |_: &i32| text("menu").shortcut(Shortcut::cmd('n'), |s| *s += 1));
        ui.frame((300.0, 100.0));
        assert!(dispatch(&mut ui, MenuCommand::Key(Shortcut::cmd('n')), ""));
        assert_eq!(ui.state, 1);
    }
    #[test]
    fn menu_editing_reaches_focused_field_and_undo_history() {
        let mut ui = crate::Ui::new(String::from("before"), |s: &String| text_field(s, "", |s: &mut String, value| *s = value).size(300.0, 50.0));
        ui.frame((300.0, 50.0));
        ui.pointer_down((30.0, 25.0), false); ui.pointer_up((30.0, 25.0));
        let command = |a| MenuCommand::Standard(a);
        assert!(dispatch(&mut ui, command(NativeMenuAction::SelectAll), ""));
        assert!(dispatch(&mut ui, command(NativeMenuAction::Copy), ""));
        assert_eq!(ui.selection_text().as_deref(), Some("before"));
        assert!(dispatch(&mut ui, command(NativeMenuAction::Paste), "after"));
        assert_eq!(ui.state, "after");
        assert!(dispatch(&mut ui, command(NativeMenuAction::Undo), ""));
        assert_eq!(ui.state, "before");
        assert!(dispatch(&mut ui, command(NativeMenuAction::Redo), ""));
        assert_eq!(ui.state, "after");
        dispatch(&mut ui, command(NativeMenuAction::SelectAll), "");
        assert!(dispatch(&mut ui, command(NativeMenuAction::Cut), ""));
        assert_eq!(ui.state, "");
    }
}
