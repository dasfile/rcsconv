#!/usr/bin/env python3
"""Чтение LAS/LAZ как источника точек для `.rcs`.

Чтение делает **laspy** (LAS 1.0–1.4, LAZ через бэкенд `laszip` или `lazrs`) —
см. `.clinerules` п.10. Наша часть — правила формата `.rcs`: 16-битные цвет и
интенсивность приводятся к 8 битам (`.pts`/RCS хранят байт), координаты можно
масштабировать и сдвигать (`--units-scale`, `--recenter`).

Примеры:
    python tools/las_read.py golden/level_7.laz
    python tools/las_read.py "golden/*.laz" -o out/pts   # (см. tools/las2pts.py)
"""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

from cloud import Bounds, Point, StreamingSource, clamp8, scale_for, to8

try:                       # laspy нужен только LAS/LAZ
    import laspy
except ImportError:        # остальные инструменты работают без него
    laspy = None           # type: ignore[assignment]

CHUNK_POINTS = 1_000_000


@dataclass(frozen=True)
class LasOptions:
    """Как превращать LAS в точки `.rcs`."""

    units: float = 1.0          # множитель координат (футы → метры: 0.3048)
    recenter: str = "none"      # none | center | min — вычитание из координат
    color: bool = True
    intensity: bool = True
    color_scale: str = "auto"       # auto | 257 | 1
    intensity_scale: str = "auto"   # auto | 257 | 1


def _require_laspy() -> None:
    if laspy is None:
        raise ValueError(
            "для LAS/LAZ нужен laspy: pip install laspy lazrs "
            "(остальные инструменты проекта работают без него)"
        )


def _info(path: str | Path):
    """Заголовок + выборка атрибутов (общая функция с `las2pts.scan`)."""
    _require_laspy()
    from las2pts import scan      # общая реализация: header + sample

    return scan(path)


def _shift(info, recenter: str) -> tuple[float, float, float]:
    """Сдвиг координат для `--recenter` (по bbox из заголовка)."""
    if recenter == "center":
        return tuple((low + high) / 2.0 for low, high in zip(info.lows, info.highs))
    if recenter == "min":
        return tuple(info.lows)
    return (0.0, 0.0, 0.0)


def _scales(info, options: LasOptions) -> tuple[float, float]:
    """Множители цвета и интенсивности (16 бит → 8 бит)."""
    color = scale_for(info.color_max, options.color_scale) if info.has_color else 1.0
    intensity = (
        scale_for(info.intensity_max, options.intensity_scale)
        if info.has_intensity
        else 1.0
    )
    return color, intensity


def bounds(path: str | Path, options: LasOptions | None = None) -> Bounds:
    """bbox и число точек из заголовка (с учётом `units` и `recenter`)."""
    options = options or LasOptions()
    info = _info(path)
    shift = _shift(info, options.recenter)
    low = tuple((value - shift[axis]) * options.units for axis, value in enumerate(info.lows))
    high = tuple((value - shift[axis]) * options.units for axis, value in enumerate(info.highs))
    return low, high, info.points


def read_las(path: str | Path, options: LasOptions | None = None) -> Iterator[Point]:
    """Потоково читать LAS/LAZ как точки `.rcs` (цвет/интенсивность → 0…255)."""
    options = options or LasOptions()
    info = _info(path)
    shift = _shift(info, options.recenter)
    color_scale, intensity_scale = _scales(info, options)
    take_color = options.color and info.has_color
    take_intensity = options.intensity and info.has_intensity
    units = options.units

    with laspy.open(path) as reader:      # type: ignore[union-attr]
        for chunk in reader.chunk_iterator(CHUNK_POINTS):
            xs, ys, zs = chunk.x, chunk.y, chunk.z
            red = chunk.red if take_color else None
            green = chunk.green if take_color else None
            blue = chunk.blue if take_color else None
            strong = chunk.intensity if take_intensity else None
            for index in range(len(xs)):
                yield Point(
                    (xs[index] - shift[0]) * units,
                    (ys[index] - shift[1]) * units,
                    (zs[index] - shift[2]) * units,
                    to8(int(strong[index]), intensity_scale) if strong is not None else 0,
                    to8(int(red[index]), color_scale) if red is not None else 0,
                    to8(int(green[index]), color_scale) if green is not None else 0,
                    to8(int(blue[index]), color_scale) if blue is not None else 0,
                )


def source(path: str | Path, options: LasOptions | None = None) -> StreamingSource:
    """Источник точек для `rcs_build.build_from_source`."""
    options = options or LasOptions()
    path = Path(path)
    return StreamingSource(
        path=path,
        bounds_fn=lambda item: bounds(item, options),
        points_fn=lambda item: read_las(item, options),
    )
