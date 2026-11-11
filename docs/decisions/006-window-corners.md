# 006. Углы прозрачного окна с системным размытием (macOS 26)

## Что видно

`App::blur(true)` + `Titlebar::Overlay`: в углах окна между нашим содержимым и чёрной кромкой
системной обводки виден светлый «просвет» — радиусы разные.

## Причина (замерено на живом окне, кропы 2x)

- Маска окна macOS 26 режет наше содержимое по своему радиусу (~26–30 px при 2x, непрерывный угол),
  как у любого окна системы (референс — тот же радиус).
- `with_blur` в winit — это приватный `CGSSetWindowBackgroundBlurRadius`. Область размытия, а с ней
  тень и её тёмный ободок, WindowServer строит со **своим, меньшим** радиусом (~20 px при 2x).
- Между двумя кривыми — размытый фон без нашей тонировки: тот самый просвет. На прямых краях всё
  совпадает: чёрная линия + светлая внутренняя — штатная обводка тёмного окна.

Проверено: без `blur` обводка идёт точно по маске (просвета нет); `set_has_shadow(false/true)`,
повторный `set_blur` после показа окна и поздний `set_blur` форму не меняют (поздний ещё и прячет
содержимое). Изнутри рисования не исправить: просвет лежит вне нашей маски.

## Решение

В ядре без `unsafe` сделать нечего; оставляем `blur` как есть и документируем. Исправление —
отказаться от `CGSSetWindowBackgroundBlurRadius` и подложить под Metal-слой `NSVisualEffectView`,
он берёт форму и тень окна от системы:

```text
let NSView = window.raw_window_handle() -> AppKit(h).ns_view           // unsafe: сырой указатель
let vev = NSVisualEffectView::new(mtm)
vev.setMaterial(NSVisualEffectMaterial::UnderWindowBackground)  // или HUDWindow / Sidebar
vev.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow)
vev.setState(NSVisualEffectState::Active)
vev.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | ViewHeightSizable)
vev.setFrame(content_view.bounds())
content_view.addSubview_positioned_relativeTo(&vev, NSWindowOrderingMode::Below, None)
ns_window.invalidateShadow()
```

(macOS 26: альтернатива — `NSGlassEffectView` с `cornerRadius`.) Это `objc2-app-kit` и сырой
`ns_view` из raw-window-handle — `unsafe`, поэтому только в отдельной фиче/крейте платформы, если
решим её завести. Заодно это даёт настраиваемую силу размытия (материалы), которой у
`CGSSetWindowBackgroundBlurRadius` через winit нет.
