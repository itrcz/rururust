//! Interactive smoke: `cargo run --example file_dialogs -- files` (or `folder`).
//! Use fixtures rather than a personal directory; cancel should print an empty result.
fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("folder") => println!("{:?}", ruru::try_pick_folder()),
        _ => println!("{:?}", ruru::pick_files()),
    }
}
