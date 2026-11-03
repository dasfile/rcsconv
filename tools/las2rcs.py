#!/usr/bin/env python3
"""Сборка `.rcs` (ADOCT) из LAS/LAZ — CLI.

Один инструмент на `.las` и `.laz` (laspy читает оба; бэкенд `laszip` или `lazrs`).
Чтение — `tools/las_read.py`, октодерево и сборка — `tools/rcs_build.py`,
аргументы и цикл — `tools/rcs_cli.py`. Формат результата — `docs/RCS-FORMAT.md`.

Особенности LAS: цвет и интенсивность 16-битные (→ 8 бит, `auto` = ÷257 если
максимум > 255), координаты могут быть в футах или в UTM (`--units-scale`,
`--recenter`).

Примеры:
    python tools/las2rcs.py golden/level_7.laz              # → level_7.rcs рядом
    python tools/las2rcs.py "golden/*.laz" -o out           # пачка → out/
    python tools/las2rcs.py scan.laz -o out --recenter center
    python tools/las2rcs.py scan.las --units-scale 0.3048   # футы → метры
"""

from __future__ import annotations

from pathlib import Path

from las_read import LasOptions, source as las_source
from rcs_cli import build_parser, run

__all__ = ["main", "options_from"]


def options_from(args) -> LasOptions:
    """Собрать `LasOptions` из аргументов CLI."""
    return LasOptions(
        units=args.units_scale,
        recenter=args.recenter,
        color=not args.no_color,
        intensity=not args.no_intensity,
        color_scale=args.color_scale,
        intensity_scale=args.intensity_scale,
    )


def main(argv: list[str] | None = None) -> int:
    parser = build_parser(
        label=".las/.laz",
        description="Сборка .rcs из LAS/LAZ (вход — файл, каталог или маска)",
        epilog="Примеры: las2rcs.py scan.laz | las2rcs.py D:\\scan -o out "
        "| las2rcs.py scan.las --units-scale 0.3048 --recenter center",
    )
    parser.add_argument(
        "--units-scale",
        type=float,
        default=1.0,
        help="множитель координат (футы → метры: 0.3048)",
    )
    parser.add_argument(
        "--recenter",
        choices=("none", "center", "min"),
        default="none",
        help="вычесть центр/минимум bbox (для больших координат UTM)",
    )
    parser.add_argument(
        "--no-color", action="store_true", help="не читать цвет LAS"
    )
    parser.add_argument(
        "--no-intensity", action="store_true", help="не читать интенсивность LAS"
    )
    parser.add_argument(
        "--color-scale",
        choices=("auto", "257", "1"),
        default="auto",
        help="16-битный цвет → 8 бит: auto (÷257 если max > 255), 257, 1",
    )
    parser.add_argument(
        "--intensity-scale",
        choices=("auto", "257", "1"),
        default="auto",
        help="16-битная интенсивность → 8 бит: auto, 257, 1",
    )
    args = parser.parse_args(argv)
    if args.units_scale <= 0:
        parser.error("--units-scale должен быть > 0")

    options = options_from(args)
    return run(
        args,
        parser=parser,
        suffixes=(".las", ".laz"),
        label=".las/.laz",
        make_source=lambda path: las_source(Path(path), options),
    )


if __name__ == "__main__":
    raise SystemExit(main())
