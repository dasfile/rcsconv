#!/usr/bin/env python3
"""Подвыборка `.pts`: оставить каждую N-ю точку, сохранив геометрию облака.

Зачем: получить малое облако из реального скана (для эталона ReCap). Берётся
**не** первые N точек (это обрезок одной области, ReCap такое отвергал), а каждая
`stride`-я точка по всему файлу: bbox остаётся почти как у источника, точки —
настоящие, порядок строк сохраняется.

Строки пишутся как есть (без переформатирования), поэтому точность координат и
набор колонок не меняются. Первая строка-счётчик (если она есть) пропускается при
записи и пересчитывается заново — по фактическому числу оставленных точек.

Примеры:
    python tools/pts_subset.py scan.pts -n 20000
    python tools/pts_subset.py scan.pts -n 20000 -o probes/corpus
    python tools/pts_subset.py scan.pts --stride 16 --no-header
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

from cli_paths import default_target, expand_sources, has_suffix, is_same_file

MIN_TOKENS = 3   # строка точки: x y z [i] r g b


@dataclass(frozen=True)
class Subset:
    source: Path
    target: Path
    total: int
    stride: int
    written: int


def scan_source(path: str | Path) -> tuple[bool, int]:
    """(есть ли шапка-счётчик, число строк с точками) — один проход."""
    has_header = False
    count = 0
    with Path(path).open("r", encoding="utf-8", errors="replace") as handle:
        for number, line in enumerate(handle):
            tokens = line.split()
            if not tokens:
                continue
            if number == 0 and len(tokens) == 1:
                try:
                    int(tokens[0])
                except ValueError:
                    pass
                else:
                    has_header = True
                    continue
            if len(tokens) >= MIN_TOKENS:
                count += 1
    return has_header, count


def write_subset(
    source: str | Path,
    target: str | Path,
    *,
    limit: int | None = None,
    stride: int | None = None,
    count_header: bool | None = None,
) -> Subset:
    """Записать каждую `stride`-ю точку в `target`; вернуть статистику."""
    source, target = Path(source), Path(target)
    has_header, total = scan_source(source)
    if not total:
        raise ValueError(f"{source}: нет точек")
    if stride is None:
        if not limit or limit < 1:
            raise ValueError("нужен -n (сколько точек) или --stride")
        stride = -(-total // limit)   # ceil: оставляем не больше limit точек
    if stride < 1:
        raise ValueError("stride должен быть ≥ 1")

    expected = (total + stride - 1) // stride
    write_header = has_header if count_header is None else count_header
    target.parent.mkdir(parents=True, exist_ok=True)
    written = 0
    index = 0
    with source.open("r", encoding="utf-8", errors="replace") as src, target.open(
        "w", encoding="utf-8", newline="\n"
    ) as dst:
        if write_header:
            dst.write(f"{expected}\n")
        for number, line in enumerate(src):
            tokens = line.split()
            if not tokens:
                continue
            if number == 0 and len(tokens) == 1:
                try:
                    int(tokens[0])
                except ValueError:
                    pass
                else:
                    continue
            if len(tokens) < MIN_TOKENS:
                continue
            if index % stride == 0:
                dst.write(line.rstrip("\r\n") + "\n")
                written += 1
            index += 1

    if written != expected:
        raise ValueError(
            f"{source}: записано {written} точек, ожидалось {expected} "
            "(файл изменился во время чтения?)"
        )
    return Subset(source, target, total, stride, written)


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Подвыборка .pts: каждая N-я точка по всему файлу",
        epilog="Пример: pts_subset.py scan.pts -n 20000 -o probes/corpus",
    )
    parser.add_argument(
        "sources", nargs="+", help="входные .pts: файл, каталог или маска"
    )
    parser.add_argument(
        "-n", "--limit", type=int, help="сколько точек оставить (шаг подбирается сам)"
    )
    parser.add_argument(
        "--stride", type=int, help="явный шаг выборки (перекрывает -n)"
    )
    parser.add_argument(
        "-o", "--out-dir", help="куда писать (по умолчанию рядом с входом)"
    )
    parser.add_argument(
        "-c",
        "--count-header",
        action="store_true",
        help="первой строкой писать число точек",
    )
    parser.add_argument(
        "--no-header", action="store_true", help="не писать шапку-счётчик"
    )
    parser.add_argument(
        "-f", "--force", action="store_true", help="перезаписывать существующий .pts"
    )
    parser.add_argument(
        "-r", "--recursive", action="store_true", help="обойти подкаталоги"
    )
    args = parser.parse_args(argv)

    if args.count_header and args.no_header:
        parser.error("-c и --no-header несовместимы")
    count_header = True if args.count_header else (False if args.no_header else None)
    if not args.limit and not args.stride:
        parser.error("укажите -n (сколько точек) или --stride")

    sources = expand_sources(args.sources, ".pts", recursive=args.recursive)
    wrong = [path for path in sources if has_suffix(path, ".rcs")]
    if wrong:
        print(f"Ошибка: {wrong[0]} — это .rcs, нужен .pts", file=sys.stderr)
        return 2
    if not sources:
        print("Ошибка: входные .pts не найдены", file=sys.stderr)
        return 2

    written = skipped = failed = 0
    for source in sources:
        _, total = scan_source(source)
        stride = args.stride or max(1, -(-total // max(args.limit or 1, 1)))
        expected = (total + stride - 1) // stride
        name = f"{source.stem}_n{expected}.pts"
        target = (
            default_target(source, ".pts", args.out_dir).with_name(name)
            if args.out_dir
            else source.with_name(name)
        )
        if target.exists() and not args.force:
            print(f"{source} → {target}: пропущено (файл есть, есть -f)")
            skipped += 1
            continue
        if is_same_file(source, target):
            print(f"{source}: выход совпадает с входом", file=sys.stderr)
            failed += 1
            continue
        try:
            subset = write_subset(
                source,
                target,
                limit=args.limit,
                stride=args.stride,
                count_header=count_header,
            )
        except (OSError, ValueError) as exc:
            print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
            failed += 1
            continue
        size = subset.target.stat().st_size / 1e6
        print(
            f"{source} → {subset.target}: точек {subset.written} из {subset.total} "
            f"(каждая {subset.stride}-я), {size:.2f} МБ"
        )
        written += 1

    if len(sources) > 1:
        print(f"итого: записано {written}, пропущено {skipped}, ошибок {failed}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
