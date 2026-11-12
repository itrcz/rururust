use ruru::prelude::*;
use ruru::{KeyInput, Ui};
use ruru::{menu, MenuItem, MenuStyle, Placement, Shortcut};

struct S {
    n: i32,
    on: bool,
    v: f32,
}

fn view(s: &S) -> View<S> {
    vstack((
        button("add", |s: &mut S| s.n += 1).size(200.0, 100.0),
        toggle(s.on, |s: &mut S, on| s.on = on),
        slider(s.v, |s: &mut S, v| s.v = v).width(200.0),
    ))
    .align(Align::Start)
    .place(Align::Start, Align::Start)
}

fn ui() -> Ui<S> {
    let mut ui = Ui::new(S { n: 0, on: false, v: 0.0 }, |s: &S| zstack(view(s)).fill());
    ui.frame((400.0, 400.0));
    ui
}

#[test]
fn tap_fires_only_when_released_inside() {
    let mut ui = ui();
    ui.pointer_down((100.0, 50.0), false);
    ui.pointer_up((100.0, 50.0));
    assert_eq!(ui.state.n, 1);
    ui.pointer_down((100.0, 50.0), false);
    ui.pointer_up((390.0, 390.0)); // dragged away: cancelled
    assert_eq!(ui.state.n, 1);
}

#[test]
fn toggle_flips() {
    let mut ui = ui();
    ui.pointer_down((25.0, 115.0), false);
    ui.pointer_up((25.0, 115.0));
    assert!(ui.state.on);
}

#[test]
fn slider_tracks_the_pointer() {
    let mut ui = ui();
    ui.pointer_down((100.0, 145.0), false);
    assert!((ui.state.v - 0.5).abs() < 0.01, "v = {}", ui.state.v);
    ui.pointer_move((500.0, 145.0));
    assert_eq!(ui.state.v, 1.0);
    ui.pointer_up((500.0, 145.0));
}

#[test]
fn press_animates_then_settles() {
    let mut ui = ui();
    assert!(!ui.animating);
    ui.pointer_down((100.0, 50.0), false);
    ui.frame((400.0, 400.0));
    assert!(ui.animating);
}

struct Form {
    name: String,
    city: usize,
}

fn form() -> Ui<Form> {
    let mut ui = Ui::new(Form { name: String::new(), city: 0 }, |s: &Form| {
        zstack(
            vstack((
                text_field(s.name.as_str(), "Name", |s: &mut Form, v| s.name = v).width(300.0),
                picker(&["One", "Two", "Three"], s.city, |s: &mut Form, i| s.city = i).width(300.0),
            ))
            .align(Align::Start)
            .place(Align::Start, Align::Start),
        )
        .fill()
    });
    ui.frame((400.0, 600.0));
    ui
}

#[test]
fn typing_needs_focus_and_edits_at_the_caret() {
    use ruru::KeyInput::*;
    let mut ui = form();
    assert!(!ui.text_input("x"), "nothing is focused yet");
    ui.pointer_down((150.0, 20.0), false);
    ui.pointer_up((150.0, 20.0));
    assert!(ui.has_focus());
    // Several keys before the next frame must all land.
    ui.text_input("H");
    ui.text_input("i");
    ui.text_input("й");
    assert_eq!(ui.state.name, "Hiй");
    ui.frame((400.0, 600.0));
    ui.key(Backspace);
    assert_eq!(ui.state.name, "Hi");
    ui.key(Home);
    ui.text_input(">");
    ui.key(Delete);
    assert_eq!(ui.state.name, ">i");
    ui.key(End);
    ui.key(Left);
    ui.text_input("-");
    assert_eq!(ui.state.name, ">-i");
    ui.key(Escape);
    assert!(!ui.has_focus());
    // Clicking elsewhere also drops focus.
    ui.pointer_down((150.0, 20.0), false);
    ui.pointer_up((150.0, 20.0));
    ui.pointer_down((350.0, 500.0), false);
    assert!(!ui.has_focus());
}

#[test]
fn clicking_inside_text_places_the_caret() {
    let mut ui = form();
    ui.state.name = "abcdef".into();
    ui.frame((400.0, 600.0));
    ui.pointer_down((15.0, 20.0), false); // left edge of the text
    ui.pointer_up((15.0, 20.0));
    ui.text_input("_");
    assert_eq!(ui.state.name, "_abcdef");
}

#[test]
fn picker_opens_a_menu_and_selects() {
    let mut ui = form();
    let picker = (150.0, 68.0);
    ui.pointer_down(picker, false);
    ui.pointer_up(picker);
    ui.frame((400.0, 600.0)); // menu appears below the picker
    let second_row = (150.0, 68.0 + 21.0 + 6.0 + 6.0 + 44.0 * 1.5);
    ui.pointer_down(second_row, false);
    ui.pointer_up(second_row);
    assert_eq!(ui.state.city, 1);
    // Open again and click outside: closes without changing the choice.
    ui.frame((400.0, 600.0));
    ui.pointer_down(picker, false);
    ui.pointer_up(picker);
    ui.frame((400.0, 600.0));
    ui.pointer_down((390.0, 590.0), false);
    ui.pointer_up((390.0, 590.0));
    ui.frame((400.0, 600.0));
    ui.pointer_down(second_row, false);
    ui.pointer_up(second_row);
    assert_eq!(ui.state.city, 1);
}

#[test]
fn tab_drop_selects_on_tap_and_on_drag_release() {
    struct T {
        tab: usize,
    }
    fn labels_above_glass(ui:&Ui<T>) {
        let mut labels=0;let mut panes=0;
        for cmd in &ui.list.cmds {match cmd {
            ruru::Cmd::Text(_)=>labels+=1,
            ruru::Cmd::Quad(q) if q.p0[3]==2.0=>{
                panes+=1;assert_eq!(labels,0,"glass must not sample the tab labels");
            },_=>{},
        }}
        assert_eq!(panes,2,"both the bar and moving lens retain their glass");
        assert_eq!(labels,3,"each label is painted once, above the glass");
    }
    let mut ui = Ui::new(T { tab: 0 }, |s: &T| {
        zstack(tab_bar(&["One", "Two", "Three"], s.tab, |s: &mut T, i| s.tab = i).place(Align::Start, Align::Start)).fill()
    });
    ui.frame((400.0, 200.0));
    labels_above_glass(&ui);
    // Three equal tabs: the bar is 6 pt of padding + 3 tabs + 6 pt.
    let tab_w = 40.0 + 44.0; // rough; only the ordering of the taps matters below
    ui.pointer_down((6.0 + tab_w * 2.5, 28.0), false);
    ui.pointer_up((6.0 + tab_w * 2.5, 28.0));
    assert_eq!(ui.state.tab, 2);
    // Grab the drop, drag it left and let go below the bar: the tab under it wins.
    ui.frame((400.0, 200.0));
    labels_above_glass(&ui);
    ui.pointer_down((6.0 + tab_w * 2.5, 28.0), false);
    ui.pointer_move((10.0, 150.0));
    ui.frame((400.0, 200.0));
    labels_above_glass(&ui);
    assert!(ui.animating, "the drop is moving");
    ui.pointer_up((10.0, 150.0));
    assert_eq!(ui.state.tab, 0);
    ui.frame((400.0,200.0));labels_above_glass(&ui);
}

#[test]
fn view_can_adapt_to_the_screen_size() {
    let mut ui = Ui::new(0u32, |_: &u32, env: &Env| {
        let label = if env.is_phone() { "phone" } else { "wide" };
        button(label, move |s: &mut u32| *s = if label == "phone" { 1 } else { 2 }).fill()
    });
    ui.frame((400.0, 800.0));
    ui.pointer_down((200.0, 400.0), false);
    ui.pointer_up((200.0, 400.0));
    assert_eq!(ui.state, 1);
    ui.frame((1200.0, 800.0));
    ui.pointer_down((200.0, 400.0), false);
    ui.pointer_up((200.0, 400.0));
    assert_eq!(ui.state, 2);
}

#[test]
fn lazy_list_builds_only_visible_rows() {
    use std::cell::Cell;
    use std::rc::Rc;
    let built = Rc::new(Cell::new(0usize));
    let counter = built.clone();
    let mut ui = Ui::new((), move |_: &()| {
        let counter = counter.clone();
        scroll(list(100_000, 40.0, move |i| {
            counter.set(counter.get() + 1);
            text(format!("row {i}")).height(40.0)
        }))
    });
    ui.frame((300.0, 400.0));
    assert!(built.get() > 0 && built.get() <= 12, "built {} rows for a 400 pt viewport", built.get());
}

#[test]
fn rubber_band_springs_back_and_plain_wheel_stops_at_the_end() {
    use ruru::WheelPhase::*;
    let mut ui = Ui::new((), |_: &()| scroll(rect().height(2000.0).fill_width()));
    ui.frame((300.0, 400.0));
    // A mouse wheel cannot leave the content.
    assert!(!ui.wheel((150.0, 200.0), 500.0, Line), "already at the top");
    // A trackpad can pull past the top…
    ui.wheel((150.0, 200.0), 0.0, Started);
    assert!(ui.wheel((150.0, 200.0), 120.0, Moved));
    ui.wheel((150.0, 200.0), 0.0, Ended);
    // …and the content settles back on its own.
    let mut frames = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(4));
        ui.frame((300.0, 400.0));
        frames += 1;
        if !ui.animating && ui.wake_in.is_none_or(|w| w > 0.5) {
            break;
        }
        assert!(frames < 2000, "never settled");
    }
    assert!(frames > 3, "it should animate, not jump");
}

#[test]
fn modal_blocks_what_is_underneath_and_dismisses_on_outside_tap() {
    struct M {
        open: bool,
        taps: u32,
    }
    let mut ui = Ui::new(M { open: true, taps: 0 }, |s: &M| {
        zstack((
            button("under", |s: &mut M| s.taps += 1).fill(),
            modal(s.open, |s: &mut M| s.open = false, text("dialog").padding(40.0)),
        ))
        .fill()
    });
    for _ in 0..3 {
        ui.frame((400.0, 600.0));
    }
    ui.pointer_down((10.0, 10.0), false);
    ui.pointer_up((10.0, 10.0));
    assert_eq!(ui.state.taps, 0, "the button under the modal must not fire");
    assert!(!ui.state.open, "tapping outside dismisses");
}

#[test]
fn after_runs_later_on_the_state() {
    let mut ui = Ui::new(0u32, |_: &u32| button("go", |_: &mut u32| ruru::after(0.02, |s: &mut u32| *s = 7)).fill());
    ui.frame((200.0, 200.0));
    ui.pointer_down((100.0, 100.0), false);
    ui.pointer_up((100.0, 100.0));
    ui.frame((200.0, 200.0));
    assert_eq!(ui.state, 0);
    assert!(ui.wake_in.is_some(), "the UI asks to be woken for the timer");
    std::thread::sleep(std::time::Duration::from_millis(40));
    ui.frame((200.0, 200.0));
    assert_eq!(ui.state, 7);
}

// ---------------------------------------------------------------- background → state

fn blank() -> Ui<Vec<u32>> {
    Ui::new(Vec::new(), |_: &Vec<u32>| rect().fill())
}

/// Pumps until `done` holds or a second passes.
fn pump_until(ui: &mut Ui<Vec<u32>>, done: impl Fn(&Vec<u32>) -> bool) {
    let start = std::time::Instant::now();
    while !done(&ui.state) && start.elapsed() < std::time::Duration::from_secs(1) {
        ui.pump();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn link_delivers_from_another_thread() {
    let mut ui = blank();
    let link = ruru::link::<Vec<u32>>();
    std::thread::spawn(move || link.send(|s| s.push(42))).join().unwrap();
    assert_eq!(ui.state, Vec::<u32>::new(), "nothing applies before a pump");
    assert!(ui.pump());
    assert_eq!(ui.state, vec![42]);
    assert!(!ui.pump(), "the queue is drained");
}

#[test]
fn a_burst_of_sends_wakes_the_loop_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let mut ui = blank();
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    ruru::set_waker(move || {
        w.fetch_add(1, Ordering::SeqCst);
    });
    let link = ruru::link::<Vec<u32>>();
    for i in 0..1000 {
        link.send(move |s| s.push(i));
    }
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    ui.pump();
    assert_eq!(ui.state.len(), 1000);
    link.send(|s| s.push(0));
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "a pump re-arms the waker");
}

#[test]
fn link_keeps_order() {
    let mut ui = blank();
    let link = ruru::link::<Vec<u32>>();
    let l2 = link.clone();
    std::thread::spawn(move || {
        for i in 0..50 {
            l2.send(move |s| s.push(i));
        }
    })
    .join()
    .unwrap();
    for i in 50..100 {
        link.send(move |s| s.push(i));
    }
    ui.frame((100.0, 100.0)); // the frame pumps too
    assert_eq!(ui.state, (0..100).collect::<Vec<_>>());
}

#[test]
fn spawned_stream_arrives_whole() {
    let mut ui = blank();
    let task = ruru::spawn(|link: Link<Vec<u32>>| async move {
        for i in 0..100 {
            link.send(move |s| s.push(i));
            if i % 10 == 0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    });
    pump_until(&mut ui, |s| s.len() == 100);
    assert_eq!(ui.state, (0..100).collect::<Vec<_>>());
    drop(task);
}

#[test]
fn cancel_discards_queued_updates() {
    let mut ui = blank();
    let (sent_tx, sent_rx) = std::sync::mpsc::channel();
    let task = ruru::spawn(move |link: Link<Vec<u32>>| async move {
        link.send(|s| s.push(1));
        sent_tx.send(()).unwrap();
    });
    sent_rx.recv().unwrap();
    task.cancel();
    assert!(!ui.pump());
    assert!(ui.state.is_empty());

    let (sent_tx, sent_rx) = std::sync::mpsc::channel();
    let task = ruru::spawn(move |link: Link<Vec<u32>>| async move {
        link.send(|s| s.push(2));
        sent_tx.send(()).unwrap();
    });
    sent_rx.recv().unwrap();
    drop(task);
    ui.frame((100.0, 100.0));
    assert!(ui.state.is_empty());

    // Detached tasks run on after the handle is gone.
    let (sent_tx, sent_rx) = std::sync::mpsc::channel();
    ruru::spawn(move |link: Link<Vec<u32>>| async move {
        link.send(|s| s.push(3));
        sent_tx.send(()).unwrap();
    })
    .detach();
    sent_rx.recv().unwrap();
    assert!(ui.pump());
    assert_eq!(ui.state, vec![3]);
}

#[test]
fn worker_sees_cancellation() {
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let task = ruru::spawn(move |link: Link<Vec<u32>>| async move {
        started_tx.send(link.cancelled()).unwrap();
        go_rx.recv().unwrap();
        seen_tx.send(link.cancelled()).unwrap();
        link.send(|s| s.push(9)); // ignored
    });
    assert!(!started_rx.recv().unwrap(), "not cancelled while running");
    task.cancel();
    go_tx.send(()).unwrap();
    assert!(seen_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap());
    let mut ui = blank();
    assert!(!ui.pump());
}

#[test]
fn sleep_paces_a_task_and_cancel_cuts_it_short() {
    use std::time::{Duration, Instant};
    let mut ui = blank();
    let start = Instant::now();
    let task = ruru::spawn(|link: Link<Vec<u32>>| async move {
        for i in 0..3 {
            ruru::sleep(0.03).await;
            link.send(move |s| s.push(i));
        }
    });
    pump_until(&mut ui, |s| s.len() == 3);
    assert_eq!(ui.state, vec![0, 1, 2]);
    assert!(start.elapsed() >= Duration::from_millis(90), "three sleeps of 30 ms");
    drop(task);

    // A cancel wakes a task parked in a long sleep at once; the thread then exits.
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    struct Done(std::sync::mpsc::Sender<()>);
    impl Drop for Done {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let task = ruru::spawn(move |link: Link<Vec<u32>>| async move {
        let _done = Done(done_tx);
        ruru::sleep(60.0).await;
        link.send(|s| s.push(99));
    });
    std::thread::sleep(Duration::from_millis(20));
    let cancelled = Instant::now();
    drop(task);
    done_rx.recv_timeout(Duration::from_secs(1)).expect("task ended after cancel");
    assert!(cancelled.elapsed() < Duration::from_millis(500));
    assert!(!ui.pump());
}

#[test]
fn sleep_outside_a_task_blocks() {
    let start = std::time::Instant::now();
    let fut = ruru::sleep(0.02);
    // Polled by a foreign executor (here: a trivial one), it simply blocks until done.
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    assert!(std::future::Future::poll(std::pin::pin!(fut), &mut cx).is_ready());
    assert!(start.elapsed() >= std::time::Duration::from_millis(20));
}

#[test]
fn stuck_list_follows_a_growing_last_row() {
    struct C {
        rows: Vec<f32>,
        tapped: usize,
    }
    let mut ui = Ui::new(C { rows: vec![100.0; 20], tapped: usize::MAX }, |s: &C| {
        let rows = s.rows.clone();
        let rows = list(s.rows.len(), 120.0, move |i| rect().fill_width().height(rows[i]).on_tap(move |s: &mut C| s.tapped = i));
        scroll(vstack((rect().fill_width().height(40.0).sticky(), rows)).spacing(8.0).fill_width().padding_ltrb(18.0, 0.0, 18.0, 12.0))
            .padding_ltrb(0.0, 58.0, 0.0, 0.0)
            .stick_to_bottom()
    });
    // Frames as the runner draws them: one per update, plus follow-ups while animating.
    let settle = |ui: &mut Ui<C>| {
        ui.frame((300.0, 400.0));
        for _ in 0..200 {
            if !ui.animating {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(4));
            ui.frame((300.0, 400.0));
        }
    };
    settle(&mut ui);
    let bottom_row = |ui: &mut Ui<C>| {
        ui.state.tapped = usize::MAX;
        ui.pointer_down((50.0, 385.0), false);
        ui.pointer_up((50.0, 385.0));
        ui.state.tapped
    };
    assert_eq!(bottom_row(&mut ui), 19, "starts at the end");
    // An empty reply bubble: estimated at 120, measured at 10, so the content first grows
    // and then shrinks under a view parked at the end.
    ui.state.rows.push(10.0);
    // Words arrive with every frame and frames are slow (a software-GL browser draws
    // ~6 fps), so the scroll never gets to settle between them.
    for _ in 0..8 {
        *ui.state.rows.last_mut().unwrap() += 15.0;
        std::thread::sleep(std::time::Duration::from_millis(150));
        ui.frame((300.0, 400.0));
        if ui.animating {
            ui.frame((300.0, 400.0)); // the runner redraws at once after a row was measured
        }
        assert_eq!(bottom_row(&mut ui), 20, "the view follows the end of the growing row");
    }
}

#[test]
fn window_drag_asks_the_window_to_move_but_not_through_buttons() {
    use ruru::WindowRequest;
    let mut ui = Ui::new(0u32, |_: &u32, env: &Env| {
        // The strip is as tall as the title bar zone the runner reports.
        hstack((spacer(), button("+", |n: &mut u32| *n += 1).size(40.0, 28.0)))
            .height(env.titlebar)
            .window_drag()
    });
    ui.titlebar = 28.0;
    ui.frame((400.0, 300.0));
    assert!(ui.window_drag_at((100.0, 10.0)));
    assert!(!ui.window_drag_at((380.0, 10.0)), "buttons block native title-bar gestures");
    assert!(!ui.window_drag_at((100.0, 200.0)));

    ui.pointer_down((100.0, 10.0), false);
    ui.pointer_up((100.0, 10.0));
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::DragWindow]);

    // A second press right away is a double click: zoom instead of a drag.
    ui.pointer_down((101.0, 10.0), false);
    ui.pointer_up((101.0, 10.0));
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::Zoom]);

    // The button inside the strip is above it: a tap, not a drag.
    ui.pointer_down((380.0, 10.0), false);
    ui.pointer_up((380.0, 10.0));
    assert!(ui.requests.is_empty());
    assert_eq!(ui.state, 1);

    // Below the strip nothing is draggable; touches never drag.
    ui.pointer_down((100.0, 200.0), false);
    ui.pointer_down((100.0, 10.0), true);
    assert!(ui.requests.is_empty());
}

// ---- clip, truncate, hover, size limits, animate, padding

#[test]
fn controls_and_popover_dismissal_interrupt_titlebar_double_clicks() {
    use ruru::WindowRequest;
    let mut ui = Ui::new(false, |open: &bool| {
        hstack((
            button("More", |open: &mut bool| *open = true)
                .size(40.0, 28.0)
                .popover(*open, Placement::bottom(), |open| *open = false, text("Menu").size(100.0, 100.0)),
            spacer(),
        ))
        .height(28.0)
        .window_drag()
    });
    ui.frame((400.0, 300.0));
    let background = (100.0, 10.0);
    let button = (20.0, 10.0);
    let click = |ui: &mut Ui<bool>, pos| {
        ui.pointer_down(pos, false);
        ui.pointer_up(pos);
        ui.frame((400.0, 300.0));
    };
    click(&mut ui, background);
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::DragWindow]);
    click(&mut ui, button);
    assert!(ui.state);
    assert!(!ui.window_drag_at(background), "an open popover blocks native dragging below it");
    assert!(ui.requests.is_empty());
    click(&mut ui, background); // dismiss only, even above a draggable header
    assert!(!ui.state);
    assert!(ui.window_drag_at(background));
    assert!(ui.requests.is_empty());
    click(&mut ui, background);
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::DragWindow]);
    click(&mut ui, background);
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::Zoom]);
}

fn quads(ui: &Ui<impl Sized>, color: Color) -> Vec<[f32; 4]> {
    ui.list
        .cmds
        .iter()
        .filter_map(|c| match c {
            ruru::Cmd::Quad(q) if q.c0 == [color.r, color.g, color.b, color.a] && q.p0[3] == 0.0 => Some(q.rect),
            _ => None,
        })
        .collect()
}

fn texts(ui: &Ui<impl Sized>) -> Vec<&ruru::TextCmd> {
    ui.list.cmds.iter().filter_map(|c| if let ruru::Cmd::Text(t) = c { Some(t) } else { None }).collect()
}

