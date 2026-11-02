#!/usr/bin/env python3
"""Чтение проекта ReCap (`.rcp`): контейнер ZIP + XML-модель.

ПОДТВЕРЖДЕНО (13 проектов корпуса, docs/RCP-FORMAT.md):
  * `.rcp` — ZIP ровно из трёх записей: `<guid>.jpg`, `<guid>.xml`,
    `rs_screenshot__.bmp`;
  * точек внутри нет — XML ссылается на `<имя> Support/<имя>.rcs`;
  * `ScanImportSettings` хранит фактические настройки импорта, включая
    `density` = `Decimation grid` в мм (нет атрибута = 1 мм по умолчанию).

Примеры:
    python tools/rcp_info.py golden/probe6.rcp
    python tools/rcp_info.py "golden/*.rcp" --json
    python tools/rcp_info.py golden/level_7.rcp --entries
    python tools/rcp_info.py golden/level_7.rcp --dump-xml out
"""

from __future__ import annotations

import argparse
import json
import sys
import zipfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from xml.etree import ElementTree

from cli_paths import expand_sources, has_suffix

XML_SUFFIX = ".xml"
KIND_UNKNOWN = 11  # TextImportColumn index="11" = «колонка не используется»


@dataclass(frozen=True)
class ImportSettings:
    """`ScanImportSettings` из XML (None = атрибута нет, значение по умолчанию)."""

    unit_scale: float | None = None
    skip_lines: int | None = None
    separator: str | None = None
    columns: tuple[int, ...] = ()
    intensity_clip: int | None = None
    density: int | None = None          # мм; None = 1 мм по умолчанию
    up_direction: str | None = None


@dataclass(frozen=True)
class Scan:
    """Один скан проекта (`ShotInfo` + `VoxelTreeRunTime`)."""

    name: str
    points: int | None = None
    has_rgb: bool = False
    has_normals: bool = False
    has_intensity: bool = False
    raw_path: str | None = None
    source_type: str | None = None
    translation: tuple[float, float, float] | None = None
    settings: ImportSettings = field(default_factory=ImportSettings)


@dataclass(frozen=True)
class Project:
    """Что мы умеем прочитать из `.rcp`."""

    path: Path
    entries: tuple[tuple[str, int], ...]
    project_id: str | None = None
    version: tuple[int, int, int] | None = None
    application: str | None = None
    application_version: str | None = None
    saved_at: tuple[str, ...] = ()
    user: str | None = None
    unit_type: int | None = None
    unit_precision: int | None = None
    scans: tuple[Scan, ...] = ()


def _text(node: ElementTree.Element | None, attribute: str) -> str | None:
    """Значение атрибута (None, если узла или атрибута нет)."""
    return None if node is None else node.get(attribute)


def _number(value: str | None) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except ValueError:
        return None


def _integer(value: str | None) -> int | None:
    number = _number(value)
    return None if number is None else int(number)


def _flag(node: ElementTree.Element | None, attribute: str = "Value") -> bool:
    return _text(node, attribute) == "1"


def _settings(node: ElementTree.Element | None) -> ImportSettings:
    """Разобрать блок `ScanImportSettings` (берём первый найденный)."""
    if node is None:
        return ImportSettings()
    text = node.find("ScanImportTextSettings")
    advanced = node.find("ScanImportAdvancedSettings")
    clip = node.find("ScanImportFilterSettings/IntensityClip")
    columns = tuple(
        int(column.get("index", KIND_UNKNOWN))
        for column in ([] if text is None else text.findall("TextImportColumn"))
    )
    return ImportSettings(
        unit_scale=_number(_text(text, "unitScale")),
        skip_lines=_integer(_text(text, "skipLines")),
        separator=_text(text, "separator"),
        columns=columns,
        intensity_clip=_integer(_text(clip, "max")),
        density=_integer(_text(advanced, "density")),
        up_direction=_text(advanced, "upDirection"),
    )


def _scan(shot: ElementTree.Element, tree_node: ElementTree.Element | None) -> Scan:
    """Скан из `ShotInfo` + соответствующий ему `VoxelTreeRunTime`."""
    translation = None
    transform = shot.find("OrientedBoundingBox/tform/T")
    if transform is not None:
        translation = tuple(
            0.0 if _number(transform.get(axis)) is None else _number(transform.get(axis))
            for axis in "xyz"
        )
    node = tree_node if tree_node is not None else ElementTree.Element("empty")
    return Scan(
        name=shot.get("name") or "",
        points=_integer(_text(node.find("NumPoints"), "Value")),
        has_rgb=_flag(node.find("HasRGB")),
        has_normals=_flag(node.find("HasNormals")),
        has_intensity=_flag(node.find("HasIntensity")),
        raw_path=shot.get("rawScanPath"),
        source_type=shot.get("type"),
        translation=translation,  # type: ignore[arg-type]
        settings=_settings(shot.find("ScanImportSettings")),
    )


