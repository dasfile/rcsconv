"""Тесты контейнера на эталонных файлах.

Запуск из корня репозитория:
    python -m unittest discover -s tools/tests -v
"""

from __future__ import annotations

import struct
import sys
import unittest
import zipfile
from pathlib import Path
from tempfile import TemporaryDirectory

TOOLS = Path(__file__).resolve().parents[1]
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(TOOLS))

from make_probes import build_probes  # noqa: E402
from pts2rcs import MAX_TILE_POINTS, build_output, read_pts  # noqa: E402
from rcs_blocks import (  # noqa: E402
    TILE_RECORD_SIZE,
    RcsFormatError,
    dialect,
    iter_tiles,
    parse_file,
    tile_count,
    tile_record_size,
    tlv_block,
)
from rcs_checks import check_invariants, pts_stats  # noqa: E402
from rcs_report import check_against_pts  # noqa: E402
from rcs_write import (  # noqa: E402
    GUID_LENGTH,
    GUID_OFFSET,
    GUID_PATTERN,
    build_header,
    build_metadata_block,
    build_metadata_record,
    build_properties_block,
    diff_ranges,
    make_guid,
    normalize_guid,
    serialize,
)

GOLDEN = ROOT / "golden"
REFERENCE = GOLDEN / "reference"   # *_orig.rcs: импорты 2026-10-29 (побайтовое доказательство)
PTS = GOLDEN                       # .pts лежат рядом с эталонами (docs/CORPUS.md)

# Поля double, где ReCap даёт расхождения в 1 ULP между разными импортами:
# центр bbox (@0x10), его повтор (@0x58), полуразмеры (@0xA0).
HEADER_DOUBLE_RANGES = ((0x10, 0x28), (0x58, 0x70), (0xA0, 0xB8))
# В записи узла (`type 3`, 368 Б) эти расхождения протекают в куб и bbox: 0…96.
TILE_DOUBLE_PREFIX = 96


def ulp_prone_offsets(container) -> set[int]:
    """Смещения байт, где допустимы расхождения в 1 ULP double.

    ПОДТВЕРЖДЕНО (`golden/probe6.rcs` против `golden/reference/probe6_orig.rcs`):
    два импорта одного и того же `.pts` дают разные значения центра/полуразмеров
    (младший бит double), потому что ReCap считает их другой формулой; значения
    протекают в кубы и bbox узлов. Всё остальное совпадает побайтово.
    """
    offsets: set[int] = set()
    for start, end in HEADER_DOUBLE_RANGES:
        offsets.update(range(start, end))
    block = container.block_of_type(3)
    nodes = struct.unpack_from("<Q", container.raw, block.offset)[0]
    for index in range(nodes):
        base = block.offset + 8 + index * TILE_RECORD_SIZE
        offsets.update(range(base, base + TILE_DOUBLE_PREFIX))
    return offsets

# Ожидания, подтверждённые анализом (см. docs/RCS-FORMAT.md)
CASES = {
    "probe6": {"size": 100327, "count": 3, "tiles": 3},
    "test1": {"size": 100327, "count": 3, "tiles": 3},
    "test2": {"size": 100327, "count": 3, "tiles": 3},
    "level_7": {"size": 7447651, "count": 316890, "tiles": 316},
}


class ReferenceFileTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        if not REFERENCE.is_dir():
            raise unittest.SkipTest(f"нет каталога эталонов {REFERENCE}")
        cls.containers = {
            name: parse_file(REFERENCE / f"{name}_orig.rcs") for name in CASES
        }

    def test_signature_and_fixed_words(self) -> None:
        for name, container in self.containers.items():
            with self.subTest(name=name):
                header = container.header
                self.assertEqual(header.word4, 84)
                self.assertEqual(header.word8, 3)
                self.assertEqual(header.word12, 2)
                self.assertEqual(header.chunk_version, 5)
                self.assertTrue(header.guid.startswith("{"))
                self.assertTrue(header.guid.endswith("}"))

    def test_all_invariants_pass(self) -> None:
        for name, container in self.containers.items():
            with self.subTest(name=name):
                failed = [check for check in check_invariants(container) if not check.ok]
                self.assertEqual(
                    failed,
                    [],
                    "провалены проверки: "
                    + "; ".join(f"{c.name} ({c.detail})" for c in failed),
                )

    def test_expected_parameters(self) -> None:
        for name, expected in CASES.items():
            container = self.containers[name]
            with self.subTest(name=name):
                self.assertEqual(container.size, expected["size"])
                self.assertEqual(container.header.count, expected["count"])
                self.assertEqual(tile_count(container), expected["tiles"])
                self.assertEqual(tile_record_size(container), TILE_RECORD_SIZE)

    def test_blocks_cover_file_exactly(self) -> None:
        for name, container in self.containers.items():
            with self.subTest(name=name):
                first = container.blocks[0]
                self.assertEqual(first.offset, 0x1812D)
                self.assertEqual(
                    first.offset + sum(block.size for block in container.blocks),
                    container.size,
                )
                self.assertEqual(container.directory_offset, 0x135)

    def test_header_matches_pts_for_small_files(self) -> None:
        for name in ("probe6", "test1", "test2"):
            container = self.containers[name]
            stats = pts_stats(PTS / f"{name}.pts")
            with self.subTest(name=name):
                failed = [c for c in check_against_pts(container, stats) if not c.ok]
                self.assertEqual(
                    failed, [], "; ".join(f"{c.name} ({c.detail})" for c in failed)
                )

    def test_level7_reference_is_not_one_to_one_with_pts(self) -> None:
        """Зафиксированный факт: в эталоне меньше точек, чем в .pts.

        Поэтому пары для реверса генерируем сами (golden/), а не берём *_orig.
        """
        container = self.containers["level_7"]
        stats = pts_stats(PTS / "level_7.pts")
        self.assertEqual(container.header.count, 316890)
        self.assertEqual(stats["count"], 324680)
        self.assertNotEqual(container.header.count, stats["count"])

    def test_points_are_not_plain_f64_xyz(self) -> None:
        """Регрессия против ошибки 2025 года: координаты не лежат как f64-триплеты."""
        container = self.containers["test1"]
        stats = pts_stats(PTS / "test1.pts")
        with (PTS / "test1.pts").open("r", encoding="utf-8") as handle:
            first = handle.readline().split()
        xyz = tuple(float(value) for value in first[:3])
        probes = (
            struct.pack("<3d", *xyz),                       # f64 XYZ
            struct.pack("<3f", *xyz),                       # f32 XYZ
            struct.pack("<ddd", *(a - b for a, b in zip(xyz, stats["center"]))),
        )
        for probe in probes:
            with self.subTest(probe=probe[:8].hex()):
                self.assertEqual(container.raw.find(probe), -1)


class ProbeGeneratorTests(unittest.TestCase):
    def test_probe_set(self) -> None:
        probes = build_probes(1.0)
        self.assertGreaterEqual(len(probes), 10)
        for name, points in probes.items():
            with self.subTest(name=name):
                self.assertTrue(points, "проба без точек")
                for point in points:
                    self.assertEqual(len(point), 3)


