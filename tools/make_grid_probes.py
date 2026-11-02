"""Генератор проб для вскрытия чанковой раскладки payload'а (блок type 11).

Идея: N точек лежат в одной ячейке 2.25 м, плюс один «балласт» в 5 м.
Балласт сдвигает bbox-центр, поэтому кластер гарантированно попадает в один
узел октодерева, а число узлов = 2 (кластер + балласт).

Шаг точек по умолчанию 2 см — при ImportDensity=5 (≈5 мм) ничего не проредится,
поэтому count в файле должен быть ровно N+1.

Примеры:
    python tools/make_grid_probes.py
    python tools/make_grid_probes.py --spacing 0.01 --ballast 5.0 --out probes
"""

from __future__ import annotations

import argparse
from pathlib import Path

DEFAULT_COUNTS = (3, 4, 5, 8, 16, 32, 64, 128, 256, 512, 1024, 10000)
# N = 1 и 2 не генерируем: ReCap отклоняет скан из двух точек («could not import
# scan»), хотя bbox у него трёхмерный. ГИПОТЕЗА: импортёру нужно >= 3 точек.

# Балласт уводит bbox-центр и делает габариты трёхмерными: ReCap отклоняет
# плоские облака без интенсивности (две нулевые оси), поэтому балласт обязан
# отличаться от кластера по ВСЕМ осям.
BALLAST = (5.0, 3.0, 7.0)
BALLAST_COLOR = (255, 0, 0)


def write_pts(path: Path, rows: list[tuple[float, ...]]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            coords = " ".join(f"{value:.6f}" for value in row[:3])
            rest = " ".join(str(int(value)) for value in row[3:])
            handle.write(f"{coords} {rest}\n".rstrip() + "\n")


def cluster(count: int, spacing: float) -> list[tuple[float, ...]]:
    """`count` точек по кубической сетке с шагом spacing, цвета разные."""
    side = 1
    while side ** 3 < count:
        side += 1
    rows: list[tuple[float, ...]] = []
    index = 0
    for iz in range(side):
        for iy in range(side):
            for ix in range(side):
                if index >= count:
                    return rows
                rows.append(
                    (
                        ix * spacing,
                        iy * spacing,
                        iz * spacing,
                        (ix * 37) % 256,
                        (iy * 53) % 256,
                        (iz * 71) % 256,
                    )
                )
                index += 1
    return rows


def build(counts: tuple[int, ...], spacing: float) -> dict[str, list[tuple[float, ...]]]:
    probes: dict[str, list[tuple[float, ...]]] = {}
    for count in counts:
        rows = cluster(count, spacing)
        rows.append((*BALLAST, *BALLAST_COLOR))
        probes[f"grid_{count:05d}"] = rows

    # с интенсивностью: x y z i r g b (проверяем, распознаёт ли ReCap)
    probes["attr_intensity"] = [
        (index * spacing, 0.0, 0.0, index * 40, 200, 100, 50) for index in range(5)
    ] + [(*BALLAST, 255, *BALLAST_COLOR)]

    # 8 углов куба 5 м — проверка правила R = 2.25·2^k и нарезки на ячейки
    probes["scale_5m"] = [
        (float(ix) * 5.0, float(iy) * 5.0, float(iz) * 5.0, 10, 20, 30)
        for ix in (0, 1)
        for iy in (0, 1)
        for iz in (0, 1)
    ]
    return probes


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Сгенерировать grid-пробы для ReCap")
    parser.add_argument("--out", default="probes", help="каталог для проб")
    parser.add_argument("--spacing", type=float, default=0.02, help="шаг точек в кластере, м")
    parser.add_argument(
        "--counts",
        default=",".join(str(value) for value in DEFAULT_COUNTS),
        help="список N через запятую",
    )
    args = parser.parse_args(argv)

    counts = tuple(int(value) for value in args.counts.split(",") if value.strip())
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    probes = build(counts, args.spacing)
    for name, rows in probes.items():
        target = out / f"{name}.pts"
        write_pts(target, rows)
        print(f"{target}: {len(rows)} строк")

    print(f"\nВсего проб: {len(probes)}, каталог: {out}")
    print("Импортировать в ReCap ОДНИМ проектом, без регистрации сканов.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
