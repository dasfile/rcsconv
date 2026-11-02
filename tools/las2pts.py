#!/usr/bin/env python3
"""Импорт LAS/LAZ → `.pts` (текстовый формат, совместимый с Leica Cyclone).

Чтение делает laspy (LAS 1.0–1.4, LAZ через lazrs) — свой разбор бинаря не нужен
и не пишется. Наша часть — правила формата `.pts`:

  * первая строка — число точек (`--no-header` убирает шапку);
  * колонки `x y z [intensity] r g b` — 7/6/4/3 в зависимости от атрибутов;
  * в LAS цвет и интенсивность 16-битные (0…65535), а `.pts` ждёт 8 бит:
    масштаб считается по данным один раз (`--color-scale auto`, `--intensity-scale
    auto`): если максимум > 255 — делим на 257, иначе значения уже 8-битные;
  * координаты берутся как `las.x/.y/.z` (scale и offset laspy уже применил);
  * `--recenter` и `--units-scale` позволяют убрать большие координаты
    (UTM/футы) — по умолчанию координаты не меняются.

Примеры:
    python tools/las2pts.py scan.laz
    python tools/las2pts.py "D:\\scan\\*.laz" -o out\\pts
    python tools/las2pts.py scan.las --no-header --recenter min
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

from cli_paths import default_target, expand_sources, has_suffix, is_same_file

try:                       # laspy нужен только этому инструменту
    import laspy
except ImportError:        # остальные утилиты работают без него
    laspy = None           # type: ignore[assignment]

COLOR_16BIT = 65535.0 / 255.0   # 257: 65535 → 255
DEFAULT_PRECISION = 4           # 0.1 мм, как в сканерных .pts


@dataclass(frozen=True)
class LasInfo:
    """Что нужно знать о файле до записи (из заголовка и выборки)."""

    path: Path
    points: int
    lows: tuple[float, float, float]
    highs: tuple[float, float, float]
    has_color: bool
    has_intensity: bool
    color_max: int
    intensity_max: int
    version: str
    point_format: int
    crs: str | None


def _require_laspy() -> None:
    if laspy is None:
        raise ValueError(
            "для LAS/LAZ нужен laspy: pip install laspy lazrs "
            "(остальные инструменты проекта работают без него)"
        )


def _to8(value: int, scale: float) -> int:
    """16-битное значение LAS → 0…255 (при scale = 1 значение не меняется)."""
    result = int(round(value * scale))
    return 0 if result < 0 else (255 if result > 255 else result)


def _scale(maximum: int, mode: str) -> float:
    """Множитель для цвета/интенсивности по режиму `auto|257|1`."""
    if mode == "1":
        return 1.0
    if mode == "257":
        return 1.0 / COLOR_16BIT
    return 1.0 / COLOR_16BIT if maximum > 255 else 1.0


def scan(path: str | Path, sample: int = 200_000) -> LasInfo:
    """Заголовок + выборка атрибутов: count, bbox, максимумы цвета/интенсивности."""
    _require_laspy()
    path = Path(path)
    with laspy.open(path) as reader:
        header = reader.header
        has_color = all(
            name in header.point_format.dimension_names
            for name in ("red", "green", "blue")
        )
        has_intensity = "intensity" in header.point_format.dimension_names
        color_max = intensity_max = 0
        seen = 0
        if has_color or has_intensity:
            for chunk in reader.chunk_iterator(min(sample, header.point_count or sample)):
                seen += len(chunk.x)
                if has_intensity:
                    intensity_max = max(intensity_max, int(chunk.intensity.max()))
                if has_color:
                    color_max = max(
                        color_max,
                        int(chunk.red.max()),
                        int(chunk.green.max()),
                        int(chunk.blue.max()),
                    )
        crs = None
        try:
            parsed = header.parse_crs()
            crs = parsed.name if parsed is not None else None
        except Exception:      # CRS может отсутствовать или быть битым
            crs = None
        return LasInfo(
            path=path,
            points=int(header.point_count),
            lows=tuple(float(value) for value in header.mins),  # type: ignore[arg-type]
            highs=tuple(float(value) for value in header.maxs),  # type: ignore[arg-type]
            has_color=has_color,
            has_intensity=has_intensity,
            color_max=color_max,
            intensity_max=intensity_max,
            version=str(header.version),
            point_format=int(header.point_format.id),
            crs=crs,
        )



@dataclass(frozen=True)
class Conversion:
    target: Path
    written: int
    columns: int
    color_scale: float
    intensity_scale: float


def convert(
    source: str | Path,
    target: str | Path,
    *,
    header: bool = True,
    color: bool = True,
    intensity: bool = True,
    color_scale: str = "auto",
    intensity_scale: str = "auto",
    recenter: str = "none",
    units: float = 1.0,
    precision: int = DEFAULT_PRECISION,
) -> Conversion:
    """Записать LAS/LAZ в `.pts`; вернуть статистику конвертации."""
    _require_laspy()
    source, target = Path(source), Path(target)
    info = scan(source)
    use_color = color and info.has_color
    use_intensity = intensity and info.has_intensity
    cscale = _scale(info.color_max, color_scale) if use_color else 1.0
    iscale = _scale(info.intensity_max, intensity_scale) if use_intensity else 1.0

    if recenter == "min":
        shift = info.lows
    elif recenter == "center":
        shift = tuple((low + high) / 2.0 for low, high in zip(info.lows, info.highs))
    else:
        shift = (0.0, 0.0, 0.0)

    fmt = f"{{:.{precision}f}} {{:.{precision}f}} {{:.{precision}f}}"
    columns = 3 + (1 if use_intensity else 0) + (3 if use_color else 0)
    target.parent.mkdir(parents=True, exist_ok=True)
    written = 0
    with target.open("w", encoding="utf-8", newline="\n") as out:
        if header:
            out.write(f"{info.points}\n")
        with laspy.open(source) as reader:
            for chunk in reader.chunk_iterator(1_000_000):
                xs = (chunk.x - shift[0]) * units
                ys = (chunk.y - shift[1]) * units
                zs = (chunk.z - shift[2]) * units
                intensities = None
                colors = None
                if use_intensity:
                    intensities = [_to8(int(value), iscale) for value in chunk.intensity]
                if use_color:
                    red = [_to8(int(value), cscale) for value in chunk.red]
                    green = [_to8(int(value), cscale) for value in chunk.green]
                    blue = [_to8(int(value), cscale) for value in chunk.blue]
                    colors = list(zip(red, green, blue))
                for index, (x, y, z) in enumerate(zip(xs, ys, zs)):
                    line = fmt.format(x, y, z)
                    if intensities is not None:
                        line += f" {intensities[index]}"
                    if colors is not None:
                        red, green, blue = colors[index]
                        line += f" {red} {green} {blue}"
                    out.write(line + "\n")
                    written += 1
    if written != info.points:
        raise ValueError(
            f"{source}: прочитано {written} точек, в заголовке {info.points}"
        )
    return Conversion(target, written, columns, cscale, iscale)


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Импорт LAS/LAZ → .pts (через laspy)",
        epilog="Пример: las2pts.py scan.laz | las2pts.py \"D:\\scan\\*.laz\" -o out\\pts",
    )
    parser.add_argument(
        "sources", nargs="+", help="входные .las/.laz: файл, каталог или маска"
    )
    parser.add_argument(
        "-o", "--out-dir", help="куда писать (по умолчанию рядом с входом)"
    )
    parser.add_argument(
        "-f", "--force", action="store_true", help="перезаписывать существующий .pts"
    )
    parser.add_argument(
        "-r", "--recursive", action="store_true", help="обойти подкаталоги"
    )
    parser.add_argument(
        "--no-header", action="store_true", help="не писать шапку-счётчик"
    )
    parser.add_argument("--no-color", action="store_true", help="не писать цвет")
    parser.add_argument(
        "--no-intensity", action="store_true", help="не писать интенсивность"
    )
    parser.add_argument(
        "--color-scale",
        choices=("auto", "257", "1"),
        default="auto",
        help="auto: 16-битный цвет → 8 бит (÷257), если максимум > 255",
    )
    parser.add_argument(
        "--intensity-scale", choices=("auto", "257", "1"), default="auto"
    )
    parser.add_argument(
        "--recenter",
        choices=("none", "center", "min"),
        default="none",
        help="вычесть центр/минимум bbox (для больших координат UTM)",
    )
    parser.add_argument(
        "--units-scale", type=float, default=1.0, help="множитель координат (футы → м: 0.3048)"
    )
    parser.add_argument(
        "--precision", type=int, default=DEFAULT_PRECISION, help="знаков после точки"
    )
    args = parser.parse_args(argv)

    if args.units_scale <= 0:
        parser.error("--units-scale должен быть > 0")
    if not 0 <= args.precision <= 9:
        parser.error("--precision: допустимо 0…9")

    sources = expand_sources(args.sources, ".las", recursive=args.recursive)
    laz = expand_sources(args.sources, ".laz", recursive=args.recursive)
    sources = sources + [path for path in laz if path not in sources]
    wrong = [path for path in sources if has_suffix(path, ".pts")]
    if wrong:
        print(f"Ошибка: {wrong[0]} — это .pts, нужен .las/.laz", file=sys.stderr)
        return 2
    if not sources:
        print("Ошибка: входные .las/.laz не найдены", file=sys.stderr)
        return 2

    written = skipped = failed = 0
    crs_warned = False
    for source in sources:
        target = default_target(source, ".pts", args.out_dir)
        if target.exists() and not args.force:
            print(f"{source} → {target}: пропущено (файл есть, есть -f)")
            skipped += 1
            continue
        if is_same_file(source, target):
            print(f"{source}: выход совпадает с входом", file=sys.stderr)
            failed += 1
            continue
        try:
            info = scan(source)
            result = convert(
                source,
                target,
                header=not args.no_header,
                color=not args.no_color,
                intensity=not args.no_intensity,
                color_scale=args.color_scale,
                intensity_scale=args.intensity_scale,
                recenter=args.recenter,
                units=args.units_scale,
                precision=args.precision,
            )
        except (OSError, ValueError) as exc:
            print(f"{source}: ОШИБКА: {exc}", file=sys.stderr)
            failed += 1
            continue
        written += 1
        size = target.stat().st_size / 1e6
        print(
            f"{source} → {target}: {info.version}/формат {info.point_format}, "
            f"точек {result.written}, колонок {result.columns}, "
            f"цвет×{result.color_scale:.5g}, интенсивность×{result.intensity_scale:.5g}, "
            f"{size:.2f} МБ"
        )
        if info.crs is None and not crs_warned:
            print(
                "  ! CRS в файлах не указан — проверьте единицы (метры?). "
                "Для сканов с телефона это нормально; предупреждение "
                "показывается один раз на партию",
                file=sys.stderr,
            )
            crs_warned = True
        span = max(high - low for low, high in zip(info.lows, info.highs))
        if span > 1e5 or span < 1e-3:
            print(
                f"  ! размах bbox {span:.3g} — похоже на градусы, футы или мусор; "
                "используйте --units-scale / --recenter",
                file=sys.stderr,
            )

    if len(sources) > 1:
        print(f"итого: записано {written}, пропущено {skipped}, ошибок {failed}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
