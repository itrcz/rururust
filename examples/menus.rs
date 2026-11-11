//! Menus, popovers, tooltips, context menus and shortcuts in a chat-like window.
//!
//!   cargo run --release --example menus
//!   cargo run --release --example menus -- --screenshot out.png [menu|effort|tooltip|add|profile|context]

use ruru::prelude::*;

struct Icons {
    pencil: Image,
    pin: Image,
    plus: Image,
    fork: Image,
    clock: Image,
    share: Image,
    copy: Image,
    window: Image,
    archive: Image,
    back: Image,
    more: Image,
    bolt: Image,
    reset: Image,
    file: Image,
    goal: Image,
    gear: Image,
    user: Image,
    help: Image,
    logout: Image,
    pet: Image,
    mail: Image,
}

/// An outline icon from a 24×24 path, like SF Symbols.
fn svg(d: &str) -> Image {
    svg_w(d, 1.7)
}

fn svg_w(d: &str, w: f32) -> Image {
    let src = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="none" stroke="black" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round" d="{d}"/></svg>"#
    );
    Image::from_svg(src.as_bytes(), 64).expect("icon")
}

impl Icons {
    fn new() -> Self {
        Icons {
            pencil: svg("M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"),
            pin: svg("M9 4h6l-1 6 3 3H7l3-3zM12 16v5"),
            plus: svg("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 8v8M8 12h8"),
            fork: svg("M6 3v12M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 9c0 5-12 3-12 9"),
            clock: svg("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2"),
            share: svg("M12 15V3M8 7l4-4 4 4M5 13v6h14v-6"),
            copy: svg("M9 9h11v11H9zM5 15H4V4h11v1"),
            window: svg("M4 5h16v14H4zM4 9h16"),
            archive: svg("M3 5h18v4H3zM5 9v10h14V9M10 13h4"),
            back: svg("M19 12H5M11 6l-6 6 6 6"),
            more: svg_w("M5 12h.01M12 12h.01M19 12h.01", 3.2),
            bolt: svg("M13 3 5 14h6l-1 7 8-11h-6z"),
            reset: svg("M4 12a8 8 0 1 0 2.3-5.7M4 4v5h5"),
            file: svg("M6 3h8l4 4v14H6zM14 3v4h4"),
            goal: svg("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z"),
            gear: svg("M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2"),
            user: svg("M12 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM4 21c1-4 4-6 8-6s7 2 8 6"),
            help: svg("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5v.7M12 17h.01"),
            logout: svg("M15 4h4v16h-4M10 8l-4 4 4 4M6 12h10"),
            pet: svg("M7 13a5 5 0 0 0 10 0c0-3-2-6-5-6s-5 3-5 6zM8 5l1 3M16 5l-1 3"),
            mail: svg("M3 6h18v12H3zM3 6l9 7 9-7"),
        }
    }
}

struct Demo {
    icons: Icons,
    status: String,
    draft: String,
    effort: f32,
    effort_open: bool,
    add_open: bool,
    pinned: bool,
}

const EFFORTS: [&str; 5] = ["Minimal", "Low", "Medium", "High", "Max"];
const CHATS: [&str; 7] = [
    "Rust for cross-platform",
    "Encyclopedia page in GPT",
    "Plan Team subscriptions",
    "kubectl and API Cloud",
    "Butcher Splash prototype",
    "Walk cycle animation",
    "Check VPN on the server",
];

const BG: Color = Color::hex(0x1C1C1E);
const SIDEBAR: Color = Color::hex(0x161618);
const DIM: Color = Color::rgba(1.0, 1.0, 1.0, 0.5);

fn say(s: &mut Demo, what: impl Into<String>) {
    s.status = what.into();
}

fn icon_button(img: &Image) -> View<Demo> {
    // The glyph lights up with the highlight, as in a macOS toolbar.
    zstack(icon(img).size(18.0, 18.0)).size(28.0, 28.0).radius(7.0).hover(Color::WHITE.alpha(0.1)).color(DIM).hover_color(Color::WHITE)
}

