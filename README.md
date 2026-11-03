# RCS reverse engineering (ADOCT / Autodesk ReCap)

Рабочее место для реверса контейнера `.rcs` (Autodesk ReCap) с целью получить
обратимый читатель и писатель, проверяемый самим Revit.

- План, фазы и журнал: **[PLAN.md](PLAN.md)**
- **Спецификация `.rcs`**: **[docs/RCS-FORMAT.md](docs/RCS-FORMAT.md)**
- **Формат `.rcp`** (проект ReCap): **[docs/RCP-FORMAT.md](docs/RCP-FORMAT.md)**
- Корпус сканов и эталоны: **[docs/CORPUS.md](docs/CORPUS.md)**, **[golden/README.md](golden/README.md)**
- Передача в новый диалог: **[docs/HANDOVER.md](docs/HANDOVER.md)**
- Почему подход 2025 года не работал: **[docs/LEGACY-WRONG.md](docs/LEGACY-WRONG.md)**
- Правила работы: **[.clinerules](.clinerules)**

## Структура

```
tools/               парсер, писатель, конвертеры, CLI (основной код)
  cloud.py             точка и «источник точек» (общий протокол)
  pts_read.py          чтение .pts        → pts2rcs.py
  las_read.py          чтение .las/.laz   → las2rcs.py
  rcs_build.py         октодерево и сборка .rcs (о формате входа не знает)
  rcs_cli.py           общие аргументы и цикл по файлам
tools/tests/         85 тестов (unittest)
docs/                спецификации (.rcs, .rcp), корпус, handover
golden/              ВСЕ данные: .laz/.pts, проекты .rcp, эталоны .rcs (в git НЕ попадает)
  golden/lazimport/    вторая ветка: импорт напрямую из .laz
  golden/reference/    *_orig.rcs — побайтовое доказательство писателя
out/                 результаты генерации (в git НЕ попадает)
dumps/               эксперименты и дампы (в git НЕ попадает)
```

`golden/`, `out/`, `dumps/` перечислены в `.gitignore`: там лежат данные Autodesk
и наши результаты, они снимаются/создаются локально.

## Данные и тесты

Данные — в `golden/` (`golden/README.md`): 10 реальных облаков из iPhone, их `.laz`,
проекты `.rcp`, экспорты Unified RCS, raw `Support/*.rcs`, три маленьких пробы
(`probe6/test1/test2`) и `reference/*_orig.rcs`.

```powershell
python -m unittest discover -s tools/tests          # 85 тестов
```

Без `golden/` тесты, которым нужны эталоны, **скипаются** (`SkipTest`), а не падают —
это нормальное состояние репозитория без данных.

## Быстрый старт

```powershell
# --- .pts → .rcs, сверка, разбор (три пробы лежат в golden/) ---
python tools/pts2rcs.py golden/probe6.pts -o out           # → out/probe6.rcs
python tools/rcs_inspect.py out/probe6.rcs --pts golden/probe6.pts
python tools/rcs2pts.py out/probe6.rcs -o out -c -i         # → out/probe6.pts
python tools/validate_pts.py golden/probe6.pts out/probe6.rcs -s
python tools/rcs_write.py out/probe6.rcs out/copy.rcs       # read → write, байт-в-байт
python tools/analyze_type1.py out/probe6.rcs

# --- LAS/LAZ → .rcs напрямую (без текстового посредника) ---
python tools/las2rcs.py golden/level_7.laz -o out          # → out/level_7.rcs
python tools/las2rcs.py "golden/*.laz" -o out              # пачка
python tools/las2rcs.py scan.las --units-scale 0.3048 --recenter center
python tools/validate_pts.py golden/level_7.pts out/level_7.rcs -s

# --- корпус (реальные сканы) ---
python tools/corpus_status.py golden -g golden              # что снято, count, узлы
python tools/pts2rcs.py "golden/*.pts" -o out               # пачка
python tools/validate_pts.py "golden/*.pts" -g golden       # bbox/type 1/count
python tools/rcs_compare.py golden/probe6.rcs out/probe6.rcs  # отличия по интервалам
python tools/analyze_header.py golden/level_7.rcs --kinds   # kind 0…7 в payload'ах
python tools/analyze_payload.py golden/level_7.rcs --hex

# --- проекты ReCap (.rcp = ZIP + XML) ---
python tools/rcp_info.py golden/probe6.rcp                  # настройки импорта
python tools/rcp_info.py "golden/*.rcp" --json
python tools/rcp_info.py golden/level_7.rcp --entries

# --- импорт и подвыборка ---
python tools/las2pts.py "golden/*.laz" -o out/pts           # LAS/LAZ → .pts (laspy)
python tools/pts_subset.py golden/level_7.pts -n 20000 -o out/subset
```

CLI-правило: первый аргумент — вход (файл, каталог или маска), имя выхода утилита
подставляет сама (`<имя>.rcs` / `<имя>.pts`), существующий выход не перезаписывается
без `-f`. **Пишите результаты в `out/`**, `golden/` — read-only.

Зависимости: Python 3.11 + стандартная библиотека; `laspy` (+ бэкенд `laszip` или
`lazrs`) — только для `tools/las2pts.py`. Остальное работает без них.

## Статус

- `.rcs`: читатель и писатель готовы, спецификация выпущена; приёмка — **Revit 2023**
  на 9 реальных облаках (0.3–4.0 млн точек). Писатель собирает файл **без ReCap и без
  шаблона**; `read → write` побайтово идентичен на 13 эталонах.
- Входы: `.pts` (`pts2rcs`) и **`.las/.laz`** (`las2rcs`, без текстового посредника).
  На `level_7.laz` получается то же дерево, что у ReCap: 43 узла, глубина 3,
  блоки `type 1` и `type 0` — байт-в-байт как в `golden/level_7.rcs`.
- `.rcp`: контейнер и XML-модель разобраны (`docs/RCP-FORMAT.md`), из проекта
  читаются фактические настройки импорта, включая `Decimation grid`.
- Открытые вопросы («на понимание», не блокируют работу) — `docs/RCS-FORMAT.md` §12.

## Происхождение и лицензии

- В git попадают только **наш код** (`tools/`), **наша документация** (`docs/`,
  `PLAN.md`, `README.md`) и правила (`.clinerules`).
- `.rcs`/`.rcp`/`.laz`/`.pts` — данные, созданные Autodesk и камерой; они лежат
  локально в `golden/` и в git не входят.
- Формат `.rcs` восстановлен по поведению программы (интероперабельность);
  SDK Autodesk не использовался.
