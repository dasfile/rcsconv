#!/usr/bin/env python3
"""Октодерево и сборка `.rcs` из любого источника точек (`cloud.CloudSource`).

Модуль не знает, откуда точки: `.pts`, `.las/.laz` или что-то ещё. Формат
результата — Unified RCS (диалект v5), см. `docs/RCS-FORMAT.md` §4–§9.

Схема: дерево фиксированной глубины, в узле — до `max_per_tile` точек
(по умолчанию 65 536 — предел формата, `index` записи заголовка payload'а `u16`).
"""

from __future__ import annotations

import math
import struct
from typing import Iterable

from cloud import Bounds, CloudSource, Point
from rcs_blocks import Header, RcsFormatError
from rcs_write import (
    GUID_OFFSET,
    METADATA_HEAD_SIZE,
    METADATA_RECORD_SIZE,
    build_file,
    build_metadata_block,
    build_properties_block,
    make_guid,
    normalize_guid,
)

TILE_EDGE = 2.25          # ребро плитки, м (одинаково во всех golden-эталонах)
BASE_HALF = 2.25          # краевая полуширина корневого куба
MAX_DEPTH = 20
# Предел точек в узле задан форматом: запись заголовка payload'а — (u16 index, u8
# length, u8 kind), т.е. index = 0…65535 → не больше 65536 тел в узле (§4.2).
# ReCap сам пишет узлы и больше (до 192 402 точек), но там `index` повторяются.
MAX_TILE_POINTS = 1 << 16
BBOX_ROUND = 3            # bbox узла округляется до 1 мм
DEFAULT_CHUNK_ID = 131    # @0xD0 у всех наших эталонов (у старого level_7_orig — 11)
AXIS_SCALE = (1000, 4000, 16000)   # единиц поля на 1 м по осям x, y, z


def root_half(extent: tuple[float, float, float]) -> float:
    """Полуширина корневого куба: 2.25·2^k, минимальная покрывающая bbox."""
    need = max(abs(value) for value in extent)
    half = BASE_HALF
    while half < need - 1e-9:
        half *= 2.0
    return half


def recap_depth(half: float) -> int:
    """Глубина, на которой ребро плитки равно 2.25 м (как делает ReCap)."""
    return max(0, int(round(math.log2((2.0 * half) / TILE_EDGE))))


