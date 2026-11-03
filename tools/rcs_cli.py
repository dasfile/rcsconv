#!/usr/bin/env python3
"""Общая часть CLI конвертеров в `.rcs`: аргументы и цикл по файлам.

Правило интерфейса (`README.md`): первый аргумент — вход (файл, каталог или
маска), имя выхода подставляется само (`<имя>.rcs`), существующий выход не
перезаписывается без `-f`.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Callable

from cli_paths import default_target, expand_sources, has_suffix, is_same_file
from cloud import CloudSource
from rcs_blocks import RcsFormatError, parse_file
from rcs_build import MAX_TILE_POINTS, build_from_source, describe
from rcs_write import normalize_guid

SourceFactory = Callable[[Path], CloudSource]


def build_parser(
    *, label: str, description: str, epilog: str
) -> argparse.ArgumentParser:
    """Парсер с общими аргументами конвертеров (`pts2rcs`, `las2rcs`)."""
    parser = argparse.ArgumentParser(description=description, epilog=epilog)
    parser.add_argument(
        "sources", nargs="+", help=f"входные {label}: файл, каталог или маска"
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
    return parser


def run(
    args: argparse.Namespace,
    *,
    parser: argparse.ArgumentParser,
    suffixes: tuple[str, ...],
    label: str,
    make_source: SourceFactory,
) -> int:
    """Пройти по входам и собрать `.rcs`; вернуть код выхода."""
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if args.guid:
        try:
            args.guid = normalize_guid(args.guid)   # проверяем до чтения файлов
        except ValueError as exc:
            parser.error(str(exc))

    limit = args.max_points_per_tile
    if limit is not None and not 1 <= limit <= MAX_TILE_POINTS:
        parser.error(
            f"-m {limit}: допустимо 1…{MAX_TILE_POINTS} — "
            "в узле не больше тел, чем вмещает u16 index записи заголовка payload'а"
        )

    sources: list[Path] = []
    for suffix in suffixes:
        sources.extend(expand_sources(args.sources, suffix, recursive=args.recursive))
    unique: list[Path] = []
    for path in sources:
        if path not in unique:
            unique.append(path)
    sources = unique
    wrong = [path for path in sources if has_suffix(path, ".rcs")]
    if wrong:
        print(
            f"Ошибка: {wrong[0]} — это .rcs, а конвертер читает {label}. "
            "Обратное направление — rcs2pts.py",
            file=sys.stderr,
        )
        return 2
    if not sources:
        print(f"Ошибка: входные {label} не найдены", file=sys.stderr)
        return 2
    allowed = {suffix.lower() for suffix in suffixes}
    foreign = [
        path
        for path in sources
        if path.exists() and path.suffix.lower() not in allowed
    ]
    if foreign:
        print(
            f"Ошибка: {foreign[0]} — конвертер читает {label}",
            file=sys.stderr,
        )
        return 2

    template = parse_file(args.template) if args.template else None
    written = skipped = failed = 0
    try:
        for source in sources:
            target = default_target(source, ".rcs", args.out_dir)
            if target.exists() and not args.force:
                print(f"{source} → {target}: пропущено (файл есть, нужен -f)")
                skipped += 1
                continue
            if is_same_file(source, target):
                print(f"{source}: выход совпадает с входом", file=sys.stderr)
                failed += 1
                continue
            try:
                blob, stats = build_from_source(
                    make_source(source),
                    template,
                    guid=args.guid,
                    max_per_tile=args.max_points_per_tile,
                    metadata=args.metadata,
                )
            except (OSError, RcsFormatError, ValueError) as exc:
                print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
                failed += 1
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(blob)
            written += 1
            print(describe(source, target, stats))
    except (OSError, RcsFormatError, ValueError) as exc:
        print(f"Ошибка: {exc}", file=sys.stderr)
        return 2

    if len(sources) > 1:
        print(f"итого: записано {written}, пропущено {skipped}, ошибок {failed}")
    return 0 if failed == 0 else 1