#[test]
fn clip_cuts_hits_to_the_rounded_shape() {
    // A circle of 200 pt with a tappable square bigger than itself inside.
    let mut ui = Ui::new(0, |_: &i32| {
        zstack(zstack(rect().size(400.0, 400.0).background(Color::RED).on_tap(|n: &mut i32| *n += 1)).size(200.0, 200.0).radius(100.0).clip())
            .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    for (pos, hits) in [((100.0, 100.0), 1), ((8.0, 8.0), 1), ((190.0, 100.0), 2), ((300.0, 100.0), 2)] {
        ui.pointer_down(pos, false);
        ui.pointer_up(pos);
        assert_eq!(ui.state, hits, "tap at {pos:?}");
    }
    // The child's quad carries the circle as its clip.
    let q = ui.list.cmds.iter().find_map(|c| if let ruru::Cmd::Quad(q) = c { Some(*q) } else { None }).unwrap();
    assert_eq!(q.clip, [0.0, 0.0, 200.0, 200.0]);
    assert_eq!(q.p1[2], 100.0);
}

#[test]
fn nested_clips_intersect() {
    let mut ui = Ui::new((), |_: &()| {
        zstack(zstack(zstack(rect().size(300.0, 300.0).background(Color::RED)).size(200.0, 50.0).radius(10.0).clip())
            .size(100.0, 100.0)
            .radius(30.0)
            .clip())
        .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    let q = ui.list.cmds.iter().find_map(|c| if let ruru::Cmd::Quad(q) = c { Some(*q) } else { None }).unwrap();
    // 200x50 centered in 100x100 → x -50..150, y 25..75; cut to the outer box.
    assert_eq!(q.clip, [0.0, 25.0, 100.0, 50.0]);
}

const LONG: &str = "A very long chat title that will never fit into a narrow sidebar row";

#[test]
fn lines_and_truncate_keep_the_height() {
    let measure = |make: fn() -> View<()>| {
        let mut ui = Ui::new((), move |_: &()| vstack(make().width(120.0)).place(Align::Start, Align::Start));
        ui.frame((400.0, 400.0));
        let t = texts(&ui)[0];
        (t.size, t.fade.is_some())
    };
    let short = measure(|| text("Short"));
    let wrapped = measure(|| text(LONG));
    let ellipsis = measure(|| text(LONG).lines(1));
    let fade = measure(|| text(LONG).truncate(Truncate::Fade));
    let two = measure(|| text(LONG).lines(2));
    assert!(wrapped.0 .1 > short.0 .1 * 2.5, "{wrapped:?}");
    assert_eq!(ellipsis.0 .1, short.0 .1);
    assert_eq!(fade.0 .1, short.0 .1);
    assert_eq!(two.0 .1, short.0 .1 * 2.0);
    assert!(ellipsis.0 .0 <= 120.0 && fade.0 .0 <= 120.0);
    assert!(fade.1 && !ellipsis.1 && !short.1);
    // Text that fits does not fade.
    assert!(!measure(|| text("Short").truncate(Truncate::Fade)).1);
}

struct H {
    taps: i32,
}

fn hover_ui() -> Ui<H> {
    let mut ui = Ui::new(H { taps: 0 }, |_: &H| {
        vstack((
            button("tap", |s: &mut H| s.taps += 1).size(200.0, 100.0),
            rect().size(200.0, 100.0).background(Color::GRAY).hover(Color::RED),
            text_field("", "type", |_: &mut H, _| {}).width(200.0),
            rect().size(200.0, 100.0).hover(Color::GREEN).cursor(Cursor::ResizeH),
        ))
        .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 600.0));
    ui
}

#[test]
fn hover_redraws_only_when_the_target_changes() {
    let mut ui = hover_ui();
    assert!(!ui.pointer_move((300.0, 50.0)), "nothing under the mouse");
    assert!(ui.pointer_move((100.0, 50.0)), "onto the button");
    assert_eq!(ui.cursor, Cursor::Pointer);
    assert!(!ui.pointer_move((120.0, 60.0)), "same button: no frame");
    assert!(ui.pointer_move((100.0, 150.0)), "onto the hover row");
    assert_eq!(ui.cursor, Cursor::Default);
    // The highlight fades in over a few frames and then rests.
    ui.frame((400.0, 600.0));
    assert!(ui.animating);
    let mut seen_partial = false;
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame((400.0, 600.0));
        if quads(&ui, Color::RED.alpha(0.5)).is_empty() && ui.list.cmds.iter().any(|c| matches!(c, ruru::Cmd::Quad(q) if q.c0[0] == 1.0 && q.c0[3] > 0.05 && q.c0[3] < 0.95)) {
            seen_partial = true;
        }
        if !ui.animating {
            break;
        }
    }
    assert!(seen_partial, "fades in");
    assert!(!ui.animating, "settles");
    assert_eq!(quads(&ui, Color::RED).len(), 1, "fully shown");
    let field_y = 200.0 + 10.0;
    assert!(ui.pointer_move((100.0, field_y)));
    assert_eq!(ui.cursor, Cursor::Text);
    assert!(ui.pointer_move((100.0, 300.0)));
    assert_eq!(ui.cursor, Cursor::ResizeH);
    assert!(ui.pointer_leave());
    assert_eq!(ui.cursor, Cursor::Default);
    assert!(!ui.pointer_leave());
}

#[test]
fn touch_does_not_hover() {
    let mut ui = hover_ui();
    ui.pointer_down((100.0, 150.0), true);
    assert!(!ui.pointer_move((100.0, 152.0)));
    ui.pointer_up((100.0, 152.0));
    ui.frame((400.0, 600.0));
    assert!(!ui.animating);
}

#[test]
fn max_and_min_width_limit_the_size() {
    let mut ui = Ui::new((), |_: &()| {
        vstack((
            rect().fill_width().height(10.0).max_width(100.0).background(Color::RED),
            rect().width(20.0).height(10.0).min_width(50.0).background(Color::GREEN),
            text(LONG).max_width(150.0).background(Color::BLUE),
            rect().fill().max_height(30.0).min_height(5.0).background(Color::GRAY),
        ))
        .fill()
    });
    ui.frame((400.0, 400.0));
    assert_eq!(quads(&ui, Color::RED)[0][2], 100.0);
    assert_eq!(quads(&ui, Color::GREEN)[0][2], 50.0);
    let blue = quads(&ui, Color::BLUE)[0];
    assert!(blue[2] <= 150.0 && blue[3] > 40.0, "text wraps at its max width: {blue:?}");
    assert_eq!(quads(&ui, Color::GRAY)[0][3], 30.0);
}

#[test]
fn padding_sides_combine() {
    let mut ui = Ui::new((), |_: &()| {
        zstack(text("hi").padding_h(8.0).padding_v(4.0).background(Color::RED)).place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    let bg = quads(&ui, Color::RED)[0];
    let t = texts(&ui)[0];
    assert_eq!((t.pos.0 - bg[0], t.pos.1 - bg[1]), (8.0, 4.0));
    assert_eq!(bg[2] - t.size.0, 16.0);
    assert_eq!(bg[3] - t.size.1, 8.0);
}

#[test]
fn animate_glides_to_the_new_size_and_settles() {
    let mut ui = Ui::new(true, |open: &bool| {
        hstack((
            vstack(text(LONG).background(Color::BLUE)).width(if *open { 300.0 } else { 0.0 }).background(Color::RED).animate(),
            rect().fill().background(Color::GREEN),
        ))
        .fill()
    });
    let size = (800.0, 400.0);
    ui.frame(size);
    assert!(!ui.animating, "no animation on first appearance");
    assert_eq!(quads(&ui, Color::RED)[0][2], 300.0);
    let content_w = quads(&ui, Color::BLUE)[0][2];
    ui.state = false;
    let mut widths = Vec::new();
    for _ in 0..500 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame(size);
        let w = quads(&ui, Color::RED).first().map_or(0.0, |q| q[2]);
        widths.push(w);
        // The neighbour takes the space as it frees up.
        let green = quads(&ui, Color::GREEN)[0];
        assert!((green[0] - w).abs() < 0.01 && (green[2] - (800.0 - w)).abs() < 0.01, "w={w} green={green:?}");
        // The content keeps its width and is cut, not squeezed.
        if let Some(b) = quads(&ui, Color::BLUE).first() {
            assert_eq!(b[2], content_w);
        }
        if !ui.animating {
            break;
        }
    }
    assert!(widths.iter().any(|w| *w > 20.0 && *w < 280.0), "passes through the middle: {widths:?}");
    assert!(widths.windows(2).all(|p| p[1] <= p[0] + 0.01), "shrinks monotonically");
    assert_eq!(*widths.last().unwrap(), 0.0);
    assert!(!ui.animating);
    // And back.
    ui.state = true;
    for _ in 0..500 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame(size);
        if !ui.animating {
            break;
        }
    }
    assert_eq!(quads(&ui, Color::RED)[0][2], 300.0);
}

// ---------------------------------------------------------------- text editing

struct Chat {
    draft: String,
    sent: Vec<String>,
    taps: u32,
}

/// A multi-line composer (Enter sends) above a button: the button moves down as the
/// field grows. The text starts at (14, 12); rows are 22 pt.
fn chat(submit: bool) -> Ui<Chat> {
    let mut ui = Ui::new(Chat { draft: String::new(), sent: Vec::new(), taps: 0 }, move |s: &Chat| {
        let mut field = text_field(s.draft.as_str(), "Message", |s: &mut Chat, v| s.draft = v).multiline().max_lines(4);
        if submit {
            field = field.on_submit(|s: &mut Chat| s.sent.push(std::mem::take(&mut s.draft)));
        }
        vstack((field.width(300.0), button("x", |s: &mut Chat| s.taps += 1).size(300.0, 40.0)))
            .align(Align::Start)
            .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 600.0));
    ui.pointer_down((100.0, 20.0), false);
    ui.pointer_up((100.0, 20.0));
    ui
}

fn shift(ui: &mut Ui<Chat>, on: bool) {
    ui.set_modifiers(ruru::Mods { shift: on, ..Default::default() });
}

/// Taps at `y` and reports whether the button below the field took it.
fn button_at(ui: &mut Ui<Chat>, y: f32) -> bool {
    let before = ui.state.taps;
    ui.pointer_down((100.0, y), false);
    ui.pointer_up((100.0, y));
    ui.state.taps > before
}

#[test]
fn multiline_field_grows_with_its_text_up_to_max_lines() {
    let mut ui = chat(false);
    ui.frame((400.0, 600.0));
    assert!(button_at(&mut ui, 60.0), "one row: field is 46 pt tall, the button follows");
    ui.pointer_down((100.0, 20.0), false);
    ui.pointer_up((100.0, 20.0));
    for l in ["one", "\n", "two", "\n", "three"] {
        if l == "\n" {
            ui.key(KeyInput::Enter); // no on_submit: Enter breaks the line
        } else {
            ui.text_input(l);
        }
    }
    assert_eq!(ui.state.draft, "one\ntwo\nthree");
    ui.frame((400.0, 600.0));
    assert!(!button_at(&mut ui, 60.0), "three rows: 90 pt of field");
    assert!(button_at(&mut ui, 95.0));
    // Long text wraps and grows too, but stops at max_lines (4 rows = 112 pt).
    ui.pointer_down((100.0, 20.0), false);
    ui.pointer_up((100.0, 20.0));
    ui.text_input(&" word".repeat(80));
    ui.frame((400.0, 600.0));
    assert!(!button_at(&mut ui, 110.0));
    assert!(button_at(&mut ui, 116.0), "capped at four rows; the rest scrolls inside");
}

#[test]
fn enter_submits_and_shift_enter_breaks_the_line() {
    let mut ui = chat(true);
    ui.text_input("hello");
    shift(&mut ui, true);
    ui.key(KeyInput::Enter);
    shift(&mut ui, false);
    ui.text_input("world");
    assert_eq!(ui.state.draft, "hello\nworld");
    ui.key(KeyInput::Enter);
    assert_eq!(ui.state.sent, vec!["hello\nworld".to_string()]);
    assert_eq!(ui.state.draft, "");
    assert!(ui.has_focus(), "the composer keeps focus after sending");
    ui.frame((400.0, 600.0));
    ui.text_input("next");
    assert_eq!(ui.state.draft, "next", "typing after a send starts from the cleared draft");
    // Alt+Enter also breaks the line.
    ui.set_modifiers(ruru::Mods { alt: true, ..Default::default() });
    ui.key(KeyInput::Enter);
    assert_eq!(ui.state.draft, "next\n");
}

#[test]
fn shift_arrows_select_and_typing_replaces_the_selection() {
    let mut ui = chat(false);
    ui.text_input("hello world");
    shift(&mut ui, true);
    for _ in 0..5 {
        ui.key(KeyInput::Left);
    }
    assert_eq!(ui.selection_text().as_deref(), Some("world"));
    shift(&mut ui, false);
    ui.text_input("there");
    assert_eq!(ui.state.draft, "hello there");
    // Left with a selection collapses it to its start.
    ui.key(KeyInput::SelectAll);
    assert_eq!(ui.selection_text().as_deref(), Some("hello there"));
    ui.key(KeyInput::Left);
    ui.text_input(">");
    assert_eq!(ui.state.draft, ">hello there");
    // Alt+Shift+Right selects by words.
    ui.set_modifiers(ruru::Mods { shift: true, alt: true, ..Default::default() });
    ui.key(KeyInput::Right);
    assert_eq!(ui.selection_text().as_deref(), Some("hello"));
    ui.set_modifiers(ruru::Mods::default());
    ui.key(KeyInput::Backspace);
    assert_eq!(ui.state.draft, "> there");
}

#[test]
fn mouse_drag_and_double_click_select() {
    let mut ui = chat(false);
    ui.text_input("alpha beta gamma");
    ui.frame((400.0, 600.0));
    // Drag from the left edge of the text to far right of it.
    ui.pointer_down((14.0, 23.0), false);
    ui.pointer_move((60.0, 23.0));
    ui.pointer_move((290.0, 23.0));
    ui.pointer_up((290.0, 23.0));
    assert_eq!(ui.selection_text().as_deref(), Some("alpha beta gamma"));
    // A double click picks the word under the pointer, a triple click the paragraph.
    std::thread::sleep(std::time::Duration::from_millis(450));
    let beta = (14.0 + 60.0, 23.0);
    ui.pointer_down(beta, false);
    ui.pointer_up(beta);
    ui.pointer_down(beta, false);
    ui.pointer_up(beta);
    assert_eq!(ui.selection_text().as_deref(), Some("beta"));
    ui.pointer_down(beta, false);
    ui.pointer_up(beta);
    assert_eq!(ui.selection_text().as_deref(), Some("alpha beta gamma"));
}

#[test]
fn copy_cut_paste_go_through_the_runner() {
    use ruru::WindowRequest;
    let mut ui = chat(false);
    ui.text_input("copy me");
    ui.key(KeyInput::Copy);
    assert!(ui.requests.is_empty(), "nothing selected, nothing copied");
    ui.key(KeyInput::SelectAll);
    ui.key(KeyInput::Copy);
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::SetClipboard("copy me".into())]);
    ui.key(KeyInput::Cut);
    assert_eq!(std::mem::take(&mut ui.requests), vec![WindowRequest::SetClipboard("copy me".into())]);
    assert_eq!(ui.state.draft, "");
    assert!(ui.paste("line 1\r\nline 2"));
    assert_eq!(ui.state.draft, "line 1\nline 2", "line endings are normalized");
    ui.key(KeyInput::Escape);
    assert!(!ui.paste("x"), "no focused field");
}

#[test]
fn single_line_field_pastes_line_breaks_as_spaces() {
    let mut ui = form();
    ui.pointer_down((150.0, 20.0), false);
    ui.pointer_up((150.0, 20.0));
    ui.paste("a\nb");
    assert_eq!(ui.state.name, "a b");
    ui.key(KeyInput::Enter);
    assert!(!ui.has_focus(), "Enter in a single-line field without on_submit ends editing");
}

#[test]
fn undo_and_redo_restore_text_in_steps() {
    let mut ui = chat(false);
    ui.text_input("h");
    ui.text_input("i");
    ui.text_input(" ");
    ui.text_input("y");
    ui.text_input("o");
    assert_eq!(ui.state.draft, "hi yo");
    ui.key(KeyInput::Undo);
    assert_eq!(ui.state.draft, "hi ", "a word is one step");
    ui.key(KeyInput::Undo);
    ui.key(KeyInput::Undo);
    assert_eq!(ui.state.draft, "");
    ui.key(KeyInput::Redo);
    assert_eq!(ui.state.draft, "hi");
    ui.paste("!");
    ui.key(KeyInput::Redo);
    assert_eq!(ui.state.draft, "hi!", "an edit drops the redo steps");
    ui.key(KeyInput::Undo);
    assert_eq!(ui.state.draft, "hi");
}

#[test]
fn cyrillic_and_emoji_move_and_delete_by_whole_characters() {
    let mut ui = chat(false);
    ui.text_input("Привет 👍🏽 мир 👨‍👩‍👧");
    ui.key(KeyInput::Backspace);
    assert_eq!(ui.state.draft, "Привет 👍🏽 мир ", "a family emoji is one grapheme");
    ui.key(KeyInput::Home);
    ui.key(KeyInput::Right);
    ui.key(KeyInput::Delete);
    assert_eq!(ui.state.draft, "Пивет 👍🏽 мир ");
    ui.set_modifiers(ruru::Mods { alt: true, ..Default::default() });
    ui.key(KeyInput::Right); // to the end of «Пивет»
    ui.set_modifiers(ruru::Mods::default());
    ui.key(KeyInput::Right);
    ui.key(KeyInput::Right);
    ui.key(KeyInput::Backspace);
    assert_eq!(ui.state.draft, "Пивет  мир ", "the skin-toned thumb goes as a whole");
    ui.frame((400.0, 600.0));
    // Clicking in the middle of the text lands on a character boundary.
    for x in [20.0, 37.0, 51.0, 66.0, 90.0] {
        ui.pointer_down((x, 23.0), false);
        ui.pointer_up((x, 23.0));
        ui.text_input("|");
        assert!(ui.state.draft.contains('|'));
        ui.key(KeyInput::Backspace);
        assert_eq!(ui.state.draft, "Пивет  мир ");
        std::thread::sleep(std::time::Duration::from_millis(410));
    }
}

#[test]
fn up_and_down_move_between_rows() {
    let mut ui = chat(false);
    ui.paste("abc\nlonger line\nxy");
    ui.frame((400.0, 600.0));
    ui.key(KeyInput::Up);
    ui.text_input("1");
    assert_eq!(ui.state.draft, "abc\nlo1nger line\nxy", "keeps the column");
    ui.key(KeyInput::Up);
    ui.key(KeyInput::End);
    ui.text_input("2");
    assert_eq!(ui.state.draft, "abc2\nlo1nger line\nxy");
    ui.key(KeyInput::Down);
    ui.key(KeyInput::Down);
    ui.key(KeyInput::Down);
    ui.text_input("3");
    assert_eq!(ui.state.draft, "abc2\nlo1nger line\nxy3", "past the last row: the end");
    ui.key(KeyInput::Up);
    ui.key(KeyInput::Home);
    ui.text_input("4");
    assert_eq!(ui.state.draft, "abc2\n4lo1nger line\nxy3");
    // Clicking on the second row puts the caret there.
    ui.frame((400.0, 600.0));
    ui.pointer_down((14.0, 12.0 + 22.0 + 11.0), false);
    ui.pointer_up((14.0, 12.0 + 22.0 + 11.0));
    ui.text_input("5");
    assert_eq!(ui.state.draft, "abc2\n54lo1nger line\nxy3");
}

#[test]
fn caret_blinks_with_timers_not_continuous_frames() {
    let mut ui = chat(false);
    ui.frame((400.0, 600.0));
    assert!(!ui.animating || ui.wake_in.is_some());
    let w = ui.wake_in.expect("a focused field wakes the UI to blink");
    assert!(w > 0.0 && w <= 0.52);
    ui.text_input("abc");
    ui.key(KeyInput::SelectAll);
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        ui.frame((400.0, 600.0)); // let the focus ring and the press settle
    }
    assert!(!ui.animating);
    assert!(ui.wake_in.is_none(), "a selection doesn't blink: no wake-ups");
}

// ---- typography: line height, letter spacing, span weight/size, plates, icons, flex

fn text_size(make: impl Fn() -> View<()> + 'static) -> (f32, f32) {
    let mut ui = Ui::new((), move |_: &()| vstack(make()).place(Align::Start, Align::Start));
    ui.frame((600.0, 400.0));
    texts(&ui)[0].size
}

#[test]
fn line_height_sets_the_row_pitch() {
    let base = text_size(|| text(LONG).font_size(14.0).width(120.0));
    let airy = text_size(|| text(LONG).font_size(14.0).line_height(1.625).width(120.0));
    let rows = (base.1 / 18.0).round();
    assert!(rows >= 3.0, "{base:?}");
    assert_eq!(base.1, rows * 18.0); // 14 * 1.3
    assert_eq!(airy.1, rows * 23.0); // 14 * 1.625, rounded
    assert_eq!(text_size(|| text("one").font_size(14.0).line_height(1.625)).1, 23.0);
}

#[test]
fn letter_spacing_weight_and_size_of_spans() {
    let plain = text_size(|| text("Spacing").font_size(14.0));
    let loose = text_size(|| text("Spacing").font_size(14.0).letter_spacing(0.1));
    // 7 letters, 1.4 pt each.
    assert!((loose.0 - plain.0 - 9.8).abs() < 1.5, "{plain:?} {loose:?}");
    let regular = text_size(|| rich_text(vec![Span::new("Semibold words")]));
    let semi = text_size(|| rich_text(vec![Span::new("Semibold words").weight(Weight::Semibold)]));
    let bold = text_size(|| rich_text(vec![Span::new("Semibold words").bold()]));
    assert!(regular.0 < semi.0 && semi.0 < bold.0, "{regular:?} {semi:?} {bold:?}");
    let big = text_size(|| rich_text(vec![Span::new("a "), Span::new("Big").size(34.0)]));
    assert!(big.1 >= 44.0 && regular.1 == 22.0, "{big:?}");
}

#[test]
fn background_plate_follows_the_span_and_keeps_the_line_height() {
    let plate = Color::hex(0x303030);
    let row = move |bg: bool| {
        let code = Span::new("wgpu").code();
        let code = if bg { code.background(plate) } else { code };
        rich_text(vec![Span::new("use "), code, Span::new(" here")]).font_size(14.0)
    };
    let without = text_size(move || row(false));
    let mut ui = Ui::new((), move |_: &()| vstack(row(true)).place(Align::Start, Align::Start));
    ui.frame((600.0, 400.0));
    let with = texts(&ui)[0].size;
    assert_eq!(with.1, without.1, "the plate does not make the line taller");
    // Room on both sides: 2 x 0.3 em.
    assert!((with.0 - without.0 - 8.4).abs() < 1.0, "{without:?} {with:?}");
    let q = quads(&ui, plate);
    assert_eq!(q.len(), 1);
    let [x, y, w, h] = q[0];
    assert!(x > 20.0 && w > 30.0 && w < 50.0, "{:?}", q[0]);
    assert!(y >= 0.0 && y + h <= with.1 && (h - 15.1).abs() < 0.5, "{:?}", q[0]);

    // Wrapped across two rows: one plate per row.
    let mut ui = Ui::new((), move |_: &()| {
        vstack(rich_text(vec![Span::new("x "), Span::new("alpha beta gamma delta").code().background(plate)]).font_size(14.0).width(110.0))
            .place(Align::Start, Align::Start)
    });
    ui.frame((600.0, 400.0));
    let q = quads(&ui, plate);
    assert!(q.len() >= 2 && q[1][1] > q[0][1], "{q:?}");
}

#[test]
fn inline_icon_takes_room_in_the_line() {
    let img = Image::from_rgba(2, 2, vec![255; 16]);
    let plain = text_size(|| rich_text(vec![Span::new("egui")]).font_size(14.0));
    let img2 = img.clone();
    let mut ui = Ui::new((), move |_: &()| {
        vstack(rich_text(vec![Span::icon(&img2), Span::new("egui")]).font_size(14.0)).place(Align::Start, Align::Start)
    });
    ui.frame((600.0, 400.0));
    let with = texts(&ui)[0].size;
    assert_eq!(with.1, plain.1);
    assert!((with.0 - plain.0 - 14.0 * 1.3).abs() < 1.0, "{plain:?} {with:?}");
    assert_eq!(ui.list.images.len(), 1);
}

#[test]
fn flex_shares_the_free_space_by_weight() {
    let mut ui = Ui::new((), |_: &()| {
        hstack((
            rect().width(40.0).height(10.0).background(Color::GRAY),
            rect().height(10.0).flex(1.0).background(Color::RED),
            rect().height(10.0).flex(3.0).background(Color::GREEN),
            rect().height(10.0).fill_width().background(Color::BLUE),
        ))
        .spacing(10.0)
        .width(570.0)
        .place(Align::Start, Align::Start)
    });
    ui.frame((600.0, 400.0));
    // 570 - 40 - 3 * 10 = 500 shared 1 : 3 : 1.
    assert_eq!(quads(&ui, Color::RED)[0][2], 100.0);
    assert_eq!(quads(&ui, Color::GREEN)[0][2], 300.0);
    assert_eq!(quads(&ui, Color::BLUE)[0][2], 100.0);
    assert_eq!(quads(&ui, Color::GREEN)[0][0], 160.0);
    // In a vstack it shares the height.
    let mut ui = Ui::new((), |_: &()| {
        vstack((rect().width(10.0).flex(1.0).background(Color::RED), rect().width(10.0).flex(2.0).background(Color::GREEN)))
            .height(300.0)
            .place(Align::Start, Align::Start)
    });
    ui.frame((600.0, 400.0));
    assert_eq!(quads(&ui, Color::RED)[0][3], 100.0);
    assert_eq!(quads(&ui, Color::GREEN)[0][3], 200.0);
}

const MD: &str = "Use `wgpu` and **bold**.\n\n| Name | Kind | Note |\n| --- | --- | --- |\n| a | b | c |\n| d | e | f |\n";

fn md_ui(style: MarkdownStyle) -> Ui<()> {
    let mut ui = Ui::new((), move |_: &()| vstack(markdown_styled(MD, &style)).width(400.0).place(Align::Start, Align::Start));
    ui.frame((600.0, 600.0));
    ui
}

#[test]
fn markdown_style_sets_type_gaps_tables_and_code_plates() {
    let plate = Color::hex(0x303030);
    let rule = Color::hex(0x222222);
    let style = MarkdownStyle {
        size: 14.0,
        line_height: 1.625,
        gap: 13.0,
        code_background: Some(plate),
        table: ruru::TableStyle {
            size: Some(12.0),
            line_height: Some(1.875),
            cell_padding: (8.0, 9.0),
            rule: Some((1.0, rule)),
            columns: vec![0.25, 0.375, 0.375],
            ..Default::default()
        },
        ..Default::default()
    };
    let ui = md_ui(style);
    let t = texts(&ui);
    // A paragraph, then 3 header cells and 2 rows of 3.
    assert_eq!(t.len(), 1 + 9);
    assert_eq!(t[0].size.1, 23.0);
    // Columns at 0, 25 % and 62.5 % of 400 in every row; cells are 22.5 → 23 pt rows padded by 9.
    let xs: Vec<f32> = t[1..4].iter().map(|c| c.pos.0).collect();
    assert_eq!(xs, vec![0.0, 100.0, 250.0]);
    assert_eq!(t[4].pos.0, 0.0);
    assert_eq!(t[9].pos.0, 250.0);
    assert_eq!(t[1].pos.1, 23.0 + 13.0 + 9.0);
    assert_eq!(t[1].size.1, 23.0);
    // A rule between rows (2), none outside; one plate under `wgpu`.
    let rules = quads(&ui, rule);
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0][1], t[1].pos.1 + 23.0 + 9.0);
    assert_eq!(quads(&ui, plate).len(), 1);
    // The default style keeps the old look: no plates, 17 pt text at 1.3.
    let ui = md_ui(MarkdownStyle::default());
    assert_eq!(texts(&ui)[0].size.1, 22.0);
    assert!(quads(&ui, plate).is_empty());
}

