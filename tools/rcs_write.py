#!/usr/bin/env python3
"""Сериализация контейнера RCS (ADOCT): `Container` → байты.

ПОДТВЕРЖДЕНО (2026-10-29): `read → write` даёт побайтово идентичный файл на всех
эталонах (`golden/*.rcs`, `golden/reference/*.rcs`), кроме строки GUID.

Раскладка файла (Little-endian, выравнивания нет):

    0x000 … 0x109   ADOCT-заголовок (поля — см. `rcs_blocks.Header`)
    0x109 … 0x129   нули
    0x129           u32 blockCount
    0x12D           u32 = 2
    0x131 … 0x135   нули
    0x135           директория: blockCount записей `(offset u64, size u64, type u64)`
    0x141 … 0x1812D нули: терминатор директории и резерв
    0x1812D … EOF   блоки подряд, в порядке директории

Блоки:
    type 3  — дерево узлов (`u64 count` + `count × 368`) — собирается из `TileInfo`;
    type 11 — данные точек (payload'ы узлов подряд) — собирается из payload'ов;
    type 1, 0, 5 — копируются как есть: их структура не разобрана.

Примеры:
    python tools/rcs_write.py golden/level_7.rcs out/level_7_copy.rcs
    python tools/rcs_write.py golden/probe6.rcs              # только самопроверка
"""

from __future__ import annotations

import argparse
import re
import struct
import sys
import uuid
from pathlib import Path

from rcs_blocks import (
    Container,
    RcsFormatError,
    iter_tiles,
    parse_file,
    read_block,
    tile_payload,
)

MAGIC = b"ADOCT\x00\x00\x00"
GUID_OFFSET = 0xE3          # начало строки GUID '{8_4_4_4_12}'
GUID_LENGTH = 38
HEADER_END = 0x135          # начало директории блоков
BLOCK_COUNT_OFFSET = 0x129  # u32; проверено: равно числу блоков
HEADER_WORD_0x12D = 2       # u32; во всех эталонах 2
FIRST_BLOCK_OFFSET = 0x1812D
DIR_RECORD_SIZE = 24
TILE_RECORD_SIZE = 368
BODY_SIZE = 16
METADATA_WORD_COUNT = 32     # u32 в заголовке блока type 1
METADATA_HEAD_SIZE = 4 * METADATA_WORD_COUNT   # 128 байт заголовка блока type 1
METADATA_RECORD_SIZE = 72    # байт на запись в блоке type 1

# GUID в RCS — строка из 38 символов: {8_4_4_4_12}, нижний регистр, подчёркивания.
GUID_PATTERN = re.compile(
    r"^\{[0-9a-fA-F]{8}[-_][0-9a-fA-F]{4}[-_][0-9a-fA-F]{4}"
    r"[-_][0-9a-fA-F]{4}[-_][0-9a-fA-F]{12}\}$"
)


def make_guid() -> str:
    """Случайный GUID в формате ReCap (`{8_4_4_4_12}`)."""
    return "{%s}" % str(uuid.uuid4()).replace("-", "_")


def normalize_guid(text: str) -> str:
    """Проверить GUID и привести к виду ReCap: подчёркивания, нижний регистр.

    Принимаются и дефисы (привычный вид uuid4), на выходе всегда подчёркивания.
    """
    value = text.strip()
    if not GUID_PATTERN.match(value):
        raise ValueError(
            f"GUID должен быть в формате {{8_4_4_4_12}}, получено {text!r}"
        )
    return value.replace("-", "_").lower()


