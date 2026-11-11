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
cargo run --release --example menus                          # меню, поповеры, тултипы, контекстное меню, шорткаты
./web/build.sh && python3 -m http.server -d web 8080          # браузер: http://localhost:8080
./web/build.sh menus                                          # другой пример в браузере: http://localhost:8080/?ex=menus
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
| Доли ширины | `.flex(k)` — ребёнок `hstack`/`vstack` берёт долю свободного места по весу (`.fill_*()` = 1): `hstack((a.flex(1.0), b.flex(1.5), c.flex(1.5)))` — колонки при любой ширине |
| Пределы размера | `.max_width(w)` `.min_width(w)` `.max_height(h)` `.min_height(h)` — `.fill_width().max_width(720.0)`: колонка чтения, не шире 720 |
| Обрезка | `.clip()` — дети режутся по скруглённой форме узла (`.radius()`), вложенные клипы пересекаются; текст режется прямоугольником. С `.border()` дети режутся по внутренней кромке рамки, а рамка рисуется поверх детей и hover |
| Тени | `.shadow(цвет, blur, y)` — одна мягкая тень; `.shadow_with(Shadow::new(цвет, blur, y).offset(x, y).spread(s))` — до двух слоёв (ключевая + рассеянная), третий заменяет второй; `Shadow::new(Color::WHITE.alpha(0.1), 0.0, 0.0).spread(0.5).inset()` — внутренняя тень/светлая кромка поверх детей. Каждый слой — один квад |
| Углы | `.radius(r)` — круговая дуга, как в macOS; `.smooth()` — непрерывные углы-сквирклы, как в iOS (у стекла всегда) |
| Анимация размера | `.animate()` — новый размер узла доезжает пружиной, содержимое режется краем, а не сжимается: `sidebar.width(if open { 280.0 } else { 0.0 }).animate()`. Пример: `examples/panels.rs` |
| Наведение и курсор | `.hover(цвет)` — подсветка поверх фона, проявляется ~0.1 с; `.cursor(Cursor::Pointer / Text / ResizeH / …)`; `.hover_color(цвет)` — цвет текста и иконок-шаблонов узла под мышью (глиф кнопки белеет), с тем же затуханием; по умолчанию «рука» у нажимаемого и у ссылок, I-beam у поля и у выделяемого текста; `ui.cursor` |
| Текст | `text`, `rich_text(Vec<Span>)`; `.lines(n)` — не больше n строк с «…», `.truncate(Truncate::Fade)` — затухание к краю (одна строка, если `.lines` не задан); `.line_height(1.6)` — межстрочный в долях кегля (по умолчанию 1.3); `.letter_spacing(0.01)` — разрядка в em |
| Фрагменты (`Span`) | `.bold()` `.italic()` `.code()` `.color(c)`, `.link(url)`, `.weight(Weight::Semibold)`, `.size(pt)` (строка подрастает), `.background(c)` — скруглённая плашка под фрагментом с полями по бокам, режется по строкам при переносе и не меняет высоту строки; `Span::icon(&img)` — значок в строке размером 1 em, красится цветом фрагмента |
| Markdown | `markdown(src)` / `markdown_with(src, images)`; `markdown_styled(src, &MarkdownStyle { size: 14.0, line_height: 1.625, strong: Weight::Semibold, code_background: Some(c), ..MarkdownStyle::from(&*env.theme) })` — кегль, межстрочный, отступ между блоками, цвет и иконки ссылок (`link_icons`: по префиксу URL), заголовки, inline-код, `code_block`, `table` (`TableStyle`: кегль, поля ячеек, линии между строками, вес шапки, `columns` — доли ширины), `quote`; `Default` = вид `markdown()` в тёмной теме; таблицы — колонками |
| Картинки и иконки | `image(&Image)`, `icon(&Image)` — глиф, красится цветом текста; `Image::decode` (PNG/JPEG), `Image::from_svg` |
| Контролы | `button` `toggle` `slider` `text_field` `picker` `tab_bar`; `slider(v, f).steps(5)` — прилипает к 5 делениям и рисует точки; `.slider_style(SliderStyle { track_height, track, track_border, fill, knob, knob_size, knob_shadow, tick_size, tick_on, tick_off })` |
| Поповер | `.popover(open, Placement::top().start(), \|s\| s.open = false, content)` на якоре — слой поверх всего у якоря; переворачивается на другую сторону и сдвигается, чтобы не вылезать за окно; клик мимо только закрывает (не доходит до того, что под ним), Escape тоже; появляется за ~120 мс. `Placement::top/bottom/left/right()` + `.start/center/end()` + `.gap(pt)` + `.margin(pt)` (поле от края окна, 8) + `.offset(dx, dy)` |
| Меню | `menu(vec![MenuItem::action("Rename", f).icon(&img).shortcut("⌥⌘R"), MenuItem::separator(), MenuItem::section("Plugins"), MenuItem::submenu("Copy", items)])`; у пункта `.hint(..)` `.disabled()` `.checked(b)`; подсветка под мышью, ↑↓ ←→ Enter Esc; `.menu_style(MenuStyle { highlight, row_height, font_size, .. })`: ещё `highlight_inset` (подсветка ниже шага строки), `icon_gap`, `separator_height`, `shortcut_size/_color`, `section_size/_weight/_color/_height/_gap`, тени `shadow` + `ambient` + `rim` (inset). Меню в `scroll`/`.clip()` внутри поповера подсвечивается как обычно. Открыть: `.menu(menu(..))` / `.menu_at(placement, ..)` — по клику на якорь, `.context_menu(menu(..))` — по правому клику у курсора, или внутри `.popover` (выбор пункта вызывает `on_dismiss`) |
| Тултип | `.tooltip("Back")`, `.tooltip_shortcut("Back", "⌘[")`, `.tooltip_with(view)`; место — `.tooltip_at(Placement::bottom().gap(2.0))` (по умолчанию снизу, зазор 6) — через 0.5 с покоя мыши (таймер, не кадры), соседний сразу; клик и уход прячут; ввод не перехватывает |
| Шорткаты | `.shortcut(Shortcut::cmd('k'), \|s\| ..)` на корне или поддереве; `.shift()` `.alt()`, `Shortcut::key('/')`; в поле ввода текстовые комбинации (обычные клавиши, Cmd+A/C/V/X/Z/Y) достаются полю. `.on_secondary(f)` — правый клик |
| Выделение текста | `.selectable()` на контейнере или `list` — текст внутри выделяется мышью (протяжка, двойной клик — слово, тройной — абзац, Shift+клик — продлить, протяжка к краю `scroll` — автоскролл) и копируется Cmd/Ctrl+C как plain text, Cmd/Ctrl+A — всё; одно выделение через строки ленивого списка, невидимые строки строятся только при копировании. `.selection_color(c)` — цвет подсветки (и в полях ввода) для поддерева; `.copy_as("- ")` — чем заменить текст поддерева при копировании (`""` — пропустить); `.copy_sep("\t")` — разделитель между детьми (по умолчанию `\n` в `vstack`, пробел в `hstack`, пустая строка между строками `list`); `ui.selection_text()`. `markdown()` выделяем сам: блоки через пустую строку, пункты списков с `- ` / `1. `, ячейки таблиц через таб: `scroll(list(n, 80.0, row).selectable().selection_color(Color::WHITE.alpha(0.22)))` |
| Ссылки | `Span::new("docs").link(url).color(c)` — рука над ссылкой, подчёркивание при наведении; `.on_link(\|s, url\| ..)` на поддереве — свой обработчик, без него клик открывает http(s)/mailto в браузере (`WindowRequest::OpenUrl`); нажатие на ссылке, ушедшее дальше 4 pt, становится выделением. В `markdown()` ссылки живые |
| Ввод текста | `text_field(value, placeholder, on_change)`: `.multiline()` / `.max_lines(n)` — растёт от 1 строки до `n`, дальше скролл; `.on_submit(f)` — Enter отправляет, Shift/Alt+Enter — перенос; `.plain()` — без своего фона, отступов и кольца фокуса (контейнер рисует приложение); `.font_size()`, `.placeholder_color(c)`. Выделение мышью / Shift+стрелки / двойной и тройной клик / Cmd+A, слова — Alt/Ctrl+←→, Cmd+←→ — края строки, буфер обмена Cmd/Ctrl+C/X/V, undo/redo Cmd+Z / Shift+Cmd+Z, IME (preedit подчёркнут) |
| Скролл | `scroll(...)`: резинка на концах, инерция, `.on_refresh(refreshing, f)`, `.indicator(Auto/Always/Never)`, `.stick_to_bottom()`; `.sticky()` у детей. Полоса прокрутки как в macOS: появляется при скролле и при наведении на правый край, под курсором расширяется и показывает дорожку; ползунок тащится мышью, клик по дорожке — страница к месту клика. Трекпад: инерция ОС, упёршись в край, даёт один отскок; пальцы на трекпаде держат растяжение |
| Длинные списки | `list(count, estimated_height, |i| row)` — строятся только видимые строки, высота строк любая. При смене ширины (ресайз окна) лента у низа остаётся у низа, иначе на месте остаётся верхняя видимая строка; строки, впервые измеренные выше экрана, растут вверх и не сдвигают видимое |
| Оверлеи | `modal` `sheet` `drawer` `snackbar` — последними в корневом `zstack` |
| Стекло | `.glass()` / `.glass_with(Glass)`; `Glass::regular()` `clear()` `frosted()` `dark()` `light()`; ручки 0..1: `.blur(x)` — размытие фона (0 — прозрачное), `.distortion(x)` — линза у кромки (0 — плоское), `.tint(цвет)` + `.opacity(a)` — цвет и плотность тонировки; `.strength(k)` масштабирует всё; поля `Glass` — тонкая настройка. Близкие панели слипаются сами. Демо — «Glass lab» в `examples/showcase.rs` |
| Адаптивность | `fn view(s: &S, env: &Env)`; `env.class()` → `Phone` / `Tablet` / `Desktop` |
| Время | `after(seconds, |s: &mut S| ...)` — таймер на состоянии (скрыть снекбар, закончить обновление) |
| Буфер обмена | `copy_text(текст)` из любого обработчика — кнопка «копировать» под сообщением; уходит как `WindowRequest::SetClipboard` (из тапа — сразу, иначе со следующим кадром) |
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

