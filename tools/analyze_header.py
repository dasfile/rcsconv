#!/usr/bin/env python3
"""Оффлайн-анализ заголовка payload'а узла (внутри блока type 11).

Заголовок = первые `payload_size - 16·count` байт payload'а узла. ПОДТВЕРЖДЕНО
на 88 узлах (экспорт `level_7`, raw `level_7 Support`, `attr_intensity`):

  * длина заголовка всегда кратна 4 → он состоит из 4-байтовых записей;
  * запись = `(u16 index, u8 length, u8 kind)` (LE), `length` кратен 8,
    `kind` ∈ 0…8;
  * записей с `kind == 8` ровно `count`, их `index` — перестановка `0…count-1`.

Что остаётся открытым и что проверяет этот скрипт:
  * назначение записей с `kind` 0…7 (`length` — в байтах? в битах? смещение?);
  * связь `index` в записях `kind == 8` с порядком тел точек в payload'е.

Примеры:
    python tools/analyze_header.py golden/level_7.rcs
    python tools/analyze_header.py golden/attr_intensity_import1.rcs --dump 0
    python tools/analyze_header.py "golden/* Support/*.rcs" --kinds
"""

from __future__ import annotations

import argparse
import glob
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

from rcs_blocks import RcsFormatError, iter_tiles, parse_file, tile_payload

BODY = 16                 # байт на тело точки (подтверждено)
RECORD_SIZE = 4           # байт на запись заголовка (подтверждено)
POINT_KIND = 8            # kind записи «на точку» (подтверждено)


@dataclass(frozen=True)
class Record:
    """Запись заголовка: 4 байта `(u16 index, u8 length, u8 kind)`."""

    index: int
    length: int
    kind: int


def header_bytes(container, tile) -> bytes:
    """Срез заголовка payload'а узла (всё, что перед телами точек)."""
    payload = tile_payload(container, tile)
    overhead = tile.payload_size - tile.count * BODY
    if overhead < 0:
        raise RcsFormatError(
            f"узел {tile.index}: payload {tile.payload_size} < {tile.count}×{BODY}"
        )
    return payload[:overhead]


def parse_records(header: bytes) -> list[Record]:
    """Разложить заголовок на 4-байтовые записи."""
    if len(header) % RECORD_SIZE:
        raise RcsFormatError(f"длина заголовка {len(header)} не кратна {RECORD_SIZE}")
    return [
        Record(*struct.unpack_from("<HBB", header, offset))
        for offset in range(0, len(header), RECORD_SIZE)
    ]


# --------------------------------------------------------------------------- #
#  Инварианты
# --------------------------------------------------------------------------- #

def check_node(tile, records: list[Record]) -> list[tuple[str, bool, str]]:
    """Проверить структурные инварианты заголовка одного узла."""
    checks: list[tuple[str, bool, str]] = []
    point_records = [record for record in records if record.kind == POINT_KIND]

    checks.append(
        (
            "записей kind 8 ровно count",
            len(point_records) == tile.count,
            f"{len(point_records)} == {tile.count}",
        )
    )
    indices = sorted(record.index for record in point_records)
    checks.append(
        (
            "index записей kind 8 — перестановка 0…count-1",
            indices == list(range(tile.count)),
            f"уникальных {len(set(indices))} из {tile.count}",
        )
    )
    bad_lengths = [record for record in records if record.length % 8]
    checks.append(
        ("length кратен 8", not bad_lengths, f"нарушений {len(bad_lengths)}")
    )
    bad_kinds = [record for record in records if record.kind > POINT_KIND]
    checks.append(
        (f"kind ∈ 0…{POINT_KIND}", not bad_kinds, f"нарушений {len(bad_kinds)}")
    )
    out_of_range = [
        record
        for record in records
        if record.kind != POINT_KIND and record.index >= tile.count
    ]
    checks.append(
        (
            "index записей kind 0…7 < count",
            not out_of_range,
            f"нарушений {len(out_of_range)}"
            + (f" (макс {max(r.index for r in out_of_range)})" if out_of_range else ""),
        )
    )


    return checks