#[test]
fn placeholder_takes_its_own_color() {
    let gray = Color::hex(0x686868);
    let mut ui = Ui::new((), move |_: &()| {
        vstack(text_field("", "Ask anything", |_: &mut (), _| {}).font_size(14.0).placeholder_color(gray))
            .width(300.0)
            .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    assert_eq!(texts(&ui)[0].color, gray);
}

// ---- text selection (docs/decisions/002-text-selection.md)

const SEL: Color = Color::BLUE.alpha(0.3);

/// Text commands with their content position: `(x, y, w, h)` on screen, in paint order.
fn text_rects(ui: &Ui<impl Sized>) -> Vec<(f32, f32, f32, f32)> {
    texts(ui).iter().map(|t| (t.pos.0, t.pos.1, t.size.0, t.size.1)).collect()
}

fn drag(ui: &mut Ui<impl Sized + 'static>, from: (f32, f32), to: (f32, f32)) {
    ui.pointer_down(from, false);
    ui.pointer_move(((from.0 + to.0) * 0.5, (from.1 + to.1) * 0.5));
    ui.pointer_move(to);
    ui.pointer_up(to);
}

fn click(ui: &mut Ui<impl Sized + 'static>, at: (f32, f32)) {
    ui.pointer_down(at, false);
    ui.pointer_up(at);
}

fn clipboard(ui: &mut Ui<impl Sized + 'static>) -> Option<String> {
    ui.requests.clear();
    ui.key(KeyInput::Copy);
    ui.requests.iter().find_map(|r| match r {
        ruru::WindowRequest::SetClipboard(t) => Some(t.clone()),
        _ => None,
    })
}

#[test]
fn selectable_text_drags_double_and_triple_clicks_and_shift_extends() {
    let mut ui = Ui::new((), |_: &()| {
        vstack((text("Привет, дорогой мир\nвторая строка"), button("ok", |_: &mut ()| {})))
            .selectable()
            .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 300.0));
    let (x, y, w, h) = text_rects(&ui)[0];
    let row1 = y + h * 0.25;
    // Drag from the start past the end of the first row: the whole first paragraph.
    drag(&mut ui, (x + 1.0, row1), (x + w + 30.0, row1));
    assert_eq!(ui.selection_text().as_deref(), Some("Привет, дорогой мир"));
    ui.frame((400.0, 300.0));
    assert!(!quads(&ui, SEL).is_empty(), "highlight painted");
    // A plain click drops it.
    click(&mut ui, (x + 5.0, row1));
    assert_eq!(ui.selection_text(), None);
    ui.frame((400.0, 300.0));
    assert!(quads(&ui, SEL).is_empty());
    // Double click: a word; triple: the paragraph.
    let word = (x + w * 0.45, row1);
    click(&mut ui, word);
    click(&mut ui, word);
    assert_eq!(ui.selection_text().as_deref(), Some("дорогой"));
    click(&mut ui, word);
    assert_eq!(ui.selection_text().as_deref(), Some("Привет, дорогой мир"));
    // Shift+click extends from the anchor into the second paragraph.
    std::thread::sleep(std::time::Duration::from_millis(450)); // not a fourth click
    click(&mut ui, (x + 1.0, row1));
    ui.set_modifiers(ruru::Mods { shift: true, ..Default::default() });
    click(&mut ui, (x + w - 2.0, y + h * 0.75)); // past the end of the shorter second row
    ui.set_modifiers(ruru::Mods::default());
    assert_eq!(ui.selection_text().as_deref(), Some("Привет, дорогой мир\nвторая строка"));
    assert_eq!(clipboard(&mut ui).as_deref(), Some("Привет, дорогой мир\nвторая строка"));
    // Escape clears; a button inside the area still works (it is above the area).
    assert!(ui.key(KeyInput::Escape));
    assert_eq!(ui.selection_text(), None);
    // The pointer over the text is an I-beam.
    ui.pointer_move((x + 5.0, row1));
    assert_eq!(ui.cursor, Cursor::Text);
}

#[test]
fn selection_spans_texts_with_separators_copy_as_and_sticky_order() {
    let mut ui = Ui::new((), |_: &()| {
        vstack((
            text("Title"),
            hstack((text("•").copy_as("- "), text("item"))).copy_sep(""),
            hstack((text("a"), text("b"))),
            image(&Image::from_rgba(1, 1, vec![255; 4])).size(10.0, 10.0).copy_as("[img]"),
            text("hidden").copy_as(""),
            text("end"),
        ))
        .selectable()
        .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    assert!(ui.key(KeyInput::SelectAll));
    assert_eq!(clipboard(&mut ui).as_deref(), Some("Title\n- item\na b\n[img]\nend"));
    // Part of it: from inside "Title" into "item" takes the marker whole.
    let r = text_rects(&ui);
    drag(&mut ui, (r[0].0 + r[0].2 * 0.5, r[0].1 + 5.0), (r[2].0 + r[2].2 - 1.0, r[2].1 + 5.0));
    let t = ui.selection_text().unwrap();
    assert!(t.ends_with("\n- item") && t.len() < "Title\n- item".len(), "{t:?}");

    // Sticky headers are painted last but numbered in tree order.
    let mut ui = Ui::new((), |_: &()| {
        scroll(vstack((text("Header").sticky(), text("one"), text("two"))).selectable()).height(300.0)
    });
    ui.frame((300.0, 300.0));
    let r = text_rects(&ui);
    let (header, two) = (r.iter().find(|t| t.1 < 5.0).unwrap(), r.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap());
    drag(&mut ui, (header.0 + 1.0, header.1 + 5.0), (two.0 + two.2 + 20.0, two.1 + 5.0));
    assert_eq!(ui.selection_text().as_deref(), Some("Header\none\ntwo"));
}

#[test]
fn selection_runs_across_lazy_rows_and_copies_rows_off_screen() {
    use std::cell::Cell;
    use std::rc::Rc;
    let built = Rc::new(Cell::new(0usize));
    let counter = built.clone();
    let mut ui = Ui::new((), move |_: &()| {
        let counter = counter.clone();
        scroll(
            list(1000, 40.0, move |i| {
                counter.set(counter.get() + 1);
                text(format!("row {i}")).height(40.0)
            })
            .selectable(),
        )
    });
    let size = (300.0, 400.0);
    ui.frame(size);
    ui.frame(size);
    let per_frame = {
        let b = built.get();
        ui.frame(size);
        built.get() - b
    };
    // Drag from row 0 to row 2.
    drag(&mut ui, (2.0, 15.0), (200.0, 95.0));
    assert_eq!(ui.selection_text().as_deref(), Some("row 0\n\nrow 1\n\nrow 2"));
    // A selection costs no rows off screen in a normal frame.
    let b = built.get();
    ui.frame(size);
    assert_eq!(built.get() - b, per_frame);
    assert!(!quads(&ui, SEL).is_empty());
    // Scroll far away (the anchor goes off screen) and Shift+click: the selection grows.
    for _ in 0..10 {
        ui.wheel((150.0, 200.0), -200.0, ruru::WheelPhase::Line);
    }
    ui.frame(size);
    let r = text_rects(&ui);
    let far = r.iter().find(|t| t.1 > 100.0).unwrap();
    ui.set_modifiers(ruru::Mods { shift: true, ..Default::default() });
    click(&mut ui, (far.0 + far.2 + 10.0, far.1 + 5.0));
    ui.set_modifiers(ruru::Mods::default());
    let t = ui.selection_text().unwrap();
    assert!(t.starts_with("row 0\n\nrow 1") && t.matches("\n\n").count() > 40, "{}", &t[t.len() - 20..]);
    // Cmd+A, Cmd+C: every row, each built once for the copy.
    ui.key(KeyInput::SelectAll);
    let b = built.get();
    let all = clipboard(&mut ui).unwrap();
    assert_eq!(built.get() - b, 1000);
    assert!(all.starts_with("row 0\n\nrow 1\n\n") && all.ends_with("row 998\n\nrow 999"));
    // A typed-into field takes the focus and the selection goes.
    let b = built.get();
    ui.frame(size);
    assert_eq!(built.get() - b, per_frame, "select-all builds nothing extra while painting");
}

#[test]
fn selection_drag_at_the_edge_scrolls_until_released() {
    let mut ui = Ui::new((), |_: &()| scroll(list(1000, 40.0, |i| text(format!("row {i}")).height(40.0)).selectable()));
    let size = (300.0, 400.0);
    ui.frame(size);
    ui.pointer_down((2.0, 15.0), false);
    ui.pointer_move((150.0, 395.0)); // inside the bottom 24 pt
    ui.frame(size);
    assert!(ui.animating, "keeps scrolling without input");
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        ui.frame(size);
    }
    let t = ui.selection_text().unwrap();
    let last: usize = t.rsplit("row ").next().unwrap().trim().parse().unwrap_or(0);
    assert!(last >= 10, "selection followed the scroll: ends at row {last}");
    ui.pointer_up((150.0, 395.0));
    let mut frames = 0;
    let before = ui.selection_text();
    while ui.animating {
        std::thread::sleep(std::time::Duration::from_millis(5));
        ui.frame(size);
        frames += 1;
        assert!(frames < 400, "idle after release");
    }
    assert_eq!(ui.selection_text(), before, "released: no more scrolling");
}

#[derive(Default)]
struct Links {
    opened: Vec<String>,
}

fn links_ui(handler: bool) -> Ui<Links> {
    let mut ui = Ui::new(Links::default(), move |_: &Links| {
        let line = rich_text(vec![
            Span::new("See the "),
            Span::new("documentation").link("https://example.com/docs").color(Color::TEAL),
            Span::new(" or "),
            Span::new("this").link("javascript:alert(1)"),
        ]);
        let v = vstack(line).selectable().place(Align::Start, Align::Start);
        if handler {
            v.on_link(|s: &mut Links, url| s.opened.push(url.to_string()))
        } else {
            v
        }
    });
    ui.frame((500.0, 200.0));
    ui
}

#[test]
fn links_open_on_click_underline_on_hover_and_select_on_drag() {
    let mut ui = links_ui(false);
    let (x, y, w, h) = text_rects(&ui)[0];
    // "documentation" sits around 40 % of the line.
    let link = (x + w * 0.4, y + h * 0.5);
    ui.pointer_move(link);
    assert_eq!(ui.cursor, Cursor::Pointer);
    ui.frame((500.0, 200.0));
    assert_eq!(quads(&ui, Color::TEAL).len(), 1, "underlined while hovered");
    click(&mut ui, link);
    assert_eq!(ui.requests, vec![ruru::WindowRequest::OpenUrl("https://example.com/docs".into())]);
    // Only http(s) and mailto open without a handler.
    ui.requests.clear();
    click(&mut ui, (x + w - 3.0, y + h * 0.5));
    assert!(ui.requests.is_empty());
    // Off the link: no hand, no underline.
    ui.pointer_move((x + 3.0, y + h * 0.5));
    assert_eq!(ui.cursor, Cursor::Text);
    ui.frame((500.0, 200.0));
    assert!(quads(&ui, Color::TEAL).is_empty());
    // A press on the link that moves away selects instead of opening.
    drag(&mut ui, link, (x + 2.0, y + h * 0.5));
    assert!(ui.requests.is_empty());
    let t = ui.selection_text().unwrap();
    assert!(t.starts_with("See the ") && t.len() > 8, "{t:?}");

    // With `.on_link()` the app gets every URL.
    let mut ui = links_ui(true);
    click(&mut ui, link);
    click(&mut ui, (x + w - 3.0, y + h * 0.5));
    assert!(ui.requests.is_empty());
    assert_eq!(ui.state.opened, vec!["https://example.com/docs".to_string(), "javascript:alert(1)".to_string()]);
}

#[test]
fn markdown_copies_as_plain_text_and_its_links_are_live() {
    let src = "# Title\n\nSee [the docs](https://example.com) now.\n\n- one\n- two\n  1. nested\n\n| a | b |\n|---|---|\n| 1 |   |\n\n```\nfn main() {}\n```";
    let mut ui = Ui::new(Vec::<String>::new(), move |_: &Vec<String>| {
        scroll(markdown(src).on_link(|s: &mut Vec<String>, url| s.push(url.to_string())))
    });
    ui.frame((500.0, 800.0));
    ui.key(KeyInput::SelectAll);
    assert_eq!(
        clipboard(&mut ui).as_deref(),
        Some("Title\n\nSee the docs now.\n\n- one\n- two\n  1. nested\n\na\tb\n1\t\n\nfn main() {}")
    );
    // The link text is clickable.
    let r = text_rects(&ui);
    let para = r[1];
    click(&mut ui, (para.0 + para.2 * 0.45, para.1 + para.3 * 0.5));
    assert_eq!(ui.state, vec!["https://example.com".to_string()]);
}

// ---------------------------------------------------------------- popovers, menus, tooltips

#[test]
fn independent_window_states_receive_only_their_links_and_escape_works_in_a_field() {
    struct Main(u32);
    struct Inspector { open: bool, query: String }
    let mut main = Ui::new(Main(0), |s: &Main| text(s.0.to_string()));
    let mut inspector = Ui::new(Inspector { open: true, query: String::new() }, |s: &Inspector|
        text_field(&s.query, "Filter", |s: &mut Inspector, v| s.query = v).size(200.0, 40.0)
            .shortcut(Shortcut::named(KeyInput::Escape), |s: &mut Inspector| s.open = false));
    ruru::link::<Main>().send(|s| s.0 = 7);
    ruru::link::<Inspector>().send(|s| s.query = "stream".into());
    assert!(inspector.pump());
    assert_eq!(main.state.0, 0);
    assert!(main.pump());
    assert_eq!(main.state.0, 7);
    assert_eq!(inspector.state.query, "stream");
    inspector.frame((200.0, 40.0));
    inspector.pointer_down((20.0, 20.0), false); inspector.pointer_up((20.0, 20.0));
    assert!(inspector.key_shortcut(ruru::ShortcutKey::Named(KeyInput::Escape)));
    assert!(!inspector.state.open);
    assert_eq!(main.state.0, 7);
}

#[test]
fn menu_action_can_replace_its_popover_with_an_escape_dismissible_modal() {
    let mut ui = Ui::new(1u8, |s: &u8| zstack((
        rect().size(30.0, 30.0).popover(*s == 1, Placement::bottom(), |s: &mut u8| *s = 0,
            menu(vec![MenuItem::action("Open inspector", |s: &mut u8| *s = 2)])),
        modal(*s == 2, |s: &mut u8| *s = 0, text("Inspector").padding(30.0)),
    )).fill());
    ui.frame((400.0, 400.0));
    ui.key(KeyInput::Down);
    ui.key(KeyInput::Enter);
    assert_eq!(ui.state, 2, "source dismissal must happen before the action");
    ui.frame((400.0, 400.0));
    ui.key(KeyInput::Escape);
    assert_eq!(ui.state, 0);
}

#[derive(Default)]
struct M {
    log: Vec<&'static str>,
    pop: bool,
    effort: f32,
    under: u32,
    sec: u32,
    draft: String,
    k: u32,
}

fn menus_view(s: &M) -> View<M> {
    let items = vec![
        MenuItem::action("Rename", |s: &mut M| s.log.push("rename")).shortcut("⌘R"),
        MenuItem::action("Pin", |s: &mut M| s.log.push("pin")).disabled(),
        MenuItem::separator(),
        MenuItem::submenu(
            "Copy",
            vec![MenuItem::action("Link", |s: &mut M| s.log.push("link")), MenuItem::action("Text", |s: &mut M| s.log.push("text"))],
        ),
        MenuItem::action("Archive", |s: &mut M| s.log.push("archive")),
    ];
    zstack((
        // Everything else lies on this: a click that leaks through counts here.
        rect().fill().on_tap(|s: &mut M| s.under += 1).on_secondary(|s: &mut M| s.sec += 1),
        rect().size(40.0, 30.0).menu(menu(items)).place(Align::Start, Align::Start),
        rect()
            .size(30.0, 30.0)
            .on_tap(|s: &mut M| s.pop = true)
            .popover(
                s.pop,
                Placement::bottom(),
                |s: &mut M| s.pop = false,
                vstack(slider(s.effort, |s: &mut M, v| s.effort = v).steps(5)).width(200.0).padding(10.0),
            )
            .place(Align::End, Align::End),
        rect().size(30.0, 30.0).tooltip("Back").place(Align::End, Align::Start),
        text_field(s.draft.as_str(), "", |s: &mut M, v| s.draft = v).width(150.0).place(Align::Start, Align::End),
        rect().size(50.0, 30.0).context_menu(menu(vec![MenuItem::action("Ctx", |s: &mut M| s.log.push("ctx"))])),
    ))
    .fill()
    .shortcut(Shortcut::cmd('k'), |s: &mut M| s.k += 1)
    .shortcut(Shortcut::key('/'), |s: &mut M| s.k += 10)
    .shortcut(Shortcut::cmd('a'), |s: &mut M| s.k += 100)
}

const W: (f32, f32) = (400.0, 400.0);

fn menus() -> Ui<M> {
    let mut ui = Ui::new(M::default(), menus_view);
    ui.frame(W);
    ui
}

/// Runs frames until the animations are over (layers fade in for ~120 ms).
fn settle(ui: &mut Ui<M>) {
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(12));
        ui.frame(W);
        if !ui.animating {
            break;
        }
    }
}

fn tap(ui: &mut Ui<M>, at: (f32, f32)) {
    ui.pointer_down(at, false);
    ui.pointer_up(at);
    ui.frame(W);
}

// The menu opens under its 40×30 anchor at (8, 36): panel padding 5, rows 28, separator 11.
const RENAME: (f32, f32) = (100.0, 55.0);
const PIN: (f32, f32) = (100.0, 83.0);
const COPY: (f32, f32) = (100.0, 122.0);
const ARCHIVE: (f32, f32) = (100.0, 150.0);
// The submenu opens to the right of Copy, its first row level with it.
const LINK: (f32, f32) = (270.0, 122.0);

fn open_menu(ui: &mut Ui<M>) {
    tap(ui, (20.0, 15.0));
    settle(ui);
}

#[test]
fn menu_opens_on_tap_picks_and_closes() {
    let mut ui = menus();
    open_menu(&mut ui);
    tap(&mut ui, PIN); // disabled: nothing, the menu stays
    assert!(ui.state.log.is_empty());
    tap(&mut ui, RENAME);
    assert_eq!(ui.state.log, ["rename"]);
    settle(&mut ui);
    tap(&mut ui, RENAME); // closed: the click lands on what is underneath
    assert_eq!(ui.state.under, 1);
    assert_eq!(ui.state.log, ["rename"]);
}

#[test]
fn click_outside_closes_without_reaching_below() {
    let mut ui = menus();
    open_menu(&mut ui);
    tap(&mut ui, (300.0, 300.0));
    assert_eq!(ui.state.under, 0, "the closing click must not leak through");
    settle(&mut ui);
    tap(&mut ui, RENAME);
    assert_eq!(ui.state.under, 1);
    assert!(ui.state.log.is_empty());
}

#[test]
fn menu_hover_highlights_and_enter_picks() {
    let mut ui = menus();
    open_menu(&mut ui);
    ui.pointer_move(ARCHIVE);
    ui.frame(W);
    assert!(ui.key(KeyInput::Enter));
    assert_eq!(ui.state.log, ["archive"]);
}

#[test]
fn menu_keyboard_skips_disabled_and_walks_submenus() {
    let mut ui = menus();
    open_menu(&mut ui);
    ui.key(KeyInput::Down); // Rename
    ui.key(KeyInput::Down); // Pin is disabled, the separator is skipped: Copy
    ui.frame(W);
    ui.key(KeyInput::Right); // opens Copy with its first row lit
    settle(&mut ui);
    ui.key(KeyInput::Left); // back to the parent
    ui.frame(W);
    ui.key(KeyInput::Enter); // Copy again, from Enter
    settle(&mut ui);
    ui.key(KeyInput::Down); // Text
    ui.frame(W);
    ui.key(KeyInput::Enter);
    assert_eq!(ui.state.log, ["text"]);
    ui.frame(W);
    assert!(!ui.key(KeyInput::Escape), "the menu is closed");
}

#[test]
fn submenu_opens_on_hover_and_escape_closes_level_by_level() {
    let mut ui = menus();
    open_menu(&mut ui);
    ui.pointer_move(COPY);
    settle(&mut ui);
    ui.pointer_move(LINK);
    ui.frame(W);
    ui.key(KeyInput::Escape); // closes the submenu only
    settle(&mut ui);
    tap(&mut ui, LINK);
    assert!(ui.state.log.is_empty() && ui.state.under == 0, "the submenu was closed, the menu was not");
    // The menu (still open) closes with Escape.
    settle(&mut ui);
    ui.key(KeyInput::Escape);
    settle(&mut ui);
    tap(&mut ui, RENAME);
    assert_eq!(ui.state.under, 1);
    // Clicking the submenu row opens it too.
    open_menu(&mut ui);
    tap(&mut ui, COPY);
    settle(&mut ui);
    tap(&mut ui, LINK);
    assert_eq!(ui.state.log, ["link"]);
}

#[test]
fn popover_flips_above_and_stays_in_the_window() {
    let mut ui = menus();
    tap(&mut ui, (385.0, 385.0));
    assert!(ui.state.pop);
    settle(&mut ui);
    // No room below the anchor at the bottom edge: it opens above (y 316..364). Centered on
    // the anchor it would cross the right edge; it is slid in to x 192..392, so the slider's
    // value range is 216..368.
    ui.pointer_down((380.0, 340.0), false);
    ui.pointer_up((380.0, 340.0));
    assert_eq!(ui.state.effort, 1.0);
    ui.pointer_down((292.0 + 15.0, 340.0), false); // 0.6 snaps to the stop at 0.5
    ui.pointer_up((292.0 + 15.0, 340.0));
    assert_eq!(ui.state.effort, 0.5);
    ui.frame(W);
    assert!(ui.key(KeyInput::Escape));
    assert!(!ui.state.pop);
    assert_eq!(ui.state.under, 0);
}

#[test]
fn popover_click_outside_calls_dismiss_only() {
    let mut ui = menus();
    tap(&mut ui, (385.0, 385.0));
    settle(&mut ui);
    tap(&mut ui, (100.0, 200.0));
    assert!(!ui.state.pop);
    assert_eq!(ui.state.under, 0);
}

#[test]
fn steps_snap_the_slider() {
    let mut ui = Ui::new(0.0f32, |v: &f32| zstack(slider(*v, |s: &mut f32, v| *s = v).steps(3).width(228.0)).fill());
    ui.frame(W);
    // Value range 200 wide starting at x 100 + 14.
    for (x, want) in [(120.0, 0.0), (200.0, 0.5), (240.0, 0.5), (300.0, 1.0)] {
        ui.pointer_down((x, 200.0), false);
        ui.pointer_up((x, 200.0));
        assert_eq!(ui.state, want, "at x {x}");
    }
}

fn text_cmds(ui: &Ui<M>) -> usize {
    ui.list.cmds.iter().filter(|c| matches!(c, ruru::Cmd::Text(_))).count()
}

#[test]
fn tooltip_waits_for_the_mouse_to_rest() {
    let mut ui = menus();
    let before = text_cmds(&ui);
    ui.pointer_move((385.0, 15.0));
    ui.frame(W);
    assert_eq!(text_cmds(&ui), before, "not yet");
    assert!(!ui.animating, "waiting costs no frames");
    let wake = ui.wake_in.expect("a timer for the tooltip");
    assert!(wake > 0.4 && wake < 0.52, "wake in {wake}");
    std::thread::sleep(std::time::Duration::from_secs_f32(wake));
    ui.frame(W);
    assert_eq!(text_cmds(&ui), before + 1, "shown");
    settle(&mut ui);
    // It takes no input: a click under it reaches the view below.
    ui.pointer_move((370.0, 45.0));
    ui.frame(W);
    tap(&mut ui, (370.0, 45.0));
    assert_eq!(ui.state.under, 1);
    settle(&mut ui);
    assert_eq!(text_cmds(&ui), before, "gone once the mouse left");
    // A click on the view hides it too.
    ui.pointer_move((385.0, 15.0));
    ui.frame(W);
    std::thread::sleep(std::time::Duration::from_millis(10));
    ui.frame(W);
    assert_eq!(text_cmds(&ui), before + 1, "right after another one it shows at once");
    tap(&mut ui, (385.0, 15.0));
    settle(&mut ui);
    assert_eq!(text_cmds(&ui), before);
}

#[test]
fn shortcuts_fire_but_leave_text_keys_to_a_field() {
    use ruru::ShortcutKey::Char;
    let mut ui = menus();
    let cmd = ruru::Mods { command: true, ..Default::default() };
    ui.set_modifiers(cmd);
    assert!(ui.key_shortcut(Char('K')));
    assert!(ui.key_shortcut(Char('a')));
    ui.set_modifiers(ruru::Mods::default());
    assert!(ui.key_shortcut(Char('/')));
    assert!(!ui.key_shortcut(Char('k')), "plain K is not bound");
    assert_eq!(ui.state.k, 111);
    tap(&mut ui, (75.0, 380.0)); // focus the field
    assert!(ui.has_focus());
    assert!(!ui.key_shortcut(Char('/')), "typing");
    ui.set_modifiers(cmd);
    assert!(!ui.key_shortcut(Char('a')), "select all in the field");
    assert!(ui.key_shortcut(Char('k')));
    assert_eq!(ui.state.k, 112);
}

#[test]
fn secondary_click_opens_the_context_menu_at_the_pointer() {
    let mut ui = menus();
    assert!(ui.secondary_click((200.0, 250.0)));
    assert_eq!(ui.state.sec, 1, "plain on_secondary");
    ui.frame(W);
    assert!(ui.secondary_click((190.0, 190.0)));
    settle(&mut ui);
    // Opens right below the pointer: its first row is at y 197..225.
    tap(&mut ui, (250.0, 210.0));
    assert_eq!(ui.state.log, ["ctx"]);
    assert_eq!(ui.state.under, 0);
    assert_eq!(ui.state.sec, 1);
}

// ---------------------------------------------------------------- trackpad at the edge

/// Content top (screen y of the tall red rect) in a 400 pt scroll view.
fn top_y(ui: &Ui<()>) -> f32 {
    quads(ui, Color::RED)[0][1]
}

fn edge_ui() -> Ui<()> {
    let mut ui = Ui::new((), |_: &()| scroll(rect().height(2000.0).fill_width().background(Color::RED)));
    ui.frame((300.0, 400.0));
    ui
}

/// One trackpad event as macOS sends it, ~8 ms apart, with a frame after each.
fn pad(ui: &mut Ui<()>, dy: f32, phase: ruru::WheelPhase, ys: &mut Vec<f32>) {
    ui.wheel((150.0, 200.0), dy, phase);
    std::thread::sleep(std::time::Duration::from_millis(8));
    ui.frame((300.0, 400.0));
    ys.push(top_y(ui));
}

#[test]
fn trackpad_momentum_into_the_edge_bounces_once_and_returns_monotonically() {
    use ruru::WheelPhase::*;
    let mut ui = edge_ui();
    let mut ys = Vec::new();
    // A flick towards the top: fingers, lift, then OS momentum with decaying deltas that keeps
    // coming long after the content met the edge.
    pad(&mut ui, 0.0, Started, &mut ys);
    for _ in 0..4 {
        pad(&mut ui, 12.0, Moved, &mut ys);
    }
    pad(&mut ui, 0.0, Ended, &mut ys);
    let mut d = 30.0;
    pad(&mut ui, d, Started, &mut ys);
    for _ in 0..60 {
        d *= 0.93;
        pad(&mut ui, d, Moved, &mut ys);
    }
    pad(&mut ui, 0.0, Ended, &mut ys);
    for _ in 0..80 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        ui.frame((300.0, 400.0));
        ys.push(top_y(&ui));
    }
    // Once it starts coming back it never goes out again: one bounce, no stutter.
    let peak = ys.iter().cloned().enumerate().fold((0, f32::MIN), |m, (i, y)| if y > m.1 { (i, y) } else { m }).0;
    for w in ys[peak..].windows(2) {
        assert!(w[1] <= w[0] + 0.01, "the return stutters: {ys:?}");
    }
    assert!(ys[peak] > 5.0, "the momentum should carry past the edge: {ys:?}");
    assert_eq!(*ys.last().unwrap(), 0.0, "settles on the edge");
}