class GoldenTests(unittest.TestCase):
    """Эталоны, сгенерированные ReCap 2020 из наших .pts (golden/README.md)."""

    NAMES = ("probe6", "test1", "test2", "level_7")

    @classmethod
    def setUpClass(cls) -> None:
        if not (GOLDEN / "probe6.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")
        cls.golden = {name: parse_file(GOLDEN / f"{name}.rcs") for name in cls.NAMES}

    def test_chunk_id_is_131(self) -> None:
        """`chunk_id` (@0xD0) = 131 во всех golden-файлах (у level_7_orig — 11)."""
        for name, container in self.golden.items():
            with self.subTest(name=name):
                self.assertEqual(container.header.chunk_id, 131)

    def test_invariants_pass(self) -> None:
        for name, container in self.golden.items():
            with self.subTest(name=name):
                failed = [check for check in check_invariants(container) if not check.ok]
                self.assertEqual(
                    failed, [], "; ".join(f"{c.name} ({c.detail})" for c in failed)
                )

    def test_header_matches_pts(self) -> None:
        for name in ("probe6", "test1", "test2"):
            container = self.golden[name]
            stats = pts_stats(PTS / f"{name}.pts")
            with self.subTest(name=name):
                failed = [c for c in check_against_pts(container, stats) if not c.ok]
                self.assertEqual(failed, [], "; ".join(f"{c.name} ({c.detail})" for c in failed))

    def test_golden_and_reference_differ_only_in_guid_and_doubles(self) -> None:
        """Два импорта одного `.pts`: GUID плюс 1 ULP в double центра/полуразмеров.

        ПОДТВЕРЖДЕНО: `golden/*.rcs` (импорт 2026-11-02) и `reference/*_orig.rcs`
        (импорт 2026-10-29) одного размера; различия — строка GUID и младший бит
        double в полях центра/полуразмеров bbox и в кубах узлов (см.
        `ulp_prone_offsets`). Больше ничего не меняется — экспорт детерминирован
        во всём, кроме этих полей.
        """
        for name in ("probe6", "test1", "test2"):
            golden = self.golden[name]
            reference = parse_file(REFERENCE / f"{name}_orig.rcs")
            with self.subTest(name=name):
                self.assertEqual(golden.size, reference.size)
                diffs = {
                    i
                    for i in range(golden.size)
                    if golden.raw[i] != reference.raw[i]
                }
                self.assertTrue(diffs, "ожидались различия хотя бы в GUID")
                allowed = set(range(reference.header.guid_offset, reference.header.guid_end))
                allowed |= ulp_prone_offsets(golden) | ulp_prone_offsets(reference)
                outside = sorted(diffs - allowed)
                self.assertEqual(
                    outside,
                    [],
                    f"различия вне GUID/double на смещениях {[hex(i) for i in outside[:10]]}",
                )

    def test_level7_golden_parameters(self) -> None:
        container = self.golden["level_7"]
        tiles = iter_tiles(container)
        block11 = container.block_of_type(11)
        self.assertEqual(container.header.count, 317548)
        self.assertEqual(len(tiles), 43)
        self.assertEqual(sum(tile.payload_size for tile in tiles), block11.size)
        # Открытый вопрос: сумма точек по узлам на 10 меньше count заголовка.
        self.assertEqual(sum(tile.count for tile in tiles), 317538)

    def test_single_point_tiles_of_probe6(self) -> None:
        container = self.golden["probe6"]
        tiles = iter_tiles(container)
        self.assertEqual(len(tiles), 3)
        for tile in tiles:
            with self.subTest(index=tile.index):
                self.assertEqual(tile.count, 1)
                self.assertEqual(tile.payload_size, 20)
                # узел с одной точкой: реальный bbox вырожден в точку
                self.assertEqual(tile.points_min, tile.points_max)
                # куб узла — октант корневого куба (ребро 2.25)
                for lo, hi in zip(tile.cube_min, tile.cube_max):
                    self.assertAlmostEqual(hi - lo, 2.25, places=9)

        center = container.header.center
        # координаты в записи узла округлены до 1 мм (0.9444 -> 0.944)
        expected = {
            tuple(round(value, 3) for value in (x - c for x, c in zip(point, center)))
            for point in (
                (-0.111111111111111, 0.111111111111111, -1.111111111111111),
                (-1.444444444444444, 1.0, -1.0),
                (-0.111111111111111, 3.111111111111111, -3.111111111111111),
            )
        }
        actual = {
            tuple(round(value, 3) for value in tile.points_min) for tile in tiles
        }
        self.assertEqual(actual, expected)

    def test_tile_point_bbox_is_rounded_to_mm(self) -> None:
        """bbox точек узла — мм-кратный, с точностью до ошибки double.

        ПОДТВЕРЖДЕНО: значения получены округлением до 1 мм; в файле они лежат
        как double, поэтому проверяем «равно мм-кратному» с допуском 1e-9
        (у экспорта 2026-11-02 младший бит double гуляет, см. `ulp_prone_offsets`).
        """
        tiles = iter_tiles(self.golden["probe6"])
        raw_values = [
            value for tile in tiles for value in tile.points_min
        ]
        self.assertTrue(raw_values)
        for value in raw_values:
            self.assertAlmostEqual(round(value, 3), value, places=9)

    def test_tile_cubes_are_disjoint(self) -> None:
        tiles = iter_tiles(self.golden["probe6"])
        for i, left in enumerate(tiles):
            for right in tiles[i + 1 :]:
                overlap = all(
                    max(lo_a, lo_b) < min(hi_a, hi_b) - 1e-9
                    for lo_a, hi_a, lo_b, hi_b in zip(
                        left.cube_min, left.cube_max, right.cube_min, right.cube_max
                    )
                )
                with self.subTest(left=left.index, right=right.index):
                    self.assertFalse(overlap, "кубы узлов пересекаются")


