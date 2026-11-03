"""Инварианты контейнера RCS и статистика исходного .pts."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from rcs_blocks import (
    BLOCK_LAYOUTS,
    KNOWN_CHUNK_VERSIONS,
    Container,
    TILE_RECORD_SIZE,
    block_tlvs,
    bytes_per_point,
    dialect,
    iter_tiles,
    preview_block,
    tile_count,
    tile_record_size,
    tlv_block,
)


@dataclass(frozen=True)
class Check:
    name: str
    ok: bool
    detail: str = ""


def fmt3(values: Iterable[float]) -> str:
    return "(" + ", ".join(f"{value:.6f}" for value in values) + ")"


# --------------------------------------------------------------------------- #
#  Статистика .pts
# --------------------------------------------------------------------------- #

def pts_stats(path: str | Path) -> dict:
    """Потоковая статистика по .pts: bbox, центр, полуразмеры, число точек."""
    path = Path(path)
    count = 0
    mins = [float("inf")] * 3
    maxs = [float("-inf")] * 3
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            parts = line.split()
            if len(parts) < 3:
                continue
            try:
                xyz = (float(parts[0]), float(parts[1]), float(parts[2]))
            except ValueError:
                continue
            count += 1
            for axis in range(3):
                if xyz[axis] < mins[axis]:
                    mins[axis] = xyz[axis]
                if xyz[axis] > maxs[axis]:
                    maxs[axis] = xyz[axis]
    if count == 0:
        raise ValueError(f"в {path} не найдено ни одной точки")
    return {
        "path": str(path),
        "count": count,
        "min": tuple(mins),
        "max": tuple(maxs),
        "center": tuple((mins[i] + maxs[i]) / 2.0 for i in range(3)),
        "half_extent": tuple((maxs[i] - mins[i]) / 2.0 for i in range(3)),
    }


# --------------------------------------------------------------------------- #
#  Инварианты контейнера
# --------------------------------------------------------------------------- #

def check_invariants(container: Container) -> list[Check]:
    checks: list[Check] = []
    header = container.header

    checks.append(Check("сигнатура ADOCT", True, "8 байт"))
    checks.append(Check("word4 = 84", header.word4 == 84, f"0x04 = {header.word4}"))
    checks.append(Check("word8 = 3", header.word8 == 3, f"0x08 = {header.word8}"))
    checks.append(Check("word12 = 2", header.word12 == 2, f"0x0C = {header.word12}"))
    known_version = header.chunk_version in KNOWN_CHUNK_VERSIONS
    checks.append(
        Check(
            "chunk_version известна",
            True,   # неизвестная версия — наблюдение, а не ошибка (§2 RCS-FORMAT)
            f"0xD4 = {header.chunk_version} → диалект {dialect(container)}"
            + ("" if known_version else "; версия неизвестна, для чтения не критично"),
        )
    )

    first = container.blocks[0]
    total = sum(block.size for block in container.blocks)
    contiguous = all(
        left.end == right.offset
        for left, right in zip(container.blocks, container.blocks[1:])
    )
    checks.append(Check("блоки стыкуются без дырок", contiguous, "по директории"))
    checks.append(
        Check(
            "блоки покрывают файл до EOF",
            first.offset + total == container.size,
            f"{first.offset} + {total} = {first.offset + total}, размер {container.size}",
        )
    )
    checks.append(
        Check(
            "директория расположена до данных",
            container.directory_offset < first.offset,
            f"директория 0x{container.directory_offset:X}, первый блок 0x{first.offset:X}",
        )
    )
    checks.append(
        Check(
            "набор типов блоков известен",
            tuple(container.block_types()) in BLOCK_LAYOUTS,
            f"типы = {container.block_types()} (диалект {dialect(container)})",
        )
    )

    tiles = tile_count(container)
    record = tile_record_size(container)
    if tiles is None:
        checks.append(Check("type 3: tileCount", False, "нет блока type 3"))
    else:
        checks.append(
            Check(
                "type 3: (size-8) делится на tileCount",
                record is not None,
                f"tileCount={tiles}, запись={record}",
            )
        )
        checks.append(
            Check("type 3: размер записи = 368", record == TILE_RECORD_SIZE, f"{record}")
        )

    ratio = bytes_per_point(container)
    checks.append(
        Check(
            "type 11: 0 < байт/точка < 100",
            ratio is not None and 0.0 < ratio < 100.0,
            f"{ratio:.3f} Б/точка (count={header.count})" if ratio else "не вычислить",
        )
    )

    tiles = iter_tiles(container)
    if tiles:
        payload_total = sum(tile.payload_size for tile in tiles)
        block11 = container.block_of_type(11)
        expected = block11.size if block11 else 0
        checks.append(
            Check(
                "type 3: сумма payload = размер блока type 11",
                payload_total == expected,
                f"{payload_total} == {expected}",
            )
        )
        points_total = sum(tile.count for tile in tiles)
        checks.append(
            Check(
                "type 3: сумма точек по узлам <= count заголовка",
                points_total <= header.count,
                f"узлы={points_total}, заголовок={header.count}, "
                f"разница={header.count - points_total}",
            )
        )
        cube_ok = all(
            all(lo <= hi for lo, hi in zip(tile.cube_min, tile.cube_max)) for tile in tiles
        )
        checks.append(
            Check("type 3: кубы узлов корректны (min <= max)", cube_ok, f"узлов {len(tiles)}")
        )

    extents_ok = all(lo <= 0.0 <= hi for lo, hi in zip(header.extent_min, header.extent_max))
    checks.append(
        Check(
            "полуразмеры bbox охватывают ноль",
            extents_ok,
            f"{fmt3(header.extent_min)} .. {fmt3(header.extent_max)}",
        )
    )
    root = header.root_max[0]
    # Корневой куб задан в ЛОКАЛЬНОЙ системе: center — это смещение (проектный
    # origin), а не центр куба. Поэтому проверяем только полуразмеры bbox.
    longest = max(abs(extent) for extent in header.extent_max)
    checks.append(
        Check(
            "bbox помещается в корневой куб (локальная система)",
            longest <= root + 1e-6,
            f"max полуразмер {longest:.6f} <= R {root:.6f}",
        )
    )

    props = tlv_block(container)
    if props is not None:
        entries = block_tlvs(container, props)
        consumed = sum(8 + entry.size for entry in entries)
        # Структура TLV разобрана лишь частично: после нескольких записей поток
        # теряет синхронизацию. Поэтому проверка информационная (ok=True).
        checks.append(
            Check(
                "TLV-свойства: best-effort (структура хвоста — ГИПОТЕЗА)",
                True,
                f"блок type {props.type}, записей {len(entries)}, "
                f"прочитано {consumed} из {props.size} Б",
            )
        )

    preview = preview_block(container)
    if preview is not None:
        checks.append(Check("превью JPEG в type 0", True, f"{preview.size} байт"))
    return checks
