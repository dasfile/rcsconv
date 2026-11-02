#!/usr/bin/env python3
"""CLI: извлечь точки из `.rcs` в `.pts` (вход — файл, каталог или маска).

Имя выхода утилита делает сама: то же имя с расширением `.pts` (рядом с входом
или в `-o`).

Формат вывода — как у сканеров (Leica Cyclone):
  * `-c/--count-header` — первой строкой число точек (так приходят `.pts`
    из `output_tests/test_4`); без флага шапки нет, как в `golden/*.pts`;
  * `-i/--intensity` — 7 колонок `x y z intensity r g b`, иначе 6 `x y z r g b`.

Точки пишутся потоково, по узлам октодерева: порядок внутри узла исходный,
узлы идут в порядке обхода. Сквозной порядок точек не сохраняется — в RCS
нет идентификатора точки.

Примеры:
    python tools/rcs2pts.py golden/level_7.rcs           # → golden/level_7.pts
    python tools/rcs2pts.py golden -o out                # все *.rcs каталога → out/
    python tools/rcs2pts.py golden/level_7.rcs -c -i     # как скан: счётчик + 7 колонок
"""

from __future__ import annotations

import argparse
import sys
from typing import TextIO

from cli_paths import default_target, expand_sources, has_suffix, is_same_file
from rcs_blocks import Container, RcsFormatError, iter_tiles, parse_file
from rcs_points import decode_tile


def write_pts(
    container: Container,
    handle: TextIO,
    *,
    local: bool = False,
    intensity: bool = False,
    count_header: bool = False,
) -> int:
    """Записать точки контейнера в `.pts`; вернуть число точек."""
    center = (0.0, 0.0, 0.0) if local else container.header.center
    tiles = iter_tiles(container)
    if count_header:
        handle.write(f"{sum(tile.count for tile in tiles)}\n")
    written = 0
    for tile in tiles:
        for point in decode_tile(container, tile):
            line = (
                f"{point.x + center[0]:.6f} {point.y + center[1]:.6f} "
                f"{point.z + center[2]:.6f} "
            )
            if intensity:
                line += f"{point.intensity} {point.r} {point.g} {point.b}\n"
            else:
                line += f"{point.r} {point.g} {point.b}\n"
            handle.write(line)
            written += 1
    return written


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="RCS -> PTS (вход — файл, каталог или маска)",
        epilog="Примеры: rcs2pts.py cloud.rcs | rcs2pts.py D:\\scan -o out "
        "| rcs2pts.py cloud.rcs -c -i",
    )
    parser.add_argument(
        "sources", nargs="+", help="входные .rcs: файл, каталог или маска"
    )
    parser.add_argument(
        "-o", "--out-dir", help="куда писать (по умолчанию рядом с входом)"
    )
    parser.add_argument(
        "-f", "--force", action="store_true", help="перезаписывать существующий .pts"
    )
    parser.add_argument(
        "-r", "--recursive", action="store_true", help="обойти подкаталоги"
    )
    parser.add_argument(
        "-c",
        "--count-header",
        action="store_true",
        help="первой строкой писать число точек (формат сканера Leica)",
    )
    parser.add_argument(
        "-i",
        "--intensity",
        action="store_true",
        help="писать 7 колонок: x y z intensity r g b",
    )
    parser.add_argument(
        "--local",
        action="store_true",
        help="писать локальные координаты (без смещения центра из заголовка)",
    )
    parser.add_argument(
        "--stdout",
        action="store_true",
        help="писать в stdout (только для одного входа)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = build_parser()
    args = parser.parse_args(argv)

    sources = expand_sources(args.sources, ".rcs", recursive=args.recursive)
    pts_sources = [path for path in sources if has_suffix(path, ".pts")]
    if pts_sources:
        print(
            f"Ошибка: {pts_sources[0]} — это .pts, а rcs2pts читает .rcs. "
            "Обратное направление — pts2rcs.py",
            file=sys.stderr,
        )
        return 2
    if not sources:
        print("Ошибка: входные .rcs не найдены", file=sys.stderr)
        return 2
    if args.stdout and len(sources) > 1:
        print("Ошибка: --stdout возможен только для одного входа", file=sys.stderr)
        return 2

    written = skipped = failed = 0
    for source in sources:
        try:
            container = parse_file(source)
        except (OSError, RcsFormatError, ValueError) as exc:
            print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
            failed += 1
            continue

        if args.stdout:
            count = write_pts(
                container,
                sys.stdout,
                local=args.local,
                intensity=args.intensity,
                count_header=args.count_header,
            )
            print(f"{source}: {count} точек → stdout", file=sys.stderr)
            written += 1
            continue

        target = default_target(source, ".pts", args.out_dir)
        if target.exists() and not args.force:
            print(f"{source} → {target}: пропущено (файл есть, есть -f)")
            skipped += 1
            continue
        if is_same_file(source, target):
            print(f"{source}: выход совпадает с входом", file=sys.stderr)
            failed += 1
            continue
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("w", encoding="utf-8", newline="\n") as handle:
                count = write_pts(
                    container,
                    handle,
                    local=args.local,
                    intensity=args.intensity,
                    count_header=args.count_header,
                )
        except (OSError, RcsFormatError, ValueError) as exc:
            print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
            failed += 1
            continue
        print(f"{source} → {target}: {count} точек")
        written += 1

    if len(sources) > 1:
        print(f"итого: записано {written}, пропущено {skipped}, ошибок {failed}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())


if __name__ == "__main__":
    raise SystemExit(main())
