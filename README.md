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
| Раскладка | `vstack` `hstack` `zstack` `spacer` `rect`, `.padding()` `.padding_h()` / `.padding_v()` (складываются) `.size()` `.fill_width()` `.place()` |
| Пределы размера | `.max_width(w)` `.min_width(w)` `.max_height(h)` `.min_height(h)` — `.fill_width().max_width(720.0)`: колонка чтения, не шире 720 |
| Обрезка | `.clip()` — дети режутся по скруглённой форме узла (`.radius()`), вложенные клипы пересекаются; текст режется прямоугольником |
| Анимация размера | `.animate()` — новый размер узла доезжает пружиной, содержимое режется краем, а не сжимается: `sidebar.width(if open { 280.0 } else { 0.0 }).animate()`. Пример: `examples/panels.rs` |
| Наведение и курсор | `.hover(цвет)` — подсветка поверх фона, проявляется ~0.1 с; `.cursor(Cursor::Pointer / Text / ResizeH / …)`; по умолчанию «рука» у нажимаемого, I-beam у поля; `ui.cursor` |
| Текст | `text`, `rich_text(Vec<Span>)` (жирный, курсив, код, цвет), `markdown(src)` / `markdown_with(src, images)`; `.lines(n)` — не больше n строк с «…», `.truncate(Truncate::Fade)` — затухание к краю (одна строка, если `.lines` не задан) |
| Картинки и иконки | `image(&Image)`, `icon(&Image)` — глиф, красится цветом текста; `Image::decode` (PNG/JPEG), `Image::from_svg` |
| Контролы | `button` `toggle` `slider` `text_field` `picker` `tab_bar` |
| Ввод текста | `text_field(value, placeholder, on_change)`: `.multiline()` / `.max_lines(n)` — растёт от 1 строки до `n`, дальше скролл; `.on_submit(f)` — Enter отправляет, Shift/Alt+Enter — перенос; `.plain()` — без своего фона, отступов и кольца фокуса (контейнер рисует приложение); `.font_size()`. Выделение мышью / Shift+стрелки / двойной и тройной клик / Cmd+A, слова — Alt/Ctrl+←→, Cmd+←→ — края строки, буфер обмена Cmd/Ctrl+C/X/V, undo/redo Cmd+Z / Shift+Cmd+Z, IME (preedit подчёркнут) |
| Скролл | `scroll(...)`: резинка на концах, инерция, `.on_refresh(refreshing, f)`, `.indicator(Auto/Always/Never)`, `.stick_to_bottom()`; `.sticky()` у детей |
| Длинные списки | `list(count, estimated_height, |i| row)` — строятся только видимые строки, высота строк любая |
| Оверлеи | `modal` `sheet` `drawer` `snackbar` — последними в корневом `zstack` |
| Стекло | `.glass()` / `.glass_with(Glass)`; `Glass::regular()` `clear()` `frosted()`, `.strength(k)`; близкие панели слипаются сами |
| Адаптивность | `fn view(s: &S, env: &Env)`; `env.class()` → `Phone` / `Tablet` / `Desktop` |
| Время | `after(seconds, |s: &mut S| ...)` — таймер на состоянии (скрыть снекбар, закончить обновление) |
| Фон | `spawn(|link| async move {..})` → `Task` (drop = отмена), `link.send(|s| ..)` из любого потока, `sleep(секунды).await` |
| Окно (десктоп) | `App::transparent(true)`, `App::blur(true)` — размытый рабочий стол за окном (macOS; где нет — просто прозрачно), `.background(цвет с альфой)` — тонировка; `App::titlebar(Titlebar::Overlay)` — контент под заголовком, светофор на месте, отступы в `env.titlebar` / `env.window_buttons`; `.window_drag()` — область таскает окно, двойной клик — zoom. Пример: `examples/window.rs` |
| Отладка | `App::show_fps(true)`, `ruru::headless::render_png` |

Модель: состояние `S` → `view(&S)` → дерево → layout → draw list → GPU. Обработчики — замыкания
`Fn(&mut S)`. Кадр рисуется только по событию, по таймеру или пока идёт анимация.

Фоновая работа: обновления состояния приходят из другого потока через `Link` и применяются в начале
следующего кадра. Пока `Task` жив, работа идёт; сбросил его (`s.reply = None`) — отменилась.