# --------------------------------------------------------------------------- #
#  Вывод
# --------------------------------------------------------------------------- #

def histogram(values) -> dict[int, int]:
    result: dict[int, int] = {}
    for value in values:
        result[value] = result.get(value, 0) + 1
    return dict(sorted(result.items()))


def describe_node(tile, records: list[Record], verbose: bool) -> None:
    kinds = histogram(record.kind for record in records)
    others = [record for record in records if record.kind != POINT_KIND]
    length_sum = sum(record.length for record in others)
    print(
        f"node {tile.index:2d} N={tile.count:6d} hdr={len(records) * RECORD_SIZE:7d} "
        f"recs={len(records):6d} M={len(records) - tile.count:6d} "
        f"kinds={kinds} sum_len(k!=8)={length_sum}"
    )
    if verbose:
        print(f"   lengths: {histogram(record.length for record in records)}")
        for kind in sorted({record.kind for record in others}):
            subset = [record for record in others if record.kind == kind]
            print(
                f"   kind {kind}: n={len(subset):5d} "
                f"index {min(r.index for r in subset)}..{max(r.index for r in subset)} "
                f"length={histogram(r.length for r in subset)}"
            )
    for name, ok, detail in check_node(tile, records):
        if not ok:
            print(f"   !!! {name}: {detail}")


def dump_node(container, tile, records: list[Record]) -> None:
    """Полный дамп записей узла + первые тела точек."""
    print(f"--- узел {tile.index}: N={tile.count}, записей {len(records)}")
    for number, record in enumerate(records):
        print(
            f"{number:6d}  index={record.index:6d}  "
            f"length={record.length:4d}  kind={record.kind}"
        )
    payload = tile_payload(container, tile)
    overhead = tile.payload_size - tile.count * BODY
    for number in range(min(tile.count, 16)):
        start = overhead + number * BODY
        body = payload[start : start + BODY]
        x, y, z = (int.from_bytes(body[i : i + 2], "little") for i in (0, 2, 4))
        print(
            f"body {number:6d}  x={x:5d} y={y:5d} z={z:5d} "
            f"i={body[8]:3d} bgr=({body[9]},{body[10]},{body[11]})"
        )


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Анализ заголовков payload'ов узлов (type 11)"
    )
    parser.add_argument("paths", nargs="+", help="файлы .rcs (допускаются маски)")
    parser.add_argument("--kinds", action="store_true", help="разбивка записей по kind")
    parser.add_argument("--dump", type=int, metavar="NODE", help="дамп записей узла")
    parser.add_argument("--max-nodes", type=int, default=0, help="ограничить число узлов")
    args = parser.parse_args(argv)

    files: list[Path] = []
    for pattern in args.paths:
        matches = sorted(glob.glob(pattern))
        if matches:
            files.extend(Path(match) for match in matches)
        else:
            files.append(Path(pattern))

    total_nodes = 0
    violations = 0
    for path in files:
        try:
            container = parse_file(path)
        except (OSError, RcsFormatError) as exc:
            print(f"{path}: ОШИБКА: {exc}", file=sys.stderr)
            continue
        tiles = iter_tiles(container)
        dialect = "raw" if container.header.chunk_version == 2 else "export"
        print(f"=== {path} | {dialect} | узлов {len(tiles)}")
        for tile in tiles[: args.max_nodes or len(tiles)]:
            try:
                records = parse_records(header_bytes(container, tile))
            except RcsFormatError as exc:
                print(f"node {tile.index}: ОШИБКА: {exc}")
                violations += 1
                continue
            total_nodes += 1
            violations += sum(1 for _, ok, _ in check_node(tile, records) if not ok)
            if args.dump is not None and tile.index == args.dump:
                dump_node(container, tile, records)
            else:
                describe_node(tile, records, args.kinds)

    print(f"\nвсего узлов: {total_nodes}, нарушений инвариантов: {violations}")
    return 0 if violations == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
