#!/usr/bin/env python3
"""CLI: сводка по контейнеру RCS + проверка инвариантов.

Примеры:
    python tools/rcs_inspect.py golden/reference/probe6_orig.rcs
    python tools/rcs_inspect.py golden/reference/probe6_orig.rcs --pts golden/probe6.pts
    python tools/rcs_inspect.py golden/reference/*.rcs
    python tools/rcs_inspect.py golden/reference/level_7_orig.rcs --json
    python tools/rcs_inspect.py golden/reference/probe6_orig.rcs --dump 64
"""

from __future__ import annotations

import argparse
import glob
import sys
from pathlib import Path

from rcs_blocks import RcsFormatError, parse_file, read_block
from rcs_checks import pts_stats
from rcs_report import summary, to_json, to_text


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Разбор контейнера RCS (ADOCT) и проверка инвариантов"
    )
    parser.add_argument("paths", nargs="+", help="файлы .rcs (допускаются маски)")
    parser.add_argument(
        "--pts",
        metavar="FILE",
        help="сверить заголовок с исходным .pts (bbox, count)",
    )
    parser.add_argument("--json", action="store_true", help="вывод в JSON")
    parser.add_argument(
        "--dump",
        type=int,
        metavar="N",
        default=0,
        help="hex-дамп первых N байт каждого блока",
    )
    return parser


def expand(patterns: list[str]) -> list[Path]:
    files: list[Path] = []
    for pattern in patterns:
        matches = sorted(glob.glob(pattern))
        if not matches:
            files.append(Path(pattern))
        else:
            files.extend(Path(match) for match in matches)
    return files


def hexdump(data: bytes, limit: int) -> str:
    rows = []
    for offset in range(0, min(len(data), limit), 16):
        chunk = data[offset : offset + 16]
        text = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        rows.append(f"    {offset:04X}  {chunk.hex(' '):<47}  {text}")
    return "\n".join(rows)


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    args = build_parser().parse_args(argv)

    stats = None
    if args.pts:
        try:
            stats = pts_stats(args.pts)
        except (OSError, ValueError) as exc:
            print(f"Ошибка чтения .pts: {exc}", file=sys.stderr)
            return 2

    failures = 0
    for path in expand(args.paths):
        try:
            container = parse_file(path)
        except (OSError, RcsFormatError) as exc:
            print(f"{path}: ОШИБКА: {exc}", file=sys.stderr)
            failures += 1
            continue

        report = summary(container, stats)
        if args.json:
            print(to_json(report))
        else:
            print(to_text(report))
            if args.dump:
                for block in container.blocks:
                    print(
                        f"  -- dump block type {block.type} "
                        f"@0x{block.offset:X} (первые {args.dump} Б)"
                    )
                    print(hexdump(read_block(container, block), args.dump))
            print()
        if not report["ok"]:
            failures += 1

    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
