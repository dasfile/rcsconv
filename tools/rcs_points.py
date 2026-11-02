"""Декодер данных точек (блок type 11).

ПОДТВЕРЖДЕНО на golden/probe6, golden/test1, golden/test2 и узле 37 golden/level_7:

    тело точки (16 байт, LE):
      [0:2]  u16  x: (x - cube_min.x) в 1 мм
      [2:4]  u16  y: (y - cube_min.y) в 1/4 мм   (масштаб 4)
      [4:6]  u16  z: (z - cube_min.z) в 1/16 мм  (масштаб 16)
      [6:8]  u16  не расшифровано (в эталонах 0)
      [8]    u8   интенсивность (0..255) — подтверждено пробой attr_intensity
      [9:12] u8×3 цвет в порядке B, G, R
      [12:16]     не расшифровано (в эталонах 0)

Узел с несколькими точками: payload = заголовок + count × запись, где запись —
16 или 20 байт (у 20-байтовых записей 4 байта в начале — не расшифрованы).
Длина заголовка заранее неизвестна, поэтому она подбирается: проверяем варианты
`stride ∈ {16, 20}` и принимаем тот, при котором восстановленный bbox точек
совпадает с `points_min/points_max` из записи узла (допуск 2 мм).
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

from rcs_blocks import Container, RcsFormatError, TileInfo, tile_payload

# Масштабы осей: сколько единиц поля приходится на 1 мм
AXIS_SCALE = (1, 4, 16)
BODY_SIZE = 16
BBOX_TOLERANCE = 2e-3  # метры


@dataclass(frozen=True)
class Point:
    x: float
    y: float
    z: float
    r: int
    g: int
    b: int
    intensity: int = 0
    extra: tuple[int, ...] = ()


def decode_body(body: bytes, cube_min: tuple[float, float, float]) -> Point:
    """Декодировать 16-байтовое тело точки."""
    if len(body) != BODY_SIZE:
        raise ValueError(f"тело точки должно быть {BODY_SIZE} байт, получено {len(body)}")
    raw = struct.unpack_from("<3H", body, 0)
    coords = tuple(
        cube_min[axis] + raw[axis] / AXIS_SCALE[axis] / 1000.0 for axis in range(3)
    )
    blue, green, red = body[9], body[10], body[11]
    rest = (body[6], body[7], *body[12:16])
    return Point(coords[0], coords[1], coords[2], red, green, blue, body[8], rest)


def _bbox(points: list[Point]) -> tuple[tuple[float, ...], tuple[float, ...]]:
    xs = [point.x for point in points]
    ys = [point.y for point in points]
    zs = [point.z for point in points]
    return (min(xs), min(ys), min(zs)), (max(xs), max(ys), max(zs))


def _bbox_matches(tile: TileInfo, points: list[Point]) -> bool:
    low, high = _bbox(points)
    return all(
        abs(low[axis] - tile.points_min[axis]) <= BBOX_TOLERANCE
        and abs(high[axis] - tile.points_max[axis]) <= BBOX_TOLERANCE
        for axis in range(3)
    )


def decode_tile(container: Container, tile: TileInfo) -> list[Point]:
    """Декодировать все точки узла, подбирая размер записи по bbox из записи узла."""
    payload = tile_payload(container, tile)
    if tile.count == 0:
        return []

    for stride in (BODY_SIZE, 20):
        header = len(payload) - tile.count * stride
        if header < 0:
            continue
        points = []
        for index in range(tile.count):
            start = header + index * stride + (stride - BODY_SIZE)
            points.append(decode_body(payload[start : start + BODY_SIZE], tile.cube_min))
        if _bbox_matches(tile, points):
            return points

    raise RcsFormatError(
        f"узел {tile.index}: не удалось подобрать раскладку payload "
        f"({tile.payload_size} Б, {tile.count} точек)"
    )


def decode_all(container: Container) -> list[Point]:
    """Декодировать все узлы (импорт внутри функции — избегаем цикла)."""
    from rcs_blocks import iter_tiles

    points: list[Point] = []
    for tile in iter_tiles(container):
        points.extend(decode_tile(container, tile))
    return points
