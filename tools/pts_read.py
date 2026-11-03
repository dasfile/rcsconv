#!/usr/bin/env python3
"""Чтение `.pts` (текст Leica Cyclone): x y z [intensity] r g b.

Формат описан в `docs/RCS-FORMAT.md`, Приложение D: первая строка может быть
числом точек, колонок 3…7, интенсивность необязательна.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Iterator

from cloud import Bounds, Point, add_to_bounds, clamp8, empty_bounds, finish_bounds


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
                clamp8(intensity),
                clamp8(red),
                clamp8(green),
                clamp8(blue),
            )


def bounds(path: str | Path) -> Bounds:
    """bbox `.pts` и число точек (один потоковый проход)."""
    lows, highs = empty_bounds()
    count = 0
    for point in read_pts(path):
        add_to_bounds(lows, highs, point)
        count += 1
    return finish_bounds(lows, highs, count, Path(path))


def main(argv: list[str] | None = None) -> int:
    """Мелочь для отладки: `python tools/pts_read.py file.pts` печатает bbox."""
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if not argv:
        argv = sys.argv[1:]
    for name in argv:
        low, high, count = bounds(name)
        print(f"{name}: точек {count}, bbox {low} .. {high}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