## Тема

Цвета и размеры встроенных виджетов — в одной структуре `Theme` с публичными полями
(`src/theme.rs`): `color` (bg, surface, raised, popover, text, text_muted, border, divider, accent,
on_accent, fill, fill_strong, hover, pressed, selection, focus, scrim, link, danger, success, warning),
`text` (large_title, title, headline, body, subhead, footnote, caption, caption2 — `TextStyle { size,
line_height, weight, tracking }`), `control` (отступы, радиусы, высоты, кольцо фокуса, скроллбар, тултип),
`shadow`, `motion` (пружины, задержка тултипа), `space`, `radius`, стекло таб-бара и пикера, `menu` /
`slider` (`None` — выводятся из палитры).

```rust
App::new(state, view).theme(Theme::light().accent(Color::hex(0x4D81EF)));      // одна тема
App::new(state, view).theme_with(|s: &State| if s.dark { Theme::dark() } else { Theme::light() }); // от состояния
App::new(state, view).theme_auto(Theme::light(), Theme::dark());               // за системой
ui.set_theme(Theme::light());                                                  // без окна

fn view(s: &State, env: &Env) -> View<State> {
    let t = &env.theme;
    vstack((
        text("Settings").style(t.text.title),                      // кегль, межстрочный, вес, разрядка разом
        text("Synced").style(t.text.footnote).color(t.color.text_muted),
        button("Save", save),                                       // accent, on_accent, отступы и радиус — из темы
        button("Delete", del).background(t.color.danger),           // модификатор на узле всегда побеждает тему
    ))
}
```

