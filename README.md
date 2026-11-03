# rcsconv — обратимый конвертер Autodesk ReCap `.rcs`

Читатель и **писатель** проприетарного контейнера `.rcs` (сигнатура `ADOCT`,
эталонная версия ReCap 2020 / 6.0.0.110). Собирает `.rcs` **без ReCap и без
шаблона** — из `.pts`, `.las` или `.laz` — и файл открывается в Revit.

Проект вырос из реверса: спецификация формата (`docs/RCS-FORMAT.md`) и формат
проекта `.rcp` (`docs/RCP-FORMAT.md`) — такая же ценность, как код.

## Что умеет

| Вход | Выход | Команда |
|---|---|---|
| `.pts` (текст Leica, 3–7 колонок) | `.rcs` (Unified RCS, v5) | `tools/pts2rcs.py` |
| `.las` / `.laz` | `.rcs` | `tools/las2rcs.py` |
| `.rcs` | `.pts` | `tools/rcs2pts.py` |
| `.rcs` | проверка инвариантов, bbox, `type 1` | `tools/rcs_inspect.py`, `tools/validate_pts.py` |
| `.rcp` (проект ReCap) | настройки импорта из XML | `tools/rcp_info.py` |

Дополнительно: подвыборка `.pts`, LAS/LAZ → `.pts`, сравнение двух `.rcs`,
разбор узлов и метаданных, чек-лист корпуса.

**Автономный `rcsconv.exe` (Rust)** уже собирает `.rcs` из `.las/.laz` и `.pts` —
один статический файл 0.6 МБ, без Python и DLL, побайтовый паритет с Python-версией
(кроме GUID): [`docs/ROADMAP.md`](docs/ROADMAP.md). Чтение `.rcs` в нём — следующий шаг.

## Быстрый старт

```powershell
# .pts → .rcs и обратно (три маленьких пробы лежат в golden/)
python tools/pts2rcs.py golden/probe6.pts -o out           # → out/probe6.rcs
python tools/rcs2pts.py out/probe6.rcs -o out -c -i         # → out/probe6.pts
python tools/rcs_write.py out/probe6.rcs out/copy.rcs       # read → write, байт-в-байт
python tools/validate_pts.py golden/probe6.pts out/probe6.rcs -s

# LAS/LAZ → .rcs, без текстового посредника
python tools/las2rcs.py golden/level_7.laz -o out
python tools/las2rcs.py "golden/*.laz" -o out
python tools/las2rcs.py scan.las --units-scale 0.3048 --recenter center

# корпус и проекты
python tools/corpus_status.py golden -g golden              # что снято, count, узлы
python tools/validate_pts.py "golden/*.pts" -g golden       # сводка по всем парам
python tools/rcp_info.py "golden/*.rcp"                     # настройки импорта из XML
```

Правило CLI: первый аргумент — вход (файл, каталог или маска), имя выхода
подставляется само (`<имя>.rcs` / `<имя>.pts`), существующий файл не
перезаписывается без `-f`. Результаты пишите в `out/`.

Требования: Python 3.11 + `numpy`. `laspy` (+ бэкенд `laszip` или `lazrs`) нужен
только для LAS/LAZ: `pip install -e ".[las]"`.

Rust-порт собирается без MSVC — нужен GNU-хост `x86_64-pc-windows-gnu`
(установка тулчейна — ниже, `rustup` не обязателен):

```powershell
cd rust
cargo build --release      # → rust/target/release/rcsconv.exe (статический CRT)
cargo test                 # тесты крейтов (42)
```

Готовые команды `.exe` (то же поведение, что у скриптов: вход — файл, каталог или
маска, выход `<имя>.rcs`, без `-f` существующий файл не перезаписывается):

```powershell
rust\target\release\rcsconv.exe las2rcs golden\level_7.laz -o out
rust\target\release\rcsconv.exe las2rcs "golden\*.laz" -o out -f
rust\target\release\rcsconv.exe las2rcs scan.las --units-scale 0.3048 --recenter center
rust\target\release\rcsconv.exe pts2rcs golden\probe6.pts -o out
rust\target\release\rcsconv.exe --help
```

### Тулчейн: портативная установка, без `rustup`

`rustup` не обязателен: тулчейн ставится **копированием** распакованного архива,
профиль пользователя остаётся чистым (никаких `.rustup`/`.cargo` в `C:\Users\...`).