```rust
s.reply = Some(spawn(|link: Link<App>| async move {
    for word in answer.split_inclusive(' ') {
        if link.cancelled() { return; }
        sleep(0.03).await;
        link.send(move |s| s.text.push_str(word));
    }
}));
```

Кто встраивает `Ui` без `app`, сам вызывает `set_waker` (на UI-потоке, чтобы `send` будил цикл), а на wasm
ещё и `set_spawner`.

## Брать только часть

```toml
ruru = { version = "0.1", default-features = false }                            # дерево, layout, ввод, анимации, текст → DrawList
ruru = { version = "0.1", default-features = false, features = ["renderer"] }   # + wgpu-рендерер: встроить в свой движок
ruru = { version = "0.1", default-features = false, features = ["app"] }        # + окно и цикл событий
```

Дополнительно: `decode` (PNG/JPEG), `svg`, `markdown` (+ моноширинный шрифт), `headless`, `webgl`
(запасной путь для браузеров без WebGPU, ~1 МБ wasm). `Ui` не знает про окно: ему передают
размер и события (`frame`, `pointer_down/move/up`, `wheel`, `key`, `text_input`, `set_modifiers`, `paste`,
`ime_preedit`), он отдаёт `DrawList` и просьбы к окну в `ui.requests` (`DragWindow`, `Zoom`,
`SetClipboard(текст)`); `ime_area()` — где показать окно IME. Фича `clipboard` (по умолчанию) —
системный буфер на десктопе через `arboard` (+18 КБ к бинарнику); без неё буфер внутренний.

## Как устроено

| Файл | Что делает |
| --- | --- |
| `src/view.rs` | Декларативное дерево `View<S>`, конструкторы и модификаторы |
| `src/layout.rs` | Однопроходный layout в духе SwiftUI; id узлов для состояния между кадрами |
| `src/ui.rs` | Ядро без привязки к платформе: покраска в `DrawList`, пружины, физика скролла, ввод, таймеры |
| `src/render.rs` | wgpu: один инстансный пайплайн на все фигуры, снимок сцены + пирамида размытия для стекла |
| `src/shaders/` | SDF-сквирклы, градиенты, тени, стекло (преломление по Снеллу, дисперсия, слияние панелей) |
| `src/text.rs` | Шейпинг и кэш текста (cosmic-text), встроенные шрифты Inter и JetBrains Mono; геометрия текста: точка ↔ байт, каретка, прямоугольники выделения, слово / абзац |
| `src/edit.rs` | Редактор `text_field`: каретка, выделение, undo, буфер обмена, IME, многострочность (подмодуль `ui`) |
| `src/task.rs` | Канал «фон → состояние»: `Link`, `Task`, `spawn`, `sleep`, `Ui::pump` |
| `src/markdown.rs` | Markdown → обычные view |
| `src/app.rs` | Окно и события (winit), один путь для native и web |

Приёмы стекла взяты из [liquid-rust](https://github.com/al1h3n/liquid-rust) (MIT/Apache-2.0).

## Известные проблемы и чего пока нет

- WebKit под WebGPU иногда теряет `queue.writeTexture` (пропадали буквы). В `app.rs` стоит обход:
  в WebKit загрузки текстур идут через `copyBufferToTexture`. Кто встраивает только `renderer`
  в свою web-страницу, обход не получает. В настоящем Safari не проверено.
- Поля ввода: в браузере нет IME и экранной клавиатуры (нет скрытого DOM-поля; символы, которые браузер
  вставляет не через `keydown`, — IME, эмодзи-панель, диктовка — теряются); буфер обмена в вебе работает
  через события `copy`/`cut`/`paste` (Chromium; Safari может их не присылать без DOM-выделения).
  На Linux системный буфер — только X11 (под Wayland через XWayland). Нет тач-выделения (ручек),
  контекстного меню, двунаправленного текста сверх того, что даёт cosmic-text; I-beam курсора нет.
- Текст нельзя выделить и скопировать (важно для чата); ссылки в markdown не кликаются; таблицы — упрощённо.
- Резинка и pull-to-refresh от трекпада работают только на macOS (нужны фазы жеста); в браузере и
  с мышью скролл останавливается на краях. Тач-перетаскивание работает везде.
- Нет accessibility, тем, горизонтального скролла, диффинга дерева.
- Сборки под iOS/Android/Windows/Linux не проверялись.
