#!/usr/bin/env python3
"""Общее для конвертеров в `.rcs`: точка облака и «источник» точек.

Источник — это файл, который умеет отдать две вещи:

  * `bounds()` — bbox и число точек (один проход по файлу);
  * `points()` — поток точек (можно вызывать сколько нужно раз).

Так `tools/rcs_build.py` не знает ничего о форматах входа: чтобы добавить новый
(`.pts`, `.las/.laz`, в будущем `.e57`), достаточно нового модуля чтения и
фабрики источника — см. `tools/pts_read.py`, `tools/las_read.py`.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterator, Protocol

LOW = tuple[float, float, float]
HIGH = tuple[float, float, float]
Bounds = tuple[LOW, HIGH, int]


@dataclass(frozen=True)
class Point:
    """Точка облака: координаты (м) + интервал/интенсивность и цвет 0…255."""

    x: float
    y: float
    z: float
    intensity: int = 0
    r: int = 0
    g: int = 0
    b: int = 0


class CloudSource(Protocol):
    """Что должен уметь источник точек для `rcs_build.build_from_source`."""

    path: Path

    def bounds(self) -> Bounds:
        """bbox (min, max) и число точек."""

    def points(self) -> Iterator[Point]:
        """Поток точек; вызывается несколько раз (границы, глубина, сборка)."""


@dataclass(frozen=True)
class StreamingSource:
    """Источник из двух функций чтения (путь + bounds + points)."""

    path: Path
    bounds_fn: Callable[[Path], Bounds]
    points_fn: Callable[[Path], Iterator[Point]]

    def bounds(self) -> Bounds:
        return self.bounds_fn(self.path)

    def points(self) -> Iterator[Point]:
        return self.points_fn(self.path)


def empty_bounds() -> tuple[list[float], list[float]]:
    """Заготовка для накопления bbox потоком."""
    return [math.inf] * 3, [-math.inf] * 3


def add_to_bounds(lows: list[float], highs: list[float], point: Point) -> None:
    for axis, value in enumerate((point.x, point.y, point.z)):
        if value < lows[axis]:
            lows[axis] = value
        if value > highs[axis]:
            highs[axis] = value


def finish_bounds(
    lows: list[float], highs: list[float], count: int, path: Path
) -> Bounds:
    if not count:
        raise ValueError(f"в {path} нет точек")
    return (lows[0], lows[1], lows[2]), (highs[0], highs[1], highs[2]), count


def clamp8(value: int) -> int:
    """Привести значение к 0…255."""
    return 0 if value < 0 else (255 if value > 255 else value)


def scale_for(maximum: int, mode: str = "auto") -> float:
    """Множитель для цвета/интенсивности: 16-битные LAS-атрибуты → 8 бит."""
    if mode == "1":
        return 1.0
    if mode == "257":
        return 1.0 / 257.0
    return 1.0 / 257.0 if maximum > 255 else 1.0


def to8(value: int, scale: float) -> int:
    """Значение атрибута с множителем → 0…255 (при scale = 1 не меняется)."""
    return clamp8(int(round(value * scale)))
