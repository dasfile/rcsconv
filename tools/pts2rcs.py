#!/usr/bin/env python3
"""Сборка `.rcs` (ADOCT) из `.pts` — CLI.

Раскладка файла и ограничения формата — `docs/RCS-FORMAT.md`. Код разделён так,
чтобы новый формат входа добавлялся одним модулем чтения:

    tools/pts_read.py   чтение `.pts`              (этот конвертер)
    tools/las_read.py   чтение `.las/.laz`         (tools/las2rcs.py)
    tools/rcs_build.py  октодерево и сборка `.rcs` (про формат входа не знает)
    tools/rcs_cli.py    общий разбор аргументов и цикл по файлам

Схема узлов: дерево фиксированной глубины, в узле — до `-m` точек. По умолчанию
`-m` = auto = 65536 (предел формата: `index` записи заголовка payload'а — `u16`).
При таком пределе на `level_7` получается ровно 43 узла и глубина 3 — как в
`golden/level_7`; меньшее значение даёт более мелкое дерево и больший файл.

Примеры:
    python tools/pts2rcs.py golden/probe6.pts            # → probe6.rcs рядом
    python tools/pts2rcs.py golden -o out                # все *.pts каталога → out/
    python tools/pts2rcs.py "D:\\scan\\*.pts" -m 200       # мельче дерево
    python tools/pts2rcs.py golden/probe6.pts -o out -t golden/probe6.rcs \\
        --metadata template
"""

from __future__ import annotations

from pathlib import Path

from cloud import Point, StreamingSource
from pts_read import bounds, read_pts
from rcs_build import MAX_TILE_POINTS, build_from_source, describe
from rcs_cli import build_parser, run

__all__ = [
    "MAX_TILE_POINTS",
    "Point",
    "bounds",
    "build_output",
    "describe",
    "main",
    "read_pts",
    "source_of",
]


def source_of(path: str | Path) -> StreamingSource:
    """`.pts` как источник точек для `rcs_build.build_from_source`."""
    path = Path(path)
    return StreamingSource(path=path, bounds_fn=bounds, points_fn=read_pts)


def build_output(
    source: str | Path,
    template=None,
    *,
    guid: str | None = None,
    max_per_tile: int | None = None,
    metadata: str = "auto",
) -> tuple[bytes, dict]:
    """Собрать `.rcs` из `.pts` (совместимость с прежним API конвертера)."""
    return build_from_source(
        source_of(source),
        template,
        guid=guid,
        max_per_tile=max_per_tile,
        metadata=metadata,
    )


def main(argv: list[str] | None = None) -> int:
    parser = build_parser(
        label=".pts",
        description="Сборка .rcs из .pts (вход — файл, каталог или маска)",
        epilog="Примеры: pts2rcs.py cloud.pts | pts2rcs.py D:\\scan -o out "
        "| pts2rcs.py \"D:\\scan\\*.pts\" -m 200",
    )
    args = parser.parse_args(argv)
    return run(args, parser=parser, suffixes=(".pts",), label=".pts", make_source=source_of)


if __name__ == "__main__":
    raise SystemExit(main())