def cell_of(local: tuple[float, float, float], half: float, depth: int) -> tuple[int, int, int]:
    """Индекс ячейки октодерева на данной глубине (0…2^depth-1 по каждой оси)."""
    edge = (2.0 * half) / (1 << depth)
    return tuple(  # type: ignore[return-value]
        min((1 << depth) - 1, max(0, int((local[axis] + half) // edge))) for axis in range(3)
    )


def subtree_key(cell: tuple[int, int, int], depth: int) -> tuple[int, ...]:
    """Ключ DFS-порядка: индексы октантов от корня к листу (x + 2y + 4z)."""
    key = []
    for level in range(depth, 0, -1):
        mask = 1 << (level - 1)
        x = 1 if cell[0] & mask else 0
        y = 1 if cell[1] & mask else 0
        z = 1 if cell[2] & mask else 0
        key.append(x + 2 * y + 4 * z)
    return tuple(key)


def choose_depth(
    source: CloudSource, center, half: float, max_per_tile: int = 1
) -> int:
    """Увеличивать глубину, пока в ячейке не больше `max_per_tile` точек."""
    depth = recap_depth(half)
    while depth <= MAX_DEPTH:
        seen: dict[tuple[int, int, int], int] = {}
        collision = False
        for point in source.points():
            local = (point.x - center[0], point.y - center[1], point.z - center[2])
            cell = cell_of(local, half, depth)
            seen[cell] = seen.get(cell, 0) + 1
            if seen[cell] > max_per_tile:
                collision = True
                break
        if not collision:
            return depth
        depth += 1
    raise RcsFormatError("не удалось развести точки по ячейкам — есть дубликаты")


def level_counts(cells: Iterable[tuple[int, int, int]], depth: int) -> list[int]:
    """Число занятых узлов октодерева на каждом уровне `0…depth`.

    ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §4.3, `tools/analyze_type1.py`): именно эти
    числа ReCap пишет в заголовок блока `type 1`, а их сумма — число 72-байтовых
    записей блока. Родитель уровня `L` для листа `cell` — это `cell >> (depth − L)`.
    """
    cells = list(cells)
    if not cells:
        return []
    counts: list[int] = []
    for level in range(depth + 1):
        shift = depth - level
        counts.append(len({tuple(value >> shift for value in cell) for cell in cells}))
    return counts


def encode_body(point: Point, center, cube_min) -> bytes:
    """16-байтовое тело точки.

    ПОДТВЕРЖДЕНО на golden/probe6: дельта до `cube_min` сначала округляется до
    1 мм, затем умножается на масштаб оси (x — 1 мм, y — ¼ мм, z — 1/16 мм).
    """
    local = (point.x - center[0], point.y - center[1], point.z - center[2])
    raw = tuple(
        int(round(round(local[axis] - cube_min[axis], 3) * AXIS_SCALE[axis]))
        for axis in range(3)
    )
    if not all(0 <= value <= 0xFFFF for value in raw):
        raise RcsFormatError(f"координата не влезает в u16: {raw}")
    return (
        struct.pack("<3H", *raw)
        + b"\x00\x00"
        + bytes((point.intensity, point.b, point.g, point.r))
        + b"\x00\x00\x00\x00"
    )


def build_tile_block(
    points_by_cell: dict[tuple[int, int, int], list[Point]],
    center,
    half: float,
    depth: int,
) -> tuple[bytes, bytes]:
    """Собрать блок type 3 и блок type 11 (payload'ы узлов)."""
    edge = (2.0 * half) / (1 << depth)
    records = bytearray()
    payloads = bytearray()
    for cell in sorted(points_by_cell, key=lambda item: subtree_key(item, depth)):
        points = points_by_cell[cell]
        cube_min = tuple(-half + index * edge for index in cell)
        cube_max = tuple(value + edge for value in cube_min)
        locals_ = [
            (point.x - center[0], point.y - center[1], point.z - center[2])
            for point in points
        ]
        bbox_min = tuple(
            round(min(local[axis] for local in locals_), BBOX_ROUND) for axis in range(3)
        )
        bbox_max = tuple(
            round(max(local[axis] for local in locals_), BBOX_ROUND) for axis in range(3)
        )
        bodies = b"".join(encode_body(point, center, cube_min) for point in points)
        # Заголовок payload'а: по записи (index, 0, 8) на точку — см. RCS-FORMAT §4.2.
        head = b"".join(
            struct.pack("<HBB", number, 0, 8) for number in range(len(points))
        )
        payload = head + bodies

        records += struct.pack(
            "<12d", *cube_min, *cube_max, *bbox_min, *bbox_max
        )
        header = (1, len(points), len(payload), 0, 0, 0, 0, 0)
        records += struct.pack("<8I", *header)
        tail = [0] * 60
        tail[27] = len(points)      # подтверждено на одноточечных узлах
        records += struct.pack("<60I", *tail)
        payloads += payload

    block3 = struct.pack("<Q", len(points_by_cell)) + bytes(records)
    return block3, bytes(payloads)


def build_from_source(
    source: CloudSource,
    template=None,
    *,
    guid: str | None = None,
    max_per_tile: int | None = None,
    metadata: str = "auto",
) -> tuple[bytes, dict]:
    """Собрать `.rcs` из источника точек. Шаблон не нужен: все блоки генерируются.

    `max_per_tile` — предел точек в узле (наш выбор глубины дерева). `None` (по
    умолчанию) = `auto` = `MAX_TILE_POINTS` (65536, предел формата по `u16 index`):
    дерево дробится только если иначе файл станет невалидным.

    `metadata`: `auto`/`generated` — блок `type 1` из октодерева (слова = занятые
    узлы по уровням, записей = сумма слов, как пишет ReCap); `minimal` — вырожденный
    `(1,)` + 1 запись; `template` — взять блок из `template` (побайтовое
    воспроизведение эталона).
    """
    if max_per_tile is None:
        max_per_tile = MAX_TILE_POINTS
    if not 1 <= max_per_tile <= MAX_TILE_POINTS:
        raise RcsFormatError(
            f"max_points_per_tile={max_per_tile}: допустимо 1…{MAX_TILE_POINTS} "
            "(в узле не может быть больше тел, чем вмещает u16 index)"
        )
    mode = "generated" if metadata == "auto" else metadata
    if mode == "template" and template is None:
        raise RcsFormatError("режим metadata='template' требует шаблон")
    if mode not in ("template", "generated", "minimal"):
        raise RcsFormatError(f"неизвестный режим metadata: {mode!r}")

    low, high, count = source.bounds()
    center = (
        (low[0] + high[0]) / 2.0,
        (low[1] + high[1]) / 2.0,
        (low[2] + high[2]) / 2.0,
    )
    extent = tuple(
        round((high[axis] - low[axis]) / 2.0, BBOX_ROUND) for axis in range(3)
    )
    half = root_half(extent)
    depth = choose_depth(source, center, half, max_per_tile)

    points_by_cell: dict[tuple[int, int, int], list[Point]] = {}
    for point in source.points():
        local = (point.x - center[0], point.y - center[1], point.z - center[2])
        points_by_cell.setdefault(cell_of(local, half, depth), []).append(point)

    block3, points_block = build_tile_block(points_by_cell, center, half, depth)

    words: tuple[int, ...] = ()
    if mode == "template":
        source_block = template.block_of_type(1) if template else None
        if source_block is None:
            raise RcsFormatError("в шаблоне нет блока type 1")
        metadata_block = template.raw[source_block.offset : source_block.end]
    elif mode == "minimal":
        metadata_block = build_metadata_block()
    else:
        words = tuple(level_counts(points_by_cell, depth))
        metadata_block = build_metadata_block(words)

    chunk_id = template.header.chunk_id if template is not None else DEFAULT_CHUNK_ID
    guid_text = normalize_guid(guid) if guid else make_guid()
    header = Header(
        word4=84,
        word8=3,
        word12=2,
        center=center,
        zeros28=(0.0, 0.0, 0.0),
        unit=(1.0, 1.0, 1.0),
        center2=center,
        root_min=(-half, -half, -half),
        root_max=(half, half, half),
        extent_min=tuple(-value for value in extent),
        extent_max=extent,
        chunk_id=chunk_id,
        chunk_version=5,
        count=count,
        flags=b"\x01\x01\x00",
        guid=guid_text,
        guid_offset=GUID_OFFSET,
    )
    blocks = [
        (3, block3),
        (1, metadata_block),
        (11, points_block),
        (0, build_properties_block()),
    ]
    blob = build_file(header, blocks, guid=guid_text)
    stats = {
        "points": count,
        "tiles": len(points_by_cell),
        "depth": depth,
        "tile_edge": (2.0 * half) / (1 << depth),
        "root_half": half,
        "center": center,
        "extent": extent,
        "metadata": mode,
        "metadata_words": words,
        "metadata_records": (len(metadata_block) - METADATA_HEAD_SIZE)
        // METADATA_RECORD_SIZE,
        "max_points_per_tile": max(
            (len(points) for points in points_by_cell.values()), default=0
        ),
        "max_per_tile": max_per_tile,
        "size": len(blob),
    }
    return blob, stats


def describe(source, target, stats: dict) -> str:
    """Одна строка отчёта по файлу."""
    words = stats["metadata_words"]
    metadata = (
        f"{stats['metadata']} {'/'.join(map(str, words))} ({stats['metadata_records']} записей)"
        if words
        else f"{stats['metadata']} ({stats['metadata_records']} записей)"
    )
    return (
        f"{source} → {target}: точек {stats['points']}, узлов {stats['tiles']} "
        f"(глубина {stats['depth']}, ребро {stats['tile_edge']:.3f} м), "
        f"макс. {stats['max_points_per_tile']}/узел, "
        f"{stats['size'] / 1e6:.2f} МБ, type 1: {metadata}"
    )