`Theme::dark()` — тема по умолчанию, совпадает с прежним видом (системные цвета iOS, контролы «под
палец»). `.accent(c)` красит кнопки, слайдеры, тумблеры, кольцо фокуса, каретку и выделение. Фон окна —
`theme.color.bg`, если не задан `App::background`. Свой стиль меню или слайдера от темы:
`MenuStyle { row_height: 24.0, ..MenuStyle::from(&*env.theme) }`, так же `SliderStyle::from`,
`MarkdownStyle::from`. Одна тема на окно: поддерево перекрашивают модификаторы. Решение —
`docs/decisions/005-theme.md`.

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
`SetClipboard(текст)`, `OpenUrl(url)`); `ime_area()` — где показать окно IME. Фича `clipboard` (по умолчанию) —
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
| `src/select.rs` | Выделение и копирование read-only текста (`.selectable()`), ссылки (подмодуль `ui`) |
| `src/popup.rs` | Слои над деревом: поповеры, меню с подменю и клавиатурой, контекстные меню, тултипы; шорткаты (подмодуль `ui`) |
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
  контекстного меню, двунаправленного текста сверх того, что даёт cosmic-text.
- Выделение текста в ленте (v1): только мышь — нет тач-выделения (long-press, ручки), протяжки по
  словам после двойного клика, Shift+стрелок; копируется только plain text (без rich/HTML); область —
  один `list` либо поддерево без списка (вложенные списки в одной области не поддержаны); картинки
  при выделении не подсвечиваются (но копируются через `.copy_as`); вставка строк в начало ленты
  сдвигает выделение (при уменьшении числа строк оно сбрасывается), стриминг текста под выделением
  может сдвинуть его край. В вебе текст для `copy` пересобирается только при смене выделения: если
  текст под неизменным выделением дописался, копируется прежний. `OpenUrl` на iOS/Android пока
  ничего не делает.
- У таблиц нет выравнивания колонок (`:---:`) и ширины по содержимому — только доли.
- Резинка и pull-to-refresh от трекпада работают только на macOS (нужны фазы жеста); в браузере и
  с мышью скролл останавливается на краях. Тач-перетаскивание работает везде.
- Нет accessibility, тем, горизонтального скролла, диффинга дерева.
- Сборки под iOS/Android/Windows/Linux не проверялись.