/// A macOS 26 dark menu: rows on a 28.5 pt step, small dimmed shortcuts, 9 pt separators, a
/// close key shadow over a wide ambient one and a faint light rim inside the border.
fn mac_menu() -> MenuStyle {
    MenuStyle {
        background: Color::hex(0x2E2D2D),
        radius: 15.0,
        border: Some((0.5, Color::WHITE.alpha(0.15))),
        shadow: Some(Shadow::new(Color::BLACK.alpha(0.3), 3.0, 1.0)),
        ambient: Some(Shadow::new(Color::BLACK.alpha(0.35), 24.0, 8.0).spread(-2.0)),
        rim: Some(Shadow::new(Color::WHITE.alpha(0.05), 0.0, 0.0).spread(1.0).inset()),
        padding: 4.5,
        row_height: 28.5,
        row_radius: 10.0,
        row_padding: 8.0,
        icon_gap: 7.0,
        highlight: Color::hex(0x3F3E3E),
        dim: Color::hex(0x959494),
        separator: Color::hex(0x3F3E3E),
        separator_height: 9.0,
        shortcut_size: Some(11.5),
        ..MenuStyle::default()
    }
}

fn more_menu(s: &Demo) -> View<Demo> {
    let i = &s.icons;
    menu(vec![
        MenuItem::action("Rename", |s: &mut Demo| say(s, "Rename")).icon(&i.pencil).shortcut(Shortcut::cmd('r').alt()),
        MenuItem::action(if s.pinned { "Unpin" } else { "Pin" }, |s: &mut Demo| s.pinned = !s.pinned)
            .icon(&i.pin)
            .shortcut(Shortcut::cmd('p').alt()),
        MenuItem::separator(),
        MenuItem::action("New side chat", |s: &mut Demo| say(s, "New side chat")).icon(&i.plus).shortcut(Shortcut::cmd('s').alt()),
        MenuItem::submenu(
            "Fork",
            vec![
                MenuItem::action("From here", |s: &mut Demo| say(s, "Fork from here")),
                MenuItem::action("From the start", |s: &mut Demo| say(s, "Fork from the start")),
                MenuItem::action("Into a new project", |s: &mut Demo| say(s, "Fork into a project")).disabled(),
            ],
        )
        .icon(&i.fork),
        MenuItem::action("Add scheduled task…", |s: &mut Demo| say(s, "Schedule")).icon(&i.clock),
        MenuItem::separator(),
        MenuItem::action("Share", |s: &mut Demo| say(s, "Share")).icon(&i.share),
        MenuItem::submenu(
            "Copy",
            vec![
                MenuItem::action("Link", |s: &mut Demo| say(s, "Copied link")),
                MenuItem::action("Markdown", |s: &mut Demo| say(s, "Copied markdown")),
            ],
        )
        .icon(&i.copy),
        MenuItem::separator(),
        MenuItem::action("Open in new window", |s: &mut Demo| say(s, "New window")).icon(&i.window),
        MenuItem::separator(),
        MenuItem::action("Archive", |s: &mut Demo| say(s, "Archived")).icon(&i.archive).shortcut(Shortcut::cmd('a').shift()),
    ])
}

fn add_menu(s: &Demo, width: f32) -> View<Demo> {
    let i = &s.icons;
    menu(vec![
        MenuItem::section("Add"),
        MenuItem::action("Files and folders", |s: &mut Demo| say(s, "Files")).icon(&i.file),
        MenuItem::action("Goal", |s: &mut Demo| say(s, "Goal")).icon(&i.goal).hint("Set a goal to keep pursuing"),
        MenuItem::action("Plan mode", |s: &mut Demo| say(s, "Plan mode")).icon(&i.clock).hint("Turn plan mode on"),
        MenuItem::section("Plugins"),
        MenuItem::action("Documents", |s: &mut Demo| say(s, "Documents")).icon(&i.file).hint("Create and edit documents"),
        MenuItem::action("Spreadsheets", |s: &mut Demo| say(s, "Sheets")).icon(&i.copy).hint("Create and edit spreadsheets"),
    ])
    .menu_style(MenuStyle {
        row_height: 28.5,
        highlight_inset: 2.25,
        row_radius: 12.0,
        section_size: Some(12.5),
        section_weight: Weight::Regular,
        section_color: Some(Color::hex(0x969696)),
        section_gap: 6.0,
        ..MenuStyle::default()
    })
    .width(width)
}

fn profile_menu(s: &Demo) -> View<Demo> {
    let i = &s.icons;
    menu(vec![
        MenuItem::submenu(
            "Ilya Trikoz",
            vec![
                MenuItem::action("Personal", |s: &mut Demo| say(s, "Personal")).checked(true),
                MenuItem::action("Team", |s: &mut Demo| say(s, "Team")),
            ],
        )
        .icon(&i.user)
        .hint("Pro"),
        MenuItem::separator(),
        MenuItem::action("Show Pet", |s: &mut Demo| say(s, "Pet")).icon(&i.pet),
        MenuItem::action("Invite a friend", |s: &mut Demo| say(s, "Invite")).icon(&i.mail),
        MenuItem::action("Profile", |s: &mut Demo| say(s, "Profile")).icon(&i.user),
        MenuItem::action("Settings", |s: &mut Demo| say(s, "Settings")).icon(&i.gear).shortcut(Shortcut::cmd(',')),
        MenuItem::separator(),
        MenuItem::submenu("Help", vec![MenuItem::action("Docs", |s: &mut Demo| say(s, "Docs"))]).icon(&i.help),
        MenuItem::action("Log out", |s: &mut Demo| say(s, "Log out")).icon(&i.logout),
    ])
}

