#!/usr/bin/env python3
"""Чек-лист корпуса: для каждого `.pts` — есть ли эталон ReCap и что осталось.

Проверяет в каталоге эталонов (`golden/corpus` по умолчанию) три артефакта:

  * `<имя>.rcp`                  — проект ReCap (создаётся при импорте);
  * `<имя> Support/<имя>.rcs`    — raw-диалект (создаётся при импорте);
  * `<имя>.rcs`                  — экспорт Unified RCS (делается вручную) — **эталон**.
    Если точного имени нет, принимается единственный `<имя>*.rcs` (например,
    `..._exported_unify.rcs`); при нескольких кандидатах — предупреждение.

Для raw и export печатается `count` из заголовка: сразу видно, на каком шаге
ReCap теряет точки (у `attic_south_ost`: 1 086 140 → 1 085 855 → 1 002 250).

Примеры:
    python tools/corpus_status.py "D:\\scan" -g "D:\\scan"
    python tools/corpus_status.py probes/corpus -g golden/corpus
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

from analyze_type1 import octree_depth
from cli_paths import expand_sources
from pts_subset import scan_source
from rcs_blocks import RcsFormatError, iter_tiles, parse_file

DEFAULT_GOLDEN = Path(__file__).resolve().parents[1] / "golden"


@dataclass(frozen=True)
class RcsSummary:
    count: int
    tiles: int
    depth: int
    max_tile: int


@dataclass(frozen=True)
class Status:
    name: str
    points: int | None
    project: bool
    raw: bool
    export: Path | None
    raw_count: int | None = None
    export_info: RcsSummary | None = None
    warning: str | None = None

    @property
    def ready(self) -> bool:
        return self.export is not None

    @property
    def note(self) -> str:
        if self.export is not None:
            return "готово"
        if self.raw or self.project:
            return "нужен экспорт Unified RCS"
        return "нужен проект ReCap"


def rcs_summary(path: str | Path) -> RcsSummary:
    """`count`, узлы, глубина, максимум точек в узле."""
    container = parse_file(path)
    tiles = iter_tiles(container)
    return RcsSummary(
        count=container.header.count,
        tiles=len(tiles),
        depth=octree_depth(container),
        max_tile=max((tile.count for tile in tiles), default=0),
    )


def find_export(golden: Path, name: str) -> tuple[Path | None, str | None]:
    """Экспорт эталона: точное `<имя>.rcs`, иначе единственный `<имя>*.rcs`."""
    exact = golden / f"{name}.rcs"
    if exact.exists():
        return exact, None
    candidates = sorted(path for path in golden.glob(f"{name}*.rcs") if path.is_file())
    if len(candidates) == 1:
        return candidates[0], None
    if len(candidates) > 1:
        names = ", ".join(path.name for path in candidates[:4])
        return None, f"несколько кандидатов: {names} — переименуйте в {name}.rcs"
    return None, None


def status_of(source: Path, golden: Path) -> Status:
    """Что уже есть для этого `.pts` в каталоге эталонов."""
    name = source.stem
    _, points = scan_source(source)
    raw_path = golden / f"{name} Support" / f"{name}.rcs"
    export, warning = find_export(golden, name)
    raw_count = None
    if raw_path.exists():
        try:
            raw_count = parse_file(raw_path).header.count
        except (OSError, RcsFormatError, ValueError):
            raw_count = None
    export_info = None
    if export is not None:
        try:
            export_info = rcs_summary(export)
        except (OSError, RcsFormatError, ValueError) as exc:
            print(f"{export.name}: ОШИБКА чтения: {exc}", file=sys.stderr)
            export, warning = None, f"{export.name} не читается"
    return Status(
        name=name,
        points=points or None,
        project=(golden / f"{name}.rcp").exists(),
        raw=raw_path.exists(),
        export=export,
        raw_count=raw_count,
        export_info=export_info,
        warning=warning,
    )


def _describe(status: Status) -> str:
    """Одна строка отчёта по облаку."""
    parts = [f"{status.points if status.points is not None else '?':>10} точек"]
    parts.append("проект да" if status.project else "проекта нет")
    if status.raw:
        raw = "raw да" + (f" ({status.raw_count})" if status.raw_count else "")
    else:
        raw = "raw нет"
    parts.append(raw)
    if status.export_info is not None:
        info = status.export_info
        delta = ""
        if status.points:
            difference = info.count - status.points
            delta = f", {difference:+d}" if difference else ", ="
        parts.append(
            f"экспорт да ({info.count}{delta}; {info.tiles} узлов, гл. {info.depth}, "
            f"макс. {info.max_tile})"
        )
    else:
        parts.append("экспорта нет")
    line = f"{status.name:28s} " + " | ".join(parts) + f" | {status.note}"
    if status.warning:
        line += f"\n{'':28s} ! {status.warning}"
    return line


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Чек-лист корпуса: какие эталоны ReCap уже сняты",
        epilog="Пример: corpus_status.py \"D:\\scan\" -g D:\\scan",
    )
    parser.add_argument(
        "sources", nargs="+", help="каталоги/маски с .pts (сканы корпуса)"
    )
    parser.add_argument(
        "-g",
        "--golden",
        default=str(DEFAULT_GOLDEN),
        help=f"каталог эталонов (по умолчанию {DEFAULT_GOLDEN})",
    )
    parser.add_argument(
        "-r", "--recursive", action="store_true", help="обойти подкаталоги"
    )
    args = parser.parse_args(argv)

    golden = Path(args.golden)
    sources = expand_sources(args.sources, ".pts", recursive=args.recursive)
    if not sources:
        print("Ошибка: входные .pts не найдены", file=sys.stderr)
        return 2

    statuses = [status_of(source, golden) for source in sources]
    for status in statuses:
        print(_describe(status))

    ready = sum(1 for status in statuses if status.ready)
    print(
        f"\nвсего {len(statuses)}, готово {ready}, осталось {len(statuses) - ready}; "
        f"эталоны ищутся в {golden}"
    )
    missing = [status for status in statuses if not status.ready]
    if missing:
        print("снять в ReCap (Import → Export Unified RCS):")
        for status in missing:
            print(f"  - {status.name} ({status.note})")
    return 0 if ready == len(statuses) else 1


if __name__ == "__main__":
    raise SystemExit(main())
