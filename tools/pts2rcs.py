#!/usr/bin/env python3
"""Генерация `.rcs` (ADOCT) из `.pts` — этап 2.

Схема узлов: дерево фиксированной глубины, в узле — до `-m` точек. По умолчанию
`-m` = auto = 65536 (предел формата: `index` записи заголовка payload'а — `u16`).
При таком пределе на `level_7` получается ровно 43 узла и глубина 3 — как в
`golden/level_7`; меньшее значение даёт более мелкое дерево и больший файл.

ПОДТВЕРЖДЕНО на `golden/probe6.rcs`, `golden/test2.rcs` (по 3 одноточечных узла):
  * локальная система = точка − центр bbox; корневой куб центрирован в нуле;
  * ребро плитки = 2.25 м, корневой куб = ±2.25·2^k (минимальный, покрывающий bbox);
  * запись узла: куб, bbox точек (округлён до 1 мм), `header_u32 = (1, count, payload, 0…)`,
    `tail_u32` — нули, кроме `tail[27] = count` (на одноточечных узлах 1);
  * порядок узлов — обход октодерева (на глубине 1: `x + 2y + 4z`).

Блоки `type 1` (метаданные) и `type 0` (свойства) генерируются по спецификации:
`type 1` — слова = занятые узлы октодерева по уровням, записей = их сумма
(`docs/RCS-FORMAT.md` §4.3); `type 0` — фиксированные TLV. Шаблон не нужен.

Примеры:
    python tools/pts2rcs.py golden/probe6.pts            # → golden/probe6.rcs
    python tools/pts2rcs.py golden -o out                # все *.pts каталога → out/
    python tools/pts2rcs.py "D:\\scan\\*.pts" -m 200        # мельче дерево
    python tools/pts2rcs.py golden/probe6.pts -o out -t golden/probe6.rcs \\
        --metadata template
"""

from __future__ import annotations

import argparse
import math
import struct
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Iterator

from cli_paths import default_target, expand_sources, has_suffix, is_same_file
from rcs_blocks import Container, Header, RcsFormatError, parse_file
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


@dataclass(frozen=True)
class Point:
    x: float
    y: float
    z: float
    intensity: int
    r: int
    g: int
    b: int


def read_pts(path: str | Path) -> Iterator[Point]:
    """Потоково читать `.pts`: x y z [intensity] r g b."""
    path = Path(path)
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            parts = line.split()
            if len(parts) < 3:
                continue
            try:
                x, y, z = (float(parts[0]), float(parts[1]), float(parts[2]))
            except ValueError:
                continue
            numbers = []
            for token in parts[3:]:
                try:
                    numbers.append(int(round(float(token))))
                except ValueError:
                    numbers.append(0)
            if len(numbers) >= 4:
                intensity, red, green, blue = numbers[0], numbers[1], numbers[2], numbers[3]
            elif len(numbers) == 3:
                intensity, red, green, blue = 0, numbers[0], numbers[1], numbers[2]
            else:
                intensity = numbers[0] if numbers else 0
                red = green = blue = 0
            yield Point(
                x, y, z,
                max(0, min(255, intensity)),
                max(0, min(255, red)),
                max(0, min(255, green)),
                max(0, min(255, blue)),
            )


def bounds(path: str | Path) -> tuple[tuple[float, float, float], tuple[float, float, float], int]:
    """bbox `.pts` и число точек (один потоковый проход)."""
    lows = [math.inf] * 3
    highs = [-math.inf] * 3
    count = 0
    for point in read_pts(path):
        for axis, value in enumerate((point.x, point.y, point.z)):
            lows[axis] = min(lows[axis], value)
            highs[axis] = max(highs[axis], value)
        count += 1
    if not count:
        raise RcsFormatError(f"в {path} нет точек")
    return (lows[0], lows[1], lows[2]), (highs[0], highs[1], highs[2]), count


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


def choose_depth(path: str | Path, center, half: float, max_per_tile: int = 1) -> int:
    """Увеличивать глубину, пока в ячейке не больше `max_per_tile` точек."""
    depth = recap_depth(half)
    while depth <= MAX_DEPTH:
        seen: dict[tuple[int, int, int], int] = {}
        collision = False
        for point in read_pts(path):
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


AXIS_SCALE = (1000, 4000, 16000)   # единиц поля на 1 м по осям x, y, z


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