#[test]
fn trackpad_new_gesture_during_the_bounce_follows_the_fingers() {
    use ruru::WheelPhase::*;
    let mut ui = edge_ui();
    let mut ys = Vec::new();
    // Pull past the top, let go, momentum runs out against the edge…
    pad(&mut ui, 0.0, Started, &mut ys);
    for _ in 0..4 {
        pad(&mut ui, 15.0, Moved, &mut ys);
    }
    pad(&mut ui, 0.0, Ended, &mut ys);
    pad(&mut ui, 10.0, Started, &mut ys);
    pad(&mut ui, 5.0, Moved, &mut ys);
    pad(&mut ui, 0.0, Ended, &mut ys);
    // …and the fingers come down again at once, pulling the same way.
    pad(&mut ui, 0.0, Started, &mut ys);
    let caught = *ys.last().unwrap();
    assert!(caught > 0.0, "still stretched when the fingers land");
    // Fingers resting on the pad hold the stretch, like NSScrollView.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        ui.frame((300.0, 400.0));
    }
    let held = top_y(&ui);
    assert!((held - caught).abs() < 0.5, "resting fingers let go: {caught} → {held}");
    let mut last = held;
    for _ in 0..4 {
        pad(&mut ui, 10.0, Moved, &mut ys);
        let y = *ys.last().unwrap();
        assert!(y > last && y - last <= 10.0, "the new gesture must move the content: {last} → {y}");
        last = y;
    }
    // Pushing back by the same amount retraces the stretch.
    for _ in 0..4 {
        pad(&mut ui, -10.0, Moved, &mut ys);
    }
    assert!((top_y(&ui) - held).abs() < 0.5, "back and forth is not reversible: {held} → {}", top_y(&ui));
    pad(&mut ui, 0.0, Ended, &mut ys);
}

// ---------------------------------------------------------------- resizing a chat feed

const PARA: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. ";

/// A feed like a chat: rows of wrapping text of different lengths, each with a marker quad
/// on top whose blue channel encodes the row.
fn feed(stick: bool) -> Ui<()> {
    Ui::new((), move |_: &()| {
        let rows = list(60, 120.0, |i| {
            vstack((rect().height(2.0).fill_width().background(Color::rgba(1.0, 0.0, i as f32 / 64.0, 1.0)), text(PARA.repeat(1 + i % 4))))
                .align(Align::Start)
                .padding_v(8.0)
        })
        .selectable();
        let column = zstack(rows.fill_width().max_width(700.0)).fill_width();
        let s = scroll(column.padding_ltrb(16.0, 10.0, 16.0, 30.0)).fill();
        if stick { s.stick_to_bottom() } else { s }
    })
}

fn marker_y(ui: &Ui<()>, i: usize) -> Option<f32> {
    quads(ui, Color::rgba(1.0, 0.0, i as f32 / 64.0, 1.0)).first().map(|r| r[1])
}

/// Bottom of the last row's text on screen.
fn last_text_bottom(ui: &Ui<()>) -> f32 {
    texts(ui).iter().map(|t| t.pos.1 + t.size.1).fold(f32::MIN, f32::max)
}

#[test]
fn resizing_keeps_a_stuck_feed_on_the_bottom_every_frame() {
    let mut ui = feed(true);
    for _ in 0..4 {
        ui.frame((640.0, 500.0));
    }
    let rest = last_text_bottom(&ui);
    assert!(marker_y(&ui, 59).is_some(), "starts at the end");
    // Dragging the window edge: a new width every frame.
    let mut w = 640.0;
    for _ in 0..40 {
        w -= 9.0;
        ui.frame((w, 500.0));
        let b = last_text_bottom(&ui);
        assert!((b - rest).abs() < 1.0, "at width {w} the end jumped: {rest} → {b}");
    }
    for _ in 0..40 {
        w += 9.0;
        ui.frame((w, 500.0));
        let b = last_text_bottom(&ui);
        assert!((b - rest).abs() < 1.0, "at width {w} the end jumped: {rest} → {b}");
    }
}

#[test]
fn scrolling_up_into_unmeasured_rows_moves_by_the_wheel_delta() {
    let mut ui = feed(true);
    for _ in 0..4 {
        ui.frame((640.0, 500.0));
    }
    // Rows above the end were never measured: each comes in at its real height, not 120.
    for _ in 0..25 {
        let before: Vec<(usize, f32)> = (0..60).filter_map(|i| marker_y(&ui, i).map(|y| (i, y))).collect();
        ui.wheel((300.0, 250.0), 100.0, ruru::WheelPhase::Line);
        ui.frame((640.0, 500.0));
        for (i, y) in before {
            if let Some(y2) = marker_y(&ui, i) {
                assert!((y2 - y - 100.0).abs() < 0.5, "row {i} moved {} instead of 100", y2 - y);
            }
        }
    }
}

#[test]
fn resizing_keeps_the_top_row_of_a_parked_feed_in_place() {
    use ruru::WheelPhase::*;
    let mut ui = feed(false);
    for _ in 0..3 {
        ui.frame((640.0, 500.0));
    }
    // Scroll into the middle and let every row on the way be measured.
    for _ in 0..30 {
        ui.wheel((300.0, 250.0), -150.0, Line);
        ui.frame((640.0, 500.0));
        ui.frame((640.0, 500.0));
    }
    let top = (0..60).find(|&i| marker_y(&ui, i).is_some_and(|y| y >= 0.0)).unwrap();
    let y0 = marker_y(&ui, top).unwrap();
    let mut w = 640.0;
    for _ in 0..30 {
        w -= 9.0;
        ui.frame((w, 500.0));
        let y = marker_y(&ui, top).unwrap_or(f32::NAN);
        assert!((y - y0).abs() < 1.0, "at width {w} row {top} moved {y0} → {y}");
    }
    for _ in 0..30 {
        w += 9.0;
        ui.frame((w, 500.0));
        let y = marker_y(&ui, top).unwrap_or(f32::NAN);
        assert!((y - y0).abs() < 1.0, "at width {w} row {top} moved {y0} → {y}");
    }
}

// ---------------------------------------------------------------- selection in a chat window

const ANSWER: &str = "First paragraph of the answer with some words.\n\n| key | value |\n|---|---|\n| alpha | beta gamma |\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nLast line.";

struct Gw {
    draft: String,
    open: bool,
}

/// The window of the chat app this was reported against: a title bar to drag the window
/// by, a rounded clipped panel with an outline on top, a sidebar, the feed (a selectable
/// lazy list of markdown in a centered column, stuck to the bottom) and a composer.
fn grom() -> Ui<Gw> {
    use ruru::{CodeBlockStyle, MarkdownStyle, TableStyle};
    let style = std::rc::Rc::new(MarkdownStyle {
        code_block: CodeBlockStyle { radius: 8.0, padding: 12.0, ..Default::default() },
        table: TableStyle { rule: Some((1.0, Color::WHITE.alpha(0.1))), ..Default::default() },
        ..Default::default()
    });
    let mut ui = Ui::new(Gw { draft: String::new(), open: true }, move |s: &Gw| {
        let style = style.clone();
        let hover_btn = || zstack(rect().size(16.0, 16.0)).size(28.0, 28.0).radius(6.0).hover(Color::WHITE.alpha(0.1));
        let titlebar = hstack((hover_btn().on_tap(|s: &mut Gw| s.open = !s.open), text("Title").fill_width(), hover_btn()))
            .fill_width()
            .height(40.0)
            .window_drag();
        let rows = list(3, 120.0, move |k| {
            let actions = hstack((rect().size(16.0, 16.0), rect().size(16.0, 16.0))).spacing(8.0).copy_as("");
            vstack((ruru::markdown_styled(ANSWER, &style), actions)).spacing(8.0).align(Align::Start).fill_width().padding_v(k as f32 + 12.0)
        })
        .selectable();
        let column = zstack(rows.fill_width().max_width(600.0)).fill_width();
        let feed = scroll(column.padding_ltrb(24.0, 12.0, 24.0, 20.0)).stick_to_bottom().fill().color(Color::WHITE);
        let field = text_field(s.draft.as_str(), "Ask", |s: &mut Gw, v| s.draft = v).multiline().plain();
        let composer = zstack(vstack((field, hstack((rect().size(20.0, 20.0), spacer())).fill_width())).fill_width().max_width(600.0).padding(12.0).background(Color::hex(0x303030)).border(1.0, Color::WHITE.alpha(0.1)).radius(16.0))
            .fill_width()
            .padding(12.0);
        let workspace = vstack((feed, composer)).fill();
        let sidebar = hstack((vstack(text("chat")).fill(), rect().width(1.0).fill_height())).width(if s.open { 200.0 } else { 0.0 }).fill_height().animate();
        let outline = rect().fill().radius(12.0).border(1.0, Color::WHITE.alpha(0.1));
        let panel = zstack((hstack((sidebar, workspace)).fill(), outline)).fill().background(Color::hex(0x202020)).radius(12.0).clip();
        let body = hstack((rect().width(50.0).fill_height(), panel)).fill().padding_ltrb(0.0, 0.0, 8.0, 8.0);
        vstack((titlebar, body)).fill().selection_color(Color::hex(0x2060ff).alpha(0.4))
    });
    for _ in 0..6 {
        ui.frame((1000.0, 1200.0));
    }
    ui
}

/// A mouse drag as a person does it: frames in between, the pointer moving in steps.
fn drag_live(ui: &mut Ui<Gw>, from: (f32, f32), to: (f32, f32)) {
    ui.pointer_move(from);
    ui.frame((1000.0, 1200.0));
    ui.pointer_down(from, false);
    ui.frame((1000.0, 1200.0));
    for k in 1..=8 {
        let t = k as f32 / 8.0;
        ui.pointer_move((from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
        ui.frame((1000.0, 1200.0));
    }
    ui.pointer_up(to);
    ui.frame((1000.0, 1200.0));
}

/// Text rects of the feed, top to bottom (title, sidebar and field placeholder left out).
fn feed_texts(ui: &Ui<Gw>) -> Vec<(f32, f32, f32, f32)> {
    let mut r: Vec<_> = text_rects(ui).into_iter().filter(|r| r.0 > 280.0 && r.1 > 45.0 && r.1 < 1000.0).collect();
    r.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.partial_cmp(&b.0).unwrap()));
    r
}

#[test]
fn chat_window_selects_paragraphs_cells_and_code() {
    let mut ui = grom();
    let t = feed_texts(&ui);
    assert!(t.len() >= 3 * 6, "three messages on screen: {t:?}");
    // A paragraph: from its start to the middle.
    let p = t[0];
    drag_live(&mut ui, (p.0 + 1.0, p.1 + p.3 * 0.5), (p.0 + p.2 * 0.5, p.1 + p.3 * 0.5));
    let s = ui.selection_text().unwrap_or_default();
    assert!(s.starts_with("First par") && s.len() > 10, "paragraph: {s:?}");
    assert!(ui.cursor == ruru::Cursor::Text, "an I-beam over text");

    // A table cell.
    let cell = *t.iter().find(|r| r.1 > p.1 + p.3 + 5.0 && r.0 > p.0 + 30.0).unwrap();
    drag_live(&mut ui, (cell.0 + 1.0, cell.1 + cell.3 * 0.5), (cell.0 + cell.2 - 1.0, cell.1 + cell.3 * 0.5));
    let s = ui.selection_text().unwrap_or_default();
    assert!(!s.is_empty(), "table cell selects");

    // The code block, whole, keeps its lines and indent.
    let code = *t.iter().filter(|r| r.3 > 40.0).next().expect("a three-line code block");
    drag_live(&mut ui, (code.0 + 1.0, code.1 + 2.0), (code.0 + code.2 + 40.0, code.1 + code.3 - 2.0));
    let s = ui.selection_text().unwrap_or_default();
    assert_eq!(s, "fn main() {\n    println!(\"hi\");\n}", "code block");
    assert_eq!(clipboard(&mut ui).as_deref(), Some("fn main() {\n    println!(\"hi\");\n}"));

    // Across two messages.
    let second = t.iter().filter(|r| r.0 == p.0 && r.1 > code.1 + code.3).nth(1).copied().unwrap();
    drag_live(&mut ui, (p.0 + 1.0, p.1 + p.3 * 0.5), (second.0 + 20.0, second.1 + second.3 * 0.5));
    let s = ui.selection_text().unwrap_or_default();
    assert!(s.contains("Last line.") && s.contains("fn main"), "across messages: {s:?}");
}

// ---------------------------------------------------------------- scroll bar

fn bar_ui() -> Ui<()> {
    let mut ui = Ui::new((), |_: &()| {
        let rows = list(60, 40.0, |i| text(format!("row {i} with a line of text that runs right up to the edge of the view")).fill_width().height(40.0));
        scroll(vstack((rect().height(1.0).fill_width(), rows)).background(Color::RED).selectable())
    });
    ui.frame((300.0, 400.0));
    ui
}

/// The scroll bar thumb: a narrow quad at the right edge.
fn thumb(ui: &Ui<()>) -> Option<[f32; 4]> {
    ui.list.cmds.iter().find_map(|c| match c {
        ruru::Cmd::Quad(q) if q.rect[0] > 280.0 && q.rect[2] <= 10.0 && q.rect[3] > 20.0 && q.rect[3] < 390.0 => Some(q.rect),
        _ => None,
    })
}

fn settle_bar(ui: &mut Ui<()>) {
    // Spring duration depends on frame time; an exact frame count can end just before
    // settling under parallel test load. Keep a hard deadline and still require rest.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        ui.frame((300.0, 400.0));
        if !ui.animating || std::time::Instant::now() >= deadline { break; }
    }
}

#[test]
fn scroll_hover_and_its_bar_widening_are_separate_springs() {
    let tint = Color::RED.alpha(0.5);
    let mut ui = Ui::new((), move |_: &()| {
        let rows = list(60, 40.0, |i| text(format!("row {i}")).fill_width().height(40.0));
        scroll(rows).indicator(Indicator::Always).hover(tint)
    });
    ui.frame((300.0, 400.0));
    ui.pointer_move((150.0, 200.0));
    settle_bar(&mut ui);
    assert_eq!(quads(&ui, tint).len(), 1, "hovering the content tints the scroll view");
    let t = thumb(&ui).expect("always shown");
    assert!(t[2] < 8.0, "the hover does not widen the bar: {t:?}");
    ui.pointer_move((294.0, 200.0));
    settle_bar(&mut ui);
    assert!(thumb(&ui).expect("shown")[2] > 8.0, "the bar widens under the pointer");
    assert_eq!(quads(&ui, tint).len(), 1, "and the view stays tinted, not flickering with the bar");
}

#[test]
fn scroll_bar_shows_on_hover_at_the_edge_and_widens() {
    let mut ui = bar_ui();
    settle_bar(&mut ui);
    assert!(thumb(&ui).is_none(), "hidden while idle");
    ui.pointer_move((150.0, 200.0));
    settle_bar(&mut ui);
    assert!(thumb(&ui).is_none(), "the middle of the view does not reveal it");
    assert_eq!(ui.cursor, ruru::Cursor::Text);
    ui.pointer_move((294.0, 200.0));
    settle_bar(&mut ui);
    let t = thumb(&ui).expect("hovering the edge reveals the bar");
    assert!(t[2] > 8.0, "and widens it: {t:?}");
    assert_eq!(ui.cursor, ruru::Cursor::Default, "an arrow over the bar, not an I-beam");
    assert!(!ui.animating && ui.wake_in.is_none_or(|w| w > 0.5), "no redraws while it rests under the pointer: {} {:?}", ui.animating, ui.wake_in);
    // Leaving: it narrows and fades within about a second, through wake-ups.
    ui.pointer_move((150.0, 200.0));
    ui.frame((300.0, 400.0));
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        ui.frame((300.0, 400.0));
    }
    assert!(thumb(&ui).is_none(), "fades out");
}

#[test]
fn scroll_bar_thumb_drags_proportionally_and_the_track_pages() {
    let mut ui = bar_ui();
    ui.pointer_move((294.0, 200.0));
    settle_bar(&mut ui);
    let t = thumb(&ui).unwrap();
    let (x, y) = (t[0] + t[2] * 0.5, t[1] + t[3] * 0.5);
    ui.pointer_down((x, y), false);
    for k in 1..=5 {
        ui.pointer_move((x - 30.0, y + 20.0 * k as f32)); // sideways drift does not matter
        ui.frame((300.0, 400.0));
        assert_eq!(ui.cursor, ruru::Cursor::Default);
    }
    ui.pointer_up((x - 30.0, y + 100.0));
    ui.frame((300.0, 400.0));
    // Content 2401, view 400: 100 pt of thumb travel over (392 - thumb) of track.
    let len = 392.0 * 400.0 / 2401.0;
    let want = 100.0 * 2001.0 / (392.0 - len);
    assert!((-top_y(&ui) - want).abs() < 1.0, "offset {} want {want}", -top_y(&ui));
    assert!(ui.selection_text().is_none_or(|s| s.is_empty()), "the drag selects nothing");
    // Dragging past the end stops on it: no rubber band.
    let t = thumb(&ui).unwrap();
    ui.pointer_down((t[0] + 2.0, t[1] + 5.0), false);
    ui.pointer_move((t[0] + 2.0, 2000.0));
    ui.pointer_up((t[0] + 2.0, 2000.0));
    ui.frame((300.0, 400.0));
    assert_eq!(-top_y(&ui), 2001.0);
    // A click on the track above the thumb pages up by most of a screen.
    ui.pointer_move((294.0, 30.0));
    ui.frame((300.0, 400.0));
    ui.pointer_down((294.0, 30.0), false);
    ui.pointer_up((294.0, 30.0));
    ui.frame((300.0, 400.0));
    assert_eq!(-top_y(&ui), 2001.0 - 360.0);
}

#[derive(Default)]
struct Over {
    pop: bool,
    modal: bool,
    taps: u32,
}

const FADE: Color = Color::hex(0x202020);
const BOX: Color = Color::hex(0x404040);
const POP: Color = Color::hex(0x606060);

/// A chat-like column: a feed, a fade laid over its lower part and a tappable "composer"
/// floating over its end; the bar stops above the composer.
fn over_ui() -> Ui<Over> {
    let mut ui = Ui::new(Over::default(), |s: &Over| {
        let rows = list(60, 40.0, |i| text(format!("row {i}")).fill_width().height(40.0));
        zstack((
            scroll(rows).indicator_insets(0.0, 80.0),
            rect().fill_width().height(150.0).background(FADE).place(Align::Center, Align::End),
            rect()
                .fill_width()
                .height(70.0)
                .background(BOX)
                .on_tap(|s: &mut Over| s.taps += 1)
                .popover(s.pop, Placement::top(), |s: &mut Over| s.pop = false, rect().size(280.0, 300.0).background(POP))
                .place(Align::Center, Align::End),
            modal(s.modal, |s: &mut Over| s.modal = false, rect().size(100.0, 100.0)),
        ))
        .fill()
    });
    ui.frame((300.0, 400.0));
    ui
}

fn settle_over(ui: &mut Ui<Over>) {
    for _ in 0..80 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        ui.frame((300.0, 400.0));
    }
}

/// Paint order of the first quad matching `f`.
fn order(ui: &Ui<impl Sized>, f: impl Fn(&[f32; 4], &[f32; 4]) -> bool) -> Option<usize> {
    ui.list.cmds.iter().position(|c| matches!(c, ruru::Cmd::Quad(q) if f(&q.rect, &q.c0)))
}

#[test]
fn scroll_bar_lies_over_content_above_the_scroll_but_under_popovers_and_modals() {
    let mut ui = over_ui();
    let is = |c: Color| move |_: &[f32; 4], q: &[f32; 4]| *q == [c.r, c.g, c.b, c.a];
    let thumb_at = |r: &[f32; 4], _: &[f32; 4]| r[0] > 280.0 && r[2] <= 10.0 && r[3] > 20.0;
    // Hovering the edge where the fade lies over the feed still reveals the bar.
    ui.pointer_move((294.0, 280.0));
    settle_over(&mut ui);
    let t = ui
        .list
        .cmds
        .iter()
        .find_map(|c| match c {
            ruru::Cmd::Quad(q) if thumb_at(&q.rect, &q.c0) => Some(q.rect),
            _ => None,
        })
        .expect("the bar shows under the pointer");
    assert!(order(&ui, thumb_at).unwrap() > order(&ui, is(FADE)).unwrap(), "the bar is painted over the fade");
    assert!(order(&ui, thumb_at).unwrap() > order(&ui, is(BOX)).unwrap(), "and over the composer");
    // It stops above the composer: track 400 - 80, from 4 to 316.
    assert!(t[1] + t[3] <= 316.0 + 0.01, "the bar ends above the composer: {t:?}");
    // A press on the strip over the fade grabs the bar; one on the composer at the same
    // x still taps the composer.
    ui.pointer_down((294.0, 300.0), false);
    ui.pointer_up((294.0, 300.0));
    ui.frame((300.0, 400.0));
    assert_eq!(ui.state.taps, 0, "the bar took the press, not what lies under it");
    ui.pointer_move((294.0, 370.0));
    ui.frame((300.0, 400.0));
    click(&mut ui, (294.0, 370.0));
    assert_eq!(ui.state.taps, 1, "below the inset the composer gets the press");
    // A popover and a modal cover the bar.
    ui.pointer_move((294.0, 200.0));
    ui.state.pop = true;
    settle_over(&mut ui);
    assert!(order(&ui, is(POP)).unwrap() > order(&ui, thumb_at).unwrap(), "the popover is over the bar");
    ui.state.pop = false;
    ui.state.modal = true;
    settle_over(&mut ui);
    let scrim = ui.list.cmds.iter().position(|c| matches!(c, ruru::Cmd::Quad(q) if q.rect == [0.0, 0.0, 300.0, 400.0] && q.c0[3] > 0.0 && q.c0[3] < 1.0));
    assert!(scrim.unwrap() > order(&ui, thumb_at).unwrap(), "the modal's scrim is over the bar");
}

#[test]
fn scroll_bar_press_keeps_the_text_selection() {
    let mut ui = bar_ui();
    ui.key(KeyInput::SelectAll);
    ui.frame((300.0, 400.0));
    assert!(ui.selection_text().is_some_and(|s| s.len() > 100));
    ui.pointer_move((294.0, 200.0));
    settle_bar(&mut ui);
    let t = thumb(&ui).unwrap();
    click(&mut ui, (t[0] + 2.0, t[1] + 5.0));
    assert!(ui.selection_text().is_some_and(|s| s.len() > 100), "a click on the bar is not a click on the text");
}

#[test]
fn copy_text_from_a_handler_reaches_the_clipboard() {
    let mut ui = Ui::new(String::from("answer text"), |_: &String| {
        button("copy", |s: &mut String| ruru::copy_text(s.clone())).fill()
    });
    ui.frame((200.0, 200.0));
    click(&mut ui, (100.0, 100.0));
    assert_eq!(ui.requests, vec![ruru::WindowRequest::SetClipboard("answer text".into())], "right in the tap");
    // From a timer: with the next frame.
    ui.requests.clear();
    ruru::after(0.0, |_: &mut String| ruru::copy_text("later"));
    ui.frame((200.0, 200.0));
    ui.frame((200.0, 200.0));
    assert_eq!(ui.requests, vec![ruru::WindowRequest::SetClipboard("later".into())]);
}

#[test]
fn bordered_clip_draws_its_border_over_children_and_cuts_them_inside_it() {
    let border = Color::WHITE.alpha(0.1);
    let mut ui = Ui::new(0, move |_: &i32| {
        zstack(rect().fill().background(Color::RED).on_tap(|n: &mut i32| *n += 1))
            .size(200.0, 100.0)
            .background(Color::BLUE)
            .radius(20.0)
            .border(1.0, border)
            .clip()
            .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    // The corner outside the arc is not the child's; just inside it is.
    for (pos, taps) in [((3.0, 3.0), 0), ((9.0, 9.0), 1), ((100.0, 50.0), 2)] {
        click(&mut ui, pos);
        assert_eq!(ui.state, taps, "tap at {pos:?}");
    }
    let qs: Vec<ruru::Quad> = ui.list.cmds.iter().filter_map(|c| if let ruru::Cmd::Quad(q) = c { Some(*q) } else { None }).collect();
    let last = qs.last().unwrap();
    assert_eq!(last.border, [border.r, border.g, border.b, border.a], "the border comes last, over the child");
    let red = Color::RED;
    let child = qs.iter().find(|q| q.c0 == [red.r, red.g, red.b, red.a]).unwrap();
    assert_eq!(child.clip, [1.0, 1.0, 198.0, 98.0], "children are cut inside the border");
    assert_eq!(child.p1[2], 19.0, "with the inner radius");
}

// ---------------------------------------------------------------- pixel grid

fn on_grid(v: f32, scale: f32) -> bool {
    let p = v * scale;
    (p - p.round()).abs() < 1e-3
}

#[test]
fn scrolled_content_stays_on_the_pixel_grid_and_text_keeps_to_its_plate() {
    use ruru::WheelPhase::*;
    const PLATE: Color = Color::RED;
    const LINE: Color = Color::GREEN;
    for scale in [1.0, 1.5, 2.0] {
        let mut ui = Ui::new((), |_: &()| {
            let row = |i: usize| {
                vstack((
                    rect().height(7.3 + i as f32 * 0.37).fill_width(),
                    text(format!("Plate {i}")).padding(3.3).background(PLATE).radius(4.0),
                    rect().height(0.5).fill_width().background(LINE),
                ))
                .align(Align::Start)
                .padding_ltrb(10.25, 0.0, 0.0, 0.0)
            };
            scroll(vstack((0..40).map(row).collect::<Vec<_>>()).spacing(5.1).fill_width())
        });
        ui.scale = scale;
        let size = (300.0, 400.0);
        ui.frame(size);
        ui.wheel((150.0, 200.0), 0.0, Started);
        let mut rel = std::collections::HashMap::new();
        for k in 0..12 {
            ui.wheel((150.0, 200.0), -(3.0 + k as f32 * 0.137), Moved);
            ui.frame(size);
            let plates = quads(&ui, PLATE);
            let lines = quads(&ui, LINE);
            let ts = texts(&ui);
            assert!(!plates.is_empty() && !lines.is_empty() && ts.len() == plates.len());
            for q in plates.iter().chain(&lines) {
                assert!(q.iter().all(|&v| on_grid(v, scale)), "scale {scale}: quad off the grid {q:?}");
            }
            for l in &lines {
                assert!((l[3] * scale - 1.0).abs() < 1e-3, "scale {scale}: hairline is one pixel, got {l:?}");
            }
            for (t, p) in ts.iter().zip(&plates) {
                assert!(on_grid(t.pos.0, scale) && on_grid(t.pos.1, scale), "scale {scale}: text at {:?}", t.pos);
                let d = ((t.pos.0 - p[0]) * scale, (t.pos.1 - p[1]) * scale);
                let d = (d.0.round(), d.1.round());
                // At every offset the same distance from a text to its plate: they move as one.
                assert_eq!(*rel.entry(t.key).or_insert(d), d, "scale {scale}: text moved against its plate");
            }
        }
    }
}

#[test]
fn animate_snaps_without_size_judder() {
    // A sliding sidebar at 1.5x: edges land on pixels, the neighbour still meets it, the
    // content inside keeps its size, and the width only ever moves one way.
    let mut ui = Ui::new(true, |open: &bool| {
        hstack((
            vstack(rect().size(200.0, 40.0).background(Color::BLUE)).width(if *open { 280.3 } else { 0.0 }).background(Color::RED).animate(),
            rect().fill().background(Color::GREEN),
        ))
        .fill()
    });
    let scale = 1.5;
    ui.scale = scale;
    let size = (800.0, 400.0);
    ui.frame(size);
    ui.state = false;
    let mut widths = Vec::new();
    for _ in 0..500 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame(size);
        let red = quads(&ui, Color::RED).first().copied().unwrap_or([0.0; 4]);
        let green = quads(&ui, Color::GREEN)[0];
        assert!(red.iter().chain(&green).all(|&v| on_grid(v, scale)), "{red:?} {green:?}");
        assert!((green[0] - red[2]).abs() < 1e-3, "the neighbour meets it: {red:?} {green:?}");
        if let Some(b) = quads(&ui, Color::BLUE).first() {
            assert!((b[2] - 200.0).abs() < 1e-3 && (b[3] - 40.0).abs() < 1e-3, "content keeps its size: {b:?}");
        }
        widths.push(red[2]);
        if !ui.animating {
            break;
        }
    }
    assert!(widths.len() > 5);
    assert!(widths.windows(2).all(|p| p[1] <= p[0] + 1e-3), "no step back: {widths:?}");
    assert_eq!(*widths.last().unwrap(), 0.0);
}

/// Frames of `ui` until it is still, recording the first quad of `color` each frame.
fn track<T: 'static>(ui: &mut Ui<T>, size: (f32, f32), color: Color) -> Vec<[f32; 4]> {
    let mut seen = Vec::new();
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame(size);
        if let Some(q) = quads(ui, color).first() {
            seen.push(*q);
        }
        if !ui.animating {
            break;
        }
    }
    seen
}