fn sidebar(s: &Demo) -> View<Demo> {
    let rows: Vec<View<Demo>> = CHATS
        .iter()
        .enumerate()
        .map(|(n, title)| {
            let ctx = menu(vec![
                MenuItem::action("Open", move |s: &mut Demo| say(s, format!("Open #{n}"))),
                MenuItem::action("Rename", move |s: &mut Demo| say(s, format!("Rename #{n}"))),
                MenuItem::separator(),
                MenuItem::action("Delete", move |s: &mut Demo| say(s, format!("Delete #{n}"))),
            ]);
            text(*title)
                .font_size(13.0)
                .truncate(Truncate::Fade)
                .padding_xy(10.0, 6.0)
                .fill_width()
                .radius(7.0)
                .hover(Color::WHITE.alpha(0.06))
                .on_tap(move |s: &mut Demo| say(s, format!("Chat #{n}")))
                .context_menu(ctx)
        })
        .collect();
    let avatar = hstack((
        zstack(text("IT").font_size(11.0).weight(Weight::Semibold)).size(26.0, 26.0).background(Color::INDIGO).radius(13.0),
        text("Ilya Trikoz").font_size(13.0),
    ))
    .spacing(8.0)
    .padding(6.0)
    .radius(8.0)
    .hover(Color::WHITE.alpha(0.06))
    .menu_at(Placement::top().start(), profile_menu(s));
    vstack((
        text("Recents").font_size(12.0).color(DIM).padding_ltrb(10.0, 0.0, 0.0, 4.0),
        vstack(rows).spacing(1.0),
        spacer(),
        avatar,
    ))
    .align(Align::Start)
    .padding_ltrb(8.0, 48.0, 8.0, 10.0)
    .width(220.0)
    .fill_height()
    .background(SIDEBAR)
}

fn effort_popover(s: &Demo) -> View<Demo> {
    let level = EFFORTS[(s.effort * 4.0).round() as usize];
    vstack((
        hstack((
            icon(&s.icons.bolt).size(16.0, 16.0).color(DIM),
            spacer(),
            text(level).font_size(15.0).weight(Weight::Semibold).color(Color::BLUE),
            spacer(),
            zstack(icon(&s.icons.reset).size(16.0, 16.0).color(DIM)).on_tap(|s: &mut Demo| s.effort = 0.75),
        )),
        text("GPT-6 Astra ›").font_size(12.0).color(DIM),
        slider(s.effort, |s: &mut Demo, v| s.effort = v).steps(5).slider_style(SliderStyle {
            track_height: 24.0,
            track: Color::hex(0x434242),
            track_border: Some((1.0, Color::hex(0x535252))),
            fill: Color::hex(0x4D81EF),
            tick_on: Color::hex(0x83A7F4),
            tick_off: Some(Color::hex(0x727171)),
            ..SliderStyle::default()
        }),
    ))
    .spacing(8.0)
    .padding(14.0)
    .width(260.0)
    .background(Color::hex(0x2A2A2C))
    .border(0.5, Color::WHITE.alpha(0.12))
    .radius(16.0)
    .shadow(Color::BLACK.alpha(0.4), 28.0, 8.0)
}

fn composer(s: &Demo, width: f32) -> View<Demo> {
    let plus = zstack(text("+").font_size(18.0).color(DIM))
        .size(28.0, 28.0)
        .radius(14.0)
        .hover(Color::WHITE.alpha(0.1))
        .tooltip_shortcut("Add", "⌘U")
        .on_tap(|s: &mut Demo| s.add_open = true);
    let effort = hstack((text(EFFORTS[(s.effort * 4.0).round() as usize]).font_size(12.5), text("⌄").font_size(12.0).color(DIM)))
        .spacing(5.0)
        .padding_xy(12.0, 6.0)
        .background(Color::WHITE.alpha(0.07))
        .radius(14.0)
        .on_tap(|s: &mut Demo| s.effort_open = !s.effort_open)
        .popover(s.effort_open, Placement::top(), |s: &mut Demo| s.effort_open = false, effort_popover(s));
    vstack((
        text_field(s.draft.as_str(), "Ask anything", |s: &mut Demo, v| s.draft = v).plain().font_size(14.0),
        hstack((plus, spacer(), effort)).spacing(8.0),
    ))
    .spacing(10.0)
    .padding(10.0)
    .width(width)
    .background(Color::hex(0x2A2A2C))
    .radius(18.0)
    // The list sits over the whole composer; choosing a row calls `on_dismiss` too.
    .popover(s.add_open, Placement::top().start(), |s: &mut Demo| s.add_open = false, add_menu(s, width))
}

