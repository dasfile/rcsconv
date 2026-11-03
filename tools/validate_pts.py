#!/usr/bin/env python3
"""Приёмочная сверка пары (`.pts`, `.rcs`): точки источника против контейнера.

Печатает: число точек (шапка `.pts` и факт), bbox источника, `count` заголовка
RCS, сумму точек по узлам, bbox из заголовка и из узлов, глубину дерева,
слова/`K` блока `type 1`, максимум точек в узле — и вердикт по критериям
приёмки (`docs/RCS-FORMAT.md` §10): count = число точек, bbox ±1e-3.

Примеры:
    python tools/validate_pts.py golden/probe6.pts golden/probe6.rcs
    python tools/validate_pts.py golden/level_7.pts out/auto/level_7.rcs
    python tools/validate_pts.py "D:\\scan\\*.pts"            # пары по именам
"""

from __future__ import annotations

import argparse
import math
import sys
from dataclasses import dataclass
from pathlib import Path

from analyze_type1 import (
    header_words,
    metadata_block_bytes,
    metadata_records,
    octree_counts,
    octree_depth,
)
from cli_paths import default_target, expand_sources, has_suffix
from corpus_status import find_export
from pts2rcs import read_pts
from rcs_blocks import RcsFormatError, dialect, iter_tiles, parse_file

BBOX_TOLERANCE = 1e-3   # метры, критерий приёмки
BBOX_SOFT_TOLERANCE = 1e-2   # эталон ReCap прорежен: крайние точки могут «плыть»


@dataclass(frozen=True)
class PtsStats:
    path: Path
    count: int
    header_count: int | None
    lows: tuple[float, float, float]
    highs: tuple[float, float, float]
    colored: int
    max_intensity: int


@dataclass(frozen=True)
class RcsStats:
    path: Path
    dialect: str
    count: int
    node_points: int
    tiles: int
    depth: int
    header_lows: tuple[float, float, float]
    header_highs: tuple[float, float, float]
    node_lows: tuple[float, float, float] | None
    node_highs: tuple[float, float, float] | None
    words: list[int]
    records: int
    max_tile_points: int


def pts_stats(path: str | Path) -> PtsStats:
    """Один потоковый проход по `.pts`: bbox, точки, цвет, интенсивность."""
    path = Path(path)
    header_count: int | None = None
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        first = handle.readline().split()
        if len(first) == 1:
            try:
                header_count = int(first[0])
            except ValueError:
                header_count = None
    lows = [math.inf] * 3
    highs = [-math.inf] * 3
    count = colored = max_intensity = 0
    for point in read_pts(path):
        count += 1
        for axis, value in enumerate((point.x, point.y, point.z)):
            lows[axis] = min(lows[axis], value)
            highs[axis] = max(highs[axis], value)
        if point.r or point.g or point.b:
            colored += 1
        max_intensity = max(max_intensity, point.intensity)
    if not count:
        raise RcsFormatError(f"{path}: нет точек")
    return PtsStats(
        path=path,
        count=count,
        header_count=header_count,
        lows=tuple(lows),  # type: ignore[arg-type]
        highs=tuple(highs),  # type: ignore[arg-type]
        colored=colored,
        max_intensity=max_intensity,
    )


def rcs_stats(path: str | Path) -> RcsStats:
    """Сводка по RCS: заголовок, узлы, `type 1`, глубина."""
    path = Path(path)
    container = parse_file(path)
    tiles = iter_tiles(container)
    header = container.header
    node_lows: tuple[float, float, float] | None = None
    node_highs: tuple[float, float, float] | None = None
    for tile in tiles:
        if not tile.count:
            continue
        low = tuple(header.center[axis] + tile.points_min[axis] for axis in range(3))
        high = tuple(header.center[axis] + tile.points_max[axis] for axis in range(3))
        if node_lows is None or node_highs is None:
            node_lows, node_highs = low, high  # type: ignore[assignment]
        else:
            node_lows = tuple(  # type: ignore[assignment]
                min(a, b) for a, b in zip(node_lows, low)
            )
            node_highs = tuple(  # type: ignore[assignment]
                max(a, b) for a, b in zip(node_highs, high)
            )
    raw = metadata_block_bytes(container)
    half = tuple(-value for value in header.extent_min)
    return RcsStats(
        path=path,
        dialect=dialect(container),
        count=header.count,
        node_points=sum(tile.count for tile in tiles),
        tiles=len(tiles),
        depth=octree_depth(container),
        header_lows=tuple(  # type: ignore[arg-type]
            header.center[axis] - half[axis] for axis in range(3)
        ),
        header_highs=tuple(  # type: ignore[arg-type]
            header.center[axis] + half[axis] for axis in range(3)
        ),
        node_lows=node_lows,
        node_highs=node_highs,
        words=header_words(raw),
        records=len(metadata_records(raw)),
        max_tile_points=max((tile.count for tile in tiles), default=0),
    )


def _close(left, right, tolerance: float = BBOX_TOLERANCE) -> bool:
    return all(abs(a - b) <= tolerance for a, b in zip(left, right))