/// The frames of a scale-in end exactly on the snapped rect: once an edge is within a pixel
/// of where it rests, it only takes steps of a fraction of a pixel (no jump to the grid when
/// the scale reaches 1). Without overshoot (`one_way`) every edge also moves only one way.
fn assert_lands_on_grid(seen: &[[f32; 4]], scale: f32, one_way: bool, what: &str) {
    assert!(seen.len() > 5, "{what}: animated, {} frames", seen.len());
    let last = *seen.last().unwrap();
    assert!(last.iter().all(|&v| on_grid(v, scale)), "{what}: rests on the grid {last:?}");
    for c in 0..4 {
        let v: Vec<f32> = seen.iter().map(|q| q[c] * scale).collect();
        let end = *v.last().unwrap();
        let near = v.iter().position(|x| (x - end).abs() < 1.0).unwrap();
        assert!(v[near..].windows(2).all(|p| (p[1] - p[0]).abs() < 0.5), "{what}: edge {c} jumps near the end {v:?}");
        let steps: Vec<f32> = v.windows(2).map(|p| p[1] - p[0]).filter(|d| d.abs() > 1e-4).collect();
        assert!(!one_way || steps.iter().all(|d| d.signum() == steps[0].signum()), "{what}: edge {c} turns back {v:?}");
    }
}

#[test]
fn popover_modal_and_press_settle_without_a_snap_jump() {
    const PANEL: Color = Color::hex(0x336699);
    const DIALOG: Color = Color::hex(0x663399);
    const KEY: Color = Color::hex(0x996633);
    // Everything sits off the pixel grid, so snapping has something to do at the end.
    let mut ui = Ui::new((false, false), |s: &(bool, bool)| {
        zstack((
            rect()
                .size(31.3, 17.7)
                .popover(s.0, Placement::bottom().gap(5.3), |s: &mut (bool, bool)| s.0 = false, rect().size(120.3, 40.7).background(PANEL))
                .padding_ltrb(37.3, 21.7, 0.0, 0.0)
                .place(Align::Start, Align::Start),
            rect().size(90.3, 30.6).background(KEY).on_tap(|_: &mut (bool, bool)| {}).padding_ltrb(0.0, 0.0, 13.3, 11.7).place(Align::End, Align::End),
            modal(s.1, |s: &mut (bool, bool)| s.1 = false, rect().size(150.3, 70.9).background(DIALOG)),
        ))
        .fill()
    });
    let size = (401.0, 303.0);
    for scale in [1.5, 2.0] {
        ui.scale = scale;
        ui.state = (false, false);
        track(&mut ui, size, PANEL);
        ui.state.0 = true;
        assert_lands_on_grid(&track(&mut ui, size, PANEL), scale, true, "popover");
        ui.state = (false, false);
        track(&mut ui, size, PANEL);
        ui.state.1 = true;
        assert_lands_on_grid(&track(&mut ui, size, DIALOG), scale, false, "modal");
        ui.state = (false, false);
        track(&mut ui, size, DIALOG);
        // A pressed button shrinks a little; let go, it grows back onto the grid.
        let k = quads(&ui, KEY)[0];
        let at = (k[0] + 10.0, k[1] + 10.0);
        ui.pointer_down(at, false);
        track(&mut ui, size, KEY);
        ui.pointer_up(at);
        assert_lands_on_grid(&track(&mut ui, size, KEY), scale, true, "press");
    }
}

fn clipped_menu_view(s: &M) -> View<M> {
    let items = || {
        vec![
            MenuItem::action("One", |s: &mut M| s.log.push("one")),
            MenuItem::action("Two", |s: &mut M| s.log.push("two")),
            MenuItem::action("Three", |s: &mut M| s.log.push("three")),
        ]
    };
    zstack((
        rect()
            .size(30.0, 30.0)
            .on_tap(|s: &mut M| s.pop = true)
            .popover(
                s.pop,
                Placement::bottom().start(),
                |s: &mut M| s.pop = false,
                zstack(scroll(menu(items()).width(200.0)))
                    .height(80.0)
                    .clip()
                    .glass_with(Glass::dark())
                    .background(Color::BLACK.alpha(0.5))
                    .border(0.5, Color::WHITE.alpha(0.2))
                    .radius(12.0)
                    .shadow(Color::BLACK.alpha(0.4), 20.0, 6.0),
            )
            .place(Align::Start, Align::Start),
        rect().size(30.0, 30.0).menu(zstack(scroll(menu(items()))).height(80.0).clip()).place(Align::End, Align::Start),
    ))
    .fill()
}

#[test]
fn menu_inside_scroll_and_clip_still_hovers() {
    let lit = Color::WHITE.alpha(0.09);
    for (anchor, x) in [((15.0, 15.0), 60.0), ((385.0, 15.0), 340.0)] {
        let mut ui = Ui::new(M::default(), clipped_menu_view);
        ui.frame(W);
        tap(&mut ui, anchor);
        settle(&mut ui);
        assert!(quads(&ui, lit).is_empty());
        // Below the 30 pt anchor with a gap of 6: rows start 5 below the panel top, 28 high.
        ui.pointer_move((x, 36.0 + 5.0 + 28.0 + 14.0));
        ui.frame(W);
        assert_eq!(quads(&ui, lit).len(), 1, "the row under the mouse is lit (anchor {anchor:?})");
        assert!(ui.key(KeyInput::Enter));
        assert_eq!(ui.state.log, ["two"]);
        // The mouse rests while the menu grows in under it: the row it ends up on is lit.
        let mut ui = Ui::new(M::default(), clipped_menu_view);
        ui.frame(W);
        tap(&mut ui, anchor);
        ui.pointer_move((x, 68.0)); // on Two while the panel is scaled, on One once it is not
        settle(&mut ui);
        assert!(ui.key(KeyInput::Enter));
        assert_eq!(ui.state.log, ["one"], "anchor {anchor:?}");
    }
}

// ---------------------------------------------------------------- styling knobs (menus, slider, hover, shadows)

const PANEL: Color = Color::rgba(0.1, 0.2, 0.3, 1.0);

fn styled_menu(st: MenuStyle) -> Ui<M> {
    let mut ui = Ui::new(M::default(), move |_: &M| {
        zstack(
            menu(vec![
                MenuItem::section("Add"),
                MenuItem::action("One", |s: &mut M| s.log.push("one")).shortcut("⌘R").checked(true),
                MenuItem::separator(),
                MenuItem::section("More"),
                MenuItem::action("Two", |s: &mut M| s.log.push("two")),
            ])
            .menu_style(MenuStyle { background: PANEL, border: None, shadow: None, ..st.clone() })
            .place(Align::Start, Align::Start),
        )
        .fill()
    });
    ui.frame(W);
    ui
}

fn panel_h(ui: &Ui<M>) -> f32 {
    quads(ui, PANEL)[0][3]
}

#[test]
fn menu_style_sets_separator_and_section_rows() {
    let base = panel_h(&styled_menu(MenuStyle::default()));
    // padding 5 ×2, sections 26 ×2, rows 28 ×2, separator 11
    assert_eq!(base, 10.0 + 52.0 + 56.0 + 11.0);
    let st = MenuStyle { separator_height: 9.0, section_height: Some(23.0), section_gap: 10.0, ..Default::default() };
    // The gap goes above the second section only.
    assert_eq!(panel_h(&styled_menu(st)), 10.0 + 46.0 + 10.0 + 56.0 + 9.0);
}

#[test]
fn menu_style_sets_section_and_shortcut_text() {
    let (sec, key) = (Color::hex(0x969696), Color::hex(0x7B7B7B));
    let ui = styled_menu(MenuStyle::default());
    let t = texts(&ui);
    // section, check, label, shortcut, section, label
    let (h_label, h_key, h_sec) = (t[2].size.1, t[3].size.1, t[0].size.1);
    let st = MenuStyle { shortcut_size: Some(10.0), shortcut_color: Some(key), section_size: Some(13.0), section_weight: Weight::Regular, section_color: Some(sec), ..Default::default() };
    let ui = styled_menu(st);
    let t = texts(&ui);
    assert_eq!(t[0].color, sec);
    assert!(t[0].size.1 > h_sec, "section text at 13 pt is taller than at 11.5");
    assert_eq!(t[3].color, key);
    assert!(t[3].size.1 < h_key && h_key == h_label, "shortcut at 10 pt");
}

#[test]
fn menu_style_sets_icon_gap_and_highlight_inset() {
    let x = |ui: &Ui<M>| texts(ui)[2].pos.0;
    let x0 = x(&styled_menu(MenuStyle::default()));
    let mut ui = styled_menu(MenuStyle { icon_gap: 15.0, highlight_inset: 2.0, ..Default::default() });
    assert_eq!(x(&ui) - x0, 7.0, "the label moves with the gap");
    // Row One: under the 26 pt section, 5 pt padding: y 31..59.
    ui.pointer_move((60.0, 45.0));
    ui.frame(W);
    // The separator has the same color; the highlight is the tall one.
    let lit: Vec<_> = quads(&ui, Color::WHITE.alpha(0.09)).into_iter().filter(|r| r[3] > 2.0).collect();
    assert_eq!(lit.len(), 1);
    assert_eq!((lit[0][1], lit[0][3]), (33.0, 24.0), "28 pt row, 2 pt inset each side");
}

#[test]
fn placement_margin_and_offset() {
    let at = |p: Placement| {
        let mut ui = Ui::new(M { pop: true, ..M::default() }, move |s: &M| {
            zstack(
                rect()
                    .size(20.0, 20.0)
                    .popover(s.pop, p, |s: &mut M| s.pop = false, rect().size(100.0, 50.0).background(PANEL))
                    .place(Align::End, Align::Start),
            )
            .fill()
        });
        ui.frame(W);
        settle(&mut ui);
        let r = quads(&ui, PANEL)[0];
        (r[0], r[1])
    };
    // Right edges aligned would cross the window edge: slid back in.
    assert_eq!(at(Placement::bottom().end().gap(6.0)), (292.0, 26.0), "8 pt from the edge by default");
    assert_eq!(at(Placement::bottom().end().margin(6.0)), (294.0, 26.0));
    assert_eq!(at(Placement::bottom().end().margin(0.0).offset(-4.0, 2.0)), (296.0, 28.0));
}

fn tip_view_at(_: &M) -> View<M> {
    zstack(rect().size(30.0, 30.0).tooltip("Back").tooltip_at(Placement::bottom().gap(2.0)).place(Align::Start, Align::Start)).fill()
}

#[test]
fn tooltip_at_moves_the_tooltip() {
    let mut ui = Ui::new(M::default(), tip_view_at);
    ui.frame(W);
    ui.pointer_move((15.0, 15.0));
    ui.frame(W);
    std::thread::sleep(std::time::Duration::from_millis(520));
    ui.frame(W);
    settle(&mut ui);
    let t = texts(&ui);
    let tip = t.last().expect("tooltip shown");
    // The bubble starts 2 pt under the 30 pt anchor (6 by default).
    assert_eq!(tip.pos.1, 37.0);
}

#[test]
fn slider_style_sets_track_fill_and_knob() {
    let (track, fill, knob) = (Color::hex(0x434242), Color::hex(0x4D81EF), Color::hex(0xEEEEEE));
    let st = SliderStyle { track_height: 24.0, track, fill, knob, knob_size: 20.0, knob_shadow: None, track_border: Some((1.0, Color::hex(0x535252))), ..Default::default() };
    let mut ui = Ui::new(0.5f32, move |v: &f32| zstack(slider(*v, |s: &mut f32, v| *s = v).slider_style(st).width(220.0).place(Align::Start, Align::Start)).fill());
    ui.frame(W);
    let t = quads(&ui, track);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0], [0.0, 0.0, 220.0, 24.0], "the slider is as high as its tallest part");
    let f = quads(&ui, fill);
    assert_eq!(f[0][2], 200.0 * 0.5 + 10.0, "filled to the knob's center");
    let k = quads(&ui, knob);
    assert_eq!(k[0][2], 20.0);
    // Value range: knob centers from 10 to 210.
    ui.pointer_down((60.0, 12.0), false);
    ui.pointer_up((60.0, 12.0));
    assert_eq!(ui.state, 0.25);
    ui.pointer_down((10.0,12.0),false);
    ui.pointer_up((10.0,12.0));
    ui.frame(W);
    assert_eq!(ui.state,0.0);
    assert!(quads(&ui,fill).is_empty(),"minimum must not leave a colored cap beside the knob");
    assert_eq!(quads(&ui,track).len(),1);
    assert_eq!(quads(&ui,knob).len(),1);
}

#[test]
fn hover_color_recolors_the_text() {
    let gray = Color::hex(0x999999);
    let mut ui = Ui::new(M::default(), move |_: &M| {
        zstack(zstack(text("…")).size(40.0, 40.0).color(gray).hover_color(Color::WHITE).place(Align::Start, Align::Start)).fill()
    });
    ui.frame(W);
    assert_eq!(texts(&ui)[0].color, gray);
    ui.pointer_move((20.0, 20.0));
    settle(&mut ui);
    assert_eq!(texts(&ui)[0].color, Color::WHITE);
    ui.pointer_move((200.0, 200.0));
    settle(&mut ui);
    assert_eq!(texts(&ui)[0].color, gray);
}

fn shadow_quads(ui: &Ui<impl Sized>) -> Vec<ruru::Quad> {
    ui.list.cmds.iter().filter_map(|c| if let ruru::Cmd::Quad(q) = c { Some(*q) } else { None }).collect()
}

#[test]
fn two_shadows_and_an_inset_rim() {
    let key = Shadow::new(Color::BLACK.alpha(0.3), 6.0, 2.0);
    let ambient = Shadow::new(Color::BLACK.alpha(0.2), 40.0, 16.0).offset(4.0, 16.0).spread(-4.0);
    let rim = Shadow::new(Color::WHITE.alpha(0.12), 0.0, 0.0).spread(0.5).inset();
    let mut ui = Ui::new(0, move |_: &i32| {
        zstack(
            zstack(rect().fill().background(Color::RED))
                .size(100.0, 100.0)
                .background(PANEL)
                .radius(10.0)
                .shadow_with(key)
                .shadow_with(ambient)
                .shadow_with(rim)
                .place(Align::Center, Align::Center),
        )
        .fill()
    });
    ui.frame(W);
    let q = shadow_quads(&ui);
    let kinds: Vec<f32> = q.iter().map(|q| q.p0[3]).collect();
    // key, ambient, panel, child, rim: the rim lies over the child.
    assert_eq!(kinds, [1.0, 1.0, 0.0, 0.0, 1.0]);
    assert_eq!(q[0].rect, [150.0, 152.0, 100.0, 100.0]);
    assert_eq!(q[1].rect, [158.0, 170.0, 92.0, 92.0], "moved by (4, 16), shrunk by 4");
    assert_eq!(q[1].p0[0], 6.0, "radius shrinks with the spread");
    assert_eq!(q[4].rect, [150.0, 150.0, 100.0, 100.0]);
    assert_eq!((q[4].border[0], q[4].c1[3]), (1.0, 0.5), "inset flag and spread");
    // The old modifier is the first layer.
    let ui = Ui::new(0, |_: &i32| zstack(rect().size(10.0, 10.0).background(PANEL).shadow(Color::BLACK, 8.0, 3.0)).fill());
    let mut ui = ui;
    ui.frame(W);
    let q = shadow_quads(&ui);
    assert_eq!(q.len(), 2);
    assert_eq!((q[0].p0[3], q[0].c1), (1.0, [0.0, 3.0, 0.0, 0.0]));
}

// ---- theme (docs/decisions/005-theme.md)

fn near(a: [f32; 4], c: Color) -> bool {
    (a[0] - c.r).abs() < 0.002 && (a[1] - c.g).abs() < 0.002 && (a[2] - c.b).abs() < 0.002 && (a[3] - c.a).abs() < 0.002
}

/// Fills (`c0`) and borders of the frame's quads.
fn fills(ui: &Ui<impl Sized>) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    ui.list.cmds.iter().filter_map(|c| if let ruru::Cmd::Quad(q) = c { Some((q.rect, q.c0, q.border)) } else { None }).collect()
}

