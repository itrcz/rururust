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
| `src/view.rs` | Декларативное дерево `View<S>`: `vstack/hstack/zstack/text/image/button/toggle/slider/text_field/picker/tab_bar/scroll` и модификаторы |
| `src/layout.rs` | Однопроходный layout в духе SwiftUI (родитель предлагает размер, ребёнок выбирает) |
| `src/ui.rs` | Ядро без привязки к платформе: состояние, покраска в `DrawList`, пружины, ввод |
| `src/render.rs` | wgpu-рендерер: один инстансный пайплайн на все фигуры, резкая копия сцены и пирамида размытия для стекла, текстуры картинок |
| `src/shaders/` | SDF-сквирклы (как continuous corners в iOS), градиенты, тени, стекло |
| `src/text.rs` | Шейпинг и кэш текста (cosmic-text + glyphon), встроенный шрифт Inter |
| `src/app.rs` | Окно и события (winit), один путь для native и web |
| `src/headless.rs` | Рендер в PNG для скриншотов и визуальных тестов |

Модель: состояние `S` → `view(&S)` → дерево → layout → draw list → GPU. Обработчики — замыкания
`Fn(&mut S)`. Кадр рисуется только по событию или пока идёт анимация; в простое CPU = 0%.

## Стекло

`.glass()` — материал как в iOS: фон под стеклом слегка матируется и преломляется через фаску
по краю (закон Снелла, дисперсия), кромка подсвечивается цветом фона. Параметры — в `Glass`
(`frost`, `refraction`, `depth`, `dispersion`, `rim`, `tint`), пресеты `Glass::regular()`,
`clear()`, `frosted()`, общая сила эффекта — `.strength(k)`. Приёмы шейдера взяты из
[liquid-rust](https://github.com/al1h3n/liquid-rust) (MIT/Apache-2.0).

`tab_bar(...)` — плавающий таб-бар с «каплей»: по тапу она перетекает к выбранному табу,
при зажатии увеличивается и её можно перетаскивать.

## Чего пока нет

Выделение текста, буфер обмена и IME в полях ввода (и экранная клавиатура в мобильном
браузере), многострочный ввод, иконки, accessibility, горизонтальный скролл и rubber-band,
диффинг дерева (сейчас оно пересобирается целиком на каждый кадр), темы, слияние соседних
стёкол. Сборки под iOS/Android не проверялись.

Известная проблема: в тестовой сборке WebKit (Playwright) после анимаций иногда пропадают
отдельные буквы; нативно и в Chromium не воспроизводится. `cargo run --release --example stress -- out.png`
гоняет этот сценарий нативно.
