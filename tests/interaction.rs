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
    let mut ui = Ui::new(T { tab: 0 }, |s: &T| {
        zstack(tab_bar(&["One", "Two", "Three"], s.tab, |s: &mut T, i| s.tab = i).place(Align::Start, Align::Start)).fill()
    });
    ui.frame((400.0, 200.0));
    // Three equal tabs: the bar is 6 pt of padding + 3 tabs + 6 pt.
    let tab_w = 40.0 + 44.0; // rough; only the ordering of the taps matters below
    ui.pointer_down((6.0 + tab_w * 2.5, 28.0), false);
    ui.pointer_up((6.0 + tab_w * 2.5, 28.0));
    assert_eq!(ui.state.tab, 2);
    // Grab the drop, drag it left and let go below the bar: the tab under it wins.
    ui.frame((400.0, 200.0));
    ui.pointer_down((6.0 + tab_w * 2.5, 28.0), false);
    ui.pointer_move((10.0, 150.0));
    ui.frame((400.0, 200.0));
    assert!(ui.animating, "the drop is moving");
    ui.pointer_up((10.0, 150.0));
    assert_eq!(ui.state.tab, 0);
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
    for _ in 0..80 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        ui.frame((300.0, 400.0));
    }
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