| Что | Где на машине владельца |
|---|---|
| Тулчейн: `rustc`, `cargo`, `std`, `rustc_driver-*.dll`, `libgcc_s_seh-1.dll`, `libwinpthread-1.dll` | `C:\mingw64\rust` (`bin\`, `lib\`, `libexec\`, `share\`, `etc\`) |
| Кеш крейтов (`registry\`, `.global-cache`) | `C:\mingw64\rust\cargo-home` — машинная переменная `CARGO_HOME` |
| Линкер | MinGW/WinLibs (gcc 16.2) — `C:\mingw64\bin`, уже в системном `PATH` |

Машинные переменные (действуют для всех пользователей и для новых процессов):

```powershell
# PATH: ... C:\mingw64\bin\;C:\mingw64\rust\bin; ...  ← rust\bin сразу после mingw64\bin
[Environment]::SetEnvironmentVariable('PATH', $newPath, 'Machine')
[Environment]::SetEnvironmentVariable('CARGO_HOME', 'C:\mingw64\rust\cargo-home', 'Machine')
```

Проверка после установки: `rustc -vV` → `host: x86_64-pc-windows-gnu`,
`rustc --print sysroot` → `C:\mingw64\rust`, `cargo test --workspace --offline` → 42 passed
(офлайн работает за счёт перенесённого кеша). `rust-toolchain.toml`
(`channel = "stable"`) читает только `rustup` — без него файл инертен, но нужен CI
(Ubuntu ставит `rustup` сам).

**Обновление Rust:** скачать `rust-<версия>-x86_64-pc-windows-gnu.tar.gz`
(≈533 МБ) и распаковать **поверх** `C:\mingw64\rust`; `cargo-home` не трогается.
То же самое можно сделать временной установкой `rustup`.

На другой машине годится и классический `rustup`: `rustup-init.exe --profile minimal`
из [rustup.rs](https://rustup.rs/), затем `rustup set default-host x86_64-pc-windows-gnu`
— иначе `cargo` возьмёт MSVC-тулчейн. `rust-toolchain.toml` специально не фиксирует
триплет, чтобы сборка работала и на Linux (CI).

`rust/target/` (артефакты сборки, ~300 МБ) — рабочая папка на C:, в git не входит,
чистится `cargo clean`.

## Как проверялось

Главное в проекте — не «размер файла совпал», а проверяемость:

- **85 тестов** (`python -m unittest discover -s tools/tests`); без данных часть
  классов скипается, а не падает;
- `read → write` даёт **побайтово идентичный** файл на эталонах;
- файлы, собранные `pts2rcs`, **открыты в Revit 2023** на 9 реальных облаках
  (0.3–4.0 млн точек), габариты сверены;
- на `level_7` блоки `type 1` (метаданные) и `type 0` (свойства) совпадают с
  эталоном ReCap **байт-в-байт**; из `.laz` получается то же дерево (43 узла, глубина 3);
- `export ⊂ raw` — экспорт Unified только прореживает импортированное облако
  (проверено на 1.07 млн точек);
- что **не** проверено — честно перечислено: линковка Revit-модели, `.e57`,
  точный механизм прореживания при экспорте (`docs/RCS-FORMAT.md` §12).

## Как это сделано

Проект сделан в паре «человек + ИИ-агент»: постановка задач, съёмка, данные и
приёмка в Revit — владелец проекта; код, анализ формата и тексты документации —
агент Cline (модель DeepSeek V4.1 Flash 1017) в VS Code.

Поэтому каждый факт о формате помечен **ПОДТВЕРЖДЕНО / ГИПОТЕЗА / ОПРОВЕРГНУТО**
и опирается на тест или скрипт (`docs/RCS-FORMAT.md` §0). В разделе «ОПРОВЕРГНУТО»
(§13) собраны в том числе ошибки раннего подхода 2025 года — чтобы их никто не
повторил; короткий пост-мортем — [`docs/LESSONS.md`](docs/LESSONS.md).

## Что внутри

```
tools/                 парсер, писатель, конвертеры, CLI
  cloud.py              точка и «источник точек» (общий протокол)
  pts_read.py           .pts            → pts2rcs.py
  las_read.py           .las/.laz       → las2rcs.py
  rcs_build.py          октодерево и сборка .rcs (о формате входа не знает)
  rcs_cli.py            общие аргументы и цикл по файлам
  rcs_blocks.py         контейнер: заголовок, директория, блоки
  rcs_points.py         тела точек (16 Б) и заголовок payload'а узла
  rcs_write.py          сериализация: собрать файл из блоков
  analyze_*.py          разбор `type 1`, заголовков, payload'ов
tools/tests/           85 тестов (unittest)
rust/                  Rust-порт (rcsconv.exe): rcs-format, rcs-cloud, cloud-pts, cloud-las, rcsconv
docs/                  RCS-FORMAT.md, RCP-FORMAT.md, CORPUS.md, HANDOVER.md, ROADMAP.md, LESSONS.md, GIT.md
golden/                данные и эталоны (в git НЕ входят)
out/                   результаты прогонов (в git НЕ входят)
```

Документация: **[PLAN.md](PLAN.md)** (статус, фазы, журнал) ·
**[docs/RCS-FORMAT.md](docs/RCS-FORMAT.md)** (спецификация `.rcs`) ·
**[docs/RCP-FORMAT.md](docs/RCP-FORMAT.md)** (формат `.rcp`) ·
**[docs/CORPUS.md](docs/CORPUS.md)** (корпус и эталоны) ·
**[docs/HANDOVER.md](docs/HANDOVER.md)** (точка входа) ·
**[docs/ROADMAP.md](docs/ROADMAP.md)** · **[docs/GIT.md](docs/GIT.md)**.

## Данные

В репозитории **нет** файлов, созданных Autodesk, и тяжёлых облаков: эталоны
снимаются локально в `golden/` (см. `golden/README.md` и `docs/CORPUS.md`).
`golden/`, `out/`, `dumps/` перечислены в `.gitignore`.

## Лицензия

MIT — см. [LICENSE](LICENSE). Проект не связан с Autodesk; ReCap, Revit и
соответствующие торговые марки принадлежат Autodesk.