/// Frames until the springs rest (a focus ring fading in).
fn rest(ui: &mut Ui<impl Sized + 'static>, size: (f32, f32)) {
    for _ in 0..200 {
        ui.frame(size);
        if !ui.animating {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

const PINK_ACCENT: Color = Color::hex(0xE0338A);

struct Themed {
    on: bool,
    v: f32,
    draft: String,
    dark: bool,
}

fn themed_view(s: &Themed) -> View<Themed> {
    vstack((
        button("Go", |_: &mut Themed| {}),
        toggle(s.on, |s: &mut Themed, on| s.on = on),
        slider(s.v, |s: &mut Themed, v| s.v = v).width(200.0),
        text_field(s.draft.as_str(), "Name", |s: &mut Themed, v| s.draft = v).width(300.0),
    ))
    .spacing(10.0)
    .align(Align::Start)
    .place(Align::Start, Align::Start)
}

fn themed() -> Ui<Themed> {
    Ui::new(Themed { on: true, v: 0.5, draft: "hello".into(), dark: true }, |s: &Themed| zstack(themed_view(s)).fill())
}

#[test]
fn accent_reaches_button_toggle_slider_focus_caret_and_selection() {
    let mut ui = themed();
    ui.set_theme(Theme::dark().accent(PINK_ACCENT));
    ui.frame((400.0, 400.0));
    let q = fills(&ui);
    let accent: Vec<_> = q.iter().filter(|(_, c, _)| near(*c, PINK_ACCENT)).map(|(r, ..)| *r).collect();
    // Button background (top-left, first), toggle track (on) and the filled part of the slider.
    assert!(accent.len() >= 3, "button, toggle and slider take the accent: {accent:?}");
    assert!(accent.iter().any(|r| r[1] < 1.0 && r[3] > 40.0), "the button at the top");
    assert!(accent.iter().any(|r| (r[2] - 51.0).abs() < 0.5 && (r[3] - 31.0).abs() < 0.5), "the toggle track");
    // Focus the field: ring, caret and (after select-all) the selection follow the accent.
    let field = q.iter().filter(|(r, ..)| (r[2] - 300.0).abs() < 0.5).map(|(r, ..)| *r).last().unwrap();
    ui.pointer_down((field[0] + 50.0, field[1] + field[3] * 0.5), false);
    ui.pointer_up((field[0] + 50.0, field[1] + field[3] * 0.5));
    rest(&mut ui, (400.0, 400.0));
    let q = fills(&ui);
    assert!(q.iter().any(|(_, _, b)| near(*b, PINK_ACCENT.alpha(0.55))), "focus ring in the accent");
    assert!(q.iter().any(|(r, c, _)| near(*c, PINK_ACCENT) && (r[2] - 2.0).abs() < 0.01), "caret in the accent");
    ui.key(KeyInput::SelectAll);
    ui.frame((400.0, 400.0));
    assert!(fills(&ui).iter().any(|(_, c, _)| near(*c, PINK_ACCENT.alpha(0.3))), "selection in the accent");
}

#[test]
fn modifiers_on_a_view_beat_the_theme() {
    let mut t = Theme::dark().accent(PINK_ACCENT);
    t.text.body.size = 30.0;
    let mut ui = Ui::new((), |_: &()| {
        vstack((
            button("A", |_: &mut ()| {}).background(Color::RED).radius(3.0),
            button("B", |_: &mut ()| {}).padding_h(0.0),
            text("big"),
            text("small").font_size(10.0),
        ))
        .align(Align::Start)
        .place(Align::Start, Align::Start)
    });
    ui.set_theme(t.clone());
    ui.frame((400.0, 400.0));
    let q = fills(&ui);
    let a = q.iter().find(|(_, c, _)| near(*c, Color::RED)).expect("explicit background");
    assert_eq!(ui.list.cmds.iter().find_map(|c| if let ruru::Cmd::Quad(q) = c { Some(q.p0[0]) } else { None }), Some(3.0));
    let b = q.iter().find(|(_, c, _)| near(*c, PINK_ACCENT)).expect("B keeps the theme's background").0;
    // `.padding_h()` keeps the theme's vertical padding; the label is the headline style.
    assert!(b[3] > 2.0 * t.control.button_padding.1 + 17.0 && b[2] < a.0[2], "B: {b:?}, A: {:?}", a.0);
    let tx = texts(&ui);
    let (big, small) = (tx[2].size.1, tx[3].size.1);
    assert!(big > 35.0 && small < 15.0, "body size from the theme, explicit size wins: {big} {small}");
}

#[test]
fn switching_the_theme_repaints_the_same_ui() {
    let mut ui = themed();
    ui.theme_with(|s: &Themed| if s.dark { Theme::dark() } else { Theme::light() });
    ui.frame((400.0, 400.0));
    assert_eq!(ui.clear_color(), Color::BLACK);
    assert!(fills(&ui).iter().any(|(_, c, _)| near(*c, Theme::dark().color.accent)));
    assert_eq!(texts(&ui)[0].color, Color::WHITE, "label on the accent");
    ui.state.dark = false;
    ui.frame((400.0, 400.0));
    let light = Theme::light();
    assert_eq!(ui.clear_color(), Color::WHITE);
    assert!(fills(&ui).iter().any(|(_, c, _)| near(*c, light.color.accent)), "button in the light accent");
    assert!(fills(&ui).iter().any(|(_, c, _)| near(*c, light.color.fill)), "field in the light fill");
    assert_eq!(texts(&ui).last().unwrap().color, Color::BLACK, "field text in the light text color");
    // An explicit background wins over the theme's.
    ui.background = Some(Color::hex(0x123456));
    assert_eq!(ui.clear_color(), Color::hex(0x123456));
}

#[test]
fn view_reads_the_theme_from_env() {
    let mut ui = Ui::new((), |_: &(), env: &Env| text("x").style(env.theme.text.title).color(env.theme.color.danger));
    ui.set_theme(Theme::light());
    ui.frame((200.0, 200.0));
    let t = texts(&ui);
    assert_eq!(t[0].color, Theme::light().color.danger);
    assert!(t[0].size.1 > 25.0, "title size: {}", t[0].size.1);
}

// ---- gaps found in GromAgent: menu icons, padding on leaves, menu border, sections, max_height

fn image_quads(ui: &Ui<impl Sized>) -> Vec<ruru::Quad> {
    ui.list.cmds.iter().filter_map(|c| match c {
        ruru::Cmd::Quad(q) if q.p0[3] == 3.0 => Some(*q),
        _ => None,
    }).collect()
}

fn plain_menu(bg: Color) -> MenuStyle {
    MenuStyle { background: bg, glass: None, border: None, shadow: None, ambient: None, rim: None, ..MenuStyle::from(&Theme::dark()) }
}

#[test]
fn menu_icon_is_a_template_and_menu_image_keeps_its_colors() {
    let pic = Image::from_rgba(2, 2, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]);
    let (a, b) = (pic.clone(), pic);
    let mut ui = Ui::new((), move |_: &()| {
        let items = vec![MenuItem::action("Mono", |_: &mut ()| {}).icon(&a), MenuItem::action("Color", |_: &mut ()| {}).image(&b)];
        zstack(menu(items)).place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    let q = image_quads(&ui);
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].c1[1], 1.0, "icon: tinted with the row color");
    assert_eq!(q[1].c1[1], 0.0, "image: painted as is");
    assert!(q[1].rect[1] > q[0].rect[1]);
}

#[test]
fn padding_moves_the_content_of_leaf_views() {
    let red = Color::hex(0xFF0000);
    let track = Color::hex(0x00FF00);
    let mut ui = Ui::new((), move |_: &()| {
        let slider_st = SliderStyle { track, ..SliderStyle::from(&Theme::dark()) };
        vstack((
            menu(vec![MenuItem::action("Row", |_: &mut ()| {})]).menu_style(plain_menu(red)).padding_ltrb(10.0, 6.0, 4.0, 2.0),
            slider(0.0, |_: &mut (), _| {}).slider_style(slider_st).width(200.0).padding_ltrb(10.0, 6.0, 4.0, 2.0),
            toggle(false, |_: &mut (), _| {}).padding_ltrb(10.0, 6.0, 4.0, 2.0),
        ))
        .align(Align::Start)
        .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    let panel = quads(&ui, red)[0];
    assert_eq!((panel[0], panel[1]), (10.0, 6.0), "menu panel inside the padding");
    let rows: Vec<_> = texts(&ui).iter().map(|t| t.pos).collect();
    assert!(rows[0].1 > 6.0);
    let menu_h = panel[3] + 8.0;
    let t = quads(&ui, track)[0];
    assert_eq!(t[0], 10.0, "slider track starts after the left padding");
    assert_eq!(t[2], 200.0 - 14.0, "and ends before the right one");
    let sh = SliderStyle::from(&Theme::dark()).knob_size.max(SliderStyle::from(&Theme::dark()).track_height);
    assert!((t[1] + t[3] * 0.5 - (menu_h + 6.0 + sh * 0.5)).abs() < 0.6, "track centered in the content: {t:?}");
    let k = Theme::dark().control.toggle;
    let toggle = quads(&ui, Theme::dark().color.fill_strong).into_iter().find(|q| q[2] == k.0).expect("toggle track");
    assert_eq!(toggle[0], 10.0);
    assert_eq!(toggle[3], k.1, "the track keeps its size");
}

#[test]
fn menu_border_lies_over_its_translucent_background() {
    let edge = Color::WHITE.alpha(0.15);
    for glass in [None, Some(Glass::dark())] {
        let mut ui = Ui::new((), move |_: &()| {
            let st = MenuStyle { glass, border: Some((0.5, edge)), background: Color::BLACK.alpha(0.5), ..plain_menu(Color::CLEAR) };
            zstack(menu(vec![MenuItem::action("Row", |_: &mut ()| {})]).menu_style(st)).place(Align::Start, Align::Start)
        });
        ui.frame((400.0, 400.0));
        let all = fills(&ui);
        let bg = all.iter().position(|(_, c, _)| *c == [0.0, 0.0, 0.0, 0.5]).expect("background");
        let border = all.iter().rposition(|(_, _, b)| *b == [1.0, 1.0, 1.0, 0.15]).expect("border");
        assert!(border > bg, "glass {:?}: border painted after the fill", glass.is_some());
        assert_eq!(all[bg].2, [0.0; 4], "the fill itself has no border");
    }
}

#[test]
fn menu_rows_and_sections_take_their_own_height() {
    let mut ui = Ui::new((), |_: &()| {
        let st = MenuStyle { padding: 0.0, row_height: 28.0, section_height: Some(20.0), section_gap: 4.0, ..plain_menu(Color::CLEAR) };
        let items = vec![
            MenuItem::section("A").height(12.0),
            MenuItem::action("one", |_: &mut ()| {}),
            MenuItem::section("B"),
            MenuItem::action("two", |_: &mut ()| {}).height(32.0),
        ];
        zstack(menu(items).menu_style(st).background(Color::RED)).place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    assert_eq!(quads(&ui, Color::RED)[0][3], 12.0 + 28.0 + (4.0 + 20.0) + 32.0);
}

#[test]
fn scroll_with_max_height_hugs_short_content_and_limits_long() {
    for (rows, want) in [(3usize, 3.0 * 30.0), (20, 200.0)] {
        let mut ui = Ui::new((), move |_: &()| {
            let col = vstack((0..rows).map(|_| rect().fill_width().height(30.0)).collect::<Vec<_>>());
            zstack(scroll(col).max_height(200.0).width(100.0).background(Color::RED)).place(Align::Start, Align::Start)
        });
        ui.frame((400.0, 400.0));
        assert_eq!(quads(&ui, Color::RED)[0][3], want, "{rows} rows");
    }
    // Inside a popover, which offers no size at all.
    let mut ui = Ui::new((), |_: &()| {
        let col = vstack((0..20).map(|_| rect().fill_width().height(30.0)).collect::<Vec<_>>());
        zstack(scroll(col).max_height(150.0).max_width(120.0).background(Color::RED)).place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    let q = quads(&ui, Color::RED)[0];
    assert_eq!((q[2], q[3]), (120.0, 150.0));
}

#[test]
fn text_field_line_height_and_letter_spacing_are_settable() {
    let mut ui = Ui::new((), |_: &()| {
        let f = |k: Option<f32>| {
            let t = text_field("one\ntwo\nthree", "", |_: &mut (), _| {}).multiline().max_lines(5).font_size(14.0).plain();
            match k {
                Some(k) => t.line_height(k).letter_spacing(0.05),
                None => t,
            }
        };
        // Fits one row at 280 pt unless spaced out.
        let one = |em: f32, c: Color| {
            zstack(text_field("the quick brown fox jumps over a dog", "", |_: &mut (), _| {}).multiline().max_lines(3).font_size(14.0).plain().letter_spacing(em))
                .width(280.0)
                .background(c)
        };
        vstack((zstack(f(None)).background(Color::RED), zstack(f(Some(1.625))).background(Color::GREEN), one(0.0, Color::BLUE), one(0.2, Color::GRAY)))
            .width(300.0)
            .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    assert_eq!(quads(&ui, Color::RED)[0][3], 3.0 * (14.0f32 * 1.3).round());
    assert_eq!(quads(&ui, Color::GREEN)[0][3], 3.0 * (14.0f32 * 1.625).round());
    assert_eq!(quads(&ui, Color::BLUE)[0][3], 18.0);
    assert_eq!(quads(&ui, Color::GRAY)[0][3], 36.0, "spaced out, it wraps");
}

#[test]
fn tabular_digits_keep_their_width() {
    let mut ui = Ui::new((), |_: &()| {
        vstack((
            text("111"),
            text("888"),
            text("111").tabular(),
            text("888").tabular(),
            rich_text(vec![Span::new("t "), Span::new("111").tabular()]),
            rich_text(vec![Span::new("t "), Span::new("888").tabular()]),
        ))
        .align(Align::Start)
        .place(Align::Start, Align::Start)
    });
    ui.frame((400.0, 400.0));
    let w: Vec<f32> = texts(&ui).iter().map(|t| t.size.0).collect();
    assert!(w[1] - w[0] > 2.0, "proportional ones are narrow: {w:?}");
    assert_eq!(w[2], w[3], "tabular");
    assert_eq!(w[4], w[5], "tabular run");
}

#[test]
fn theme_picks_the_font_and_falls_back_to_inter() {
    let mut ui = Ui::new((), |_: &()| zstack(text("Hamburgefonstiv")).place(Align::Start, Align::Start));
    ui.frame((400.0, 400.0));
    let inter = texts(&ui)[0].size.0;
    assert_eq!(ui.text.font_family(), "Inter");
    ui.set_theme(Theme::dark().font(Font::Named("No Such Font 42".into())));
    ui.frame((400.0, 400.0));
    assert_eq!(ui.text.font_family(), "Inter", "a missing font leaves Inter");
    assert_eq!(texts(&ui)[0].size.0, inter);
    assert_eq!(ui.theme().text.title.tracking, 0.0, "Inter's title tracking is dropped with another font");
    assert!(Theme::dark().text.title.tracking < -0.01);
    ui.set_theme(Theme::dark().font(Font::System));
    ui.frame((400.0, 400.0));
    #[cfg(target_os = "macos")]
    {
        assert_eq!(ui.text.font_family(), "System Font");
        assert!((texts(&ui)[0].size.0 - inter).abs() > 3.0, "re-shaped in SF: {} vs {inter}", texts(&ui)[0].size.0);
    }
    ui.set_theme(Theme::dark());
    ui.frame((400.0, 400.0));
    assert_eq!(ui.text.font_family(), "Inter");
    assert_eq!(texts(&ui)[0].size.0, inter);
}

// ---------------------------------------------------------------- split

struct Sp {
    w: f32,
    closed: bool,
}

const SIDE: Color = Color::hex(0x802020);
const SIDE_IN: Color = Color::hex(0x208020);
const MAIN: Color = Color::hex(0x202080);

fn split_ui(trailing: bool) -> Ui<Sp> {
    let mut ui = Ui::new(Sp { w: 300.0, closed: false }, move |s: &Sp| {
        let panel = zstack(rect().fill().background(SIDE_IN)).background(SIDE).min_width(200.0).max_width(400.0);
        let v = split(s.w, |s: &mut Sp, w| s.w = w, panel, rect().fill().background(MAIN))
            .collapsible(s.closed, 120.0, |s: &mut Sp, c| s.closed = c)
            .reset_width(260.0);
        if trailing { v.trailing() } else { v }
    });
    ui.frame((800.0, 400.0));
    ui
}

fn settle_split(ui: &mut Ui<Sp>) {
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame((800.0, 400.0));
        if !ui.animating {
            break;
        }
    }
}

#[test]
fn split_in_a_rounded_panel_keeps_its_inner_corners_square() {
    // The side panel is cut from the rounded parent clip at the divider: its outer corners
    // follow the parent's radius, the two at the divider stay square (also mid-glide).
    let mut ui = Ui::new(Sp { w: 300.0, closed: false }, |s: &Sp| {
        let panel = zstack(rect().fill().background(SIDE_IN)).background(SIDE).min_width(200.0).max_width(400.0);
        split(s.w, |s: &mut Sp, w| s.w = w, panel, rect().fill().background(MAIN))
            .collapsible(s.closed, 120.0, |s: &mut Sp, c| s.closed = c)
            .fill()
            .border(1.0, Color::WHITE)
            .radius(12.0)
            .clip()
    });
    let side = |ui: &Ui<Sp>| {
        let c = SIDE;
        ui.list.cmds.iter().find_map(|q| match q {
            ruru::Cmd::Quad(q) if q.c0 == [c.r, c.g, c.b, c.a] => Some(*q),
            _ => None,
        })
    };
    ui.frame((800.0, 400.0));
    let q = side(&ui).unwrap();
    assert!(q.p1[2] > 10.0, "rounded clip: {:?}", q.p1);
    assert_eq!(q.np[3], 6.0, "top right and bottom right square, left corners round");
    ui.state.closed = true;
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        ui.frame((800.0, 400.0));
    }
    let q = side(&ui).unwrap();
    assert!(ui.animating, "still gliding");
    assert_eq!(q.np[3], 6.0, "mid-glide: {:?}", q.clip);
    // The main area's clip is the parent's own: all four corners round.
    let m = MAIN;
    let main = ui.list.cmds.iter().find_map(|q| match q {
        ruru::Cmd::Quad(q) if q.c0 == [m.r, m.g, m.b, m.a] => Some(*q),
        _ => None,
    });
    assert_eq!(main.unwrap().np[3], 0.0);
}

#[test]
fn split_drag_resizes_one_to_one_within_min_and_max() {
    let mut ui = split_ui(false);
    assert_eq!(quads(&ui, SIDE)[0], [0.0, 0.0, 300.0, 400.0]);
    assert_eq!(quads(&ui, MAIN)[0], [301.0, 0.0, 499.0, 400.0], "the main area starts after the hairline (0.5 pt, on the grid)");
    // Grabbed 0.5 pt right of the line's left edge: the line keeps that offset to the pointer.
    ui.pointer_move((300.5, 200.0));
    ui.frame((800.0, 400.0));
    ui.pointer_down((300.5, 200.0), false);
    for x in [320.5, 351.5, 390.5] {
        ui.pointer_move((x, 200.0));
        ui.frame((800.0, 400.0));
        assert_eq!(ui.state.w, x - 0.5);
        assert_eq!(quads(&ui, SIDE)[0][2], x - 0.5, "follows 1:1, no spring");
        assert_eq!(quads(&ui, MAIN)[0][0], x + 0.5);
        assert_eq!(ui.cursor, ruru::Cursor::ResizeH);
    }
    ui.pointer_move((700.0, 200.0));
    assert_eq!(ui.state.w, 400.0, "max");
    ui.pointer_move((150.0, 200.0));
    assert_eq!(ui.state.w, 200.0, "min (above the collapse threshold)");
    assert!(!ui.state.closed);
    ui.pointer_up((150.0, 200.0));
    ui.frame((800.0, 400.0));
    assert_eq!(quads(&ui, SIDE)[0][2], 200.0, "no glide after a drag");
}

#[test]
fn split_collapses_past_the_threshold_and_opens_from_the_edge() {
    let mut ui = split_ui(false);
    ui.pointer_down((300.5, 200.0), false);
    ui.pointer_move((100.0, 200.0));
    assert!(ui.state.closed, "dragged under 120: collapses");
    assert_eq!(ui.state.w, 300.0, "and keeps its width for next time");
    // It glides shut; the content inside keeps its width (cut, not squeezed).
    let mut seen = Vec::new();
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ui.frame((800.0, 400.0));
        if let Some(q) = quads(&ui, SIDE_IN).first() {
            assert!(q[2] >= 300.0 - 1e-3, "content keeps its width: {q:?}");
        }
        seen.push(quads(&ui, MAIN)[0][0]);
        if !ui.animating {
            break;
        }
    }
    assert!(seen.iter().any(|x| *x > 20.0 && *x < 280.0), "glides: {seen:?}");
    assert_eq!(quads(&ui, MAIN)[0], [0.0, 0.0, 800.0, 400.0]);
    ui.pointer_up((100.0, 200.0));
    // From the edge, dragged out past the threshold: it opens.
    ui.pointer_move((1.0, 200.0));
    ui.frame((800.0, 400.0));
    assert_eq!(ui.cursor, ruru::Cursor::ResizeH, "the edge of a collapsed panel can be grabbed");
    ui.pointer_down((1.0, 200.0), false);
    ui.pointer_move((60.0, 200.0));
    assert!(ui.state.closed, "not yet past the threshold");
    ui.pointer_move((250.0, 200.0));
    assert!(!ui.state.closed);
    settle_split(&mut ui);
    ui.pointer_move((261.0, 200.0));
    ui.frame((800.0, 400.0));
    assert_eq!(ui.state.w, 260.0, "then follows the pointer again");
    assert_eq!(quads(&ui, SIDE)[0][2], 260.0);
    ui.pointer_up((261.0, 200.0));
}

#[test]
fn split_divider_lights_up_on_hover_and_double_click_resets() {
    let mut ui = split_ui(false);
    let theme = ruru::Theme::default();
    let line = |ui: &Ui<Sp>| {
        ui.list.cmds.iter().find_map(|c| match c {
            ruru::Cmd::Quad(q) if q.rect[0] == 300.0 && q.rect[2] == 1.0 => Some(q.c0),
            _ => None,
        })
    };
    let d = theme.color.divider;
    assert_eq!(line(&ui), Some([d.r, d.g, d.b, d.a]));
    ui.pointer_move((297.0, 200.0)); // within the 8 pt grab strip
    settle_split(&mut ui);
    assert_eq!(ui.cursor, ruru::Cursor::ResizeH);
    let a = theme.color.accent;
    let lit = line(&ui).unwrap();
    assert!(lit.iter().zip([a.r, a.g, a.b, a.a]).all(|(x, y)| (x - y).abs() < 1e-4), "lit up in the accent color: {lit:?}");
    ui.pointer_move((290.0, 200.0));
    settle_split(&mut ui);
    assert_eq!(ui.cursor, ruru::Cursor::Default);
    assert_eq!(line(&ui), Some([d.r, d.g, d.b, d.a]));
    // Double click: back to the reset width, gliding there.
    click(&mut ui, (300.5, 200.0));
    click(&mut ui, (300.5, 200.0));
    assert_eq!(ui.state.w, 260.0);
    ui.frame((800.0, 400.0));
    assert!(ui.animating, "glides to it");
    settle_split(&mut ui);
    assert_eq!(quads(&ui, SIDE)[0][2], 260.0);
}

#[test]
fn split_toggled_by_state_glides_and_trailing_panel_sits_right() {
    let mut ui = split_ui(true);
    assert_eq!(quads(&ui, SIDE)[0], [500.0, 0.0, 300.0, 400.0]);
    assert_eq!(quads(&ui, MAIN)[0], [0.0, 0.0, 500.0, 400.0], "499.5 wide, on the pixel grid");
    // Dragging the line left widens a panel on the right.
    ui.pointer_down((499.5, 200.0), false);
    ui.pointer_move((449.5, 200.0));
    assert_eq!(ui.state.w, 350.0);
    ui.pointer_up((449.5, 200.0));
    // A button closing it: a spring, not a jump.
    ui.state.closed = true;
    ui.frame((800.0, 400.0));
    std::thread::sleep(std::time::Duration::from_millis(30));
    ui.frame((800.0, 400.0));
    let w = quads(&ui, MAIN)[0][2];
    assert!(w > 449.0 && w < 800.0, "on its way: {w}");
    settle_split(&mut ui);
    assert_eq!(quads(&ui, MAIN)[0], [0.0, 0.0, 800.0, 400.0]);
}

/// Frames of a menu growing in at 120 Hz until it rests: per frame, each text's
/// `(x, y, scale)` (device pixels) and the icon's rect (device pixels).
fn menu_text_frames(scale: f32) -> Vec<(Vec<(f32, f32, f32)>, [f32; 4])> {
    let img = Image::from_rgba(2, 2, vec![255; 16]);
    let mut ui = Ui::new(false, move |s: &bool| {
        let items = vec![
            MenuItem::action("Paste", |_: &mut bool| {}).icon(&img),
            MenuItem::action("Bold", |_: &mut bool| {}).checked(true),
        ];
        rect()
            .size(31.3, 17.7)
            .popover(*s, Placement::bottom().gap(5.3), |s: &mut bool| *s = false, menu(items))
            .padding_ltrb(37.3, 21.7, 0.0, 0.0)
            .place(Align::Start, Align::Start)
    });
    ui.scale = scale;
    ui.frame((401.0, 303.0));
    ui.state = true;
    let mut seen = Vec::new();
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        ui.frame((401.0, 303.0));
        let icon = ui.list.cmds.iter().find_map(|c| match c {
            ruru::Cmd::Quad(q) if q.p0[3] == 3.0 => Some(q.rect.map(|v| v * scale)),
            _ => None,
        });
        let t: Vec<_> = texts(&ui).iter().map(|t| (t.pos.0 * scale, t.pos.1 * scale, t.scale)).collect();
        if let Some(icon) = icon {
            seen.push((t, icon));
        }
        if !ui.animating {
            break;
        }
    }
    seen
}

/// Text in a growing menu ends where it rests without a last-frame jump: glyphon puts glyph
/// rows on whole pixels (it truncates `top`), so the painter must hand it whole-pixel rows
/// or text approaching from above its resting row sits a pixel short until the very end.
#[test]
fn menu_text_and_icon_settle_without_a_jump() {
    for scale in [1.0, 2.0] {
        let seen = menu_text_frames(scale);
        let n = seen.len();
        assert!(n > 8, "animated, {n} frames");
        let (rest, rest_icon) = &seen[n - 1];
        assert_eq!(rest.len(), 3, "icon row label, check mark, label");
        for (f, (t, icon)) in seen.iter().enumerate() {
            for (i, &(x, y, k)) in t.iter().enumerate() {
                // What glyphon draws: rows on truncated pixels, glyphs at the quantized scale.
                let row = (y + 1e-3).trunc();
                let (rx, ry, _) = rest[i];
                // Measured from the icon, which moves smoothly: never more than half a pixel
                // from where the text sits next to it at rest.
                let (dx, dy) = (x - icon[0] - (rx - rest_icon[0]), row - icon[1] - (ry - rest_icon[1]));
                if f + 7 >= n {
                    assert!(dy.abs() <= 0.5 + 1e-3 && dx.abs() < 0.5, "scale {scale}: text {i} off the icon by ({dx}, {dy}) at frame {f} of {n}");
                    assert_eq!(k, 1.0, "scale {scale}: text {i} rasterized at {k} near the end");
                }
            }
        }
        // The last animated frame is the resting one, give or take a fraction of a pixel.
        for (a, b) in seen[n - 2].0.iter().zip(rest) {
            assert!((a.0 - b.0).abs() < 0.5 && a.1 == b.1, "scale {scale}: last step {a:?} -> {b:?}");
        }
        for w in seen[n - 7..].windows(2) {
            for (a, b) in w[0].0.iter().zip(&w[1].0) {
                assert!((b.0 - a.0).abs() < 0.5 && (b.1 - a.1).abs() < 0.5, "scale {scale}: step {a:?} -> {b:?}");
            }
        }
        // The painter hands over whole-pixel rows, so the list says where glyphs land.
        for (f, (t, _)) in seen.iter().enumerate() {
            for (i, &(_, y, _)) in t.iter().enumerate() {
                assert!((y - y.round()).abs() < 1e-3, "scale {scale}: text {i} row off the pixel grid at frame {f}: {y}");
            }
        }
    }
}

#[test]
fn streaming_markdown_keeps_actions_at_bottom_in_the_same_frame() {
    use ruru::Cmd;
    let mut ui = Ui::new(String::from("Начало ответа.\n\n"), |s: &String| {
        let source = s.clone();
        scroll(list(2, 120.0, move |i| {
            if i == 0 { return text(PARA.repeat(15)); }
            vstack((ruru::markdown(&source), rect().size(48.0, 16.0).background(Color::RED)))
                .spacing(16.0).align(Align::Start).fill_width()
        }).padding_v(24.0)).stick_to_bottom()
    });
    let y = |ui: &Ui<String>| ui.list.cmds.iter().find_map(|c| match c {
        Cmd::Quad(q) if q.c0 == [Color::RED.r, Color::RED.g, Color::RED.b, Color::RED.a] => Some(q.rect[1]), _ => None,
    }).expect("action row remains visible");
    for _ in 0..6 { ui.frame((340.0, 300.0)); }
    let baseline = y(&ui);
    let markdown = "- **Сначала замерить**: переносы длинной строки не должны сдвигать иконки.\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n| One | Two |\n| --- | --- |\n| cell | content |\n\nПоследний абзац.";
    for ch in markdown.chars() {
        ui.state.push(ch);
        ui.frame((340.0, 300.0));
        assert!((y(&ui) - baseline).abs() <= 0.5, "actions jump on {ch:?}: {} vs {baseline}", y(&ui));
    }
    // Reading above the end must not start following the stream again.
    ui.wheel((160.0, 140.0), 100.0, ruru::WheelPhase::Line);
    ui.frame((340.0, 300.0));
    let before = ui.list.cmds.iter().find_map(|c| match c { Cmd::Text(t) => Some(t.pos.1), _ => None }).unwrap();
    ui.state.push_str("\n\nЕщё один абзац.");
    ui.frame((340.0, 300.0));
    let after = ui.list.cmds.iter().find_map(|c| match c { Cmd::Text(t) => Some(t.pos.1), _ => None }).unwrap();
    assert!((before - after).abs() < 1.0, "stream must not move a reader away from older text");
}

#[test]
fn window_resize_updates_split_and_wrapped_text_without_settling_frames() {
    const PANEL: Color = Color::hex(0x18406A);
    const MAIN: Color = Color::hex(0x54321A);
    for trailing in [false, true] {
        let mut ui = Ui::new((), move |_: &(), env: &Env| {
            let panel = rect().background(PANEL).min_width(100.0).max_width((env.width - 220.0).max(100.0));
            let body = text(PARA.repeat(4)).fill_width().animate();
            let main = vstack((body, rect().size(16.0, 16.0).background(Color::RED))).align(Align::Start).background(MAIN);
            let view = split(320.0, |_: &mut (), _| {}, panel, main);
            if trailing { view.trailing() } else { view }
        });
        ui.scale = 2.0;
        ui.frame((800.0, 650.0));
        for w in [700.0, 580.0, 500.0, 450.0, 510.0, 610.0, 800.0] {
            ui.frame((w, 650.0));
            let panel = quads(&ui, PANEL)[0];
            let main = quads(&ui, MAIN)[0];
            let expected = 320.0f32.min(w - 220.0);
            assert_eq!(panel[2], expected, "sidebar must follow the new constraint in this frame");
            let line = ui.theme().control.border;
            let gap = if trailing { panel[0] - main[2] } else { main[0] - panel[2] };
            assert!((gap - line).abs() < 0.51, "no unpainted gap beside the splitter: {gap}");
            let before: Vec<_> = texts(&ui).iter().map(|t| (t.pos, t.size)).collect();
            let marker = quads(&ui, Color::RED);
            assert!(!ui.animating, "resizing must not schedule settling frames at width {w}");
            ui.frame((w, 650.0));
            assert_eq!(texts(&ui).iter().map(|t| (t.pos, t.size)).collect::<Vec<_>>(), before);
            assert_eq!(quads(&ui, Color::RED), marker, "actions must not drift after the resize");
        }
    }
}

#[test]
fn live_resize_snaps_pending_animation_and_state_changes_resume_afterwards() {
    let mut ui = Ui::new(100.0, |s: &f32| rect().size(*s, 50.0).background(Color::RED).animate().place(Align::Start, Align::Start));
    ui.frame((400.0, 300.0));
    ui.state = 200.0;
    ui.frame((400.0, 300.0));
    assert!(ui.animating);
    ui.resizing = true;
    ui.frame((400.0, 300.0));
    assert_eq!(quads(&ui, Color::RED)[0][2], 200.0);
    assert!(!ui.animating);
    ui.state = 250.0;
    ui.frame((400.0, 300.0));
    assert_eq!(quads(&ui, Color::RED)[0][2], 250.0, "even between native resize events");
    ui.resizing = false;
    ui.state = 300.0;
    ui.frame((400.0, 300.0));
    assert_eq!(quads(&ui, Color::RED)[0][2], 250.0);
    assert!(ui.animating, "ordinary UI transitions are still animated");
}

#[test]
fn press_scale_one_keeps_geometry_but_retains_feedback_and_click() {
    let mut ui = Ui::new(0, |_: &i32| button("Stationary", |s: &mut i32| *s += 1)
        .size(200.0, 60.0).press_scale(1.0));
    ui.frame((200.0, 60.0));
    let label = |ui: &Ui<i32>| ui.list.cmds.iter().find_map(|c| match c {
        ruru::Cmd::Text(t) => Some((t.pos, t.size, t.scale, t.color.a)), _ => None,
    }).unwrap();
    let before = label(&ui);
    ui.pointer_down((100.0, 30.0), false);
    for _ in 0..15 {
        std::thread::sleep(std::time::Duration::from_millis(8));
        ui.frame((200.0, 60.0));
        let held = label(&ui);
        assert_eq!((held.0, held.1, held.2), (before.0, before.1, before.2));
    }
    assert!(label(&ui).3 < before.3);
    ui.pointer_up((100.0, 30.0));
    assert_eq!(ui.state, 1);
}

#[test]
fn reveal_zooms_fades_bounces_and_disables_hidden_input_without_moving_layout() {
    let mut ui = Ui::new((false, 0), |s: &(bool, i32)| hstack((
        rect().size(40.0, 40.0).background(Color::hex(0x12AB34))
            .on_tap(|s: &mut (bool, i32)| s.1 += 1).reveal(s.0),
        text("Neighbor"),
    )));
    let size = (200.0, 80.0);
    ui.frame(size);
    let neighbor = |ui: &Ui<(bool, i32)>| ui.list.cmds.iter().find_map(|c| match c {
        ruru::Cmd::Text(t) => Some(t.pos), _ => None,
    }).unwrap();
    let original = neighbor(&ui);
    let mut at = (35.0, 40.0);
    ui.pointer_down(at, false);
    ui.pointer_up(at);
    assert_eq!(ui.state.1, 0);
    let mut zoom = Vec::new();
    for shown in [true, false] {
        ui.state.0 = shown;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(8));
            ui.frame(size);
            assert_eq!(neighbor(&ui), original, "visibility must not move adjacent content");
            if let Some(q) = ui.list.cmds.iter().find_map(|c| match c {
                ruru::Cmd::Quad(q) if q.c0 == [18.0 / 255.0, 171.0 / 255.0, 52.0 / 255.0, 1.0] => Some(q), _ => None,
            }) {
                zoom.push((shown, q.rect[2], q.p1[3]));
                if shown { at = (q.rect[0] + q.rect[2] / 2.0, q.rect[1] + q.rect[3] / 2.0); }
            }
            if !shown {
                ui.pointer_down(at, false);
                ui.pointer_up(at);
                assert_eq!(ui.state.1, 1, "disappearing views must not receive clicks");
            }
            if !ui.animating { break; }
        }
        assert!(!ui.animating, "visibility animation settles");
        if shown {
            ui.pointer_down(at, false);
            ui.pointer_up(at);
            assert_eq!(ui.state.1, 1, "visible view receives clicks");
        }
    }
    assert!(zoom.iter().any(|(shown, width, alpha)| *shown && *width < 39.0 && *alpha > 0.0 && *alpha < 1.0));
    assert!(zoom.iter().any(|(shown, width, _)| *shown && *width > 40.0), "entry has a small bounce");
    assert!(zoom.iter().any(|(shown, width, alpha)| !*shown && *width < 39.0 && *alpha > 0.0 && *alpha < 1.0));
    assert!(!ui.list.cmds.iter().any(|c| matches!(c, ruru::Cmd::Quad(q) if q.c0 == [18.0 / 255.0, 171.0 / 255.0, 52.0 / 255.0, 1.0])));
}

#[test]
fn modal_blocks_background_hover_cursor_and_tooltips() {
    let mut ui = Ui::new(false, |open: &bool| {
        zstack((
            rect().fill().hover(Color::RED).cursor(Cursor::Pointer).tooltip("Background tooltip"),
            modal(*open, |open: &mut bool| *open = false,
                rect().size(120.0, 80.0).background(Color::BLUE).hover(Color::GREEN).cursor(Cursor::Text)),
        )).fill()
    });
    let size = (400.0, 300.0);
    ui.frame(size);
    ui.pointer_move((10.0, 10.0));
    assert_eq!(ui.cursor, Cursor::Pointer);
    ui.state = true;
    ui.frame(size);
    ui.pointer_move((20.0, 10.0));
    assert_eq!(ui.cursor, Cursor::Default, "the scrim blocks the background cursor");
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        ui.frame(size);
    }
    assert!(!ui.list.cmds.iter().any(|c| matches!(c, ruru::Cmd::Text(_))), "background tooltip stays hidden");
    assert!(!ui.list.cmds.iter().any(|c| matches!(c, ruru::Cmd::Quad(q) if q.c0[0] == 1.0 && q.c0[1] == 0.0 && q.c0[3] > 0.01)),
        "background hover is not painted through the scrim");
    ui.pointer_move((200.0, 150.0));
    assert_eq!(ui.cursor, Cursor::Text, "dialog controls still hover");
    ui.pointer_move((10.0, 10.0));
    ui.pointer_down((10.0, 10.0), false);
    ui.pointer_up((10.0, 10.0));
    assert!(!ui.state);
    for _ in 0..90 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        ui.frame(size);
    }
    ui.pointer_move((20.0, 10.0));
    assert_eq!(ui.cursor, Cursor::Pointer, "background hover returns after dismissal");
}

#[test]
fn modal_text_geometry_stays_fixed_while_fading_in_and_out() {
    let mut ui = Ui::new(false, |open: &bool| {
        modal(*open, |open: &mut bool| *open = false,
            vstack((text_field("Search query", "Search", |_: &mut bool, _| {}),
                text("A long search result whose glyphs must not drift horizontally")))
                .width(600.0).padding(20.0))
    });
    let size = (800.0, 500.0);
    for scale in [1.0, 1.5, 2.0] {
        ui.scale = scale;
        ui.frame(size);
        let mut reference = None;
        let mut fading_frames = 0;
        for open in [true, false] {
            ui.state = open;
            for _ in 0..90 {
                std::thread::sleep(std::time::Duration::from_millis(16));
                ui.frame(size);
                let text: Vec<_> = ui.list.cmds.iter().filter_map(|cmd| match cmd {
                    ruru::Cmd::Text(t) => Some((t.pos, t.size, t.scale)), _ => None,
                }).collect();
                if !text.is_empty() {
                    if let Some(reference) = &reference { assert_eq!(&text, reference); }
                    else { reference = Some(text); }
                    if ui.animating { fading_frames += 1; }
                }
                if !ui.animating { break; }
            }
            assert!(!ui.animating, "modal animation settles");
        }
        assert!(fading_frames > 5, "verify intermediate frames as well as the final frame");
    }
}

#[test]
fn modal_rise_keeps_text_scale_and_x_fixed_above_a_blurred_backdrop() {
    let mut ui = Ui::new(false, |open: &bool| {
        zstack((
            text("Background content"),
            modal(*open, |open: &mut bool| *open = false,
                text("Search results").padding(20.0).width(300.0).height(120.0))
                .modal_style(ModalStyle { rise: 18.0, backdrop: Some(Glass { frost: 12.0, ..Glass::dark() }) }),
        )).fill()
    });
    let size = (600.0, 400.0);
    ui.scale = 2.0;
    ui.frame(size);
    ui.state = true;
    let mut geometry = Vec::new();
    for _ in 0..90 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        ui.frame(size);
        if let Some((glass_index, glass)) = ui.list.cmds.iter().enumerate().find_map(|(i, c)| match c {
            ruru::Cmd::Quad(q) if q.p0[3] == 2.0 => Some((i, q)), _ => None,
        }) {
            assert_eq!(glass.rect, [0.0, 0.0, size.0, size.1], "blur covers the background");
            assert!(glass.p1[0] <= 12.0);
            if let Some(t) = ui.list.cmds[glass_index + 1..].iter().find_map(|c| match c {
                ruru::Cmd::Text(t) => Some(t), _ => None,
            }) { geometry.push((t.pos, t.size, t.scale)); }
        }
        if !ui.animating { break; }
    }
    assert!(!ui.animating);
    assert!(geometry.len() > 5);
    let end = geometry.last().unwrap();
    assert!(geometry[0].0.1 > end.0.1 + 5.0, "panel starts below its resting position");
    for frame in &geometry {
        assert_eq!(frame.0.0, end.0.0, "no horizontal drift");
        assert_eq!(frame.1, end.1, "text box does not scale");
        assert_eq!(frame.2, 1.0, "glyph size stays fixed");
    }
    assert!(geometry.windows(2).all(|p| p[1].0.1 <= p[0].0.1), "rise never reverses or overshoots");
}

#[test]
fn text_shimmer_reuses_geometry_moves_highlight_and_stops_when_removed() {
    let mut ui = Ui::new(true, |active: &bool| {
        let label = text("Thinking…").font_size(18.0);
        if *active { label.text_shimmer(0.2, Color::WHITE) } else { label }
    });
    ui.frame((300.0, 100.0));
    assert!(ui.animating);
    let base = ui.list.cmds.iter().find_map(|c| match c { ruru::Cmd::Text(t) => Some((t.key, t.pos, t.size)), _ => None }).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(90));
    ui.frame((300.0, 100.0));
    let runs: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c { ruru::Cmd::Text(t) => Some(t), _ => None }).collect();
    assert!(runs.len() > 1);
    for run in runs { assert_eq!((run.key, run.pos, run.size), base); }
    ui.state = false;
    ui.frame((300.0, 100.0));
    assert!(!ui.animating);
    assert_eq!(ui.list.cmds.iter().filter(|c| matches!(c, ruru::Cmd::Text(_))).count(), 1);
}

