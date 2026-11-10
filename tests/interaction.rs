use ruru::prelude::*;
use ruru::{KeyInput, Ui};

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
        assert!((green[0] - w).abs() < 0.01 && (green[2] - (800.0 - w)).abs() < 0.01);
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