def build_output(
    source: str | Path,
    template: Container | None = None,
    *,
    guid: str | None = None,
    max_per_tile: int | None = None,
    metadata: str = "auto",
) -> tuple[bytes, dict]:
    """Собрать файл из `.pts`. Шаблон не нужен: все блоки генерируются.

    `max_per_tile` — предел точек в узле (наш выбор глубины дерева). `None` (по
    умолчанию) = `auto` = `MAX_TILE_POINTS` (65536, предел формата по `u16 index`):
    дерево дробится только если иначе файл станет невалидным. Значение меньше
    (например `200`) даёт более мелкое дерево и больший файл.

    `metadata`: `auto` (по умолчанию) и `generated` — блок `type 1` собирается из
    октодерева: слова = занятые узлы по уровням, записей = сумма слов (так пишет
    ReCap, см. `docs/RCS-FORMAT.md` §4.3); `minimal` — прежний минимальный вариант
    `(1,)` + 1 запись (страховка, Revit его принимал); `template` —
    взять блок из `template` (для побайтового воспроизведения эталона).
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

    low, high, count = bounds(source)
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
    for point in read_pts(source):
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


def _describe(source: Path, target: Path, stats: dict) -> str:
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


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Сборка .rcs из .pts (вход — файл, каталог или маска)",
        epilog="Примеры: pts2rcs.py cloud.pts | pts2rcs.py D:\\scan -o out "
        "| pts2rcs.py \"D:\\scan\\*.pts\" -m 200",
    )
    parser.add_argument(
        "sources", nargs="+", help="входные .pts: файл, каталог или маска"
    )
    parser.add_argument(
        "-o", "--out-dir", help="куда писать (по умолчанию рядом с входом)"
    )
    parser.add_argument(
        "-m",
        "--max-points-per-tile",
        type=int,
        default=None,
        help=f"предел точек в узле; по умолчанию auto = {MAX_TILE_POINTS} "
        "(предел формата), меньше — мельче дерево и больше файл",
    )
    parser.add_argument(
        "-f", "--force", action="store_true", help="перезаписывать существующий .rcs"
    )
    parser.add_argument(
        "-r", "--recursive", action="store_true", help="обойти подкаталоги"
    )
    parser.add_argument(
        "-t",
        "--template",
        help="эталон, из которого берётся блок type 1 при --metadata template; "
        "по умолчанию не нужен",
    )
    parser.add_argument(
        "-g",
        "--guid",
        help="GUID результата; по умолчанию случайный (указывать только для "
        "воспроизведения конкретного эталона)",
    )
    parser.add_argument(
        "--metadata",
        choices=("auto", "template", "generated", "minimal"),
        default="auto",
        help="блок type 1: generated/auto — из октодерева (как пишет ReCap), "
        "minimal — (1,) + 1 запись, template — взять из эталона",
    )
    args = parser.parse_args(argv)

    if args.guid:
        try:
            args.guid = normalize_guid(args.guid)   # проверяем до чтения .pts
        except ValueError as exc:
            parser.error(str(exc))

    if args.max_points_per_tile is not None and not 1 <= args.max_points_per_tile <= MAX_TILE_POINTS:
        parser.error(
            f"-m {args.max_points_per_tile}: допустимо 1…{MAX_TILE_POINTS} — "
            "в узле не больше тел, чем вмещает u16 index записи заголовка payload'а"
        )

    sources = expand_sources(args.sources, ".pts", recursive=args.recursive)
    rcs_sources = [path for path in sources if has_suffix(path, ".rcs")]
    if rcs_sources:
        print(
            f"Ошибка: {rcs_sources[0]} — это .rcs, а pts2rcs читает .pts. "
            "Обратное направление — rcs2pts.py",
            file=sys.stderr,
        )
        return 2
    if not sources:
        print("Ошибка: входные .pts не найдены", file=sys.stderr)
        return 2

    template = parse_file(args.template) if args.template else None
    written = skipped = failed = 0
    try:
        for source in sources:
            target = default_target(source, ".rcs", args.out_dir)
            if target.exists() and not args.force:
                print(f"{source} → {target}: пропущено (файл есть, есть -f)")
                skipped += 1
                continue
            if is_same_file(source, target):
                print(f"{source}: выход совпадает с входом", file=sys.stderr)
                failed += 1
                continue
            try:
                blob, stats = build_output(
                    source,
                    template,
                    guid=args.guid,
                    max_per_tile=args.max_points_per_tile,
                    metadata=args.metadata,
                )
            except RcsFormatError as exc:
                print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
                failed += 1
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(blob)
            written += 1
            print(_describe(source, target, stats))
    except (OSError, RcsFormatError, ValueError) as exc:
        print(f"Ошибка: {exc}", file=sys.stderr)
        return 2

    if len(sources) > 1:
        print(f"итого: записано {written}, пропущено {skipped}, ошибок {failed}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
