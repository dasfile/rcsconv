#!/usr/bin/env python3
"""Оффлайн-анализ блока метаданных (`type 1`): слова заголовка и число записей.

Блок = 128 байт заголовка (32 × u32) + `K` записей по 72 байта.

ПОДТВЕРЖДЕНО на 13 эталонах (4 экспорта `golden/*.rcs`, 5 raw
`golden/* Support/*.rcs`, 4 `golden/reference/*.rcs`):

  * непустые слова заголовка образуют **префикс** (после первого нуля только нули);
  * `words[L]` — число **занятых узлов октодерева на уровне `L`**:
    `words[0] = 1` (корень), последнее слово = число листьев = число записей
    блока `type 3`; внутренние узлы в `type 3` не хранятся, а здесь посчитаны;
  * `K = (size − 128) / 72 = sum(words)` — полное число узлов октодерева;
  * все `K` записей идентичны (пустой диапазон атрибута, `±DOUBLE_MAX`).

Открытым остаётся смысл полей **самих записей** — во всех доступных файлах они
одинаковы, поэтому отличить «запись на узел» от «запись на уровень» нельзя.

Примеры:
    python tools/analyze_type1.py golden/level_7.rcs
    python tools/analyze_type1.py "golden/*.rcs" "golden/reference/*.rcs"
"""

from __future__ import annotations

import argparse
import glob
import math
import struct
import sys
from pathlib import Path

from rcs_blocks import Container, RcsFormatError, iter_tiles, parse_file

HEADER_SIZE = 128         # байт заголовка блока type 1 (32 × u32)
WORD_COUNT = 32           # u32 в заголовке блока type 1
RECORD_SIZE = 72          # байт на запись (подтверждено)
EDGE_TOLERANCE = 1e-9     # допуск сверки ребра корневого куба с ребром узла


# --------------------------------------------------------------------------- #
#  Разбор блока
# --------------------------------------------------------------------------- #

def metadata_block_bytes(container: Container) -> bytes:
    """Содержимое блока `type 1`."""
    block = container.block_of_type(1)
    if block is None:
        raise RcsFormatError("нет блока type 1")
    return container.raw[block.offset : block.end]


def header_words(raw: bytes) -> list[int]:
    """Значащие слова заголовка: всё до последнего ненулевого (включительно).

    ПОДТВЕРЖДЕНО: слова образуют префикс — внутри значащей части нулей нет.
    """
    if len(raw) < HEADER_SIZE:
        raise RcsFormatError(f"блок type 1 короче {HEADER_SIZE} байт: {len(raw)}")
    words = list(struct.unpack_from(f"<{WORD_COUNT}I", raw, 0))
    last = -1
    for number, word in enumerate(words):
        if word:
            last = number
    if last < 0:
        return []
    prefix = words[: last + 1]
    if 0 in prefix:
        zeros = [number for number, word in enumerate(prefix) if not word]
        raise RcsFormatError(f"слова заголовка type 1 не префикс: нули в {zeros}")
    return prefix


