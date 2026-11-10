# ruru

Маленький и быстрый UI-фреймворк на Rust с настоящим стеклом (backdrop blur + преломление).
Один код — нативно (macOS / Windows / Linux / iOS / Android) и в браузере на `<canvas>`.

```rust
use ruru::prelude::*;

struct Counter { n: i32 }

fn view(s: &Counter) -> View<Counter> {
    vstack((
        text(s.n.to_string()).font_size(64.0).weight(Weight::Bold),
        button("Add one", |s: &mut Counter| s.n += 1),
    ))
    .spacing(16.0)
    .padding(24.0)
    .glass()
    .radius(28.0)
}

fn main() {
    App::new(Counter { n: 0 }, view).run();
}
```

## Запуск

```sh
cargo run --release --example showcase                      # окно
cargo run --release --example showcase -- --screenshot a.png # рендер в PNG без окна
./web/build.sh && python3 -m http.server -d web 8080         # браузер: http://localhost:8080
cargo test
```

Нужен Rust (https://rustup.rs). Для браузера дополнительно:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # версия как в Cargo.lock
```

Страницу надо открывать через http-сервер, а не как файл. Счётчик FPS и времени кадра в углу
включается через `App::show_fps(true)`; пока он включён, кадры рисуются непрерывно.

## Как устроено

| Файл | Что делает |
| --- | --- |
| `src/view.rs` | Декларативное дерево `View<S>`: `vstack/hstack/zstack/text/button/toggle/slider/scroll` и модификаторы |
| `src/layout.rs` | Однопроходный layout в духе SwiftUI (родитель предлагает размер, ребёнок выбирает) |
| `src/ui.rs` | Ядро без привязки к платформе: состояние, покраска в `DrawList`, пружины, ввод |
| `src/render.rs` | wgpu-рендерер: один инстансный пайплайн на все фигуры + dual-Kawase блюр для стекла |
| `src/shaders/` | SDF-сквирклы (как continuous corners в iOS), градиенты, тени, стекло |
| `src/text.rs` | Шейпинг и кэш текста (cosmic-text + glyphon), встроенный шрифт Inter |
| `src/app.rs` | Окно и события (winit), один путь для native и web |
| `src/headless.rs` | Рендер в PNG для скриншотов и визуальных тестов |

Модель: состояние `S` → `view(&S)` → дерево → layout → draw list → GPU. Обработчики — замыкания
`Fn(&mut S)`. Кадр рисуется только по событию или пока идёт анимация; в простое CPU = 0%.

## Чего пока нет

Ввод текста и клавиатура/фокус, картинки и иконки, accessibility, горизонтальный скролл и
rubber-band, диффинг дерева (сейчас оно пересобирается целиком на каждый кадр), темы.
Сборки под iOS/Android не проверялись.
