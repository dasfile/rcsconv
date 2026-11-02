#!/usr/bin/env python3
"""Сравнить два .rcs побайтово и по структуре.

Печатает: размеры, отличия в заголовке по полям, отличия в таблице блоков и
сводку различающихся диапазонов байт. Отдельно отвечает на вопрос
«различаются только GUID?» — это эталон детерминированности ReCap.

Примеры:
    python tools/rcs_compare.py golden/probe6.rcs golden/reference/probe6_orig.rcs
    python tools/rcs_compare.py golden/attr_intensity_run1.rcs "golden/probes/probes Support/attr_intensity.rcs"
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from rcs_blocks import RcsFormatError, dialect, parse_file


def diff_ranges(left: bytes, right: bytes, limit: int = 20) -> tuple[int, list[tuple[int, int]]]:
    total = 0
    ranges: list[tuple[int, int]] = []
    start = previous = None
    for index in range(min(len(left), len(right))):
        if left[index] != right[index]:
            total += 1
            if start is None:
                start = previous = index
            elif index == previous + 1:
                previous = index
            else:
                ranges.append((start, previous))
                start = previous = index
    if start is not None:
        ranges.append((start, previous))
    return total, ranges[:limit]


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description="Сравнение двух .rcs")
    parser.add_argument("left")
    parser.add_argument("right")
    args = parser.parse_args(argv)

    try:
        a = parse_file(args.left)
        b = parse_file(args.right)
    except (OSError, RcsFormatError) as exc:
        print(f"Ошибка: {exc}", file=sys.stderr)
        return 2

    print(f"слева : {Path(args.left).name}  {a.size} Б, {dialect(a)}, v{a.header.chunk_version}")
    print(f"справа: {Path(args.right).name}  {b.size} Б, {dialect(b)}, v{b.header.chunk_version}")

    fields = (
        "word4", "word8", "word12", "center", "unit", "center2",
        "root_min", "root_max", "extent_min", "extent_max",
        "chunk_id", "chunk_version", "count", "flags", "guid",
    )
    changed = [name for name in fields if getattr(a.header, name) != getattr(b.header, name)]
    print("заголовок: " + ("поля совпадают" if not changed else f"различаются: {changed}"))

    if a.block_types() != b.block_types():
        print(f"блоки: типы {a.block_types()} vs {b.block_types()}")
    elif [blk.size for blk in a.blocks] != [blk.size for blk in b.blocks]:
        print(f"блоки: размеры {[blk.size for blk in a.blocks]} vs {[blk.size for blk in b.blocks]}")
    else:
        print(f"блоки: структура совпадает ({a.block_types()})")

    if a.size != b.size:
        print("размеры файлов различаются — побайтовое сравнение только по общему префиксу")

    total, ranges = diff_ranges(a.raw, b.raw)
    if total == 0 and a.size == b.size:
        print("байты: файлы идентичны")
        return 0

    print(f"байты: различается {total} из {min(a.size, b.size)}")
    for start, end in ranges:
        print(f"  0x{start:06X}…0x{end:06X} ({end - start + 1} Б)")

    guid_a = (a.header.guid_offset, a.header.guid_end)
    guid_b = (b.header.guid_offset, b.header.guid_end)
    if a.size == b.size and total and guid_a == guid_b:
        only_guid = all(guid_a[0] <= start and end < guid_a[1] for start, end in ranges)
        print("вердикт: " + ("различается только GUID" if only_guid else "есть отличия вне GUID"))
        return 0 if only_guid else 1
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
