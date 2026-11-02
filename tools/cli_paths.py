"""Общие утилиты CLI: разворачивание входов и подстановка имени выхода.

Правило интерфейса (см. README): первый аргумент — вход, имя выхода утилита
делает сама, подставляя нужное расширение. Входом может быть файл, каталог
(все файлы с нужным расширением внутри) или маска `*.pts`.
"""

from __future__ import annotations

import glob
from pathlib import Path
from typing import Iterable


def has_suffix(path: str | Path, suffix: str) -> bool:
    """Совпадает ли расширение файла (регистр не важен)."""
    return Path(path).suffix.lower() == suffix.lower()


def expand_sources(
    patterns: Iterable[str], suffix: str, recursive: bool = False
) -> list[Path]:
    """Развернуть аргументы CLI в список входных файлов с нужным расширением.

    Принимает файлы, каталоги (все `*suffix` внутри) и маски. Порядок: как в
    аргументах, внутри каталога — по имени. Дубликаты убираются.
    """
    found: list[Path] = []
    for pattern in patterns:
        path = Path(pattern)
        if path.is_dir():
            mask = f"**/*{suffix}" if recursive else f"*{suffix}"
            found.extend(sorted(item for item in path.glob(mask) if item.is_file()))
            continue
        matches = sorted(glob.glob(pattern, recursive=recursive))
        if matches:
            found.extend(Path(match) for match in matches if Path(match).is_file())
        else:
            found.append(path)

    unique: list[Path] = []
    seen: set[Path] = set()
    for path in found:
        key = path.resolve() if path.exists() else path
        if key in seen:
            continue
        seen.add(key)
        unique.append(path)
    return unique


def default_target(
    source: str | Path, suffix: str, out_dir: str | Path | None = None
) -> Path:
    """Имя выхода: то же имя с другим расширением (и, если задано, в `out_dir`)."""
    target = Path(source).with_suffix(suffix)
    return Path(out_dir) / target.name if out_dir else target


def is_same_file(left: str | Path, right: str | Path) -> bool:
    """Указывают ли пути на один и тот же файл (без падения на несуществующих)."""
    left_path, right_path = Path(left), Path(right)
    try:
        return left_path.exists() and right_path.exists() and left_path.samefile(right_path)
    except OSError:
        return left_path.resolve() == right_path.resolve()
