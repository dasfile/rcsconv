"""Генератор пробных .pts для дифференциального анализа кодирования точек.

Идея: меняем одну величину — смотрим, какие байты в .rcs меняются.
Результат складываем в probes/, импортируем в ReCap вручную (см. golden/README.md).

Примеры:
    python tools/make_probes.py
    python tools/make_probes.py --out probes --size 1.0
"""

from __future__ import annotations

import argparse
from pathlib import Path

COLOR = (200, 100, 50)


def write_pts(path: Path, points: list[tuple[float, float, float]], color=COLOR) -> None:
    lines = [
        f"{x:.17g} {y:.17g} {z:.17g} {color[0]} {color[1]} {color[2]}"
        for x, y, z in points
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_probes(size: float) -> dict[str, list[tuple[float, float, float]]]:
    """Набор проб: каждая изолирует одну переменную."""
    half = size / 2.0
    probes: dict[str, list[tuple[float, float, float]]] = {
        # одна точка в центре
        "p00_origin": [(0.0, 0.0, 0.0)],
        # по одной оси, обе стороны
        "p01_x_plus": [(half, 0.0, 0.0)],
        "p02_x_minus": [(-half, 0.0, 0.0)],
        "p03_y_plus": [(0.0, half, 0.0)],
        "p04_y_minus": [(0.0, -half, 0.0)],
        "p05_z_plus": [(0.0, 0.0, half)],
        "p06_z_minus": [(0.0, 0.0, -half)],
        # диагонали и углы корневого куба
        "p07_diag_all": [(half, half, half)],
        "p08_diag_neg": [(-half, -half, -half)],
        "p09_corners": [
            (half, half, half),
            (half, half, -half),
            (half, -half, half),
            (half, -half, -half),
            (-half, half, half),
            (-half, half, -half),
            (-half, -half, half),
            (-half, -half, -half),
        ],
        # шаг по X при фиксированных Y/Z: выявляет сетку квантования
        "p10_x_steps": [(i * size / 8.0, 0.0, 0.0) for i in range(8)],
        # две точки, отличающиеся только цветом
        "p11_color_pair": [(0.0, 0.0, 0.0), (0.0, 0.0, 0.0)],
        # три точки как в probe6 (контроль на знакомом случае)
        "p12_probe6_like": [
            (-0.111111111111111, 0.111111111111111, -1.111111111111111),
            (-1.444444444444444, 1.0, -1.0),
            (-0.111111111111111, 3.111111111111111, -3.111111111111111),
        ],
    }
    return probes


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Сгенерировать пробные .pts")
    parser.add_argument("--out", default="probes", help="каталог для проб")
    parser.add_argument(
        "--size", type=float, default=1.0, help="сторона куба проб (метры)"
    )
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    probes = build_probes(args.size)
    for name, points in probes.items():
        write_pts(out / f"{name}.pts", points)
    # отдельная проба с другим цветом для проверки RGB-кодирования
    write_pts(out / "p11_color_pair.pts", [(0.0, 0.0, 0.0), (0.0, 0.0, 0.0)], color=(0, 0, 0))

    total = sum(len(points) for points in probes.values())
    print(f"Создано проб: {len(probes)} в {out}, всего точек: {total}")
    print("Дальше: импортировать каждый .pts в ReCap -> golden/<имя>.rcs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