fn view(s: &Demo, env: &Env) -> View<Demo> {
    let main_w = env.width - 220.0;
    let toolbar = hstack((
        icon_button(&s.icons.back).on_tap(|s: &mut Demo| say(s, "Back")).tooltip_shortcut("Back", "⌘["),
        spacer(),
        text(if s.pinned { "📌 Rust for cross-platform" } else { "Rust for cross-platform" }).font_size(13.0).weight(Weight::Semibold),
        spacer(),
        icon_button(&s.icons.more).tooltip("More").menu_at(Placement::bottom().end().gap(4.5).margin(6.5), more_menu(s).menu_style(mac_menu())),
    ))
    .padding_xy(12.0, 8.0)
    .height(44.0);
    let body = vstack((
        text("Right-click a chat on the left, open … at the top right, the avatar, + or the effort button. Keyboard: ↑↓ ←→ Enter Esc in menus; ⌘K, ⌘[ anywhere.")
            .font_size(14.0)
            .line_height(1.5)
            .color(Color::WHITE.alpha(0.8)),
        text(format!("Last action: {}", s.status)).font_size(13.0).color(DIM),
    ))
    .spacing(12.0)
    .align(Align::Start)
    .padding(24.0)
    .max_width(640.0);
    let main = vstack((toolbar, body, spacer(), vstack(composer(s, (main_w - 48.0).min(640.0))).padding_ltrb(0.0, 0.0, 0.0, 16.0)))
        .fill();
    hstack((sidebar(s), main))
        .fill()
        .background(BG)
        .color(Color::WHITE.alpha(0.92))
        .shortcut(Shortcut::cmd('k'), |s: &mut Demo| say(s, "⌘K: command palette"))
        .shortcut(Shortcut::cmd('['), |s: &mut Demo| say(s, "Back (⌘[)"))
        .shortcut(Shortcut::cmd('u'), |s: &mut Demo| s.add_open = true)
}

fn main() {
    let state = Demo { icons: Icons::new(), status: "—".into(), draft: String::new(), effort: 0.75, effort_open: false, add_open: false, pinned: false };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--screenshot") {
            let scene = args.get(i + 2).cloned().unwrap_or_else(|| "menu".into());
            let mut ui = ruru::Ui::new(state, view);
            ui.background = Some(BG);
            // Input sent before frame `n`, as a user would: the scene's anchors are at fixed places
            // in the 900×600 window.
            let input = move |ui: &mut ruru::Ui<Demo>, n: u32| {
                let click = |ui: &mut ruru::Ui<Demo>, at| {
                    ui.pointer_move(at);
                    ui.pointer_down(at, false);
                    ui.pointer_up(at);
                };
                match (scene.as_str(), n) {
                    ("menu", 1) => click(ui, (874.0, 22.0)),
                    ("menu", 3) => drop(ui.pointer_move((760.0, 156.0))), // Fork
                    ("effort", 1) => ui.state.effort_open = true,
                    ("tooltip", 1) => {
                        ui.pointer_move((246.0, 22.0));
                    }
                    ("add", 1) => click(ui, (258.0, 563.0)),
                    ("add", 3) => drop(ui.pointer_move((400.0, 382.0))), // Goal
                    ("profile", 1) => click(ui, (60.0, 576.0)),
                    ("profile", 3) => drop(ui.pointer_move((80.0, 488.0))),
                    ("context", 1) => {
                        ui.pointer_move((90.0, 100.0));
                        ui.secondary_click((90.0, 100.0));
                    }
                    ("context", 3) => drop(ui.pointer_move((120.0, 135.0))),
                    _ => {}
                }
                if n > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
            };
            ruru::headless::render_png_after(&mut ui, 900, 600, 2.0, 45, input, &args[i + 1]);
            return;
        }
    }
    App::new(state, view).title("ruru menus").size(900.0, 600.0).background(BG).run();
}