def read_project(path: str | Path) -> Project:
    """Прочитать `.rcp`: записи ZIP, версию проекта, сканы и настройки импорта."""
    path = Path(path)
    if not has_suffix(path, ".rcp"):
        raise ValueError(f"{path}: ожидался .rcp")
    with zipfile.ZipFile(path) as archive:
        entries = tuple((item.filename, item.file_size) for item in archive.infolist())
        xml_names = [name for name, _ in entries if name.lower().endswith(XML_SUFFIX)]
        if not xml_names:
            raise ValueError(f"{path}: в ZIP нет XML-модели проекта")
        root = ElementTree.fromstring(archive.read(xml_names[0]))

    trees = {}
    for node in root.findall("Nodes/VoxelTreeRunTime"):
        name = _text(node.find("NodeName"), "Value") or _text(node, "Id")
        trees[name] = node
    metadata = root.find("ProjectMetaData")
    saves = [] if metadata is None else metadata.findall("SaveInformation")
    last = saves[-1] if saves else None
    project = root.find("Project")
    global_settings = None if metadata is None else metadata.find("ScanImportSettings")
    shots = [] if project is None else project.findall("ShotInfo")
    scans = []
    for shot in shots:
        scan = _scan(shot, trees.get(shot.get("name")))
        if scan.settings == ImportSettings() and global_settings is not None:
            scan = Scan(**{**asdict(scan), "settings": _settings(global_settings)})
        scans.append(scan)

    unit = root.find("UnitType")
    version: tuple[int, int, int] | None = None
    if metadata is not None and metadata.get("MajorVersion") is not None:
        version = (
            int(metadata.get("MajorVersion", 0)),
            int(metadata.get("MinorVersion", 0)),
            int(metadata.get("PatchVersion", 0)),
        )
    return Project(
        path=path,
        entries=entries,
        project_id=_text(root.find("ProjectId"), "UUID"),
        version=version,
        application=_text(last, "Application"),
        application_version=_text(last, "ApplicationVersion"),
        saved_at=tuple(save.get("Timestamp", "") for save in saves),
        user=_text(last, "UserId"),
        unit_type=_integer(_text(unit, "Value")),
        unit_precision=_integer(_text(root.find("UnitPrecision"), "Value")),
        scans=tuple(scans),
    )


def describe(project: Project) -> str:
    """Одна строка на проект + строка на каждый скан."""
    lines = [
        f"{project.path.name}: записей {len(project.entries)}, "
        f"{project.application or '?'} {project.application_version or '?'}, "
        f"XML {'.'.join(str(v) for v in (project.version or ()))}"
    ]
    if project.saved_at:
        lines.append(
            f"  сохранений {len(project.saved_at)}, последнее {project.saved_at[-1]}"
            f" ({project.user or '?'})"
        )
    for scan in project.scans:
        settings = scan.settings
        density = (
            "1 мм (по умолчанию)"
            if settings.density is None
            else f"{settings.density} мм"
        )
        skip = 0 if settings.skip_lines is None else settings.skip_lines
        used = len([column for column in settings.columns if column != KIND_UNKNOWN])
        points = "?" if scan.points is None else str(scan.points)
        lines.append(
            f"  {scan.name}: точек {points}, RGB {int(scan.has_rgb)}, "
            f"нормали {int(scan.has_normals)}, интенсивность {int(scan.has_intensity)}"
        )
        lines.append(
            f"    импорт: единицы {settings.unit_scale}, skip {skip}, "
            f"sep {settings.separator!r}, колонок {used}, "
            f"clip {settings.intensity_clip}, decimation {density}, "
            f"up {settings.up_direction}"
        )
        if scan.raw_path:
            lines.append(f"    исходник: {scan.raw_path}")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(
        description="Чтение проекта ReCap (.rcp): ZIP + XML-модель",
        epilog='Пример: rcp_info.py "golden/*.rcp" --json',
    )
    parser.add_argument("sources", nargs="+", help="файлы .rcp (допускаются маски)")
    parser.add_argument("-j", "--json", action="store_true", help="вывод в JSON")
    parser.add_argument(
        "-e", "--entries", action="store_true", help="только записи ZIP"
    )
    parser.add_argument(
        "-d", "--dump-xml", metavar="DIR", help="распаковать XML в каталог"
    )
    args = parser.parse_args(argv)

    sources = expand_sources(args.sources, ".rcp")
    missing = [path for path in sources if not path.exists()]
    if missing:
        print(f"Ошибка: нет файла {missing[0]}", file=sys.stderr)
        return 2
    if not sources:
        print("Ошибка: входные .rcp не найдены", file=sys.stderr)
        return 2

    if args.entries:
        for path in sources:
            with zipfile.ZipFile(path) as archive:
                print(f"{path.name}:")
                for item in archive.infolist():
                    print(f"  {item.file_size:>10}  {item.filename}")
        return 0

    if args.dump_xml:
        folder = Path(args.dump_xml)
        folder.mkdir(parents=True, exist_ok=True)
        for path in sources:
            with zipfile.ZipFile(path) as archive:
                for item in archive.infolist():
                    if item.filename.lower().endswith(XML_SUFFIX):
                        target = folder / f"{path.stem}.xml"
                        target.write_bytes(archive.read(item.filename))
                        print(f"{path.name} → {target}")

    projects = [read_project(path) for path in sources]
    if args.json:
        payload = []
        for project in projects:
            data = asdict(project)
            data["path"] = str(project.path)
            payload.append(data)
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        for project in projects:
            print(describe(project))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

