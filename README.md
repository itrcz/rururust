# ruru

Маленький и быстрый UI-фреймворк на Rust с настоящим стеклом (преломление, матирование,
слипающиеся панели). Один код — нативно (macOS / Windows / Linux / iOS / Android) и в браузере
на `<canvas>`.

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
cargo run --release --example showcase                       # окно (меняй размер: телефон → планшет → десктоп)
cargo run --release --example showcase -- --screenshot a.png # рендер в PNG без окна
./web/build.sh && python3 -m http.server -d web 8080          # браузер: http://localhost:8080
cargo test
```

Нужен Rust (https://rustup.rs). Для браузера дополнительно:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # версия как в Cargo.lock
```

Страницу надо открывать через http-сервер, а не как файл.

## Что есть

| Область | API |
| --- | --- |
| Раскладка | `vstack` `hstack` `zstack` `spacer` `rect`, `.padding()` `.size()` `.fill_width()` `.place()` |
| Текст | `text`, `rich_text(Vec<Span>)` (жирный, курсив, код, цвет), `markdown(src)` / `markdown_with(src, images)` |
| Картинки и иконки | `image(&Image)`, `icon(&Image)` — глиф, красится цветом текста; `Image::decode` (PNG/JPEG), `Image::from_svg` |
| Контролы | `button` `toggle` `slider` `text_field` `picker` `tab_bar` |
| Скролл | `scroll(...)`: резинка на концах, инерция, `.on_refresh(refreshing, f)`, `.indicator(Auto/Always/Never)`, `.stick_to_bottom()`; `.sticky()` у детей |
| Длинные списки | `list(count, estimated_height, |i| row)` — строятся только видимые строки, высота строк любая |
| Оверлеи | `modal` `sheet` `drawer` `snackbar` — последними в корневом `zstack` |
| Стекло | `.glass()` / `.glass_with(Glass)`; `Glass::regular()` `clear()` `frosted()`, `.strength(k)`; близкие панели слипаются сами |
| Адаптивность | `fn view(s: &S, env: &Env)`; `env.class()` → `Phone` / `Tablet` / `Desktop` |
| Время | `after(seconds, |s: &mut S| ...)` — таймер на состоянии (скрыть снекбар, закончить обновление) |
| Отладка | `App::show_fps(true)`, `ruru::headless::render_png` |

Модель: состояние `S` → `view(&S)` → дерево → layout → draw list → GPU. Обработчики — замыкания
`Fn(&mut S)`. Кадр рисуется только по событию, по таймеру или пока идёт анимация.

## Брать только часть

```toml
ruru = { version = "0.1", default-features = false }                            # дерево, layout, ввод, анимации, текст → DrawList
ruru = { version = "0.1", default-features = false, features = ["renderer"] }   # + wgpu-рендерер: встроить в свой движок
ruru = { version = "0.1", default-features = false, features = ["app"] }        # + окно и цикл событий
```

Дополнительно: `decode` (PNG/JPEG), `svg`, `markdown` (+ моноширинный шрифт), `headless`, `webgl`
(запасной путь для браузеров без WebGPU, ~1 МБ wasm). `Ui` не знает про окно: ему передают
размер и события (`frame`, `pointer_down/move/up`, `wheel`, `key`, `text_input`), он отдаёт `DrawList`.

## Как устроено

| Файл | Что делает |
| --- | --- |
| `src/view.rs` | Декларативное дерево `View<S>`, конструкторы и модификаторы |
| `src/layout.rs` | Однопроходный layout в духе SwiftUI; id узлов для состояния между кадрами |
| `src/ui.rs` | Ядро без привязки к платформе: покраска в `DrawList`, пружины, физика скролла, ввод, таймеры |
| `src/render.rs` | wgpu: один инстансный пайплайн на все фигуры, снимок сцены + пирамида размытия для стекла |
| `src/shaders/` | SDF-сквирклы, градиенты, тени, стекло (преломление по Снеллу, дисперсия, слияние панелей) |
| `src/text.rs` | Шейпинг и кэш текста (cosmic-text), встроенные шрифты Inter и JetBrains Mono |
| `src/markdown.rs` | Markdown → обычные view |
| `src/app.rs` | Окно и события (winit), один путь для native и web |

Приёмы стекла взяты из [liquid-rust](https://github.com/al1h3n/liquid-rust) (MIT/Apache-2.0).

## Известные проблемы и чего пока нет

- WebKit под WebGPU иногда теряет `queue.writeTexture` (пропадали буквы). В `app.rs` стоит обход:
  в WebKit загрузки текстур идут через `copyBufferToTexture`. Кто встраивает только `renderer`
  в свою web-страницу, обход не получает. В настоящем Safari не проверено.
- Поля ввода: нет выделения, буфера обмена, IME, многострочного ввода, экранной клавиатуры в мобильном браузере.
- Текст нельзя выделить и скопировать (важно для чата); ссылки в markdown не кликаются; таблицы — упрощённо.
- Резинка и pull-to-refresh от трекпада работают только на macOS (нужны фазы жеста); в браузере и
  с мышью скролл останавливается на краях. Тач-перетаскивание работает везде.
- `after()` — только таймеры на UI-потоке; для сетевых запросов нужен канал из фона (не сделан).
- Нет accessibility, тем, горизонтального скролла, диффинга дерева.
- Сборки под iOS/Android/Windows/Linux не проверялись.
