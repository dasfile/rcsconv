"""Парсер контейнера Autodesk ReCap (ADOCT / RCS).

Реализовано только то, что подтверждено байтами на golden/reference/*.rcs.
Всё неподтверждённое помечено как гипотеза (см. docs/RCS-FORMAT.md).

Little-endian. Заголовок НЕ выровнен на 8 байт: директория блоков начинается
с нечётного смещения (0x135 в эталонах), поэтому её положение ищется подбором
по инварианту «блоки стыкуются и заканчиваются ровно на EOF».
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field
from pathlib import Path
from typing import Sequence

MAGIC = b"ADOCT"
DIR_RECORD_SIZE = 24     # (offset u64, size u64, type u64)
TILE_RECORD_SIZE = 368   # подтверждено делением на всех эталонах
MAX_BLOCKS = 64
DIR_SCAN_WINDOW = 0x40   # сколько байт после GUID пробуем как начало директории

# Диалекты контейнера (см. docs/RCS-FORMAT.md, раздел «Два диалекта»)
#   raw    — скан сразу после импорта: chunk_version 2, есть JPEG-превью (type 0)
#   export — Unified RCS: chunk_version 5, TLV в type 0, превью нет
BLOCK_LAYOUTS = {
    (3, 1, 11, 5, 0): "raw",
    (3, 1, 11, 0): "export",
}
CHUNK_VERSIONS = {2: "raw", 5: "export"}
JPEG_SOI = b"\xff\xd8\xff"


class RcsFormatError(Exception):
    """Файл не соответствует ожидаемому контейнеру ADOCT."""


# --------------------------------------------------------------------------- #
#  Структуры
# --------------------------------------------------------------------------- #

@dataclass(frozen=True)
class Block:
    offset: int
    size: int
    type: int

    @property
    def end(self) -> int:
        return self.offset + self.size


@dataclass(frozen=True)
class Tlv:
    id: int
    offset: int
    size: int
    payload: bytes
    children: tuple["Tlv", ...] = ()


@dataclass(frozen=True)
class Header:
    word4: int                                 # 0x04: 84 во всех эталонах
    word8: int                                 # 0x08: 3 во всех (НЕ число точек)
    word12: int                                # 0x0C: 2 во всех
    center: tuple[float, float, float]         # 0x10 — центр bbox
    zeros28: tuple[float, float, float]        # 0x28 — нули
    unit: tuple[float, float, float]           # 0x40 — 1,1,1
    center2: tuple[float, float, float]        # 0x58 — повтор центра
    root_min: tuple[float, float, float]       # 0x70 — -R (корневой куб)
    root_max: tuple[float, float, float]       # 0x88 — +R
    extent_min: tuple[float, float, float]     # 0xA0 — -(hx,hy,hz)
    extent_max: tuple[float, float, float]     # 0xB8 — +(hx,hy,hz)
    chunk_id: int                              # 0xD0
    chunk_version: int                         # 0xD4 = 5
    count: int                                 # 0xD8 — гипотеза: число точек
    flags: bytes                               # 0xE0..0xE3 = 01 01 00
    guid: str                                  # "{xxxxxxxx_xxxx_...}"
    guid_offset: int

    @property
    def guid_end(self) -> int:
        return self.guid_offset + len(self.guid)


@dataclass
class Container:
    path: Path | None
    size: int
    header: Header
    directory_offset: int
    blocks: list[Block]
    blocks_trailing: int = 0     # нули-терминатор директории, байт
    raw: bytes = field(default=b"", repr=False)

    def block_of_type(self, type_id: int) -> Block | None:
        for block in self.blocks:
            if block.type == type_id:
                return block
        return None

    def block_types(self) -> list[int]:
        return [block.type for block in self.blocks]


# --------------------------------------------------------------------------- #
#  Заголовок
# --------------------------------------------------------------------------- #

def _f64x3(data: bytes, offset: int) -> tuple[float, float, float]:
    return struct.unpack_from("<3d", data, offset)


def _read_guid(data: bytes, search_from: int) -> tuple[str, int]:
    """GUID хранится строкой '{8_4_4_4_12}' без префикса длины."""
    limit = min(len(data), search_from + 0x80)
    start = data.find(b"{", search_from, limit)
    if start < 0:
        raise RcsFormatError("не найден GUID '{...}' в заголовке")
    end = data.find(b"}", start, limit)
    if end < 0:
        raise RcsFormatError("GUID не закрыт символом '}'")
    try:
        guid = data[start : end + 1].decode("ascii")
    except UnicodeDecodeError as exc:  # pragma: no cover
        raise RcsFormatError(f"GUID не является ASCII: {exc}") from exc
    return guid, start


def parse_header(data: bytes) -> Header:
    if len(data) < 0x120:
        raise RcsFormatError(f"файл слишком мал для заголовка: {len(data)} байт")
    if data[:5] != MAGIC or data[5:8] != b"\x00\x00\x00":
        raise RcsFormatError(
            f"неверная сигнатура: {data[:8]!r}, ожидалось b'ADOCT\\x00\\x00\\x00'"
        )

    word4, word8, word12 = struct.unpack_from("<III", data, 0x04)
    chunk_id, chunk_version, count = struct.unpack_from("<IIQ", data, 0xD0)
    guid, guid_offset = _read_guid(data, 0xE0)

    return Header(
        word4=word4,
        word8=word8,
        word12=word12,
        center=_f64x3(data, 0x10),
        zeros28=_f64x3(data, 0x28),
        unit=_f64x3(data, 0x40),
        center2=_f64x3(data, 0x58),
        root_min=_f64x3(data, 0x70),
        root_max=_f64x3(data, 0x88),
        extent_min=_f64x3(data, 0xA0),
        extent_max=_f64x3(data, 0xB8),
        chunk_id=chunk_id,
        chunk_version=chunk_version,
        count=count,
        flags=data[0xE0:0xE3],
        guid=guid,
        guid_offset=guid_offset,
    )


# --------------------------------------------------------------------------- #
#  Директория блоков
# --------------------------------------------------------------------------- #

def _walk_directory(data: bytes, start: int) -> list[Block] | None:
    """Читать записи (offset,size,type), пока не встретится полностью нулевая."""
    blocks: list[Block] = []
    offset = start
    while True:
        if offset + DIR_RECORD_SIZE > len(data):
            return None
        off, size, type_id = struct.unpack_from("<QQQ", data, offset)
        if off == 0 and size == 0 and type_id == 0:
            break
        if size == 0 or off < 0x100 or off + size > len(data):
            return None
        blocks.append(Block(offset=off, size=size, type=type_id))
        offset += DIR_RECORD_SIZE
        if len(blocks) > MAX_BLOCKS:
            return None
    return blocks or None


def _directory_is_valid(blocks: Sequence[Block], start: int, file_size: int) -> bool:
    """Инвариант: блоки по возрастанию, стыкуются, кончаются ровно на EOF."""
    previous_end = None
    for block in blocks:
        if block.offset < start:
            return False
        if previous_end is not None and block.offset != previous_end:
            return False
        previous_end = block.end
    return previous_end == file_size


def find_directory(data: bytes, header: Header) -> tuple[int, list[Block]]:
    """Найти директорию: она идёт после GUID с точностью до паддинга.

    Выравнивания нет, поэтому перебираем кандидатов в окне и принимаем первого,
    для которого выполняется инвариант покрытия файла.
    """
    file_size = len(data)
    window_end = min(header.guid_end + DIR_SCAN_WINDOW, file_size)
    for start in range(header.guid_end, window_end):
        blocks = _walk_directory(data, start)
        if blocks and _directory_is_valid(blocks, start, file_size):
            return start, blocks
    raise RcsFormatError(
        "директория блоков не найдена: ни один кандидат не покрывает файл целиком"
    )


# --------------------------------------------------------------------------- #
#  TLV (блок type 0 — словарь свойств; в id=10 вложенный TLV)
# --------------------------------------------------------------------------- #

def _try_tlv(blob: bytes, depth: int) -> list[Tlv] | None:
    entries = parse_tlv(blob, depth=depth)
    if not entries:
        return None
    last = entries[-1]
    if last.offset + 8 + last.size != len(blob):
        return None
    return entries


def parse_tlv(blob: bytes, depth: int = 0, base: int = 0) -> list[Tlv]:
    """Поток (id u32, size u32, payload[size]) до первого несоответствия."""
    entries: list[Tlv] = []
    offset = 0
    limit = len(blob)
    while offset + 8 <= limit:
        ident, size = struct.unpack_from("<II", blob, offset)
        if size > limit - offset - 8:
            break
        payload = blob[offset + 8 : offset + 8 + size]
        children: tuple[Tlv, ...] = ()
        if depth < 3 and size >= 8:
            nested = _try_tlv(payload, depth + 1)
            if nested:
                children = tuple(nested)
        entries.append(
            Tlv(id=ident, offset=base + offset, size=size, payload=payload, children=children)
        )
        offset += 8 + size
    return entries


# --------------------------------------------------------------------------- #
#  Сборка контейнера
# --------------------------------------------------------------------------- #

def parse_bytes(data: bytes, path: Path | None = None) -> Container:
    header = parse_header(data)
    dir_offset, blocks = find_directory(data, header)
    consumed = dir_offset + DIR_RECORD_SIZE * len(blocks)
    trailing = blocks[0].offset - consumed
    return Container(
        path=path,
        size=len(data),
        header=header,
        directory_offset=dir_offset,
        blocks=blocks,
        blocks_trailing=max(0, trailing),
        raw=data,
    )


def parse_file(path: str | Path) -> Container:
    path = Path(path)
    return parse_bytes(path.read_bytes(), path=path)


def read_block(container: Container, block: Block) -> bytes:
    return container.raw[block.offset : block.end]


# --------------------------------------------------------------------------- #
#  Производные величины
# --------------------------------------------------------------------------- #

def tile_count(container: Container) -> int | None:
    block = container.block_of_type(3)
    if block is None or block.size < 8:
        return None
    return struct.unpack_from("<Q", container.raw, block.offset)[0]


def tile_record_size(container: Container) -> int | None:
    block = container.block_of_type(3)
    count = tile_count(container)
    if block is None or not count:
        return None
    payload = block.size - 8
    return payload // count if payload % count == 0 else None


def bytes_per_point(container: Container) -> float | None:
    block = container.block_of_type(11)
    count = container.header.count
    if block is None or not count:
        return None
    return block.size / count


def block_tlvs(container: Container, block: Block) -> list[Tlv]:
    return parse_tlv(read_block(container, block))


# --------------------------------------------------------------------------- #
#  Диалекты и вспомогательные выборки
# --------------------------------------------------------------------------- #

def dialect(container: Container) -> str:
    """'raw' (импорт), 'export' (Unified RCS) или 'unknown'."""
    kind = CHUNK_VERSIONS.get(container.header.chunk_version)
    if kind is None:
        kind = BLOCK_LAYOUTS.get(tuple(container.block_types()), "unknown")
    return kind


def is_jpeg(blob: bytes) -> bool:
    """type 0 в raw-диалекте — JPEG-превью скана."""
    return blob.startswith(JPEG_SOI)


def tlv_block(container: Container) -> Block | None:
    """Блок со свойствами: type 5 (raw) или type 0 (export, если это не JPEG)."""
    block = container.block_of_type(5)
    if block is not None:
        return block
    block = container.block_of_type(0)
    if block is not None and not is_jpeg(read_block(container, block)):
        return block
    return None


def preview_block(container: Container) -> Block | None:
    block = container.block_of_type(0)
    if block is not None and is_jpeg(read_block(container, block)):
        return block
    return None


# --------------------------------------------------------------------------- #
#  Дерево узлов (блок type 3)
# --------------------------------------------------------------------------- #

@dataclass(frozen=True)
class TileInfo:
    """Запись узла октодерева (368 байт) из блока type 3.

    ПОДТВЕРЖДЕНО (см. docs/RCS-FORMAT.md):
      * cube_min/cube_max — куб узла (f64[0:6]);
      * points_min/points_max — реальный bbox точек узла (f64[6:12]);
        на одноточечных пробах обе тройки совпадают с координатой точки;
      * count — число точек узла (u32 @96+4); на probe6/test1/test2 сумма = 3;
      * payload_size — размер данных узла в блоке type 11 (u32 @96+8);
        сумма по всем узлам ровно равна размеру блока type 11.

    ГИПОТЕЗА: nodes идут в том же порядке, что и payload'ы в type 11;
    поля header_u32[0] (1..12) и header_u32[4:6] — не расшифрованы;
    массивы после @96 не разобраны.
    """

    index: int
    cube_min: tuple[float, float, float]
    cube_max: tuple[float, float, float]
    points_min: tuple[float, float, float]
    points_max: tuple[float, float, float]
    count: int
    payload_size: int
    payload_offset: int
    header_u32: tuple[int, ...]
    tail_u32: tuple[int, ...]


def iter_tiles(container: Container) -> list[TileInfo]:
    """Разобрать блок type 3. Пустой список, если блока нет."""
    block = container.block_of_type(3)
    if block is None or block.size < 8:
        return []

    count = struct.unpack_from("<Q", container.raw, block.offset)[0]
    if 8 + count * TILE_RECORD_SIZE > block.size:
        raise RcsFormatError(
            f"блок type 3: {count} × {TILE_RECORD_SIZE} не помещается в {block.size} байт"
        )

    tiles: list[TileInfo] = []
    payload_offset = 0
    for index in range(count):
        offset = block.offset + 8 + index * TILE_RECORD_SIZE
        cube_min = _f64x3(container.raw, offset)
        cube_max = _f64x3(container.raw, offset + 24)
        points_min = _f64x3(container.raw, offset + 48)
        points_max = _f64x3(container.raw, offset + 72)
        header = struct.unpack_from("<8I", container.raw, offset + 96)
        tail = struct.unpack_from("<60I", container.raw, offset + 128)

        tiles.append(
            TileInfo(
                index=index,
                cube_min=cube_min,
                cube_max=cube_max,
                points_min=points_min,
                points_max=points_max,
                count=header[1],
                payload_size=header[2],
                payload_offset=payload_offset,
                header_u32=header,
                tail_u32=tail,
            )
        )
        payload_offset += header[2]
    return tiles


def tile_payload(container: Container, tile: TileInfo) -> bytes:
    """Срез данных узла в блоке type 11 (порядок узлов — гипотеза)."""
    block = container.block_of_type(11)
    if block is None:
        raise RcsFormatError("нет блока type 11")
    start = block.offset + tile.payload_offset
    return container.raw[start : start + tile.payload_size]

