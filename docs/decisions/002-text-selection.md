# 002. Выделение и копирование текста в ленте, ссылки (план v1)

Статус: утверждено, не реализовано. Зависит от редактора поля ввода (общие примитивы в `text.rs`,
запрос `SetClipboard`) и от hover/курсоров (`Zone`, `ui.cursor`).

## Модель
- Выделение одно на `Ui`: `sel: Option<Selection { area: u64, anchor: TextPos, focus: TextPos }>`.
  Фокус поля ввода его сбрасывает, и наоборот. `area` — `child_id` контейнера с `.selectable()`.
- `TextPos { row: u32, node: u32, byte: u32 }`, лексикографический `Ord`. `row` — индекс строки
  ленивого `list` в области (0 без списка), `node` — номер `Kind::Text` в строке в порядке дерева,
  `byte` — индекс в склеенном тексте узла (при использовании зажимается до границы символа).
- id узлов как ключи не годятся: по ним нет порядка, а у невидимых строк их нет вовсе.
- Порядок покраски ≠ порядок дерева (sticky, flex). Поэтому `LNode.tn` — число текстовых узлов
  поддерева, считается в `layout()` снизу вверх; в paint база ребёнка = база родителя + сумма `tn`
  предыдущих братьев.
- Область: `.selectable()` на контейнере; корень `markdown()` ставит сам; при вложении побеждает
  внешняя. v1: область — один `list` либо поддерево без списка.
- Невидимые строки в кадре не строятся. Подсветка — только у построенных узлов. Cmd+A = MIN..MAX.
- Копирование: по Cmd+C `Ui` один раз зовёт `view(&S,&Env)`, находит область и вызывает `row(i)`
  только для `anchor.row..=focus.row`; текст собирается обходом дерева `View` без layout/paint.
- Plain text: разделитель по ближайшему общему стеку — V `"\n"`, H `" "`, между строками списка
  `"\n\n"`; `.copy_sep()` переопределяет; `.copy_as(s)` заменяет текст поддерева и делает его
  атомарным (маркер `"- "`, картинка, `""` — исключить).

## API
```rust
impl<S> View<S> {
    pub fn selectable(self) -> Self;
    pub fn copy_as(self, s: impl Into<String>) -> Self;
    pub fn copy_sep(self, s: &'static str) -> Self;
    pub fn on_link(self, f: impl Fn(&mut S, &str) + 'static) -> Self; // на поддерево
}
impl Span { pub fn link(self, url: impl Into<String>) -> Self; }
impl<S> Ui<S> { pub fn set_modifiers(&mut self, m: Mods); pub fn selection_text(&self) -> Option<String>; }
// KeyInput += Copy, SelectAll; WindowRequest += SetClipboard(String), OpenUrl(String)
list(n, 80.0, |i| bubble(markdown(&msgs[i].text))).selectable().on_link(|s, url| s.open(url))
```
Без `on_link` клик по ссылке даёт `WindowRequest::OpenUrl` (только http/https/mailto).

## По файлам
- `text.rs` — общие с редактором примитивы: `hit(key,x,y) -> usize`, `caret(key,i) -> (x,y,h)`,
  `range_rects(key, Range, &mut Vec<Rect>)`, `word_at`, `para_at`.
- `view.rs` — `Style { select, copy_as, copy_sep }`, `Span.link` (не входит в ключ шейпинга),
  `View.on_link`, `Mods { shift, command, alt }`.
- `layout.rs` — `LNode.tn`; `collect_text(&View, id, range, &mut String)` — третий обход дерева,
  обязан совпадать с layout и paint.
- `ui.rs` — `Painter { sel_area, sel_row, ord, link_fn }`; контейнер кладёт
  `Hit { Action::Select { area } }` до детей (кнопки остаются выше по Z); `Kind::List` ставит
  `sel_row = i, ord = 0`; `Kind::Text` кладёт `TextBox { area, pos, key, origin, scale, rect }` в
  `Ui.texts` и красит квады подсветки до текста; ссылки — `Zone` с `Cursor::Pointer`,
  `Hit { Action::Link }`, линия 1px при hover. Клики 1/2/3 — символ/слово/абзац, Shift+клик двигает
  `focus`. Нажатие на ссылке, ушедшее дальше 4pt, становится выделением. Точка мимо текста
  привязывается к ближайшему `TextBox`. Автоскролл при протяжке ближе 24pt к краю (`animating`).
- `markdown.rs` — корень `.selectable().copy_sep("\n\n")`; пункты одного списка в своём `vstack`;
  маркер `.copy_as("- ")`/`"1. "`; ячейка таблицы копируется с `"\t"`; `Tag::Link` → `Span.link`.
- `app.rs` — модификаторы, Cmd/Ctrl+C/A, `OpenUrl` (native `open`/`xdg-open`/`cmd /c start`, web
  `window.open`), `SetClipboard`.

## Шаги (каждый с тестом на `Ui` без окна)
1. Примитивы `text.rs` (hit/caret взаимно обратны, range_rects на переносе, word_at на кириллице).
2. `.selectable()` на одном узле: протяжка, двойной/тройной клик, Shift+клик; `selection_text()`.
3. Несколько узлов: `tn`, разделители, `copy_as`/`copy_sep`; тест со sticky на порядок.
4. Ленивый список: выделение через строки, якорь за экраном, Cmd+A → Copy → `SetClipboard`;
   число вызовов `row` за обычный кадр не растёт.
5. Автоскролл. 6. Ссылки. 7. `markdown.rs`, кадр, `app.rs`, web.

## Риски и что не делаем в v1
- Вставка строк в начало ленты сдвигает `row`: при уменьшении `count` выделение сбрасывается.
- Стриминг меняет текст под выделением: спасает зажим байта, возможен скачок.
- Не делаем: тач (long-press, ручки), протяжку по словам, Shift+стрелки в read-only, вложенные
  списки в области, rich-копирование, BiDi сверх `highlight`.

## Отвергнуто
- Ключи — id узлов: нет порядка и невидимых строк.
- `.copy_text()` от приложения: частичные строки не сопоставить с исходным markdown.
- Хранить plain text всех строк в `Ui`: память и инвалидация при стриминге.
- Один буфер на сообщение: ломает «markdown = обычные view».