def metadata_records(raw: bytes) -> list[bytes]:
    """Записи блока (по 72 байта). Бросает, если размер не `128 + 72·K`."""
    body = len(raw) - HEADER_SIZE
    if body < 0 or body % RECORD_SIZE:
        raise RcsFormatError(
            f"размер блока type 1 {len(raw)} не раскладывается в 128 + 72·K"
        )
    return [
        raw[HEADER_SIZE + index * RECORD_SIZE : HEADER_SIZE + (index + 1) * RECORD_SIZE]
        for index in range(body // RECORD_SIZE)
    ]


# --------------------------------------------------------------------------- #
#  Октодерево: сколько узлов занято на каждом уровне
# --------------------------------------------------------------------------- #

def octree_depth(container: Container) -> int:
    """Глубина октодерева: `2R / ребро узла` — степень двойки."""
    tiles = iter_tiles(container)
    if not tiles:
        return 0
    half = container.header.root_max[0]
    edge = tiles[0].cube_max[0] - tiles[0].cube_min[0]
    if edge <= 0:
        raise RcsFormatError(f"ребро узла 0: {tiles[0].cube_min}..{tiles[0].cube_max}")
    for tile in tiles:
        other = tile.cube_max[0] - tile.cube_min[0]
        if abs(other - edge) > EDGE_TOLERANCE * edge:
            raise RcsFormatError(
                f"узел {tile.index}: ребро {other} не равно ребру корня {edge}"
            )
    ratio = 2.0 * half / edge
    if ratio < 1.0:
        raise RcsFormatError(
            f"ребро узла {edge} больше корневого куба 2R = {2.0 * half}"
        )
    depth = round(math.log2(ratio))
    if abs(2.0 ** depth * edge - 2.0 * half) > EDGE_TOLERANCE * 2.0 * half:
        raise RcsFormatError(
            f"2R = {2.0 * half} не делится на ребро узла {edge} нацело"
        )
    return depth


def octree_counts(container: Container) -> list[int]:
    """Число занятых узлов октодерева на каждом уровне `0…depth`.

    Считается по записям `type 3`: листья проецируются вверх по уровням,
    родитель уровня `L` — это `cell >> (depth − L)`.
    """
    tiles = iter_tiles(container)
    if not tiles:
        return []
    half = container.header.root_max[0]
    edge = tiles[0].cube_max[0] - tiles[0].cube_min[0]
    depth = octree_depth(container)
    cells: list[tuple[int, int, int]] = []
    for tile in tiles:
        cell = tuple(
            int(round((tile.cube_min[axis] + half) / edge)) for axis in range(3)
        )
        if not all(0 <= value < (1 << depth) for value in cell):
            raise RcsFormatError(f"узел {tile.index}: ячейка {cell} вне куба 2^{depth}")
        cells.append(cell)  # type: ignore[arg-type]
    counts: list[int] = []
    for level in range(depth + 1):
        shift = depth - level
        counts.append(len({tuple(value >> shift for value in cell) for cell in cells}))
    return counts


# --------------------------------------------------------------------------- #
#  Инварианты
# --------------------------------------------------------------------------- #

def check_container(container: Container) -> list[tuple[str, bool, str]]:
    """Проверить инварианты блока `type 1` одного файла."""
    checks: list[tuple[str, bool, str]] = []
    raw = metadata_block_bytes(container)
    try:
        words = header_words(raw)
        records = metadata_records(raw)
    except RcsFormatError as exc:
        return [("блок разбирается", False, str(exc))]

    checks.append(
        (
            "размер = 128 + 72·K",
            len(raw) == HEADER_SIZE + RECORD_SIZE * len(records),
            f"{len(raw)} = 128 + 72·{len(records)}",
        )
    )
    try:
        derived = octree_counts(container)
    except RcsFormatError as exc:
        checks.append(("октодерево разбирается", False, str(exc)))
        derived = []
    if derived:
        checks.append(
            (
                "слова = занятые узлы по уровням",
                words == derived,
                f"{words} против {derived}",
            )
        )
        checks.append(
            (
                "K = сумма слов = число узлов октодерева",
                len(records) == sum(words) == sum(derived),
                f"K={len(records)}, сумма слов={sum(words)}, узлов={sum(derived)}",
            )
        )
    checks.append(
        (
            "все записи идентичны",
            len(set(records)) <= 1,
            f"уникальных {len(set(records))} из {len(records)}",
        )
    )
    return checks


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Анализ блока метаданных (type 1): слова заголовка и K"
    )
    parser.add_argument("paths", nargs="+", help="файлы .rcs (допускаются маски)")
    args = parser.parse_args(argv)

    files: list[Path] = []
    for pattern in args.paths:
        matches = sorted(glob.glob(pattern))
        if matches:
            files.extend(Path(match) for match in matches)
        else:
            files.append(Path(pattern))

    violations = 0
    for path in files:
        try:
            container = parse_file(path)
        except (OSError, RcsFormatError) as exc:
            print(f"{path}: ОШИБКА: {exc}", file=sys.stderr)
            violations += 1
            continue
        dialect = "raw" if container.header.chunk_version == 2 else "export"
        tiles = iter_tiles(container)
        print(f"=== {path} | {dialect} | узлов {len(tiles)}")
        try:
            raw = metadata_block_bytes(container)
            words = header_words(raw)
            records = metadata_records(raw)
            derived = octree_counts(container)
        except RcsFormatError as exc:
            print(f"  ОШИБКА: {exc}")
            violations += 1
            continue
        print(f"  слова {tuple(words)}, K={len(records)}, размер {len(raw)} Б")
        if derived:
            print(f"  узлы по уровням из type 3: {tuple(derived)}")
        for name, ok, detail in check_container(container):
            print(f"  [{'OK  ' if ok else 'FAIL'}] {name}: {detail}")
            violations += 0 if ok else 1

    print(f"\nфайлов: {len(files)}, нарушений: {violations}")
    return 0 if violations == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())

