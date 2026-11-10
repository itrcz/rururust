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
    let mut ui = Ui::new(S { n: 0, on: false, v: 0.0 }, |s| zstack(view(s)).fill());
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