#[cfg(feature = "markdown")]
#[test]
fn code_header_pins_inside_nested_lazy_row_copies_source_and_uses_outer_scroll() {
    use ruru::{CodeHeaderStyle, CodeBlockStyle, MarkdownStyle, Cmd};
    let source = (0..80).map(|i| format!("const cell{i} = 'Привет';\n")).collect::<String>();
    let markdown = format!("```javascript\n{source}```");
    let view_source = markdown.clone();
    let mut ui = Ui::new((), move |_: &()| {
        let source = view_source.clone();
        let style = MarkdownStyle { code_block: CodeBlockStyle {
            header: Some(CodeHeaderStyle { height:40.0, ..Default::default() }),
            background:Color::BLUE, ..Default::default()
        }, ..Default::default() };
        scroll(vstack((rect().height(60.0), list(1, 1800.0, move |_| vstack(markdown_styled(&source, &style)).fill_width()), rect().height(300.0))).fill_width()).fill()
    });
    let size = (420.0, 300.0);
    for _ in 0..4 { ui.frame(size); }
    let header_y = |ui: &Ui<()>| ui.list.cmds.iter().filter_map(|c| match c {
        Cmd::Quad(q) if q.c0 == [Color::BLUE.r,Color::BLUE.g,Color::BLUE.b,Color::BLUE.a] && (q.rect[3]-40.0).abs() < 0.1 => Some(q.rect[1]), _ => None,
    }).last().unwrap();
    assert!((header_y(&ui)-60.0).abs()<1.0);
    ui.wheel((200.0, 160.0), -240.0, ruru::WheelPhase::Line);
    for _ in 0..4 { ui.frame(size); }
    assert!(header_y(&ui).abs()<1.0, "nested code header must pin to the viewport");
    ui.pointer_down((390.0,20.0),false); ui.pointer_up((390.0,20.0));
    assert!(ui.requests.iter().any(|r| matches!(r,ruru::WindowRequest::SetClipboard(s) if s == &source)));
    let before: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c { Cmd::Text(t) if t.size.1 > 300.0 => Some(t.pos.1), _=>None }).collect();
    ui.wheel((200.0, 160.0), -90.0, ruru::WheelPhase::Line); ui.frame(size);
    let after: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c { Cmd::Text(t) if t.size.1 > 300.0 => Some(t.pos.1), _=>None }).collect();
    assert_ne!(before,after,"wheel over code must move the conversation");
    ui.wheel((200.0,160.0),-100000.0,ruru::WheelPhase::Line);
    for _ in 0..4 { ui.frame(size); }
    assert!(!ui.list.cmds.iter().any(|c| matches!(c, Cmd::Quad(q) if q.c0 == [Color::BLUE.r,Color::BLUE.g,Color::BLUE.b,Color::BLUE.a] && (q.rect[3]-40.0).abs()<0.1 && q.rect[1] >= 0.0)), "header must leave with its own code section");
}

#[test]
fn code_glass_is_one_fading_surface_and_keeps_the_cards_corner_clip() {
    use ruru::{CodeHeaderStyle, CodeBlockStyle, MarkdownStyle, Cmd};
    let mut ui = Ui::new((), |_: &()| {
        let style = MarkdownStyle { code_block: CodeBlockStyle {
            header: Some(CodeHeaderStyle { glass:Some(ruru::Glass { depth:0.0,rim:0.0,..Default::default() }), fade:22.0,..Default::default() }),
            radius:14.0, border:Some((0.5,Color::WHITE)), ..Default::default()
        },..Default::default() };
        scroll(markdown_styled("```rust\nfn main() {}\n```",&style).fill_width()).fill()
    });
    ui.frame((420.0,300.0));
    let panes: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c {Cmd::Quad(q) if q.p0[3] == 2.0=>Some(q),_=>None}).collect();
    assert_eq!(panes.len(),1,"a fade is one continuous glass surface");
    assert_eq!(panes[0].p0[2],22.0);
    assert!(panes[0].p1[2].abs()>10.0,"header must inherit rounded card clipping");
    assert_eq!((panes[0].np[3] as u32)&3,0,"both top corners must remain rounded");
}

#[test]
fn selected_round_corners_apply_to_surface_and_nested_clipping() {
    let mut ui = Ui::new((), |_: &()| zstack(
        zstack(rect().fill().background(Color::WHITE)).fill().clip()
    ).size(200.0, 120.0).background(Color::BLACK).radius(16.0)
        .round_corners([true, false, false, false]).clip());
    ui.frame((300.0, 200.0));
    let quads: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c {
        ruru::Cmd::Quad(q) => Some(q), _ => None,
    }).collect();
    assert_eq!((quads[0].np[3] as u32) >> 4, 14, "surface rounds top left only");
    assert_eq!((quads[1].np[3] as u32) & 15, 14, "child clip keeps the same corners");
    assert_eq!((quads[1].np[3] as u32) >> 4, 0, "own shape does not inherit corner selection");
    assert_eq!(quads[1].p1[2].abs(), 16.0);
}

#[test]
fn corner_mask_tracks_measured_surface_without_changing_children() {
    let mut ui = Ui::new((), |_: &()| zstack(rect().size(10.0,10.0).background(Color::WHITE))
        .size(100.0,80.0).background(Color::BLACK).radius(20.0)
        .mask_corners(Color::BLACK,[false,false,true,true]));
    ui.frame((200.0,160.0));
    let quads: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c {
        ruru::Cmd::Quad(q) => Some(q), _ => None,
    }).collect();
    assert_eq!(quads[0].np[3] as u32, 0);
    assert_eq!(quads[1].np[3] as u32, 0);
    assert_eq!(quads[2].np[3] as u32, 256 | (3 << 4));
    assert_eq!(quads[2].rect, quads[0].rect);
}

#[test]
fn backdrop_corner_mask_precedes_shadows_material_and_children() {
    let mut ui = Ui::new(80.0f32, |height: &f32| zstack(rect().size(10.0,10.0).background(Color::WHITE))
        .size(100.0,*height).radius(20.0).smooth().glass_with(Glass::regular())
        .shadow(Color::BLACK.alpha(0.1),12.0,3.0)
        .mask_backdrop_corners(Color::BLACK,[false,false,true,true]));
    for height in [80.0,140.0] {
        ui.state=height;ui.frame((200.0,180.0));
        let quads: Vec<_> = ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Quad(q)=>Some(q),_=>None}).collect();
        assert_eq!(quads[0].np[3] as u32,256 | (3 << 4));
        assert_eq!(quads[1].p0[3],1.0,"shadow follows the backdrop mask");
        assert_eq!(quads[2].p0[3],2.0,"glass follows the shadow");
        assert_eq!(quads[0].rect,quads[2].rect);
        assert_eq!(quads[0].rect[3],height);
        assert_eq!(quads[0].p0[0].abs(),quads[2].p0[0]);
        assert_eq!(quads[3].rect[2..],[10.0,10.0],"children keep their layout");
    }
    for _ in 0..80 {ui.frame((200.0,180.0));}
    assert!(!ui.animating,"a static mask never schedules redraws");
}

#[test]
fn horizontal_glass_light_is_opt_in_and_preserves_corner_flags() {
    let mut ui = Ui::new((), |_: &()| hstack((
        rect().size(100.0,50.0).glass_with(Glass::regular()),
        rect().size(100.0,50.0).radius(12.0).round_corners([true,false,false,false])
            .glass_with(Glass { horizontal_light: true, ..Glass::regular() }),
    )));
    ui.frame((300.0,100.0));
    let quads: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c {
        ruru::Cmd::Quad(q) if q.p0[3] == 2.0 => Some(q), _ => None,
    }).collect();
    assert_eq!(quads[0].np[3] as u32 & 512, 0);
    assert_eq!(quads[1].np[3] as u32, 512 | (14 << 4));
    assert_eq!(quads[0].p1[1], quads[1].p1[1], "lighting does not disable refraction");
}

#[test]
fn split_highlights_selected_border_edges_and_round_corner_during_resize() {
    let mut ui = Ui::new(240.0f32, |w: &f32| split(*w, |w: &mut f32, next| *w=next,
        rect().min_width(100.0).max_width(400.0),
        rect().background(Color::BLACK).radius(16.0).round_corners([true,false,false,false])
            .border_edges([true,false,false,true]).clip(),
    ).highlight_main_border());
    let size=(800.0,400.0);
    ui.frame(size);
    let border = |ui: &Ui<f32>| ui.list.cmds.iter().find_map(|c| match c {
        ruru::Cmd::Quad(q) if q.p0[1]>0.0 && q.rect[3]>300.0 => Some(*q), _=>None,
    }).expect("complete pane border");
    let before=border(&ui);
    assert_eq!(before.rect,[241.0,0.0,559.0,400.0]);
    assert_eq!((before.np[3] as u32 >> 4)&15,14);
    assert_eq!((before.np[3] as u32 >> 10)&15,6,"right and bottom stay hidden");
    ui.pointer_move((240.5,200.0));
    let settle=|ui: &mut Ui<f32>| {
        for _ in 0..250 {
            ui.frame(size);
            if !ui.animating {break;}
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        assert!(!ui.animating,"border light settles without idle redraws");
    };
    settle(&mut ui);
    let accent=Theme::default().color.accent;
    let lit=border(&ui);
    assert!(lit.border.iter().zip([accent.r,accent.g,accent.b,accent.a]).all(|(a,b)|(a-b).abs()<1e-4));
    ui.pointer_down((240.5,200.0),false);
    ui.pointer_move((280.5,220.0));
    ui.frame(size);
    assert_eq!(ui.state,280.0);
    assert_eq!(border(&ui).rect,[281.0,0.0,519.0,400.0]);
    assert_eq!(border(&ui).border,lit.border,"selected edges stay lit while dragging");
    assert_eq!((border(&ui).np[3] as u32 >> 10)&15,6);
    assert!(!ui.list.cmds.iter().any(|c| matches!(c,ruru::Cmd::Quad(q) if q.rect[3]>300.0 && q.rect[2]<=1.0)),"no stray straight divider through the rounded corner");
    ui.pointer_up((280.5,220.0));
    ui.pointer_move((700.0,220.0));
    settle(&mut ui);
    assert_eq!(border(&ui).border,before.border);
}

#[test]
fn hidden_border_edges_do_not_inset_children_or_change_their_borders() {
    let mut ui = Ui::new((), |_: &()| zstack(rect().fill().background(Color::WHITE))
        .size(100.0,80.0).border(2.0,Color::BLACK).border_edges([true,false,false,true]).clip());
    ui.frame((200.0,160.0));
    let quads: Vec<_> = ui.list.cmds.iter().filter_map(|c| match c {
        ruru::Cmd::Quad(q) => Some(q), _=>None,
    }).collect();
    let child=quads.iter().find(|q|q.c0==[1.0;4]).unwrap();
    let border=quads.iter().find(|q|q.p0[1]==2.0).unwrap();
    assert_eq!(child.clip,[2.0,2.0,98.0,78.0]);
    assert_eq!((child.np[3] as u32 >> 10)&15,0);
    assert_eq!((border.np[3] as u32 >> 10)&15,6);
}

#[test]
fn split_gradient_and_grip_follow_hover_drag_and_settle_on_both_sides() {
    for trailing in [false,true] {
        let mut ui=Ui::new(240.0f32,move |w: &f32| {
            let split=split(*w,|w: &mut f32,next|*w=next,rect().min_width(100.0).max_width(400.0),
                rect().background(Color::BLACK).radius(16.0).border_edges([true,false,false,true]).clip())
                .highlight_main_border_fade(0.2).resize_grip();
            if trailing {split.trailing()} else {split}
        });
        let size=(800.0,400.0);
        ui.frame(size);
        let grip=|ui: &Ui<f32>|ui.list.cmds.iter().find_map(|c|match c {
            ruru::Cmd::Quad(q) if q.p0[3]==0.0 && q.p0[1]==0.0 && q.rect[2]<5.0 && q.rect[3]>20.0=>Some(*q),_=>None,
        }).expect("resize grip");
        let border=|ui: &Ui<f32>|ui.list.cmds.iter().find_map(|c|match c {
            ruru::Cmd::Quad(q) if q.p0[1]>0.0 && q.rect[3]>300.0=>Some(*q),_=>None,
        }).unwrap();
        let settle=|ui: &mut Ui<f32>| {
            for _ in 0..250 {
                ui.frame(size);
                if !ui.animating {break;}
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
            assert!(!ui.animating,"grip returns to zero idle redraws");
        };
        let rest=grip(&ui);
        assert_eq!(rest.rect[3],Theme::default().control.split_grip.1);
        assert_eq!(border(&ui).n1,[0.2,if trailing {1.0}else{0.0},0.0,0.0]);
        assert_ne!(border(&ui).np[3] as u32 & 16384,0);
        let base=border(&ui).border;
        let at=(rest.rect[0]+rest.rect[2]*0.5,200.0);
        ui.pointer_move(at);settle(&mut ui);
        assert!(grip(&ui).rect[3]>rest.rect[3]);
        assert_eq!(border(&ui).n0,base,"fade target stays the resting border color");
        assert_ne!(border(&ui).border,base,"corner lights up");
        assert_eq!(ui.cursor,Cursor::ResizeH);
        let hover_h=grip(&ui).rect[3];
        ui.pointer_down(at,false);ui.pointer_move((at.0+20.0,at.1));settle(&mut ui);
        assert!((ui.state-if trailing {220.0}else{260.0}).abs()<1.0);
        assert!(grip(&ui).rect[3]>hover_h,"held grip stretches further");
        ui.pointer_up((at.0+20.0,at.1));ui.pointer_move((400.0,300.0));settle(&mut ui);
        assert_eq!(grip(&ui).rect[3],rest.rect[3]);
        assert_eq!(border(&ui).border,base);
    }
}

#[test]
fn animated_radius_keeps_surface_border_and_clip_together_and_settles() {
    let mut ui=Ui::new(true,|rounded: &bool| zstack(rect().fill().background(Color::WHITE))
        .size(200.0,100.0).background(Color::BLACK).border(1.0,Color::WHITE)
        .radius(if *rounded {16.0}else{0.0}).animate_radius().clip());
    let size=(300.0,200.0);ui.frame(size);
    for target in [false,true] {
        ui.state=target;
        let mut intermediate=false;
        for _ in 0..200 {
            ui.frame(size);
            let quads: Vec<_>=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Quad(q)=>Some(q),_=>None}).collect();
            let radius=quads[0].p0[0].abs();
            if radius>0.1 && radius<15.9 {intermediate=true;}
            assert_eq!(quads.last().unwrap().p0[0].abs(),radius);
            assert!((quads[1].p1[2].abs()-(radius-1.0).max(0.0)).abs()<0.01);
            if !ui.animating {break;}
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(intermediate,"radius must pass through intermediate values");
        assert!(!ui.animating,"no idle redraws after rounding settles");
    }
}

#[test]
fn resize_grip_can_align_with_a_footer_without_overflowing_during_drag() {
    let mut ui=Ui::new(240.0f32,|w:&f32|split(*w,|w:&mut f32,v|*w=v,rect().min_width(100.0),rect())
        .resize_grip_at_bottom(22.0));
    let size=(800.0,400.0);ui.frame(size);
    let grip=|ui:&Ui<f32>|ui.list.cmds.iter().find_map(|c|match c {
        ruru::Cmd::Quad(q) if q.rect[2]>2.0 && q.rect[2]<5.0 && q.rect[3]>=24.0 && q.rect[3]<50.0=>Some(q.rect),_=>None,
    }).unwrap();
    assert_eq!(grip(&ui)[1]+grip(&ui)[3]/2.0,378.0);
    ui.pointer_move((240.5,378.0));ui.pointer_down((240.5,378.0),false);
    for _ in 0..100 {
        ui.frame(size);let r=grip(&ui);
        assert!((r[1]+r[3]/2.0-378.0).abs()<1.0);
        assert!(r[1]+r[3]<=400.0);
        if !ui.animating {break;}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(grip(&ui)[3]>38.0);
}

#[test]
fn double_tap_inline_editor_focuses_selects_commits_and_cancels() {
    struct Edit { active: bool, value: String, saved: String, blurs: usize, taps: usize }
    let mut ui = Ui::new(Edit { active:false,value:"Original title".into(),saved:String::new(),blurs:0,taps:0 }, |s:&Edit| {
        let title = if s.active {
            text_field(&s.value,"",|s:&mut Edit,v|s.value=v).autofocus().select_on_focus()
                .on_submit(|s:&mut Edit|{s.saved=s.value.clone();s.active=false;})
                .on_blur(|s:&mut Edit|{s.saved=s.value.clone();s.active=false;s.blurs+=1;})
                .on_cancel(|s:&mut Edit|s.active=false)
        } else {
            text(&s.value).on_tap(|s:&mut Edit|s.taps+=1).on_double_tap(|s:&mut Edit|s.active=true)
        };
        vstack((title.size(200.0,40.0),button("Other",|_:&mut Edit|{}).size(200.0,40.0)))
            .place(Align::Start,Align::Start)
    });
    let size=(400.0,200.0);ui.frame(size);
    let at=(80.0,20.0);
    ui.pointer_down(at,false);ui.pointer_up(at);ui.frame(size);
    assert!(!ui.state.active);assert_eq!(ui.state.taps,1);
    ui.pointer_down(at,false);ui.pointer_up(at);ui.frame(size);
    assert!(ui.state.active);assert!(ui.has_focus());
    assert!(ui.animating,"autofocus schedules the selection paint");ui.frame(size);
    assert_eq!(ui.selection_text().as_deref(),Some("Original title"));
    assert!(ui.text_input("Renamed"));ui.frame(size);
    assert_eq!(ui.state.value,"Renamed");
    assert!(ui.key(KeyInput::Enter));ui.frame(size);
    assert_eq!(ui.state.saved,"Renamed");assert_eq!(ui.state.blurs,0);
    for _ in 0..2 { ui.pointer_down(at,false);ui.pointer_up(at);ui.frame(size); }
    ui.frame(size);assert!(ui.state.active,"second activation");assert!(ui.has_focus(),"second focus");assert_eq!(ui.selection_text().as_deref(),Some("Renamed"));
    ui.text_input("Cancelled");ui.frame(size);ui.key(KeyInput::Escape);ui.frame(size);
    assert!(!ui.state.active);assert_eq!(ui.state.saved,"Renamed");assert_eq!(ui.state.blurs,0);
    ui.state.active=true;ui.frame(size);ui.frame(size);
    ui.text_input("Blur saved");ui.frame(size);
    ui.pointer_down((80.0,60.0),false);ui.pointer_up((80.0,60.0));ui.frame(size);
    assert_eq!(ui.state.saved,"Blur saved");assert_eq!(ui.state.blurs,1);
    assert!(!ui.has_focus());
}

#[test]
fn intrinsic_field_uses_shaped_width_and_matching_weight() {
    let mut ui=Ui::new(String::from("Wide WWW / Узкий iii"),|value:&String|vstack((
        text(value).font_size(17.0).weight(Weight::Medium).lines(1),
        text_field(value,"",|v:&mut String,s|*v=s).plain().font_size(17.0).weight(Weight::Medium).fit_content(),
    )).align(Align::Start).place(Align::Start,Align::Start));
    ui.frame((600.0,200.0));
    let widths=|ui:&Ui<String>|ui.list.cmds.iter().filter_map(|c|match c {
        ruru::Cmd::Text(t)=>Some(t.size.0),_=>None,
    }).collect::<Vec<_>>();
    let a=widths(&ui);assert_eq!(a.len(),2);
    assert!((a[1]-a[0]-2.0).abs()<0.01,"field must match shaped label width plus caret space");
    ui.state="i".into();ui.frame((600.0,200.0));let short=widths(&ui)[1];
    assert!(short<a[1]);
    ui.state="Очень длинное название чата ".repeat(8);ui.frame((180.0,200.0));
    let field_text=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Text(t)=>Some(t),_=>None}).last().unwrap();
    assert!(field_text.clip.x+field_text.clip.w<=180.0,"long field remains clipped to offered width");
}