def _header_bytes(header: Header, guid: str, block_count: int) -> bytes:
    """Собрать ADOCT-заголовок (первые `HEADER_END` байт) из полей `Header`."""
    if header.guid_offset != GUID_OFFSET:
        raise RcsFormatError(
            f"GUID на {header.guid_offset:#x}, ожидалось {GUID_OFFSET:#x}: "
            "раскладка заголовка другая"
        )
    out = bytearray(HEADER_END)
    out[0:8] = MAGIC
    struct.pack_into("<III", out, 0x04, header.word4, header.word8, header.word12)
    for offset, triple in (
        (0x10, header.center),
        (0x28, header.zeros28),
        (0x40, header.unit),
        (0x58, header.center2),
        (0x70, header.root_min),
        (0x88, header.root_max),
        (0xA0, header.extent_min),
        (0xB8, header.extent_max),
    ):
        struct.pack_into("<3d", out, offset, *triple)
    struct.pack_into(
        "<IIQ", out, 0xD0, header.chunk_id, header.chunk_version, header.count
    )
    out[0xE0:0xE3] = header.flags
    if len(guid) != GUID_LENGTH:
        raise RcsFormatError(f"GUID должен быть {GUID_LENGTH} байт, получено {len(guid)}")
    out[GUID_OFFSET : GUID_OFFSET + GUID_LENGTH] = guid.encode("ascii")
    struct.pack_into("<II", out, BLOCK_COUNT_OFFSET, block_count, HEADER_WORD_0x12D)
    return bytes(out)


def build_header(
    container: Container, *, guid: str | None = None, block_count: int | None = None
) -> bytes:
    """Собрать ADOCT-заголовок контейнера (первые `HEADER_END` байт файла)."""
    header = container.header
    return _header_bytes(
        header,
        normalize_guid(guid) if guid is not None else header.guid,
        len(container.blocks) if block_count is None else block_count,
    )


def build_tile_block(container: Container) -> bytes:
    """Собрать блок type 3 из разобранных записей узлов."""
    tiles = iter_tiles(container)
    out = bytearray(struct.pack("<Q", len(tiles)))
    for tile in tiles:
        for triple in (
            tile.cube_min,
            tile.cube_max,
            tile.points_min,
            tile.points_max,
        ):
            out += struct.pack("<3d", *triple)
        out += struct.pack("<8I", *tile.header_u32)
        out += struct.pack(f"<{len(tile.tail_u32)}I", *tile.tail_u32)
        if len(out) - 8 != TILE_RECORD_SIZE * (tile.index + 1):
            raise RcsFormatError(f"узел {tile.index}: запись не {TILE_RECORD_SIZE} Б")
    return bytes(out)


def build_points_block(container: Container) -> bytes:
    """Собрать блок type 11: payload'ы узлов подряд."""
    return b"".join(tile_payload(container, tile) for tile in iter_tiles(container))


def build_metadata_record() -> bytes:
    """Одна запись блока метаданных (72 байта): пустой диапазон атрибута.

    ПОДТВЕРЖДЕНО: во всех 13 эталонах все записи идентичны —
    `u32 0, u32 0, +DBL_MAX ×3, −DBL_MAX ×3, u32 0, -1, -1, 0`.
    Смысл полей — ГИПОТЕЗА (диапазон значений атрибута у узла/уровня).
    """
    max_f = sys.float_info.max
    return struct.pack(
        "<II3d3dIIII", 0, 0, max_f, max_f, max_f, -max_f, -max_f, -max_f, 0,
        0xFFFFFFFF, 0xFFFFFFFF, 0,
    )


