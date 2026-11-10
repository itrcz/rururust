use ruru::prelude::*;
use ruru::Ui;

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
