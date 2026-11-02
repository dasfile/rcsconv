#!/usr/bin/env python3
"""Анализ payload'ов узлов (блок type 11) по пробным файлам ReCap.

Печатает по каждому узлу: число точек, размер payload'а, «накладные» байты
(payload − 16·N), их долю на точку и содержимое заголовка payload'а, разложенное
на 4-байтовые группы.

Примеры:
    python tools/analyze_payload.py "golden/probes/probes Support/*.rcs"
    python tools/analyze_payload.py "golden/probes/probes Support/grid_00016.rcs" --hex
"""

from __future__ import annotations

import argparse
import glob
import sys
from pathlib import Path

from rcs_blocks import RcsFormatError, iter_tiles, parse_file, tile_payload

BODY = 16


def header_groups(blob: bytes) -> list[str]:
    """Разложить заголовок payload'а на 4-байтовые группы.

    Формат групп пока не расшифрован; в эталонах подавляющее большинство групп
    выглядит как `XX 00 00 TT` — печатаем их как `value:tag`, остальные — hex.
    """
    groups: list[str] = []
    for start in range(0, len(blob) - 3, 4):
        chunk = blob[start : start + 4]
        if chunk[1] == 0 and chunk[2] == 0:
            groups.append(f"{chunk[0]}:{chunk[3]:02x}")
        else:
            groups.append(chunk.hex())
    rest = blob[len(blob) - len(blob) % 4 :]
    if rest:
        groups.append(rest.hex())
    return groups


def body_preview(blob: bytes) -> str:
    """Первое тело точки: x, y, z, байт[8], B, G, R."""
    if len(blob) < BODY:
        return "-"
    x, y, z = (int.from_bytes(blob[i : i + 2], "little") for i in (0, 2, 4))
    return f"x={x} y={y} z={z} b8=0x{blob[8]:02x} bgr=({blob[9]},{blob[10]},{blob[11]})"


def analyse(path: Path, show_hex: bool) -> list[dict]:
    container = parse_file(path)
    rows: list[dict] = []
    for tile in iter_tiles(container):
        payload = tile_payload(container, tile)
        overhead = tile.payload_size - tile.count * BODY
        rows.append(
            {
                "file": path.name,
                "node": tile.index,
                "count": tile.count,
                "payload": tile.payload_size,
                "overhead": overhead,
                "per_point": overhead / tile.count if tile.count else 0.0,
                "header_hex": payload[:overhead].hex(" "),
                "groups": header_groups(payload[:overhead]),
                "body": body_preview(payload[overhead:]) if tile.count else "-",
            }
        )
    return rows


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description="Анализ payload'ов узлов type 11")
    parser.add_argument("paths", nargs="+", help="файлы .rcs (допускаются маски)")
    parser.add_argument("--hex", action="store_true", help="печатать hex заголовка")
    parser.add_argument("--min-count", type=int, default=0, help="только узлы с N >= порога")
    args = parser.parse_args(argv)

    files: list[Path] = []
    for pattern in args.paths:
        matches = sorted(glob.glob(pattern))
        files.extend(Path(m) for m in matches) if matches else files.append(Path(pattern))

    print(f"{'файл':<26}{'узел':>5}{'N':>7}{'payload':>9}{'надб':>7}{'на точку':>10}")
    print("-" * 66)
    all_rows: list[dict] = []
    for path in files:
        try:
            rows = analyse(path, args.hex)
        except (OSError, RcsFormatError) as exc:
            print(f"{path}: ОШИБКА: {exc}", file=sys.stderr)
            continue
        for row in rows:
            if row["count"] < args.min_count:
                continue
            print(
                f"{row['file']:<26}{row['node']:>5}{row['count']:>7}"
                f"{row['payload']:>9}{row['overhead']:>7}{row['per_point']:>10.2f}"
            )
            if args.hex:
                print(f"    header: {row['header_hex']}")
                print(f"    groups: {' '.join(row['groups'])}")
                print(f"    body:   {row['body']}")
        all_rows.extend(rows)

    if all_rows:
        single = [row for row in all_rows if row["count"] == 1]
        if single:
            print(
                f"\nу узлов с 1 точкой накладные байты: "
                f"{sorted({row['overhead'] for row in single})}"
            )
        multi = sorted(
            (row for row in all_rows if row["count"] > 1),
            key=lambda row: row["count"],
        )
        if multi:
            print("N  →  надбавка (Б), надбавка/N:")
            for row in multi[:20]:
                print(f"{row['count']:>6}  {row['overhead']:>6}  {row['per_point']:>8.2f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