def _status(ok: bool, soft: bool = False) -> str:
    """`OK`, `FAIL` или `INFO` (мягкая проверка: у эталонов ReCap иначе)."""
    if ok:
        return "OK"
    return "INFO" if soft else "FAIL"


def _delta(left, right) -> float:
    """Максимальное расхождение пары троек (метры)."""
    return max(abs(a - b) for a, b in zip(left, right))


def _bbox_status(
    left_low, left_high, right_low, right_high, strict: bool
) -> tuple[str, str]:
    """Статус проверки bbox и пояснение (у эталонов допуск 1 см, в `INFO`)."""
    delta = max(
        _delta(left_low, right_low), _delta(left_high, right_high)
    )
    note = f"макс. расхождение {delta * 1000:.1f} мм"
    if delta <= BBOX_TOLERANCE:
        return "OK", note
    if not strict and delta <= BBOX_SOFT_TOLERANCE:
        return "INFO", note + " (эталон ReCap прорежен)"
    return "FAIL", note


def compare(
    pts: PtsStats, rcs: RcsStats, *, strict: bool = False
) -> list[tuple[str, str, str]]:
    """Проверки приёмки (название, статус, подробности).

    Мягкие (`INFO`) — счётчики: ReCap прореживает при экспорте и считает `count`
    не так, как наш писатель, поэтому для эталонов это наблюдение, а не FAIL.
    `strict=True` (наши файлы) включает их в вердикт и возвращает допуск bbox 1 мм.
    """
    soft = not strict
    checks: list[tuple[str, str, str]] = []
    count_ok = rcs.count == pts.count
    checks.append(
        (
            "count заголовка RCS = число точек .pts",
            _status(count_ok, soft),
            f"{rcs.count} == {pts.count}"
            if count_ok
            else f"{rcs.count} против {pts.count} (разница {rcs.count - pts.count:+d})",
        )
    )
    if pts.header_count is not None:
        checks.append(
            (
                "шапка .pts = фактическое число строк",
                _status(pts.header_count == pts.count),
                f"{pts.header_count} == {pts.count}",
            )
        )
    nodes_ok = rcs.node_points == rcs.count
    checks.append(
        (
            "сумма точек по узлам = count заголовка",
            _status(nodes_ok, soft),
            f"{rcs.node_points} == {rcs.count}",
        )
    )
    status, note = _bbox_status(
        rcs.header_lows, rcs.header_highs, pts.lows, pts.highs, strict
    )
    checks.append(
        (
            "bbox из заголовка RCS = bbox .pts",
            status,
            f"RCS {tuple(round(v, 4) for v in rcs.header_lows)}.."
            f"{tuple(round(v, 4) for v in rcs.header_highs)} против .pts "
            f"{tuple(round(v, 4) for v in pts.lows)}.."
            f"{tuple(round(v, 4) for v in pts.highs)}; {note}",
        )
    )
    if rcs.node_lows is not None and rcs.node_highs is not None:
        status, note = _bbox_status(
            rcs.node_lows, rcs.node_highs, pts.lows, pts.highs, strict
        )
        checks.append(
            (
                "bbox по узлам = bbox .pts",
                status,
                f"{tuple(round(v, 4) for v in rcs.node_lows)}.."
                f"{tuple(round(v, 4) for v in rcs.node_highs)}; {note}",
            )
        )
    return checks


def metadata_checks(rcs: RcsStats) -> list[tuple[str, str, str]]:
    """Проверки блока `type 1`; предел узла — наблюдение (у ReCap бывает больше)."""
    derived = octree_counts(parse_file(rcs.path))
    return [
        (
            "type 1: слова = занятые узлы по уровням",
            _status(rcs.words == derived),
            f"{rcs.words} против {derived}",
        ),
        (
            "type 1: K = сумма слов = число узлов октодерева",
            _status(rcs.records == sum(rcs.words) == sum(derived)),
            f"K={rcs.records}, слов={sum(rcs.words)}, узлов={sum(derived)}",
        ),
        (
            "узел > 65536 точек (предел u16 index нашего писателя)",
            _status(rcs.max_tile_points <= (1 << 16), soft=True),
            f"максимум {rcs.max_tile_points}"
            + ("" if rcs.max_tile_points <= (1 << 16) else " — у ReCap индексы повторяются"),
        ),
    ]


def sanit_report(source: Path) -> int:
    """Отчёт по одному `.pts` (без эталона): шапка, bbox, цвет, интенсивность."""
    try:
        pts = pts_stats(source)
    except (OSError, RcsFormatError, ValueError) as exc:
        print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
        return 2
    span = max(high - low for low, high in zip(pts.lows, pts.highs))
    print(f".pts: {pts.path}")
    print(
        f"  точек {pts.count}"
        + (f" (шапка {pts.header_count})" if pts.header_count is not None else "")
        + f", цветных {pts.colored} ({pts.colored / pts.count:.1%}), "
        f"max интенсивность {pts.max_intensity}"
    )
    print(
        f"  bbox {tuple(round(v, 4) for v in pts.lows)} .. "
        f"{tuple(round(v, 4) for v in pts.highs)} (размах {span:.4g} м)"
    )
    warnings: list[str] = []
    if pts.header_count is not None and pts.header_count != pts.count:
        warnings.append(
            f"шапка {pts.header_count} не равна числу строк {pts.count}"
        )
    if span > 1e5:
        warnings.append("размах больше 100 км — похоже на градусы или футы")
    if span < 1e-3:
        warnings.append("размах меньше 1 мм — проверьте единицы")
    if pts.colored == 0:
        warnings.append("нет ни одной цветной точки")
    if pts.max_intensity > 255:
        warnings.append(
            f"интенсивность до {pts.max_intensity} — не 8 бит, нужен --intensity-scale"
        )
    for warning in warnings:
        print(f"  ! {warning}")
    print(f"ИТОГ: {len(warnings)} предупреждений" if warnings else "ИТОГ: замечаний нет")
    return 0