def build_metadata_block(
    words: tuple[int, ...] = (1,), records: int | None = None
) -> bytes:
    """Собрать блок метаданных (`type 1`): 128 байт заголовка + `records × 72`.

    ПОДТВЕРЖДЕНО (`docs/RCS-FORMAT.md` §4.3, `tools/analyze_type1.py`, 13 эталонов):

      * заголовок = 32 × u32, непустая часть — префикс (дальше нули);
      * `words[L]` — число **занятых узлов октодерева на уровне `L`**:
        `words[0] = 1` (корень), последнее слово = число листьев = число записей
        блока `type 3`; внутренние узлы в `type 3` не хранятся, но здесь посчитаны;
      * число записей `K = sum(words)` — полное число узлов октодерева
        (`probe6` → `(1, 3)`, 4 записи; `level_7` → `(1, 8, 13, 43)`, 65 записей;
        `level_7_orig` → `(1, 8, 17, 64, 316)`, 406 записей);
      * все записи идентичны — пустой диапазон атрибута (`±DOUBLE_MAX`).

    `records=None` → `sum(words)`. Другое число записей запрещено: эталоны не дают
    ни одного примера, где `K ≠ sum(words)`. По умолчанию — вырожденное дерево
    `(1,)` + 1 запись (200 Б): минимальный вариант, согласованный с инвариантом.

    ГИПОТЕЗА: запись соответствует одному узлу октодерева (проверить нечем —
    во всех файлах записи одинаковы), поэтому смысл их полей остаётся открытым.
    """
    if len(words) > METADATA_WORD_COUNT:
        raise RcsFormatError(
            f"в заголовке метаданных {METADATA_WORD_COUNT} слова, передано {len(words)}"
        )
    expected = sum(words)
    if records is None:
        records = expected
    elif records != expected:
        raise RcsFormatError(
            f"записей {records}, а сумма слов заголовка {expected}: "
            "в эталонах K = sum(words)"
        )
    head = struct.pack(
        f"<{METADATA_WORD_COUNT}I", *words, *([0] * (METADATA_WORD_COUNT - len(words)))
    )
    chunk = build_metadata_record()
    if len(chunk) != METADATA_RECORD_SIZE:
        raise RcsFormatError(
            f"запись метаданных {len(chunk)} Б вместо {METADATA_RECORD_SIZE}"
        )
    return head + chunk * records


# --------------------------------------------------------------------------- #
#  Блок свойств (type 0 в экспорте, type 5 в raw)
# --------------------------------------------------------------------------- #

PROPS_VERSION = 10          # ведущее u32 блока свойств во всех наших эталонах
PROPS_UPPER = -999999.0     # f32-заглушка «верхней границы» в экспорте
PROPS_LOWER = 999999.0      # f32-заглушка «нижней границы» в экспорте


def _tlv(ident: int, payload: bytes) -> bytes:
    return struct.pack("<II", ident, len(payload)) + payload


def build_properties_block(
    *, upper: float = PROPS_UPPER, lower: float = PROPS_LOWER
) -> bytes:
    """Собрать блок свойств (type 0 в экспорте) — ровно 134 байта.

    ПОДТВЕРЖДЕНО: в экспорте блок байт-в-байт одинаков у probe6/test1/test2/level_7,
    а его цепочка TLV разобрана (см. docs/RCS-FORMAT.md §3):
    `100` — u8 версия схемы атрибутов, `101/102` — f32 границы активного атрибута
    (в экспорте заглушки ∓999999), `203…206` — f64 ±π, ±π/2, `301` — u8 флаг.
    """
    import math

    block = bytearray(struct.pack("<I", PROPS_VERSION))
    block += _tlv(100, b"\x01")
    block += _tlv(101, struct.pack("<f", upper))
    block += _tlv(102, struct.pack("<f", lower))
    block += _tlv(201, struct.pack("<I", 0))
    block += _tlv(202, struct.pack("<I", 0))
    block += _tlv(203, struct.pack("<d", math.pi))
    block += _tlv(204, struct.pack("<d", -math.pi))
    block += _tlv(205, struct.pack("<d", math.pi / 2))
    block += _tlv(206, struct.pack("<d", -math.pi / 2))
    block += _tlv(301, b"\x00")
    return bytes(block)


def default_blocks(container: Container) -> list[bytes]:
    """Пересобрать блоки 3 и 11, остальные (1/0/5) скопировать как есть."""
    blocks: list[bytes] = []
    for block in container.blocks:
        if block.type == 3:
            blocks.append(build_tile_block(container))
        elif block.type == 11:
            blocks.append(build_points_block(container))
        else:
            blocks.append(read_block(container, block))
    return blocks