#[test]
fn pointer_observation_is_opt_in_global_and_removed_with_hidden_view() {
    #[derive(Default)] struct Tracking {enabled:bool,hidden:bool,points:Vec<Option<(f32,f32)>>,clicks:usize}
    let mut ui=Ui::new(Tracking {enabled:true,..Tracking::default()},|s:&Tracking| {
        let box_view=rect().size(60.0,60.0).on_tap(|s:&mut Tracking|s.clicks+=1).reveal(!s.hidden);
        let box_view=if s.enabled {box_view.track_pointer(|s:&mut Tracking,pos|s.points.push(pos))} else {box_view};
        zstack(box_view).fill()
    });
    ui.frame((200.0,200.0));
    assert!(ui.pointer_move((195.0,20.0)),"observation extends outside mascot bounds");
    assert_eq!(ui.state.points.last(),Some(&Some((125.0,-50.0))));
    assert!(!ui.pointer_move((195.0,20.0)),"same position requests no extra frame");
    assert!(ui.pointer_leave());assert_eq!(ui.state.points.last(),Some(&None));
    ui.pointer_down((100.0,100.0),false);ui.pointer_up((100.0,100.0));assert_eq!(ui.state.clicks,1);
    ui.state.hidden=true;ui.frame((200.0,200.0));let before=ui.state.points.len();
    ui.pointer_move((190.0,190.0));assert_eq!(ui.state.points.len(),before,"reveal(false) removes observers immediately");
    ui.state.hidden=false;ui.state.enabled=false;ui.frame((200.0,200.0));
    ui.pointer_move((190.0,190.0));assert!(!ui.pointer_move((189.0,190.0)),"ordinary views stay idle on pointer motion");
}

#[test]
fn timer_scheduled_by_timer_keeps_its_wakeup_and_then_stops() {
    let mut ui=Ui::new(0usize,|_:&usize|rect().fill());
    ruru::after(0.0,|s:&mut usize|{*s+=1;ruru::after(0.01,|s:&mut usize|*s+=1);});
    ui.frame((200.0,200.0));assert_eq!(ui.state,1);assert!(ui.wake_in.is_some());
    std::thread::sleep(std::time::Duration::from_millis(20));ui.frame((200.0,200.0));
    assert_eq!(ui.state,2);assert!(ui.wake_in.is_none());assert!(!ui.animating);
}

#[test]
fn collapsed_split_can_hide_its_divider_without_interrupting_the_current_drag() {
    for trailing in [false,true] {
        let mut ui=Ui::new(Sp { w:300.0,closed:false },move |s:&Sp| {
            let split=split(s.w,|s:&mut Sp,w|s.w=w,rect().min_width(200.0).max_width(400.0),
                rect().background(MAIN).border(1.0,Theme::default().color.divider))
                .collapsible(s.closed,120.0,|s:&mut Sp,closed|s.closed=closed)
                .hide_collapsed_divider().highlight_main_border_fade(0.2).resize_grip();
            if trailing {split.trailing()}else{split}
        });
        let size=(800.0,400.0);ui.frame(size);
        let point=|width:f32|(if trailing {800.0-width}else{width},200.0);
        ui.pointer_down(point(300.5),false);
        ui.pointer_move(point(90.0));ui.frame(size);
        assert!(ui.state.closed);
        ui.pointer_move(point(260.0));ui.frame(size);
        assert!(!ui.state.closed,"the same held drag can reverse after crossing the collapse threshold");
        ui.pointer_move(point(90.0));ui.frame(size);ui.pointer_up(point(90.0));settle_split(&mut ui);
        assert!(ui.state.closed);
        ui.pointer_move(point(0.5));ui.frame(size);
        assert_ne!(ui.cursor,ruru::Cursor::ResizeH,"collapsed edge has no resize hover target");
        let border=ui.list.cmds.iter().find_map(|c|match c {
            ruru::Cmd::Quad(q) if q.p0[1]>0.0=>Some(*q),_=>None,
        }).unwrap();
        let hidden=(border.np[3] as u32 >> 10)&15;
        assert_eq!(hidden & if trailing {2}else{8},if trailing {2}else{8},"shared border is hidden");
        let divider=Theme::default().color.divider;
        assert_eq!(border.border,[divider.r,divider.g,divider.b,divider.a],"no collapsed hover highlight");
        assert!(!ui.list.cmds.iter().any(|c|matches!(c,ruru::Cmd::Quad(q) if q.rect[2]<=5.0 && q.rect[3]>20.0)),"no divider line or grip while hidden");
        ui.pointer_down(point(0.5),false);ui.pointer_move(point(300.0));ui.pointer_up(point(300.0));ui.frame(size);
        assert!(ui.state.closed,"a fresh edge gesture cannot reopen the sidebar");
        ui.state.closed=false;settle_split(&mut ui);
        assert!(!ui.state.closed,"an application control may still reopen it");
        ui.pointer_move(point(ui.state.w+0.5));ui.frame(size);
        assert_eq!(ui.cursor,ruru::Cursor::ResizeH,"opened divider remains resizable");
    }
}


#[test]
fn hover_marquee_uses_full_width_scrolls_once_and_returns_smoothly() {
    let mut ui=Ui::new(0,|_:&i32| {
        zstack((
            text("A fairly long sidebar title that needs room").hover_marquee(5000.0,24.0,8.0).fill_width(),
            text("x").size(20.0,24.0).on_tap(|s:&mut i32|*s+=1).place(Align::End,Align::Center),
        )).size(120.0,24.0).hover_scope().place(Align::Start,Align::Start)
    });
    let size=(240.0,80.0);ui.frame(size);ui.frame(size);
    let text=|ui:&Ui<i32>|ui.list.cmds.iter().find_map(|c|if let ruru::Cmd::Text(t)=c {Some((t.pos,t.clip,t.fade))}else{None}).unwrap();
    let rest=text(&ui);
    assert!(rest.2.is_some());assert_eq!(rest.1.x+rest.1.w,120.0);
    assert!(!ui.animating && ui.wake_in.is_none());
    // The action overlays the label, so hover must come from the entire row's scope.
    ui.pointer_move((110.0,12.0));ui.frame(size);
    let start=text(&ui);assert!(start.2.is_some());assert_eq!(start.1.x+start.1.w,96.0);
    std::thread::sleep(std::time::Duration::from_millis(100));ui.frame(size);ui.frame(size);
    let end=text(&ui);assert!(end.0.0<rest.0.0-20.0);assert!(end.2.is_none());
    assert!(!ui.animating && ui.wake_in.is_none(),"the completed pass must sleep");
    std::thread::sleep(std::time::Duration::from_millis(100));ui.frame(size);
    assert_eq!(text(&ui).0,end.0,"never loop while hovered");
    ui.pointer_down((110.0,12.0),false);ui.pointer_up((110.0,12.0));
    assert_eq!(ui.state,1,"hover scope does not intercept the action");
    ui.pointer_move((220.0,60.0));ui.frame(size);
    assert!((text(&ui).0.0-end.0.0).abs()<1.0,"leaving does not snap the title back");
    assert!(ui.animating);
    std::thread::sleep(std::time::Duration::from_millis(60));ui.frame(size);
    let returning=text(&ui).0.0;assert!(returning>end.0.0 && returning<rest.0.0);
    std::thread::sleep(std::time::Duration::from_millis(160));ui.frame(size);ui.frame(size);
    assert_eq!(text(&ui).0,rest.0);assert!(text(&ui).2.is_some());
    assert_eq!(text(&ui).1.w,120.0);
    assert!(!ui.animating && ui.wake_in.is_none(),"the completed return does not repaint");
}

#[test]
fn hover_marquee_does_not_animate_or_fade_fitting_labels() {
    let mut ui=Ui::new(0,|_:&i32| text("Short").hover_marquee(30.0,24.0,8.0).size(120.0,24.0).place(Align::Start,Align::Start));
    let size=(240.0,80.0);ui.frame(size);ui.frame(size);
    ui.pointer_move((60.0,12.0));ui.frame(size);ui.frame(size);
    let text=ui.list.cmds.iter().find_map(|c|if let ruru::Cmd::Text(t)=c {Some(t)}else{None}).unwrap();
    assert_eq!(text.pos.0,0.0);assert!(text.fade.is_none());
    assert!(!ui.animating && ui.wake_in.is_none());
}

fn tail_cursor(waiting:bool)->TextCursor {
    TextCursor {size:if waiting {16.0}else{7.0},reserve:16.0,gap:5.0,radius:1.5,
        color:Color::rgba(0.91,0.13,0.37,1.0),cycle_period:if waiting {5.0}else{0.0},transition_duration:0.55}
}
fn tail_marker<S:'static>(ui:&Ui<S>)->Vec<[f32;4]> {
    ui.list.cmds.iter().filter_map(|cmd|match cmd {ruru::Cmd::Quad(q) if q.c0[0]==0.91 && q.c0[1]==0.13=>Some(q.rect),_=>None}).collect()
}
#[test]
fn text_cursor_wraps_at_right_edge_without_copying_its_slot() {
    let mut ui=Ui::new((),|_:&()|text("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789").font_size(16.0)
        .text_cursor(tail_cursor(false)).width(100.0).selectable().place(Align::Start,Align::Start));
    ui.frame((200.0,200.0));ui.frame((200.0,200.0));
    let markers=tail_marker(&ui);assert_eq!(markers.len(),1);
    assert!(markers[0][0]+markers[0][2]<=100.01,"cursor must fit on its wrapped line: {:?}",markers[0]);
    assert!(!ui.animating && ui.wake_in.is_none(),"typing cursor is static");
    ui.key(KeyInput::SelectAll);
    assert_eq!(clipboard(&mut ui).as_deref(),Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"));
    for width in [40.0,53.0,67.0,81.0,100.0,127.0] {
        let mut narrow=Ui::new(width,|w:&f32|text("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789").font_size(16.0)
            .text_cursor(tail_cursor(true)).width(*w).place(Align::Start,Align::Start));
        narrow.frame((200.0,400.0));
        let marker=tail_marker(&narrow);assert_eq!(marker.len(),1);
        assert!(marker[0][0]+marker[0][2]<=width+0.01,"cursor outside width {width}: {:?}",marker[0]);
    }
}
#[test]
fn text_cursor_targets_code_body_not_sticky_header_and_stops_when_removed() {
    let mut ui=Ui::new(true,|enabled:&bool| {
        let body=markdown("```rust\nlet value = 12;\n```");
        let body=if *enabled {body.text_cursor(tail_cursor(true))}else{body};
        body.width(300.0).place(Align::Start,Align::Start)
    });
    ui.frame((400.0,300.0));let marker=tail_marker(&ui);assert_eq!(marker.len(),1);
    let texts:Vec<_>=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Text(t)=>Some(t),_=>None}).collect();
    assert!(marker[0][1]>=texts.iter().map(|t|t.pos.1).fold(0.0,f32::max));
    assert!(ui.animating || ui.wake_in.is_some());
    ui.state=false;ui.frame((400.0,300.0));ui.frame((400.0,300.0));
    assert!(tail_marker(&ui).is_empty());assert!(!ui.animating && ui.wake_in.is_none());
}
#[test]
fn text_cursor_offscreen_tail_does_not_morph_or_wake() {
    let mut ui=Ui::new((),|_:&()|scroll(vstack((text("Visible"),rect().height(600.0),text("Tail").text_cursor(tail_cursor(true))))).fill());
    ui.frame((200.0,100.0));ui.frame((200.0,100.0));
    assert!(tail_marker(&ui).is_empty());assert!(!ui.animating && ui.wake_in.is_none());
}
#[test]
fn text_cursor_empty_waiting_line_is_visible() {
    let mut ui=Ui::new((),|_:&()|text("").font_size(16.0).text_cursor(tail_cursor(true)).place(Align::Start,Align::Start));
    ui.frame((200.0,100.0));assert_eq!(tail_marker(&ui).len(),1);assert!(ui.animating || ui.wake_in.is_some());
}

#[test]
fn text_cursor_circle_pulses_without_moving_its_center() {
    let mut ui=Ui::new((),|_:&()|text("Still waiting").text_cursor(tail_cursor(true)).place(Align::Start,Align::Start));
    ui.frame((300.0,100.0));let before=tail_marker(&ui)[0];
    std::thread::sleep(std::time::Duration::from_millis(650));
    ui.frame((300.0,100.0));ui.frame((300.0,100.0));let after=tail_marker(&ui)[0];
    assert!(ui.animating,"the waiting circle pulses while visible");
    assert!(after[2]<before[2] && after[2]>before[2]*0.91);
    assert!((before[0]+before[2]/2.0-after[0]-after[2]/2.0).abs()<0.01);
    assert!((before[1]+before[3]/2.0-after[1]-after[3]/2.0).abs()<0.01);
}

#[test]
fn activity_indicator_animates_only_while_visible() {
    use ruru::prelude::*;
    let mut ui=ruru::Ui::new(0u8,|s:&u8| {
        let activity=activity_indicator(16.0,1.5,2.4).color(Color::WHITE);
        match *s {
            0=>activity.place(Align::Start,Align::Start),
            1=>vstack((rect().height(150.0),activity)).fill().clip(),
            2=>rect(),
            3=>activity.color(Color::CLEAR),
            _=>activity_indicator(16.0,1.5,0.0),
        }
    });
    ui.frame((100.0,100.0));assert!(ui.animating);
    let first=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Quad(q)=>Some(q.rect),_=>None}).collect::<Vec<_>>();
    std::thread::sleep(std::time::Duration::from_millis(40));ui.frame((100.0,100.0));
    let second=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Quad(q)=>Some(q.rect),_=>None}).collect::<Vec<_>>();
    assert_ne!(first,second,"ring advances between frames");
    for state in 1..=4 {
        ui.state=state;ui.frame((100.0,100.0));
        assert!(!ui.animating,"invisible, removed and static indicators sleep");
        assert!(ui.wake_in.is_none());
    }
}

#[test]
fn system_theme_provider_tracks_os_independently_of_explicit_overrides_and_stays_idle() {
    let observed=std::rc::Rc::new(std::cell::Cell::new((false,false)));let capture=observed.clone();
    let mut ui=Ui::new(None::<bool>,move |_:&Option<bool>,env:&Env| {
        capture.set((env.system_dark,env.theme.dark));text("Appearance").color(env.theme.color.text)
    });
    ui.theme_with_system(|choice,system_dark|if choice.unwrap_or(system_dark){Theme::dark()}else{Theme::light()});
    for (choice,system,expected) in [(None,true,true),(None,false,false),(Some(true),false,true),(Some(false),true,false),(None,true,true)] {
        ui.state=choice;ui.system_dark=system;ui.frame((300.0,100.0));ui.frame((300.0,100.0));
        assert_eq!(ui.theme().dark,expected);assert_eq!(observed.get(),(system,expected));
        assert!(!ui.animating && ui.wake_in.is_none(),"appearance has no polling timer or idle frames");
    }
}
#[test]
fn theme_provider_observes_background_changes_in_the_same_frame() {
    let mut ui=Ui::new(false,|_:&bool|text("Theme"));
    ui.theme_with_system(|light,_|if *light {Theme::light()}else{Theme::dark()});
    ui.frame((200.0,100.0));assert!(ui.theme().dark);
    ruru::link::<bool>().send(|light|*light=true);ui.frame((200.0,100.0));
    assert!(!ui.theme().dark,"the provider must run after inbox updates, not a frame later");
}

#[test]
fn switching_keyed_sticky_lists_starts_at_the_real_bottom_in_first_frame() {
    let mut ui = Ui::new(0usize, |selected: &usize| {
        let selected = *selected;
        let count = if selected == 1 { 2 } else { 60 };
        zstack(scroll(zstack(list(count, 120.0, move |i| {
            vstack((text(PARA.repeat(if selected == 1 { 18 } else { 1 + i % 5 })),
                rect().height(2.0).fill_width().background(if i + 1 == count {Color::RED} else {Color::BLUE})))
                .padding_v(8.0).fill_width()
        }).fill_width().max_width(500.0)).padding_ltrb(16.0, 10.0, 16.0, 30.0))
            .key(selected).stick_to_bottom().fill()).fill()
    });
    for selected in [0, 1, 0, 2, 1] {
        ui.state = selected;
        ui.frame((640.0, 500.0));
        let bottom = quads(&ui, Color::RED).last().map(|r| r[1] + r[3]);
        assert_eq!(bottom, Some(462.0), "first frame of chat {selected} must show its actual end");
        // Start the next navigation while reading old messages in this chat.
        ui.wheel((300.0, 250.0), 180.0, ruru::WheelPhase::Line);
        ui.frame((640.0, 500.0));
    }
}

#[test]
fn first_sticky_list_frame_measures_only_the_tail_viewport() {
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    let builds=Arc::new(AtomicUsize::new(0));
    let rows=builds.clone();
    let mut ui=Ui::new((),move |_:&()| {
        let rows=rows.clone();
        scroll(list(10_000,120.0,move |_| {
            rows.fetch_add(1,Ordering::Relaxed);
            text(PARA.repeat(4)).fill_width()
        })).stick_to_bottom()
    });
    ui.frame((600.0,500.0));
    assert!(builds.load(Ordering::Relaxed)<20,"first-frame correction must not eagerly build the history");
}

#[test]
fn scoped_hover_offset_keeps_layout_and_moves_the_hit_region_with_the_child() {
    let mut ui=Ui::new(0u8,|_:&u8|zstack(
        rect().size(20.0,20.0).background(Color::WHITE).on_tap(|s:&mut u8|*s+=1)
            .hover_offset(-40.0,0.0).place(Align::End,Align::Center)
    ).size(160.0,40.0).hover_scope().place(Align::Start,Align::Start));
    let size=(200.0,100.0);ui.frame(size);
    let position=|ui:&Ui<u8>|ui.list.cmds.iter().find_map(|cmd|match cmd {ruru::Cmd::Quad(q) if q.c0==[1.0,1.0,1.0,1.0]=>Some(q.rect),_=>None}).unwrap();
    let rest=position(&ui);assert_eq!(rest,[140.0,10.0,20.0,20.0]);
    ui.pointer_move((10.0,20.0));
    for _ in 0..80 {ui.frame(size);if !ui.animating {break;}std::thread::sleep(std::time::Duration::from_millis(5));}
    assert_eq!(position(&ui),[100.0,10.0,20.0,20.0]);
    ui.pointer_down((110.0,20.0),false);ui.pointer_up((110.0,20.0));assert_eq!(ui.state,1);
    ui.pointer_down((150.0,20.0),false);ui.pointer_up((150.0,20.0));assert_eq!(ui.state,1,"old presentation position is not clickable");
    ui.pointer_move((180.0,80.0));
    for _ in 0..80 {ui.frame(size);if !ui.animating {break;}std::thread::sleep(std::time::Duration::from_millis(5));}
    assert_eq!(position(&ui),rest);assert!(!ui.animating);
}


#[test]
fn picker_typography_and_menu_follow_custom_style_without_scaling_to_control_height() {
    let mut ui=Ui::new((32.0,0usize),|s:&(f32,usize)|picker(&["English","Русский"],s.1,|s:&mut (f32,usize),i|s.1=i)
        .size(150.0,s.0).font_size(14.0).line_height(1.5).weight(Weight::Medium));
    let mut theme=Theme::dark();theme.text.body.size=22.0;
    theme.control.picker_padding=(10.0,3.0);theme.control.picker_row=30.0;
    ui.set_theme(theme);
    for height in [32.0,52.0] {
        ui.state.0=height;ui.frame((400.0,300.0));
        let texts:Vec<_>=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Text(t)=>Some(t),_=>None}).collect();
        assert_eq!(texts[0].size.1,21.0,"explicit typography overrides the theme and stays fixed");
        assert!((texts[0].pos.1+texts[0].size.1/2.0-height/2.0).abs()<=0.5,"label centers in the measured control");
        assert_eq!(texts[1].size.1,21.0,"chevron uses the same typographic scale");
        assert!(texts[1].pos.1>=0.0 && texts[1].pos.1+texts[1].size.1<=height);
    }
    ui.pointer_down((50.0,20.0),false);ui.pointer_up((50.0,20.0));
    ui.resizing=true;ui.frame((400.0,300.0));ui.resizing=false;
    let texts:Vec<_>=ui.list.cmds.iter().filter_map(|c|match c {ruru::Cmd::Text(t)=>Some(t),_=>None}).collect();
    assert_eq!(texts.len(),5,"control label/chevron plus two choices and the selected check");
    assert!((texts[2].size.0/texts[2].scale-texts[0].size.0).abs()<0.01 && (texts[2].size.1/texts[2].scale-texts[0].size.1).abs()<0.01,"menu choice retains the control's explicit style through its opening animation");
    assert!((texts[3].size.1/texts[3].scale-21.0).abs()<0.01,"selected check uses the same typography");
    assert!(((texts[4].pos.1-texts[2].pos.1)/texts[4].scale-30.0).abs()<1.0,"theme controls menu row height");
    let second=(texts[4].pos.0+4.0,texts[4].pos.1+5.0);
    ui.pointer_down(second,false);ui.pointer_up(second);assert_eq!(ui.state.1,1);
}

#[test]
fn picker_without_font_override_uses_the_theme_body_style() {
    let mut ui=Ui::new((),|_:&()|picker(&["Choice"],0,|_:&mut (),_|{}).size(140.0,36.0));
    let mut theme=Theme::dark();theme.text.body.size=12.0;theme.text.body.line_height=1.5;
    ui.set_theme(theme);ui.frame((200.0,100.0));
    let label=ui.list.cmds.iter().find_map(|c|match c {ruru::Cmd::Text(t)=>Some(t),_=>None}).unwrap();
    assert_eq!(label.size.1,18.0);
    assert!((label.pos.1+label.size.1/2.0-18.0).abs()<=0.5);
}

#[test]
fn scroll_bottom_overlay_uses_wrapped_height_for_tail_and_scrollbar_without_losing_focus() {
    #[derive(Default)] struct Draft {text:String}
    let mut ui=Ui::new(Draft::default(),|s:&Draft|scroll(rect().height(1000.0).fill_width().background(Color::hex(0xAA0000)))
        .stick_to_bottom().indicator(Indicator::Always).indicator_insets(3.0,5.0)
        .bottom_overlay(zstack(text_field(&s.text,"Draft",|s:&mut Draft,v|s.text=v)
            .font_size(14.0).line_height(1.4).multiline().max_lines(8).plain())
            .padding(6.0).fill_width().background(Color::hex(0x0000AA))));
    let size=(240.0,300.0);
    let geometry=|ui:&Ui<Draft>,color:u32|ui.list.cmds.iter().find_map(|cmd|match cmd {
        ruru::Cmd::Quad(q) if q.c0=={let c=Color::hex(color);[c.r,c.g,c.b,c.a]}=>Some(q.rect),_=>None,
    }).unwrap();
    let mut heights=vec![];
    for draft in ["i".repeat(120),"W".repeat(120),"界".repeat(120),"👨‍👩‍👧‍👦".repeat(30)] {
        ui.state.text=draft;ui.frame(size);
        let content=geometry(&ui,0xAA0000);let overlay=geometry(&ui,0x0000AA);
        assert!((content[1]+content[3]-overlay[1]).abs()<=0.5,"parked content ends exactly above the measured overlay");
        assert_eq!(overlay[1]+overlay[3],size.1,"overlay is anchored to the bottom");
        heights.push(overlay[3]);
        let bars:Vec<_>=ui.list.cmds.iter().filter_map(|cmd|match cmd {
            ruru::Cmd::Quad(q) if q.rect[0]>size.0-10.0 && q.rect[2]<8.0=>Some(q.rect),_=>None,
        }).collect();
        for bar in bars {assert!(bar[1]+bar[3]<=overlay[1]-4.0,"indicator respects measured overlay plus its explicit inset");}
    }
    assert!(heights[0]<heights[1],"narrow and wide glyphs must not share a character-count estimate");
    ui.state.text="ab".into();ui.frame(size);
    let overlay=geometry(&ui,0x0000AA);let at=(20.0,overlay[1]+12.0);
    ui.pointer_down(at,false);ui.pointer_up(at);ui.key(ruru::KeyInput::End);ui.key(ruru::KeyInput::Left);
    for _ in 0..3 {ui.frame(size);assert!(ui.has_focus());assert!(ui.text_input("x"));}
    assert_eq!(ui.state.text,"axxxb");
}
