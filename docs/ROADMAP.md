# Дорожная карта ruru

Живой файл: обновляй при завершении задачи и перед концом сессии. Подробности — в `docs/decisions/` и README
(«Известные проблемы и чего пока нет»). Открытые запросы приложения — `GromAgent/docs/FRAMEWORK_GAPS.md`.

## Сделано

- **Ядро**: view/layout/ui, wgpu-рендер, стекло с преломлением и слиянием, картинки, SVG-иконки, контролы,
  скролл с резинкой/refresh/sticky, ленивый список, оверлеи, адаптивность через `Env`, cargo-фичи, headless.
- **Канал фон → состояние**: `spawn`, `Link::send`, `Task`, `sleep`, пробуждение цикла; стрим ответа в демо
  (решение 001). HTTP-клиента в ядре нет — он на стороне приложения.
- **Окно**: прозрачность, системный блюр, overlay-заголовок, `window_drag`, отступы `env.titlebar`.
- **Базовые модификаторы**: `.clip()`, `.lines()/.truncate()`, hover и курсоры, min/max размеры, `.animate()`.
- **Редактор текста**: multiline, выделение, буфер обмена, undo, IME (десктоп), `on_submit`, `plain`.
- **Типографика**: `line_height`, `letter_spacing`, вес и размер спанов, плашки, иконки в строке, `flex`,
  `markdown_styled`, системный шрифт, табличные цифры, хинтинг выключен с 2x.
- **Выделение текста и ссылки**: `.selectable()`, выделение через узлы и ленту, автоскролл, `Span::link`,
  `OpenUrl` (решение 002).
- **Слои**: поповеры, меню, контекстное меню, тултипы, шорткаты, `steps` у слайдера (решение 004);
  стили меню и слайдера, слоистые тени.
- **Полировка**: физика скролла у края, ресайз ленты без прыжков, круговые углы и `.smooth()`, рамка поверх
  детей, стекло `dark/light/blur/distortion/opacity`, нативный скроллбар, `copy_text`.
- **Аудит фундамента (решение 003)**: пакеты 1–6 — углы, пиксельная сетка (`Ui.scale`), тема, тени,
  текст в один буфер с LRU, типографика. `Theme` (решение 005): `App::theme`, `theme_auto`, `env.theme`.

- **Grom integration**: text-tail cursor/morph, per-grapheme blur reveal, cube companion,
  hover marquee/offset, focus-select and disabled controls, native file dialogs/drop and voice,
  explicit/system appearance, stable lazy-scroll restoration, measured `Scroll.bottom_overlay`.
- **Picker**: shared text/theme metrics for control and menu; measured label/chevron centering,
  configurable padding, compact rows, keyboard and geometry regressions.

## В работе

- **Ускорение рендера** (пакет 7 аудита, ветка `gpu-perf`): пропуск неизменившегося кадра, стекло по области.
  Приёмка: кадр со стеклом ≤ 1.2 мс в бенче, картинка попиксельно прежняя.
- **Пропадающие буквы в WebGPU-сборке WebKit**: обход через `copyBufferToTexture` стоит; в настоящем Safari
  и на нагруженном сценарии не проверено (человек).

## Следующее (по приоритету)

2. **Якорь скролла при вставке в начало ленты** — видимая строка должна стоять на месте; то же для выделения.
3. **Групповая прозрачность** (offscreen-слои) — полупрозрачный контейнер сейчас просвечивает детьми.
4. **IME и экранная клавиатура на web** — нужно скрытое DOM-поле; без него мобильный браузер непригоден для чата.
5. **Выделение на тач-экранах** — long-press и ручки; без них нет выделения на iOS/Android.
6. **Мемоизация поддеревьев** — `view` вызывается целиком на каждый кадр; сначала замеры на 1k/10k узлов.
7. **Accessibility** (AccessKit) — требование платформ, архитектурное решение сначала в `docs/`.
8. **Горизонтальный скролл** — для кода и таблиц.

### После ревью интеграции

- Picker и generic Menu ещё имеют отдельные paint paths; объединять вместе с keyboard-navigation tests.
- Перед оптимизацией больших историй измерить пересборку списка (O(total rows)) и подсветку больших
  code blocks; текущая lazy-отрисовка не означает O(visible rows) для всей layout-процедуры.
- Welcome-layout приложения всё ещё оценивает текст для скрытия приветствия в маленьком окне;
  нижняя панель обычного чата уже использует фактически измеренную высоту.

### Windows / Linux (аудит 15.11.2026, ветка `platform`)

Сделано: `cargo check` lib+examples+tests для `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, wasm —
без ошибок и предупреждений; wgpu получает display handle (GLES на Wayland) и `WGPU_BACKEND` из env;
непрозрачное окно явно просит `CompositeAlphaMode::Opaque`; `App::app_id` (Wayland app_id / X11 WM_CLASS).
Ничего не запускалось на настоящих Windows/Linux. По приоритету:

1. **Буфер обмена на чистом Wayland** — arboard без `wayland-data-control`; GNOME не реализует wlr-data-control,
   значит нужен буфер через соединение winit (smithay-clipboard) или portal. Проверка: GNOME/KDE Wayland без XWayland.
2. **Нет GPU-адаптера → panic** (`expect` в `init_gfx`): повтор с `force_fallback_adapter` (WARP/lavapipe),
   затем понятная ошибка вместо падения. Проверка: Windows RDP/VM без драйвера, Linux без Vulkan.
3. **Snap Layouts Windows 11** на своей кнопке maximize при `Titlebar::Overlay` — нужен `WM_NCHITTEST → HTMAXBUTTON`
   (сабклассинг окна, `unsafe` вне `macos.rs` → сначала запись в `docs/decisions/`).
4. **Открытие ссылок на Windows через `explorer <url>`**: explorer режет аргумент по запятым — нужен `ShellExecuteW`
   (тот же вопрос про `unsafe`). Проверка: ссылка с `,` в markdown.
5. **Иконка окна** (`App::icon` → `with_window_icon`, Windows/X11; Wayland берёт её из `.desktop` по `app_id`).
6. **Меню и стандартные шорткаты вне macOS**: `native_menu` там пустой; Ctrl+Q/Ctrl+W не обрабатываются.
   Решить: внутриоконное меню или карта `NativeMenuAction` → клавиши.
7. **Файловые диалоги** синхронные и без родительского окна (native-dialog); на Linux нужен Zenity/KDialog/YAD —
   перейти на xdg-desktop-portal, на Windows передавать HWND владельца.
8. **Системный шрифт Linux** — фиксированный список (Cantarell → Ubuntu → Noto → DejaVu), а не fontconfig/gsettings;
   эмодзи/CJK держатся на фолбэке cosmic-text и установленных Noto. Проверка: минимальный Debian/Arch.
9. **Дробный масштаб 125/150 %** (пиксельная сетка) и IME (XIM / text-input-v3, Windows TSF) — только живой прогон.

## Позже

- Сборки Windows, Linux, iOS, Android (тач, безопасные зоны, клавиатура); резинка трекпада вне macOS.
- Мультиокна.
- Упаковка приложения: `.app`, иконка, подпись.
- Разбиение на крейты `ruru-core` / `ruru-wgpu` / `ruru-app`, когда API устоится; публикация.
- Таблицы: выравнивание колонок; подсветка синтаксиса в коде; `split_view`.
- Свой рендер текста вместо glyphon — только если баг WebKit не решится иначе.