def build_file(
    header: Header,
    blocks: list[tuple[int, bytes]],
    *,
    guid: str | None = None,
    first_block_offset: int = FIRST_BLOCK_OFFSET,
) -> bytes:
    """Собрать файл с нуля: заголовок, директория, резерв, блоки.

    `blocks` — пары `(type, содержимое)` в порядке директории. Так писатель
    собирает файл без шаблона: все блоки генерируются.
    """
    if not blocks:
        raise RcsFormatError("нечего писать: список блоков пуст")
    directory_end = HEADER_END + DIR_RECORD_SIZE * len(blocks)
    if directory_end > first_block_offset:
        raise RcsFormatError(
            f"директория ({directory_end} Б) не помещается до первого блока "
            f"({first_block_offset})"
        )

    guid_text = normalize_guid(guid) if guid is not None else header.guid
    out = bytearray(_header_bytes(header, guid_text, len(blocks)))
    offset = first_block_offset
    entries = bytearray()
    for type_id, chunk in blocks:
        entries += struct.pack("<QQQ", offset, len(chunk), type_id)
        offset += len(chunk)
    out += entries
    out += bytes(first_block_offset - len(out))   # терминатор директории + резерв
    for _, chunk in blocks:
        out += chunk
    return bytes(out)


def serialize(
    container: Container,
    *,
    blocks: list[bytes] | None = None,
    guid: str | None = None,
    first_block_offset: int = FIRST_BLOCK_OFFSET,
) -> bytes:
    """Собрать файл из контейнера: блоки 3 и 11 пересобираются, остальные копируются.

    `blocks` — подмена содержимого блоков (по порядку, как в `container.blocks`).
    """
    data = default_blocks(container) if blocks is None else list(blocks)
    if len(data) != len(container.blocks):
        raise RcsFormatError(
            f"получено {len(data)} блоков, в контейнере {len(container.blocks)}"
        )
    typed = [(block.type, chunk) for block, chunk in zip(container.blocks, data)]
    return build_file(
        container.header, typed, guid=guid, first_block_offset=first_block_offset
    )


def diff_ranges(left: bytes, right: bytes) -> list[tuple[int, int]]:
    """Непрерывные интервалы (начало, конец включительно), где байты различаются."""
    if len(left) != len(right):
        raise RcsFormatError(f"размеры различаются: {len(left)} против {len(right)}")
    ranges: list[tuple[int, int]] = []
    for number in range(len(left)):
        if left[number] != right[number]:
            if ranges and ranges[-1][1] + 1 == number:
                ranges[-1] = (ranges[-1][0], number)
            else:
                ranges.append((number, number))
    return ranges


def write_file(container: Container, path: str | Path, *, guid: str | None = None) -> bytes:
    """Сериализовать контейнер и записать в файл; вернуть записанные байты."""
    blob = serialize(container, guid=guid)
    Path(path).write_bytes(blob)
    return blob


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description="Сериализация RCS: read → write")
    parser.add_argument("source", help="исходный .rcs")
    parser.add_argument("destination", nargs="?", help="куда писать (по умолчанию только проверка)")
    parser.add_argument("--ignore-guid", action="store_true", help="не считать различия в GUID")
    parser.add_argument(
        "--guid",
        help="подменить GUID (отладка/воспроизведение эталона); по умолчанию сохраняется исходный",
    )
    args = parser.parse_args(argv)

    container = parse_file(args.source)
    try:
        blob = serialize(container, guid=args.guid)
    except ValueError as exc:
        parser.error(str(exc))
    if args.destination:
        Path(args.destination).write_bytes(blob)

    ranges = diff_ranges(container.raw, blob)
    if args.ignore_guid:
        start, end = GUID_OFFSET, GUID_OFFSET + GUID_LENGTH
        ranges = [
            (low, high)
            for low, high in ranges
            if high < start or low >= end
        ]
    print(f"{args.source}: {container.size} Б → {len(blob)} Б, блоков {len(container.blocks)}")
    if not ranges:
        print("вердикт: побайтово идентично" + (" (GUID игнорируется)" if args.ignore_guid else ""))
        return 0
    print(f"вердикт: различается {sum(high - low + 1 for low, high in ranges)} байт "
          f"в {len(ranges)} интервалах")
    for low, high in ranges[:10]:
        print(f"  {low:#08x}…{high:#08x} ({high - low + 1} Б)")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
