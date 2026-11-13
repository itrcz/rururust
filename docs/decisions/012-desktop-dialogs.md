# Выбор файлов и папок на desktop

Windows использует IFileOpenDialog через native-dialog 0.9.7, Linux — установленный
Zenity, KDialog или YAD через тот же backend. macOS сохраняет существующий NSOpenPanel.
Зависимость optional, активируется фичей app только на Windows/Linux: нет GTK в линковке,
нет новой зависимости для WASM/ядра. Собственный unsafe/COM-код не добавляется.

API pick_files сохраняет Result и множественный выбор. Новый try_pick_folder отличает
отмену от сбоя, старый pick_folder сохраняет совместимость. Ошибки backend необходимо
показывать пользователю: отсутствие утилиты не должно выглядеть как отмена.

Linux-пакет должен требовать zenity; альтернативы — kdialog/yad в PATH. Нужна графическая
сессия X11/Wayland. Portal-only sandbox (Flatpak/Snap) пока не поддерживается; для него
нужен отдельный portal backend. Диалоги синхронные: вызываются только из UI-действия,
как существующий macOS API. Нет файлового/сетевого ввода-вывода при построении View.

Backend: https://github.com/native-dialog-rs/native-dialog-rs

## Проверка

`cargo run --example file_dialogs -- files` открывает множественный выбор,
`-- folder` — папку; результат печатается в stdout. Используйте временные fixtures.
На Windows backend проверен через Wine 11.0: два файла одновременно, пробелы и кириллица
в имени, выбор папки, отмена обоих диалогов. Отмена возвращает пустой список / None.
Внешний вид диалога просмотрен в Wine; тема и интеграция настоящей Windows требуют
отдельного прогона. Grom `cargo xwin check` прошёл; WASM check прошёл.
Linux backend проверен в Debian bookworm x86_64, Xvfb + Zenity: множественный выбор
с пробелами и кириллицей, папка, отмена обоих диалогов, MissingDep без backend в PATH.
Для кириллицы необходима UTF-8 locale (например LANG=C.UTF-8): Zenity в C/POSIX
locale заменяет не-ASCII символы в stdout на вопросительные знаки. Backend библиотеки
не предоставляет настройки окружения дочернего процесса; launcher должен обеспечить
UTF-8 locale. Portal backend и поддержка запуска в C/POSIX locale остаются отдельными задачами.

Web пересобран и открыт в Chromium/WebKit: canvas отрисовывается, ошибок JS нет,
снимки просмотрены. Core check сохраняет существующее предупреждение ImageData.
