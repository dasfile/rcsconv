"""Сверка контейнера с исходным .pts и JSON-отчёт."""

from __future__ import annotations

import json

from rcs_blocks import Container, bytes_per_point, dialect, tile_count, tile_record_size
from rcs_checks import Check, check_invariants, fmt3


def check_against_pts(container: Container, stats: dict, tol: float = 1e-3) -> list[Check]:
    header = container.header
    center_ok = all(abs(a - b) <= tol for a, b in zip(header.center, stats["center"]))
    extent_ok = all(
        abs(abs(a) - b) <= tol for a, b in zip(header.extent_max, stats["half_extent"])
    )
    return [
        Check(
            "центр bbox в заголовке = .pts",
            center_ok,
            f"rcs={fmt3(header.center)} pts={fmt3(stats['center'])} (tol {tol})",
        ),
        Check(
            "полуразмеры bbox = .pts",
            extent_ok,
            f"rcs={fmt3(header.extent_max)} pts={fmt3(stats['half_extent'])} (tol {tol})",
        ),
        Check(
            "count в заголовке = число точек .pts",
            header.count == stats["count"],
            f"rcs={header.count} pts={stats['count']}",
        ),
    ]


def _jsonable(value):
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    if isinstance(value, dict):
        return {key: _jsonable(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_jsonable(item) for item in value]
    return value


def summary(container: Container, stats: dict | None = None) -> dict:
    header = container.header
    checks = check_invariants(container)
    if stats:
        checks.extend(check_against_pts(container, stats))
    report = {
        "path": str(container.path) if container.path else None,
        "size": container.size,
        "dialect": dialect(container),
        "header": {
            "word4": header.word4,
            "word8": header.word8,
            "word12": header.word12,
            "center": header.center,
            "unit": header.unit,
            "root_min": header.root_min,
            "root_max": header.root_max,
            "extent_min": header.extent_min,
            "extent_max": header.extent_max,
            "chunk_id": header.chunk_id,
            "chunk_version": header.chunk_version,
            "count": header.count,
            "flags": header.flags.hex(),
            "guid": header.guid,
            "guid_offset": header.guid_offset,
        },
        "directory": {
            "offset": container.directory_offset,
            "trailing_zeros": container.blocks_trailing,
            "blocks": [
                {"offset": b.offset, "size": b.size, "type": b.type, "end": b.end}
                for b in container.blocks
            ],
        },
        "tiles": {
            "count": tile_count(container),
            "record_size": tile_record_size(container),
        },
        "bytes_per_point": bytes_per_point(container),
        "pts": (
            {
                "path": stats["path"],
                "count": stats["count"],
                "min": stats["min"],
                "max": stats["max"],
                "center": stats["center"],
                "half_extent": stats["half_extent"],
            }
            if stats
            else None
        ),
        "checks": [{"name": c.name, "ok": c.ok, "detail": c.detail} for c in checks],
        "ok": all(c.ok for c in checks),
    }
    return _jsonable(report)


def to_json(report: dict) -> str:
    return json.dumps(report, indent=2, ensure_ascii=False)


def to_text(report: dict) -> str:
    header = report["header"]
    lines = [
        f"Файл: {report['path']}  ({report['size']} байт, диалект {report['dialect']})",
        f"GUID: {header['guid']} (0x{header['guid_offset']:X})",
        f"chunk: id={header['chunk_id']} version={header['chunk_version']} "
        f"count={header['count']}",
        f"center: {fmt3(header['center'])}",
        f"root cube: {fmt3(header['root_min'])} .. {fmt3(header['root_max'])}",
        f"bbox half: {fmt3(header['extent_max'])}",
        f"директория: 0x{report['directory']['offset']:X}, "
        f"нулей-терминатор {report['directory']['trailing_zeros']} Б",
        "блоки (offset, size, type):",
    ]
    for block in report["directory"]["blocks"]:
        lines.append(
            f"  0x{block['offset']:06X} + {block['size']:>9}  type {block['type']:<3}"
        )
    tiles = report["tiles"]
    lines.append(f"тайлы: count={tiles['count']} record={tiles['record_size']}")
    ratio = report["bytes_per_point"]
    lines.append(f"type 11: {ratio:.3f} Б/точка" if ratio else "type 11: нет данных")
    if report.get("pts"):
        pts = report["pts"]
        lines.append(
            f".pts: {pts['path']} count={pts['count']} center={fmt3(pts['center'])} "
            f"half={fmt3(pts['half_extent'])}"
        )
    lines.append("проверки:")
    for check in report["checks"]:
        mark = "OK  " if check["ok"] else "FAIL"
        lines.append(f"  [{mark}] {check['name']}: {check['detail']}")
    lines.append(f"ИТОГ: {'все проверки пройдены' if report['ok'] else 'есть расхождения'}")
    return "\n".join(lines)