def validate_pair(
    source: Path, target: Path, *, verbose: bool = True, strict: bool = False
) -> int:
    """Сверить одну пару. 0 — всё сошлось, 1 — расхождения, 2 — не смогли."""
    if not target.exists():
        print(f"{source}: нет эталона {target}", file=sys.stderr)
        return 2
    try:
        pts = pts_stats(source)
        rcs = rcs_stats(target)
    except (OSError, RcsFormatError, ValueError) as exc:
        print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
        return 2

    checks = compare(pts, rcs, strict=strict) + metadata_checks(rcs)
    failed = sum(1 for _, status, _ in checks if status == "FAIL")
    infos = sum(1 for _, status, _ in checks if status == "INFO")
    if verbose:
        print(f".pts: {pts.path}")
        print(
            f"  точек {pts.count}"
            + (f" (шапка {pts.header_count})" if pts.header_count is not None else "")
            + f", цветных {pts.colored}, max интенсивность {pts.max_intensity}"
        )
        print(
            f"  bbox {tuple(round(v, 4) for v in pts.lows)} .. "
            f"{tuple(round(v, 4) for v in pts.highs)}"
        )
        print(f".rcs: {rcs.path} ({rcs.dialect})")
        print(
            f"  count {rcs.count}, узлов {rcs.tiles} (глубина {rcs.depth}), "
            f"точек по узлам {rcs.node_points}, макс. {rcs.max_tile_points}/узел"
        )
        print(
            f"  type 1: слова {tuple(rcs.words)}, K={rcs.records}; bbox из заголовка "
            f"{tuple(round(v, 4) for v in rcs.header_lows)} .. "
            f"{tuple(round(v, 4) for v in rcs.header_highs)}"
        )
        for name, status, detail in checks:
            print(f"  [{status:4s}] {name}: {detail}")
    verdict = (
        "все проверки пройдены"
        if not failed
        else f"провалено {failed}"
    )
    if infos and not failed:
        verdict += f", наблюдений {infos}"
    print(
        f"{pts.path.stem}: count {rcs.count}/{pts.count}, узлов {rcs.tiles} "
        f"(глубина {rcs.depth}), макс. {rcs.max_tile_points}/узел — {verdict}"
    )
    return 0 if not failed else 1


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Сверка .pts с .rcs по критериям приёмки (или отчёт по одному .pts)",
        epilog="Примеры: validate_pts.py golden/probe6.pts golden/probe6.rcs | "
        "validate_pts.py \"D:\\scan\\*.pts\" -g golden\\corpus | validate_pts.py scan.pts",
    )
    parser.add_argument(
        "paths", nargs="+", help=".pts (файл/каталог/маска); вторым можно указать .rcs"
    )
    parser.add_argument(
        "-g",
        "--golden",
        help="каталог с эталонами: для каждого .pts берётся <имя>.rcs (или <имя>*.rcs)",
    )
    parser.add_argument(
        "-q", "--quiet", action="store_true", help="только итог по каждой паре"
    )
    parser.add_argument(
        "-s",
        "--strict",
        action="store_true",
        help="считать расхождения count (наши файлы); у эталонов ReCap это INFO",
    )
    args = parser.parse_args(argv)

    paths = list(args.paths)
    if len(paths) == 2 and has_suffix(paths[1], ".rcs"):
        pairs = [(Path(paths[0]), Path(paths[1]))]
        missing_ok = False
    else:
        sources = expand_sources(paths, ".pts")
        if not sources:
            print("Ошибка: входные .pts не найдены", file=sys.stderr)
            return 2
        golden = Path(args.golden) if args.golden else None
        pairs = []
        for source in sources:
            if golden is not None:
                target, _ = find_export(golden, source.stem)
            else:
                target = default_target(source, ".rcs")
            pairs.append((source, target))
        missing_ok = golden is None

    failed = 0
    for source, target in pairs:
        if target is None or (missing_ok and not target.exists()):
            # эталона нет и его не искали целенаправленно — санитарный отчёт
            status = sanit_report(source)
        else:
            status = validate_pair(
                source, target, verbose=not args.quiet, strict=args.strict
            )
        if status == 1:
            failed += 1
    if len(pairs) > 1:
        print(f"итого: пар {len(pairs)}, с расхождениями {failed}")
    return 0 if not failed else 1


if __name__ == "__main__":
    raise SystemExit(main())