class CorpusTests(unittest.TestCase):
    """Реальные облака: инварианты контейнера и `type 1` на обоих ветках импорта.

    Ветка `.pts` — корень `golden/` (10 облаков + подвыборка), ветка `.laz` —
    `golden/lazimport/` (см. `docs/CORPUS.md`). Файлы большие, поэтому берём по
    одному представителю каждой ветки: малое реальное облако (0.55 МБ) и облако
    из `.laz` (20.7 МБ).
    """

    FILES = (
        GOLDEN / "level_7_n19099.rcs",
        GOLDEN / "lazimport" / "attic_entrance.rcs",
    )

    @classmethod
    def setUpClass(cls) -> None:
        from analyze_type1 import (
            header_words,
            metadata_block_bytes,
            metadata_records,
            octree_counts,
            octree_depth,
        )

        cls.header_words = staticmethod(header_words)
        cls.metadata_block_bytes = staticmethod(metadata_block_bytes)
        cls.metadata_records = staticmethod(metadata_records)
        cls.octree_counts = staticmethod(octree_counts)
        cls.octree_depth = staticmethod(octree_depth)
        cls.containers = {
            path.name: parse_file(path) for path in cls.FILES if path.exists()
        }
        if not cls.containers:
            raise unittest.SkipTest(f"нет облаков корпуса в {GOLDEN}")

    def test_invariants_pass(self) -> None:
        for name, container in self.containers.items():
            with self.subTest(name=name):
                failed = [check for check in check_invariants(container) if not check.ok]
                self.assertEqual(
                    failed, [], "; ".join(f"{c.name} ({c.detail})" for c in failed)
                )

    def test_type1_matches_octree(self) -> None:
        """Слова `type 1` = занятые узлы по уровням, `K = sum(words)` (оба диалекта)."""
        for name, container in self.containers.items():
            tiles = iter_tiles(container)
            raw = self.metadata_block_bytes(container)
            words = tuple(self.octree_counts(container))
            with self.subTest(name=name):
                self.assertEqual(len(words) - 1, self.octree_depth(container))
                self.assertEqual(words[-1], len(tiles))
                self.assertEqual(tuple(self.header_words(raw)), words)
                self.assertEqual(len(self.metadata_records(raw)), sum(words))

    def test_count_is_at_least_sum_of_tiles(self) -> None:
        """`count` заголовка ≥ суммы по узлам (у экспорта есть «хвост»).

        Наблюдения: `level_7_n19099` — 0, `level_7` — 10, `attic_entrance` — 87,
        `attic_south_ost` — 91 (docs/RCS-FORMAT.md §12). Точный смысл открыт,
        поэтому проверяем только границу и масштаб.
        """
        for name, container in self.containers.items():
            tiles = iter_tiles(container)
            total = sum(tile.count for tile in tiles)
            tail = container.header.count - total
            with self.subTest(name=name):
                self.assertGreater(container.header.count, 0)
                self.assertGreaterEqual(tail, 0)
                self.assertLess(tail, max(64, container.header.count // 1000))

    def test_points_are_colored(self) -> None:
        """Корпус цветной: у узлов ненулевой диапазон цвета (сканы iPhone/Polycam)."""
        from rcs_points import decode_all

        for name, container in self.containers.items():
            points = decode_all(container)
            with self.subTest(name=name):
                self.assertGreater(len(points), 10_000)
                colors = {(point.r, point.g, point.b) for point in points}
                self.assertGreater(len(colors), 1000, "ожидалось цветное облако")


class ChunkVersionTests(unittest.TestCase):
    """`chunk_version` (@0xD4) — версия схемы писателя, зависит от пути импорта.

    ПОДТВЕРЖДЕНО (docs/RCS-FORMAT.md §2): raw из `.pts` → 2, raw из `.laz` → 1,
    «нетекстовый» импорт → 134, экспорт Unified RCS → 5. Файл с версией 134
    (`golden/reference/raw_v134.rcs`) сохранён как свидетель: остальные инварианты
    у него не проходят (импорт «мусорного» e57), поэтому проверяем только версию.
    """

    FILES = (
        ("golden/level_7 Support/level_7.rcs", 2),
        ("golden/lazimport/attic_entrance Support/attic_entrance.rcs", 1),
        ("golden/reference/raw_v134.rcs", 134),
        ("golden/probe6.rcs", 5),
    )

    def version_check(self, container) -> "Check":
        checks = [c for c in check_invariants(container) if c.name == "chunk_version известна"]
        self.assertTrue(checks, "нет проверки версии в check_invariants")
        return checks[0]

    def test_known_versions(self) -> None:
        for relative, version in self.FILES:
            path = ROOT / relative
            if not path.exists():
                continue
            container = parse_file(path)
            with self.subTest(path=relative):
                self.assertEqual(container.header.chunk_version, version)
                self.assertTrue(self.version_check(container).ok)

    def test_unknown_version_is_not_an_error(self) -> None:
        """Неизвестная версия схемы — наблюдение, а не порча файла."""
        from dataclasses import replace

        path = GOLDEN / "probe6.rcs"
        if not path.exists():
            raise unittest.SkipTest(f"нет {path}")
        container = parse_file(path)
        alien = replace(container, header=replace(container.header, chunk_version=999))
        check = self.version_check(alien)
        self.assertTrue(check.ok)
        self.assertIn("неизвестна", check.detail)


class PointDecodingTests(unittest.TestCase):
    """Декодирование блока type 11 (см. tools/rcs_points.py)."""

    DELTA = 1e-3  # квантование: x — 1 мм, y — 0.25 мм, z — 0.0625 мм

    @classmethod
    def setUpClass(cls) -> None:
        if not (GOLDEN / "probe6.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")
        cls.golden = {
            name: parse_file(GOLDEN / f"{name}.rcs")
            for name in ("probe6", "test1", "test2", "level_7")
        }

    def assert_points_close(self, actual, expected) -> None:
        """Сравнить списки (x, y, z, r, g, b) с допуском по координатам."""
        self.assertEqual(len(actual), len(expected))
        actual_sorted = sorted(actual, key=lambda item: (item[0], item[1], item[2]))
        expected_sorted = sorted(expected, key=lambda item: (item[0], item[1], item[2]))
        for got, want in zip(actual_sorted, expected_sorted):
            for axis in range(3):
                self.assertAlmostEqual(got[axis], want[axis], delta=self.DELTA)
            self.assertEqual(got[3:6], want[3:6], "цвет должен совпадать точно")

    def test_single_point_files_decode(self) -> None:
        from rcs_points import decode_all

        expected = {
            "probe6": [
                (0.6667, 1.5, -1.0556, 43, 27, 8),
                (-0.6667, -0.6111, 1.0556, 46, 31, 12),
                (0.6667, -1.5, 0.9444, 41, 30, 12),
            ],
            "test2": [
                (-0.5, -0.5, 0.0, 0, 0, 0),
                (0.5, -0.5, 0.0, 255, 0, 0),
                (-0.5, 0.5, 0.0, 0, 255, 0),
            ],
        }
        for name, samples in expected.items():
            actual = [
                (p.x, p.y, p.z, p.r, p.g, p.b) for p in decode_all(self.golden[name])
            ]
            with self.subTest(name=name):
                self.assert_points_close(actual, samples)

    def test_points_match_pts_after_decode(self) -> None:
        from rcs_points import decode_all

        for name in ("probe6", "test1", "test2"):
            container = self.golden[name]
            center = container.header.center
            actual = [
                (p.x + center[0], p.y + center[1], p.z + center[2], p.r, p.g, p.b)
                for p in decode_all(container)
            ]
            with (PTS / f"{name}.pts").open("r", encoding="utf-8") as handle:
                expected = []
                for line in handle:
                    parts = line.split()
                    if len(parts) >= 6:
                        expected.append(
                            (*(float(value) for value in parts[:3]),
                             *(int(value) for value in parts[3:6]))
                        )
            with self.subTest(name=name):
                self.assert_points_close(actual, expected)

    def test_level7_decode_count_and_bbox(self) -> None:
        from rcs_points import decode_all

        container = self.golden["level_7"]
        points = decode_all(container)
        self.assertEqual(len(points), 317538)

        center = container.header.center
        low = [min(getattr(p, axis) + c for p in points) for axis, c in zip("xyz", center)]
        high = [max(getattr(p, axis) + c for p in points) for axis, c in zip("xyz", center)]
        stats = pts_stats(PTS / "level_7.pts")
        for axis in range(3):
            with self.subTest(axis=axis):
                self.assertAlmostEqual(low[axis], stats["min"][axis], delta=2e-3)
                self.assertAlmostEqual(high[axis], stats["max"][axis], delta=2e-3)


SUPPORT = GOLDEN


class SupportDialectTests(unittest.TestCase):
    """«Сырые» файлы из `<проект> Support/` — второй диалект (chunk_version 2)."""

    @classmethod
    def setUpClass(cls) -> None:
        paths = {
            name: SUPPORT / f"{name} Support" / f"{name}.rcs"
            for name in ("probe6", "test2", "level_7")
        }
        missing = [str(path) for path in paths.values() if not path.exists()]
        if missing:
            raise unittest.SkipTest(f"нет support-файлов: {missing}")
        cls.containers = {name: parse_file(path) for name, path in paths.items()}

    def test_invariants_pass(self) -> None:
        for name, container in self.containers.items():
            with self.subTest(name=name):
                failed = [check for check in check_invariants(container) if not check.ok]
                self.assertEqual(
                    failed, [], "; ".join(f"{c.name} ({c.detail})" for c in failed)
                )

    def test_dialect_markers(self) -> None:
        from rcs_blocks import is_jpeg, preview_block, read_block, tlv_block

        for name, container in self.containers.items():
            with self.subTest(name=name):
                self.assertEqual(container.header.chunk_version, 2)
                self.assertEqual(dialect(container), "raw")
                self.assertEqual(container.block_types(), [3, 1, 11, 5, 0])
                props = tlv_block(container)
                self.assertIsNotNone(props)
                self.assertEqual(props.type, 5)
                preview = preview_block(container)
                self.assertIsNotNone(preview)
                self.assertTrue(is_jpeg(read_block(container, preview)))

    def test_full_precision_bbox(self) -> None:
        """В raw-диалекте полуразмеры bbox не округлены до миллиметра."""
        container = self.containers["probe6"]
        half = container.header.extent_max
        self.assertAlmostEqual(half[0], 0.6666666666666666, places=12)
        self.assertAlmostEqual(half[2], 1.0555555555555554, places=12)

    def test_decode_matches_golden(self) -> None:
        from rcs_points import decode_all

        golden = parse_file(GOLDEN / "probe6.rcs")
        raw = self.containers["probe6"]
        # порядок узлов в raw и export разный — сравниваем как множества
        self.assertEqual(
            sorted((p.x, p.y, p.z, p.r, p.g, p.b) for p in decode_all(raw)),
            sorted((p.x, p.y, p.z, p.r, p.g, p.b) for p in decode_all(golden)),
        )

    def test_level7_raw_counts(self) -> None:
        from rcs_points import decode_all

        container = self.containers["level_7"]
        tiles = iter_tiles(container)
        self.assertEqual(len(tiles), 43)
        # Заголовок хранит число прочитанных точек, узлы — то, что попало в октодерево
        self.assertEqual(container.header.count, 324648)
        self.assertEqual(sum(tile.count for tile in tiles), 323353)
        self.assertEqual(len(decode_all(container)), 323353)

        center = container.header.center
        points = decode_all(container)
        low = [min(getattr(p, axis) + c for p in points) for axis, c in zip("xyz", center)]
        high = [max(getattr(p, axis) + c for p in points) for axis, c in zip("xyz", center)]
        stats = pts_stats(PTS / "level_7.pts")
        for axis in range(3):
            with self.subTest(axis=axis):
                self.assertAlmostEqual(low[axis], stats["min"][axis], delta=2e-3)
                self.assertAlmostEqual(high[axis], stats["max"][axis], delta=2e-3)


class IntensityProbeTests(unittest.TestCase):
    """Проба attr_intensity: байт [8] тела точки = интенсивность."""

    PATH = Path("golden/attr_intensity_import1.rcs")

    @classmethod
    def setUpClass(cls) -> None:
        path = ROOT / cls.PATH
        if not path.exists():
            raise unittest.SkipTest(f"нет пробы {path}")
        cls.container = parse_file(path)

    def test_intensity_matches_input(self) -> None:
        from rcs_points import decode_all

        cluster = [p for p in decode_all(self.container) if p.intensity != 255]
        self.assertEqual(sorted(p.intensity for p in cluster), [0, 40, 80, 120, 160])
        self.assertEqual(sorted(round(p.x, 3) for p in cluster), [-2.5, -2.48, -2.46, -2.44, -2.42])

    def test_ballast_has_max_intensity(self) -> None:
        from rcs_points import decode_all

        ballast = [p for p in decode_all(self.container) if p.intensity == 255]
        self.assertEqual(len(ballast), 1)
        self.assertAlmostEqual(ballast[0].x, 2.5, delta=1e-3)


class PayloadHeaderTests(unittest.TestCase):
    """Заголовок payload'а узла (блок type 11): структура из 4-байтовых записей.

    ПОДТВЕРЖДЕНО на всех узлах golden-эталонов (см. docs/RCS-FORMAT.md, §4.2):
      * длина заголовка кратна 4;
      * запись = `(u16 index, u8 length, u8 kind)`, `length` кратен 8, `kind` ∈ 0…8;
      * записей с `kind == 8` ровно `count`, их `index` — перестановка `0…count-1`.
    """

    NAMES = ("probe6", "test1", "test2", "level_7")
    SUPPORT = "golden/level_7 Support/level_7.rcs"
    FIXTURE = "golden/attr_intensity_import1.rcs"

    @classmethod
    def setUpClass(cls) -> None:
        from analyze_header import header_bytes, parse_records

        cls.header_bytes = staticmethod(header_bytes)
        cls.parse_records = staticmethod(parse_records)

        if not (GOLDEN / "level_7.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")
        cls.files = {name: parse_file(GOLDEN / f"{name}.rcs") for name in cls.NAMES}
        support = ROOT / cls.SUPPORT
        if support.exists():
            cls.files["level_7 raw"] = parse_file(support)
        fixture = ROOT / cls.FIXTURE
        if fixture.exists():
            cls.files["attr_intensity"] = parse_file(fixture)

    def test_header_is_made_of_4_byte_records(self) -> None:
        for name, container in self.files.items():
            for tile in iter_tiles(container):
                with self.subTest(name=name, node=tile.index):
                    header = self.header_bytes(container, tile)
                    self.assertEqual(len(header) % 4, 0, f"{len(header)} байт")
                    records = self.parse_records(header)
                    self.assertEqual(len(records) * 4, len(header))

    def test_kind_and_length_ranges(self) -> None:
        for name, container in self.files.items():
            for tile in iter_tiles(container):
                with self.subTest(name=name, node=tile.index):
                    records = self.parse_records(self.header_bytes(container, tile))
                    self.assertTrue(all(record.kind <= 8 for record in records))
                    self.assertTrue(all(record.length % 8 == 0 for record in records))

    def test_point_records_form_permutation(self) -> None:
        """Записи kind 8: ровно `count` штук, `index` — перестановка 0…count-1."""
        for name, container in self.files.items():
            for tile in iter_tiles(container):
                with self.subTest(name=name, node=tile.index):
                    records = self.parse_records(self.header_bytes(container, tile))
                    indices = sorted(
                        record.index for record in records if record.kind == 8
                    )
                    self.assertEqual(indices, list(range(tile.count)))

    def test_single_point_header_is_four_bytes(self) -> None:
        """Минимальный заголовок — одна запись `00 00 00 08` (index 0, kind 8)."""
        for name in ("probe6", "test1", "test2"):
            for tile in iter_tiles(self.files[name]):
                with self.subTest(name=name, node=tile.index):
                    header = self.header_bytes(self.files[name], tile)
                    self.assertEqual(header, b"\x00\x00\x00\x08")

    def test_kind8_order_is_not_the_body_order(self) -> None:
        """ОПРОВЕРГНУТО: `index` записи kind 8 — не номер тела точки в payload'е.

        На фикстуре `attr_intensity` (5 точек + балласт) тела идут в порядке
        `.pts` → [3, 1, 4, 2, 0], а записи kind 8 дают [4, 1, 2, 3, 0].
        """
        container = self.files.get("attr_intensity")
        if container is None:
            raise unittest.SkipTest("нет фикстуры attr_intensity")

        tile = next(tile for tile in iter_tiles(container) if tile.count == 5)
        records = self.parse_records(self.header_bytes(container, tile))
        kind8 = [record.index for record in records if record.kind == 8]
        self.assertEqual(kind8, [4, 1, 2, 3, 0])

        from rcs_points import decode_tile

        bodies = decode_tile(container, tile)
        # Порядок тел в payload'е: исходные точки 3, 1, 4, 2, 0 (.pts по x).
        self.assertEqual([p.intensity for p in bodies], [120, 40, 160, 80, 0])
        self.assertEqual(round(bodies[0].x, 3), -2.44)


class MetadataHeaderTests(unittest.TestCase):
    """Блок метаданных (`type 1`): слова заголовка и число записей.

    ПОДТВЕРЖДЕНО на 13 эталонах (`docs/RCS-FORMAT.md` §4.3, `tools/analyze_type1.py`):
      * `words[L]` — число занятых узлов октодерева на уровне `L`;
      * `K = sum(words)` — полное число узлов октодерева;
      * все записи идентичны.
    """

    NAMES = ("probe6", "test1", "test2", "level_7", "attr_intensity_import1")
    SUPPORT = (
        "golden/probe6 Support/probe6.rcs",
        "golden/test1 Support/test1.rcs",
        "golden/test2 Support/test2.rcs",
        "golden/level_7 Support/level_7.rcs",
    )
    REFERENCE_NAMES = ("probe6_orig.rcs", "test1_orig.rcs", "test2_orig.rcs", "level_7_orig.rcs")

    @classmethod
    def setUpClass(cls) -> None:
        from analyze_type1 import (  # noqa: E402
            header_words,
            metadata_block_bytes,
            metadata_records,
            octree_counts,
        )

        cls.header_words = staticmethod(header_words)
        cls.metadata_block_bytes = staticmethod(metadata_block_bytes)
        cls.metadata_records = staticmethod(metadata_records)
        cls.octree_counts = staticmethod(octree_counts)

        if not (GOLDEN / "level_7.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")
        cls.paths: list[Path] = [GOLDEN / f"{name}.rcs" for name in cls.NAMES]
        cls.paths += [ROOT / path for path in cls.SUPPORT]
        cls.paths += [REFERENCE / name for name in cls.REFERENCE_NAMES]

    def existing(self) -> list[Path]:
        return [path for path in self.paths if path.exists()]

    def test_words_are_occupied_nodes_per_level(self) -> None:
        """Слова заголовка = число занятых узлов на каждом уровне октодерева."""
        paths = self.existing()
        self.assertGreaterEqual(len(paths), 5, "нужны эталоны для проверки")
        for path in paths:
            container = parse_file(path)
            with self.subTest(path=str(path)):
                self.assertEqual(
                    self.header_words(self.metadata_block_bytes(container)),
                    self.octree_counts(container),
                )

    def test_record_count_is_sum_of_words(self) -> None:
        """`K` = сумма слов = полное число узлов октодерева."""
        for path in self.existing():
            container = parse_file(path)
            raw = self.metadata_block_bytes(container)
            with self.subTest(path=str(path)):
                words = self.header_words(raw)
                records = self.metadata_records(raw)
                self.assertEqual(len(records), sum(words))
                self.assertEqual(len(records), sum(self.octree_counts(container)))

    def test_records_are_identical(self) -> None:
        """Все записи блока — пустой диапазон атрибута (`±DOUBLE_MAX`)."""
        for path in self.existing():
            container = parse_file(path)
            raw = self.metadata_block_bytes(container)
            with self.subTest(path=str(path)):
                records = self.metadata_records(raw)
                self.assertEqual(len(set(records)), 1)
                self.assertEqual(records[0], build_metadata_record())

    def test_generated_block_matches_golden_probe6(self) -> None:
        """Сгенерированный `type 1` для probe6 совпадает с эталоном байт-в-байт."""
        golden = parse_file(GOLDEN / "probe6.rcs")
        words = tuple(self.octree_counts(golden))
        self.assertEqual(words, (1, 3))
        self.assertEqual(
            build_metadata_block(words), self.metadata_block_bytes(golden)
        )

    def test_records_follow_sum_of_words(self) -> None:
        """Число записей выводится из слов; рассогласование отбивается."""
        self.assertEqual(len(build_metadata_block((1, 3))), 128 + 4 * 72)
        self.assertEqual(build_metadata_block((1, 3)), build_metadata_block((1, 3), 4))
        with self.assertRaises(RcsFormatError):
            build_metadata_block((1, 3), 3)
        with self.assertRaises(RcsFormatError):
            build_metadata_block((1,) * 33)


class WriterRoundTripTests(unittest.TestCase):
    """`read → write` побайтово идентичен (writer, этап 1)."""

    @classmethod
    def setUpClass(cls) -> None:
        if not (GOLDEN / "level_7.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")
        cls.paths = [
            GOLDEN / "probe6.rcs",
            GOLDEN / "test1.rcs",
            GOLDEN / "test2.rcs",
            GOLDEN / "level_7.rcs",
            GOLDEN / "probe6 Support" / "probe6.rcs",
        ]
        if (GOLDEN / "level_7 Support" / "level_7.rcs").exists():
            cls.paths.append(GOLDEN / "level_7 Support" / "level_7.rcs")
        if (REFERENCE / "level_7_orig.rcs").exists():
            cls.paths.append(REFERENCE / "level_7_orig.rcs")

    def test_round_trip_is_byte_identical(self) -> None:
        for path in self.paths:
            with self.subTest(path=str(path)):
                container = parse_file(path)
                self.assertEqual(serialize(container), container.raw)

    def test_header_is_rebuilt_from_fields(self) -> None:
        for path in self.paths:
            with self.subTest(path=str(path)):
                container = parse_file(path)
                header = build_header(container)
                self.assertEqual(header, container.raw[: len(header)])
                self.assertEqual(len(header), container.directory_offset)

    def test_guid_substitution_changes_only_guid(self) -> None:
        container = parse_file(self.paths[0])
        fresh = "{00000000_1111_2222_3333_444444444444}"
        blob = serialize(container, guid=fresh)
        ranges = diff_ranges(container.raw, blob)
        self.assertTrue(ranges)
        for low, high in ranges:
            self.assertGreaterEqual(low, GUID_OFFSET)
            self.assertLess(high, GUID_OFFSET + GUID_LENGTH)


class Pts2RcsTests(unittest.TestCase):
    """Генерация `.rcs` из `.pts` (этап 2): сверка с эталоном ReCap."""

    @classmethod
    def setUpClass(cls) -> None:
        from pts2rcs import build_output

        cls.build_output = staticmethod(build_output)
        if not (GOLDEN / "probe6.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")

    @staticmethod
    def intensity_offsets(container) -> set[int]:
        """Смещения байта `[8]` (интенсивность) в блоке `type 11`."""
        block11 = container.block_of_type(11)
        offsets: set[int] = set()
        for tile in iter_tiles(container):
            header = tile.payload_size - 16 * tile.count
            for number in range(tile.count):
                offsets.add(
                    block11.offset + tile.payload_offset + header + number * 16 + 8
                )
        return offsets

    def test_probe6_matches_reference_except_guid_and_intensity(self) -> None:
        """`probe6.pts` → файл, совпадающий с `golden/reference/probe6_orig.rcs` до байта.

        Исключения: GUID и байт `[8]` тела (интенсивность) — в `.pts` колонки
        интенсивности нет, ReCap пишет туда произвольные значения.
        Блок `type 1` берём из эталона: у него `(1,3,0,0,0)` и 4 записи.

        Почему `reference`, а не `golden`: наш писатель считает центр bbox как
        `(min+max)/2`, и это совпадает с импортом 2026-10-29; у нового импорта
        центр отличается на 1 ULP (см. `ulp_prone_offsets`), поэтому побайтовое
        доказательство держится на `reference` (docs/RCS-FORMAT.md §8).
        """
        template = parse_file(REFERENCE / "probe6_orig.rcs")
        blob, stats = self.build_output(
            PTS / "probe6.pts",
            template,
            guid=template.header.guid,
            metadata="template",
        )
        self.assertEqual(len(blob), template.size)
        self.assertEqual(stats["points"], 3, "все точки .pts должны попасть в файл")

        intensity_offsets = self.intensity_offsets(template)
        allowed = set(range(GUID_OFFSET, GUID_OFFSET + GUID_LENGTH)) | intensity_offsets

        differences = {
            offset for offset in range(len(blob)) if blob[offset] != template.raw[offset]
        }
        self.assertEqual(
            differences - allowed,
            set(),
            f"лишние расхождения: {sorted(differences - allowed)[:10]}",
        )
        self.assertTrue(differences <= allowed)

    def test_probe6_matches_golden_without_template(self) -> None:
        """Без шаблона: `type 1` из октодерева даёт эталонный файл.

        GUID берём из эталона, чтобы сверка была честной: расхождения остаются
        в байте интенсивности (в `.pts` этой колонки нет) и в double-полях
        центра/полуразмеров (1 ULP, см. `ulp_prone_offsets`).
        """
        golden = parse_file(GOLDEN / "probe6.rcs")
        blob, stats = self.build_output(PTS / "probe6.pts", guid=golden.header.guid)
        self.assertEqual(len(blob), golden.size)
        self.assertEqual(stats["metadata"], "generated")
        self.assertEqual(stats["metadata_words"], (1, 3))
        self.assertEqual(stats["metadata_records"], 4)

        allowed = self.intensity_offsets(golden) | ulp_prone_offsets(golden)
        allowed |= set(range(golden.header.guid_offset, golden.header.guid_end))
        differences = {
            offset for offset in range(len(blob)) if blob[offset] != golden.raw[offset]
        }
        self.assertEqual(
            differences - allowed,
            set(),
            f"лишние расхождения: {sorted(differences - allowed)[:10]}",
        )

    def test_generated_file_reads_back_to_source_points(self) -> None:
        """Сгенерированный файл наш читатель возвращает в исходные точки."""
        from rcs_points import decode_all
        from tempfile import TemporaryDirectory

        source = ROOT / "probes" / "import_main" / "attr_intensity.pts"
        if not source.exists():
            raise unittest.SkipTest(f"нет {source}")
        template = parse_file(GOLDEN / "probe6.rcs")
        blob, stats = self.build_output(source, template, max_per_tile=5)
        self.assertGreater(stats["tiles"], 0)

        with TemporaryDirectory() as folder:
            path = Path(folder) / "generated.rcs"
            path.write_bytes(blob)
            container = parse_file(path)
            decoded = decode_all(container)

        expected = sorted(
            (
                round(point.x, 3),
                round(point.y, 3),
                round(point.z, 3),
                point.intensity,
                point.r,
                point.g,
                point.b,
            )
            for point in read_pts(source)
        )
        actual = sorted(
            (
                round(point.x + container.header.center[0], 3),
                round(point.y + container.header.center[1], 3),
                round(point.z + container.header.center[2], 3),
                point.intensity,
                point.r,
                point.g,
                point.b,
            )
            for point in decoded
        )
        self.assertEqual(actual, expected)


class GuidTests(unittest.TestCase):
    """GUID: по умолчанию случайный, пользовательский — проверяется и нормализуется."""

    @classmethod
    def setUpClass(cls) -> None:
        from pts2rcs import build_output

        cls.build_output = staticmethod(build_output)
        if not (GOLDEN / "probe6.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")

    def test_make_guid_matches_format(self) -> None:
        for _ in range(20):
            self.assertRegex(make_guid(), GUID_PATTERN)
            self.assertEqual(len(make_guid()), GUID_LENGTH)

    def test_default_guids_differ_between_runs(self) -> None:
        template = parse_file(GOLDEN / "probe6.rcs")
        first, _ = self.build_output(PTS / "probe6.pts", template)
        second, _ = self.build_output(PTS / "probe6.pts", template)
        guid1 = first[GUID_OFFSET : GUID_OFFSET + GUID_LENGTH].decode("ascii")
        guid2 = second[GUID_OFFSET : GUID_OFFSET + GUID_LENGTH].decode("ascii")
        self.assertNotEqual(guid1, guid2)
        self.assertRegex(guid1, GUID_PATTERN)
        self.assertRegex(guid2, GUID_PATTERN)

    def test_given_guid_is_normalized(self) -> None:
        template = parse_file(GOLDEN / "probe6.rcs")
        blob, _ = self.build_output(
            PTS / "probe6.pts",
            template,
            guid="{3A0C23EE-5B4A-4E0C-922D-A7DC56929BD1}",
        )
        written = blob[GUID_OFFSET : GUID_OFFSET + GUID_LENGTH].decode("ascii")
        self.assertEqual(written, "{3a0c23ee_5b4a_4e0c_922d_a7dc56929bd1}")

    def test_bad_guid_is_rejected(self) -> None:
        for bad in ("nope", "{1234}", "{3a0c23ee_5b4a_4e0c_922d}", "3a0c23ee_5b4a"):
            with self.subTest(guid=bad):
                with self.assertRaises(ValueError):
                    normalize_guid(bad)


class StructuralBlockTests(unittest.TestCase):
    """Блоки `type 1` (метаданные) и `type 0` (свойства) генерируются байт-в-байт."""

    # (файл, слова заголовка, число записей) — снято с эталонов
    METADATA_CASES = {
        "probe6": ((1, 3, 0, 0, 0), 4),
        "test1": ((1, 3, 0, 0, 0), 4),
        "test2": ((1, 3, 0, 0, 0), 4),
        "level_7": ((1, 8, 13, 43, 0), 65),
    }

    @classmethod
    def setUpClass(cls) -> None:
        if not (GOLDEN / "probe6.rcs").exists():
            raise unittest.SkipTest(f"нет golden-файлов в {GOLDEN}")

    def test_metadata_block_matches_golden(self) -> None:
        for name, (words, records) in self.METADATA_CASES.items():
            path = GOLDEN / f"{name}.rcs"
            if not path.exists():
                continue
            container = parse_file(path)
            block = container.block_of_type(1)
            with self.subTest(name=name):
                self.assertEqual(
                    build_metadata_block(words, records),
                    container.raw[block.offset : block.end],
                )

    def test_metadata_record_is_72_bytes(self) -> None:
        self.assertEqual(len(build_metadata_block()), 128 + 72)

    def test_properties_block_matches_golden(self) -> None:
        generated = build_properties_block()
        self.assertEqual(len(generated), 134)
        for name in ("probe6", "test1", "test2", "level_7"):
            path = GOLDEN / f"{name}.rcs"
            if not path.exists():
                continue
            container = parse_file(path)
            block = tlv_block(container)
            with self.subTest(name=name):
                self.assertEqual(
                    generated, container.raw[block.offset : block.end]
                )

    def test_generated_metadata_keeps_file_readable(self) -> None:
        """`--metadata generated`: блок `type 1` из октодерева, файл читается."""
        from analyze_type1 import octree_counts
        from pts2rcs import build_output
        from rcs_points import decode_all

        template = parse_file(GOLDEN / "probe6.rcs")
        blob, stats = build_output(
            PTS / "probe6.pts", template, metadata="generated", guid=template.header.guid
        )
        with TemporaryDirectory() as folder:
            path = Path(folder) / "meta.rcs"
            path.write_bytes(blob)
            container = parse_file(path)
            block = container.block_of_type(1)
            words = tuple(octree_counts(container))
            self.assertEqual(stats["metadata_words"], words)
            self.assertEqual(stats["metadata_records"], sum(words))
            self.assertEqual(
                container.raw[block.offset : block.end], build_metadata_block(words)
            )
            points = decode_all(container)
        self.assertEqual(len(points), 3)

    def test_minimal_metadata_mode(self) -> None:
        """`--metadata minimal`: вырожденный вариант `(1,)` + 1 запись."""
        from pts2rcs import build_output

        blob, stats = build_output(PTS / "probe6.pts", metadata="minimal")
        with TemporaryDirectory() as folder:
            path = Path(folder) / "minimal.rcs"
            path.write_bytes(blob)
            container = parse_file(path)
            block = container.block_of_type(1)
            self.assertEqual(
                container.raw[block.offset : block.end], build_metadata_block()
            )
        self.assertEqual(stats["metadata"], "minimal")
        self.assertEqual(stats["metadata_records"], 1)

    def test_file_without_template(self) -> None:
        """Писателю больше не нужен `.rcs`-шаблон: все блоки генерируются."""
        from analyze_type1 import octree_counts
        from pts2rcs import build_output
        from rcs_points import decode_all

        blob, stats = build_output(PTS / "probe6.pts")
        with TemporaryDirectory() as folder:
            path = Path(folder) / "no_template.rcs"
            path.write_bytes(blob)
            container = parse_file(path)
            self.assertEqual(container.block_types(), [3, 1, 11, 0])
            self.assertEqual(container.header.count, 3)
            self.assertEqual(container.header.chunk_id, 131)
            block = container.block_of_type(1)
            words = tuple(octree_counts(container))
            self.assertEqual(words, (1, 3))
            self.assertEqual(
                container.raw[block.offset : block.end], build_metadata_block(words)
            )
            properties = container.block_of_type(0)
            self.assertEqual(
                container.raw[properties.offset : properties.end],
                build_properties_block(),
            )
            points = decode_all(container)
        self.assertEqual(len(points), stats["points"])
        self.assertEqual(len(points), 3)

    def test_generated_file_passes_all_invariants(self) -> None:
        """Сгенерированный файл проходит все инварианты контейнера."""
        from pts2rcs import build_output

        blob, stats = build_output(PTS / "probe6.pts", max_per_tile=1)
        with TemporaryDirectory() as folder:
            path = Path(folder) / "invariants.rcs"
            path.write_bytes(blob)
            container = parse_file(path)
            failed = [check for check in check_invariants(container) if not check.ok]
        self.assertEqual(
            failed,
            [],
            "провалены проверки: " + "; ".join(f"{c.name} ({c.detail})" for c in failed),
        )
        self.assertEqual(stats["metadata"], "generated")


class CliTests(unittest.TestCase):
    """CLI: имя выхода по умолчанию, пакетная обработка, защита направления."""

    POINTS = (
        "0.0 0.0 0.0 10 20 30\n"
        "1.0 0.0 0.0 40 50 60\n"
        "0.0 1.0 0.0 70 80 90\n"
    )

    def setUp(self) -> None:
        self.folder = TemporaryDirectory()
        self.root = Path(self.folder.name)
        self.source = self.root / "cloud.pts"
        self.source.write_text(self.POINTS, encoding="utf-8")

    def tearDown(self) -> None:
        self.folder.cleanup()

    def _write_rcs(self, name: str = "cloud.rcs") -> Path:
        blob, _ = build_output(self.source)
        target = self.root / name
        target.write_bytes(blob)
        return target

    def test_pts2rcs_names_output_itself(self) -> None:
        """`pts2rcs.py cloud.pts` пишет `cloud.rcs` рядом с входом."""
        from pts2rcs import main

        self.assertEqual(main([str(self.source)]), 0)
        target = self.source.with_suffix(".rcs")
        self.assertTrue(target.exists())
        self.assertEqual(parse_file(target).header.count, 3)

    def test_pts2rcs_batch_directory(self) -> None:
        """Каталог на входе → все `*.pts` превращаются в `*.rcs`."""
        from pts2rcs import main

        (self.root / "second.pts").write_text(self.POINTS, encoding="utf-8")
        self.assertEqual(main([str(self.root)]), 0)
        self.assertTrue((self.root / "cloud.rcs").exists())
        self.assertTrue((self.root / "second.rcs").exists())
        self.assertEqual(main([str(self.root)]), 0)          # без -f: пропуск
        self.assertEqual(parse_file(self.root / "cloud.rcs").header.count, 3)

    def test_pts2rcs_does_not_overwrite_without_force(self) -> None:
        """Существующий выход не перезаписывается без `-f`."""
        from pts2rcs import main

        target = self.root / "cloud.rcs"
        target.write_bytes(b"sentinel")
        self.assertEqual(main([str(self.source)]), 0)
        self.assertEqual(target.read_bytes(), b"sentinel")
        self.assertEqual(main([str(self.source), "-f"]), 0)
        self.assertNotEqual(target.read_bytes(), b"sentinel")
        self.assertEqual(parse_file(target).header.count, 3)

    def test_pts2rcs_rejects_rcs_input(self) -> None:
        """`.rcs` на входе `pts2rcs` — ошибка направления, файл не тронут."""
        from pts2rcs import main

        rcs = self._write_rcs()
        before = rcs.read_bytes()
        self.assertEqual(main([str(rcs)]), 2)
        self.assertEqual(rcs.read_bytes(), before)

    def test_rcs2pts_rejects_pts_input(self) -> None:
        """`.pts` на входе `rcs2pts` — ошибка направления."""
        from rcs2pts import main

        self.assertEqual(main([str(self.source)]), 2)
        self.assertFalse((self.root / "cloud.rcs").exists())

    def test_rcs2pts_names_output_and_writes_count_header(self) -> None:
        """`rcs2pts.py cloud.rcs` → `cloud.pts`; `-c -i` даёт шапку и 7 колонок."""
        from rcs2pts import main

        rcs = self._write_rcs()
        out_dir = self.root / "out"
        self.assertEqual(main([str(rcs), "-o", str(out_dir), "-c", "-i"]), 0)
        lines = (out_dir / "cloud.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines[0], "3", "первая строка — число точек")
        self.assertEqual(len(lines), 4)
        self.assertEqual(len(lines[1].split()), 7)

        self.assertEqual(main([str(rcs), "-o", str(self.root / "plain")]), 0)
        plain = (self.root / "plain" / "cloud.pts").read_text(encoding="utf-8")
        self.assertEqual(len(plain.splitlines()), 3)
        self.assertEqual(len(plain.splitlines()[0].split()), 6)

    def test_auto_limit_and_validation(self) -> None:
        """`auto` = предел формата; значения вне 1…65536 отбиваются."""
        blob, stats = build_output(self.source)
        self.assertEqual(stats["max_per_tile"], MAX_TILE_POINTS)
        self.assertEqual(stats["tiles"], 3)
        self.assertEqual(stats["depth"], 1, "auto не дробит дерево без нужды")
        path = self.root / "auto.rcs"
        path.write_bytes(blob)
        self.assertEqual(parse_file(path).header.count, 3)
        with self.assertRaises(RcsFormatError):
            build_output(self.source, max_per_tile=0)
        with self.assertRaises(RcsFormatError):
            build_output(self.source, max_per_tile=MAX_TILE_POINTS + 1)

    def test_explicit_limit_still_works(self) -> None:
        """`-m 200` остаётся доступным ограничителем."""
        _, stats = build_output(self.source, max_per_tile=200)
        self.assertEqual(stats["max_per_tile"], 200)
        self.assertEqual(stats["tiles"], 3)


    def test_validate_pts_passes_for_matching_pair(self) -> None:
        """`validate_pts` на согласованной паре → код 0."""
        from validate_pts import main as validate_main

        rcs = self._write_rcs()
        self.assertEqual(validate_main([str(self.source), str(rcs)]), 0)

    def test_validate_pts_reports_bbox_mismatch(self) -> None:
        """Чужое облако с тем же числом точек → bbox не сходится, код 1."""
        from validate_pts import main as validate_main

        rcs = self._write_rcs()
        other = self.root / "other.pts"
        other.write_text(
            "100 100 100 1 2 3\n101 100 100 1 2 3\n100 101 100 1 2 3\n",
            encoding="utf-8",
        )
        self.assertEqual(validate_main([str(other), str(rcs)]), 1)


class CorpusToolTests(unittest.TestCase):
    """Подвыборка, чек-лист корпуса и пакетная сверка (на временных файлах)."""

    def setUp(self) -> None:
        self.folder = TemporaryDirectory()
        self.root = Path(self.folder.name)
        self.points_dir = self.root / "pts"
        self.points_dir.mkdir()
        self.golden = self.root / "golden"
        self.golden.mkdir()
        rows = "".join(f"{index}.0 0.0 0.0 1 2 3\n" for index in range(10))
        self.source = self.points_dir / "cloud.pts"
        self.source.write_text(f"10\n{rows}", encoding="utf-8")

    def tearDown(self) -> None:
        self.folder.cleanup()

    def test_subset_keeps_stride_and_header(self) -> None:
        """`-n 3` из 10 точек: шаг 4, шапка пересчитана, строки исходные."""
        from pts_subset import main

        out_dir = self.root / "out"
        self.assertEqual(main([str(self.source), "-n", "3", "-o", str(out_dir)]), 0)
        lines = (out_dir / "cloud_n3.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines[0], "3")
        self.assertEqual(len(lines), 4)
        self.assertEqual([line.split()[0] for line in lines[1:]], ["0.0", "4.0", "8.0"])

    def test_subset_keeps_source_header_style(self) -> None:
        """Если шапки не было — её и не будет (но выборка равномерная)."""
        from pts_subset import main

        plain = self.points_dir / "plain.pts"
        plain.write_text(
            "".join(f"{index} 0 0 1 2 3\n" for index in range(4)), encoding="utf-8"
        )
        self.assertEqual(main([str(plain), "--stride", "2"]), 0)
        lines = (self.points_dir / "plain_n2.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0].split()[0], "0")
        self.assertEqual(lines[1].split()[0], "2")

    def test_corpus_status_marks_ready_and_missing(self) -> None:
        """Чек-лист: есть экспорт → 0 (готово), нет → 1 с подсказкой."""
        from corpus_status import main

        blob, _ = build_output(self.source)
        (self.golden / "cloud.rcs").write_bytes(blob)
        self.assertEqual(main([str(self.points_dir), "-g", str(self.golden)]), 0)
        (self.golden / "cloud.rcs").unlink()
        self.assertEqual(main([str(self.points_dir), "-g", str(self.golden)]), 1)

    def test_bbox_tolerance_is_soft_for_reference(self) -> None:
        """Эталон ReCap: сдвиг bbox на 2 мм — `INFO`, на 20 мм — `FAIL`."""
        from validate_pts import validate_pair

        blob, _ = build_output(self.source)
        rcs = self.golden / "cloud.rcs"
        rcs.write_bytes(blob)

        shifted = self.root / "shifted.pts"
        shifted.write_text(
            "10\n" + "".join(f"{index}.002 0.0 0.0 1 2 3\n" for index in range(10)),
            encoding="utf-8",
        )
        far = self.root / "far.pts"
        far.write_text(
            "10\n" + "".join(f"{index}.02 0.0 0.0 1 2 3\n" for index in range(10)),
            encoding="utf-8",
        )

        self.assertEqual(validate_pair(shifted, rcs, verbose=False, strict=False), 0)
        self.assertEqual(validate_pair(shifted, rcs, verbose=False, strict=True), 1)
        self.assertEqual(validate_pair(far, rcs, verbose=False, strict=False), 1)

    def test_validate_batch_with_golden_dir(self) -> None:
        """Пакетная сверка: `-g каталог` находит эталон по имени каждого .pts."""
        from validate_pts import main as validate_main

        second = self.points_dir / "second.pts"
        second.write_text(
            "3\n0 0 0 1 2 3\n1 0 0 1 2 3\n0 1 0 1 2 3\n", encoding="utf-8"
        )
        for name in ("cloud", "second"):
            blob, _ = build_output(self.points_dir / f"{name}.pts")
            (self.golden / f"{name}.rcs").write_bytes(blob)
        self.assertEqual(
            validate_main([str(self.points_dir), "-g", str(self.golden), "-q"]), 0
        )


class Las2RcsTests(unittest.TestCase):
    """`.las/.laz` → `.rcs`: тот же писатель, другой источник точек.

    Проверяем: координаты/цвет/интенсивность доходят до файла, результат совпадает
    с `.pts`-путём (расходится только GUID), CLI ведёт себя как `pts2rcs`.
    """

    POINTS = (
        (0.1, 0.2, 0.3, 10, 200, 100, 50),
        (1.5, 0.0, 0.0, 20, 10, 20, 30),
        (0.0, 1.5, 0.0, 30, 0, 0, 0),
        (-1.25, -0.5, 1.75, 40, 255, 255, 255),
    )

    @staticmethod
    def write_las(path: Path, points, *, color: bool = True) -> Path:
        """Собрать LAS своим генератором: point format 2 (цвет+интенсивность)."""
        import laspy
        import numpy as np

        header = laspy.LasHeader(point_format=2 if color else 0, version="1.2")
        header.scales = [0.001, 0.001, 0.001]
        header.offsets = [0.0, 0.0, 0.0]
        las = laspy.LasData(header)
        las.x = np.array([row[0] for row in points], dtype=float)
        las.y = np.array([row[1] for row in points], dtype=float)
        las.z = np.array([row[2] for row in points], dtype=float)
        las.intensity = np.array([row[3] for row in points], dtype=np.uint16)
        if color:
            las.red = np.array([row[4] for row in points], dtype=np.uint16)
            las.green = np.array([row[5] for row in points], dtype=np.uint16)
            las.blue = np.array([row[6] for row in points], dtype=np.uint16)
        las.write(path)
        return path

    def test_points_color_and_intensity_reach_file(self) -> None:
        import las2rcs
        from rcs_points import decode_all

        with TemporaryDirectory() as folder:
            root = Path(folder)
            source = self.write_las(root / "cloud.las", self.POINTS)
            self.assertEqual(las2rcs.main([str(source), "-o", str(root / "out"), "-f"]), 0)
            container = parse_file(root / "out" / "cloud.rcs")

        failed = [check for check in check_invariants(container) if not check.ok]
        self.assertEqual(failed, [])
        self.assertEqual(container.header.count, len(self.POINTS))
        decoded = decode_all(container)
        self.assertEqual(len(decoded), len(self.POINTS))
        center = container.header.center
        expected = sorted(
            (row[0], row[1], row[2], row[4], row[5], row[6], row[3]) for row in self.POINTS
        )
        actual = sorted(
            (
                round(point.x + center[0], 3),
                round(point.y + center[1], 3),
                round(point.z + center[2], 3),
                point.r,
                point.g,
                point.b,
                point.intensity,
            )
            for point in decoded
        )
        for got, want in zip(actual, expected):
            for index in range(3):
                self.assertAlmostEqual(got[index], want[index], places=3)
            self.assertEqual(got[3:], want[3:])

    def test_las_and_pts_pipelines_agree(self) -> None:
        """Одни и те же точки через `.las` и `.pts` дают побайтово один файл."""
        import las2rcs
        from pts2rcs import build_output

        guid = "{11111111_2222_3333_4444_555555555555}"
        with TemporaryDirectory() as folder:
            root = Path(folder)
            source = self.write_las(root / "cloud.las", self.POINTS)
            pts = root / "cloud.pts"
            pts.write_text(
                "\n".join(
                    f"{row[0]} {row[1]} {row[2]} {row[3]} {row[4]} {row[5]} {row[6]}"
                    for row in self.POINTS
                )
                + "\n",
                encoding="utf-8",
            )
            self.assertEqual(
                las2rcs.main([str(source), "-o", str(root / "las"), "-f", "-g", guid]),
                0,
            )
            from_las = (root / "las" / "cloud.rcs").read_bytes()
            from_pts, _ = build_output(pts, guid=guid)

        self.assertEqual(from_las, from_pts)

    def test_bounds_match_pts_on_real_laz(self) -> None:
        """Заголовок LAS/LAZ даёт тот же bbox, что и `.pts` из него (0.1 мм)."""
        from las_read import bounds as las_bounds
        from pts_read import bounds as pts_bounds

        laz = GOLDEN / "level_7.laz"
        pts = GOLDEN / "level_7.pts"
        if not laz.exists() or not pts.exists():
            raise unittest.SkipTest(f"нет {laz} или {pts}")
        low_las, high_las, count_las = las_bounds(laz)
        low_pts, high_pts, count_pts = pts_bounds(pts)
        self.assertEqual(count_las, count_pts)
        for axis in range(3):
            self.assertAlmostEqual(low_las[axis], low_pts[axis], delta=1e-3)
            self.assertAlmostEqual(high_las[axis], high_pts[axis], delta=1e-3)

    def test_cli_names_output_and_refuses_rcs(self) -> None:
        import las2rcs

        with TemporaryDirectory() as folder:
            root = Path(folder)
            source = self.write_las(root / "cloud.las", self.POINTS)
            self.assertEqual(las2rcs.main([str(source)]), 0)
            self.assertTrue((root / "cloud.rcs").exists(), "имя выхода — рядом с входом")
            self.assertEqual(las2rcs.main([str(root / "cloud.rcs")]), 2)
            # .pts на входе LAS-конвертера — ошибка направления, а не «нет файлов»
            wrong = root / "cloud.pts"
            wrong.write_text("0 0 0\n", encoding="utf-8")
            self.assertEqual(las2rcs.main([str(wrong)]), 2)


class RcpInfoTests(unittest.TestCase):
    """`.rcp` = ZIP + XML: настройки импорта читаются (`docs/RCP-FORMAT.md`)."""

    XML = """<?xml version="1.0" encoding="UTF-8"?>
<Autodesk Version="1.0">
  <ProjectId UUID="{00000000-0000-0000-0000-000000000001}"/>
  <Nodes>
    <VoxelTreeRunTime Id="{synthetic}">
      <NodeName Value="cloud"/>
      <NumPoints Value="3"/>
      <HasRGB Value="1"/><HasNormals Value="0"/><HasIntensity Value="1"/>
    </VoxelTreeRunTime>
  </Nodes>
  <UnitType Value="1" IsSurveyMode="0"/>
  <UnitPrecision Value="3"/>
  <ProjectMetaData MajorVersion="1" MinorVersion="6" PatchVersion="0">
    <SaveInformation Application="ReCap" ApplicationVersion="6.0.0.110"
                     Timestamp="20260101T010101" UserId="tester" Count="0"/>
  </ProjectMetaData>
  <Project ver="1.0.0" app="Autodesk ReCap">
    <ShotInfo name="cloud" path=".\\cloud Support\\cloud.rcs"
              rawScanPath=".\\cloud.pts" type="UNSTRUCTURED">
      <OrientedBoundingBox>
        <tform><T x="1" y="2" z="3"/><S x="4" y="5" z="6"/></tform>
      </OrientedBoundingBox>
      <ScanImportSettings>
        <ScanImportTextSettings unitScale="1" skipLines="1" separator=" ">
          <TextImportColumn index="0"/><TextImportColumn index="11"/>
        </ScanImportTextSettings>
        <ScanImportFilterSettings><IntensityClip max="100"/></ScanImportFilterSettings>
        <ScanImportAdvancedSettings density="5" upDirection="Z"/>
      </ScanImportSettings>
    </ShotInfo>
  </Project>
</Autodesk>"""

    @classmethod
    def setUpClass(cls) -> None:
        from rcp_info import describe, read_project

        cls.read_project = staticmethod(read_project)
        cls.describe = staticmethod(describe)

    @staticmethod
    def make_project(folder: Path, xml: str, name: str = "cloud") -> Path:
        """Собрать минимальный `.rcp` (свой генератор, не кусок дампа)."""
        path = folder / f"{name}.rcp"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("preview.jpg", b"jpeg")
            archive.writestr("project.xml", xml)
            archive.writestr("rs_screenshot__.bmp", b"bmp")
        return path

    def test_reads_container_and_settings(self) -> None:
        with TemporaryDirectory() as folder:
            path = self.make_project(Path(folder), self.XML)
            project = self.read_project(path)

        self.assertEqual([name for name, _ in project.entries],
                         ["preview.jpg", "project.xml", "rs_screenshot__.bmp"])
        self.assertEqual(project.application, "ReCap")
        self.assertEqual(project.application_version, "6.0.0.110")
        self.assertEqual(project.version, (1, 6, 0))
        self.assertEqual(project.saved_at, ("20260101T010101",))
        self.assertEqual(project.user, "tester")
        self.assertEqual((project.unit_type, project.unit_precision), (1, 3))

        self.assertEqual(len(project.scans), 1)
        scan = project.scans[0]
        self.assertEqual(scan.name, "cloud")
        self.assertEqual(scan.points, 3)
        self.assertTrue(scan.has_rgb)
        self.assertFalse(scan.has_normals)
        self.assertTrue(scan.has_intensity)
        self.assertEqual(scan.raw_path, ".\\cloud.pts")
        self.assertEqual(scan.translation, (1.0, 2.0, 3.0))
        self.assertEqual(scan.settings.skip_lines, 1)
        self.assertEqual(scan.settings.separator, " ")
        self.assertEqual(scan.settings.columns, (0, 11))
        self.assertEqual(scan.settings.intensity_clip, 100)
        self.assertEqual(scan.settings.density, 5)
        self.assertEqual(scan.settings.up_direction, "Z")
        self.assertIn("5 мм", self.describe(project))

    def test_absent_attributes_mean_defaults(self) -> None:
        """Нет `density` → 1 мм по умолчанию; нет `skipLines` → 0 (шапки нет)."""
        xml = (
            self.XML.replace(' skipLines="1"', "")
            .replace(' density="5"', "")
        )
        with TemporaryDirectory() as folder:
            project = self.read_project(self.make_project(Path(folder), xml))

        scan = project.scans[0]
        self.assertIsNone(scan.settings.skip_lines)
        self.assertIsNone(scan.settings.density)
        self.assertIn("1 мм (по умолчанию)", self.describe(project))
        self.assertIn("skip 0", self.describe(project))

    def test_rejects_non_rcp(self) -> None:
        with self.assertRaises(ValueError):
            self.read_project(Path("cloud.pts"))

    def test_real_project_of_probe6(self) -> None:
        """Боевой `.rcp`: `probe6` импортирован из 6-колоночного `.pts` без шапки."""
        path = GOLDEN / "probe6.rcp"
        if not path.exists():
            raise unittest.SkipTest(f"нет {path}")
        project = self.read_project(path)
        self.assertEqual(project.application_version, "6.0.0.110")
        self.assertEqual(len(project.scans), 1)
        scan = project.scans[0]
        self.assertEqual(scan.points, 3)
        self.assertEqual(scan.settings.skip_lines, None)   # атрибута нет = 0
        self.assertEqual(scan.settings.intensity_clip, 100)
        self.assertIsNone(scan.settings.density)           # 1 мм по умолчанию
        # У `.pts` 6 колонок (x y z r g b), поэтому слот интенсивности (3) пропущен:
        # ReCap пишет шаблон из 7 позиций и выкидывает неиспользованную.
        self.assertEqual(
            [column for column in scan.settings.columns if column != 11],
            [0, 1, 2, 4, 5, 6],
        )
        self.assertFalse(scan.has_intensity)


class Las2PtsTests(unittest.TestCase):
    """Импорт LAS/LAZ через laspy (фикстуры генерируются в тесте)."""

    def setUp(self) -> None:
        try:
            import laspy  # noqa: F401
            import numpy  # noqa: F401
        except ImportError:
            raise unittest.SkipTest("laspy/numpy не установлены")
        self.folder = TemporaryDirectory()
        self.root = Path(self.folder.name)

    def tearDown(self) -> None:
        self.folder.cleanup()

    def _make_las(
        self,
        name: str,
        *,
        point_format: int = 2,
        coordinates: list[tuple[float, float, float]] | None = None,
        color: list[tuple[int, int, int]] | None = None,
        intensity: list[int] | None = None,
    ) -> Path:
        import laspy
        import numpy as np

        coordinates = coordinates or [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)]
        header = laspy.LasHeader(point_format=point_format, version="1.2")
        header.scales = [0.001, 0.001, 0.001]
        header.offsets = [0.0, 0.0, 0.0]
        las = laspy.LasData(header)
        las.x = np.array([point[0] for point in coordinates])
        las.y = np.array([point[1] for point in coordinates])
        las.z = np.array([point[2] for point in coordinates])
        if intensity is not None:
            las.intensity = np.array(intensity, dtype=np.uint16)
        if color is not None:
            las.red = np.array([item[0] for item in color], dtype=np.uint16)
            las.green = np.array([item[1] for item in color], dtype=np.uint16)
            las.blue = np.array([item[2] for item in color], dtype=np.uint16)
        path = self.root / name
        las.write(path)
        return path

    def test_eight_bit_color_and_intensity_are_kept(self) -> None:
        """Цвет и интенсивность уже 0…255 — не масштабируются."""
        from las2pts import main

        source = self._make_las(
            "scan.las",
            color=[(10, 20, 30), (255, 0, 128), (1, 2, 3)],
            intensity=[0, 255, 17],
        )
        self.assertEqual(main([str(source)]), 0)
        lines = (self.root / "scan.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines[0], "3")
        self.assertEqual(lines[1], "0.0000 0.0000 0.0000 0 10 20 30")
        self.assertEqual(lines[2].split()[3:], ["255", "255", "0", "128"])

    def test_sixteen_bit_attributes_are_scaled(self) -> None:
        """16-битные цвет/интенсивность → 0…255 (÷257), а не клиппинг."""
        from las2pts import main

        source = self._make_las(
            "wide.las",
            color=[(65535, 32768, 0), (0, 0, 0), (65535, 65535, 65535)],
            intensity=[65535, 0, 32768],
        )
        self.assertEqual(main([str(source)]), 0)
        lines = (self.root / "wide.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines[1], "0.0000 0.0000 0.0000 255 255 128 0")
        self.assertEqual(lines[2].split()[3:], ["0", "0", "0", "0"])

    def test_without_color_and_intensity_three_columns(self) -> None:
        """`--no-color --no-intensity` → только `x y z`."""
        from las2pts import main

        source = self._make_las("plain.las", point_format=0)
        self.assertEqual(main([str(source), "--no-intensity"]), 0)
        lines = (self.root / "plain.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines[0], "3")
        self.assertEqual(len(lines[1].split()), 3)

    def test_recenter_min_and_no_header(self) -> None:
        """`--recenter min` сдвигает в ноль, `--no-header` убирает счётчик."""
        from las2pts import main

        source = self._make_las(
            "shifted.las",
            coordinates=[(100.0, 200.0, 300.0), (101.0, 200.0, 300.0)],
            point_format=0,
        )
        self.assertEqual(
            main([str(source), "--recenter", "min", "--no-header", "--no-intensity"]),
            0,
        )
        lines = (self.root / "shifted.pts").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0], "0.0000 0.0000 0.0000")
        self.assertEqual(lines[1], "1.0000 0.0000 0.0000")

    def test_rejects_pts_input(self) -> None:
        """`.pts` на входе — ошибка направления."""
        from las2pts import main

        wrong = self.root / "cloud.pts"
        wrong.write_text("0 0 0\n", encoding="utf-8")
        self.assertEqual(main([str(wrong)]), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
