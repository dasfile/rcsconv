# RCS format specification (ADOCT container)

Reverse engineering of the Autodesk ReCap `.rcs` format. The reference version is
**ReCap 2020 (6.0.0.110)**, Unified RCS export (dialect v5). Little-endian, no
alignment.

This document describes **only what has been verified**; unproven statements are
marked explicitly.

## 0. Statuses and how things are verified

| Status | Meaning |
|---|---|
| **CONFIRMED** | a test or script proves it; the verification is referenced |
| **HYPOTHESIS** | a guess; how to check it is stated |
| **DISPROVED** | it was checked and did not hold; what disproved it is stated |

Verification tools:

- `tools/rcs_inspect.py` — container parsing + invariants;
- `tools/tests/test_container.py` — 85 tests (`python -m unittest discover -s tools/tests`);
- `tools/rcp_info.py` — reading an `.rcp` project (ZIP + XML): version, scans, import settings;
- `tools/las2rcs.py` — `.las/.laz` -> `.rcs` (via `las_read` + `rcs_build`);
- `tools/rcs_write.py` — reverse build: `read -> write` is byte-identical on 13 files;
- `tools/pts2rcs.py` — generating `.rcs` from `.pts`; `probe6` is reproduced byte for
  byte (except the GUID and two intensity bytes);
- `tools/analyze_type1.py` — the metadata block (`type 1`): header words, `K`, check
  against the octree;
- `tools/validate_pts.py` — acceptance check of a (`.pts`, `.rcs`) pair: count, bbox,
  `type 1`, tile limit; batch mode `-g reference_dir`; without `.rcs` — a sanity
  report over one `.pts`;
- `tools/pts_subset.py` — uniform `.pts` subsampling (for small reference files);
- `tools/corpus_status.py` — corpus checklist: what was captured, what is left, `count`/tiles;
- `tools/las2pts.py` — LAS/LAZ -> `.pts` import (via `laspy`, see `.clinerules` 10);
- acceptance in **Revit 2023** (23.0.10.18; the end consumer): the cloud opens and
  the dimensions are correct; ReCap 2020 was not used to open files directly;
- ReCap 2020 is the source of the reference files `golden/*.rcs`,
  `golden/reference/*_orig.rcs` and `golden/lazimport/*.rcs`.

Notation: `u8/u16/u32/u64` are unsigned integers, `f32/f64` are IEEE-754,
`DOUBLE_MAX = 1.7976931348623157e308`, `UINT_MAX = 0xFFFFFFFF`.

---

## 1. File map

```
0x0000 .. 0x0109   ADOCT header (see section 2)
0x0109 .. 0x0129   zeros
0x0129             u32 blockCount (number of directory records)
0x012D             u32 = 2
0x0131 .. 0x0135   zeros
0x0135             directory: blockCount x (offset u64, size u64, type u64)
0x0141 .. 0x1812D  zeros: directory terminator + reserve
0x1812D .. EOF     blocks back to back, in directory order
```

CONFIRMED: the directory starts at the odd offset `0x135` (no alignment); the
first block is always at `0x1812D` (98,605) regardless of the block count; the
reserve between the directory and the first block is zero-filled; blocks are
contiguous and end exactly at EOF — verified on every reference file
(`test_blocks_cover_file_exactly`).

---

## 2. Header (0x0000 .. 0x0109)

| Offset | Type | Value | Status |
|---|---|---|---|
| `0x00` | 8 bytes | `41 44 4F 43 54 00 00 00` = `ADOCT\0\0\0` | CONFIRMED |
| `0x04` | u32 | `84` (the `T` byte inside the signature) | CONFIRMED |
| `0x08` | u32 | `3` — **not** the point count (also 3 with 324,680 points) | CONFIRMED |
| `0x0C` | u32 | `2` | CONFIRMED |
| `0x10` | 3xf64 | bbox centre of the `.pts` (exact, no rounding) | CONFIRMED |
| `0x28` | 3xf64 | `0, 0, 0` | CONFIRMED |
| `0x40` | 3xf64 | `1, 1, 1` (scale) | CONFIRMED |
| `0x58` | 3xf64 | repeated centre | CONFIRMED |
| `0x70` | 3xf64 | `-R, -R, -R` — the root cube in the **local** frame | CONFIRMED |
| `0x88` | 3xf64 | `+R, +R, +R` | CONFIRMED |
| `0xA0` | 3xf64 | `-(hx, hy, hz)` — bbox half-sizes, rounded to 1 mm | CONFIRMED |
| `0xB8` | 3xf64 | `+(hx, hy, hz)` | CONFIRMED |
| `0xD0` | u32 | `chunk_id` | CONFIRMED; values in the table below |
| `0xD4` | u32 | `chunk_version` | CONFIRMED; values in the table below |
| `0xD8` | u64 | `count` — number of scan points | CONFIRMED with a caveat: for a ReCap import it exceeds the sum over tiles (section 7) |
| `0xE0` | 3 bytes | `01 01 00` | CONFIRMED |
| `0xE3` | 38 bytes | GUID string `{8_4_4_4_12}`, lower case, underscores | CONFIRMED |
| `0x109..0x129` | — | zeros | CONFIRMED |
| `0x129` | u32 | `blockCount` = number of directory records | CONFIRMED |
| `0x12D` | u32 | `2` | CONFIRMED |
| `0x131..0x135` | — | zeros | CONFIRMED |

**`chunk_id` / `chunk_version` (@0xD0 / @0xD4).** The pair depends on the **import
path**, not on "raw/export" directly (13 corpus references + third-party imports):

| Where the file came from | `chunk_id` | `chunk_version` | Blocks |
|---|---|---|---|
| raw from `.pts` (`<name> Support/`) | 131 | 2 | `[3,1,11,5,0]` |
| raw from `.laz` (`golden/lazimport/<name> Support/`) | **11** | **1** | `[3,1,11,5,0]` |
| raw from a "non-text" import (e57; `golden/reference/raw_v134.rcs`) | 131 | **134** | `[3,1,11,0]` |
| Unified RCS export | 131 | 5 | `[3,1,11,0]` |

HYPOTHESIS: the field is the writer schema version of a particular importer. For
reading and writing we rely on the block set and the common fields, so an
**unknown version is not an error**: `tools/rcs_checks.py` reports it as an
observation, and `dialect()` falls back to the block set when the version is
unknown (`tools/rcs_blocks.py`, `KNOWN_CHUNK_VERSIONS`, test `ChunkVersionTests`).

**Coordinate system.** `center` (@0x10) is an offset (the project origin), not the
centre of the root cube: for `probe6` `R = 2.25` with bbox half-sizes
`(0.667, 1.5, 1.056)`, so the invariant is `max|half_size| <= R`, not
`|center| + |half_size| <= R`.

---

## 3. Block directory

24-byte records `(offset u64, size u64, type u64)`; the end is an all-zero record.
The reserve between the directory and `0x1812D` is zero-filled.

| File | type 3 | type 1 | type 11 | type 0 | type 5 |
|---|---|---|---|---|---|
| `golden/probe6.rcs` (export) | 1,112 | 416 | 60 | 134 | — |
| `golden/level_7.rcs` (export) | 15,832 | 4,808 | 7,162,484 | 134 | — |
| `golden/probe6 Support/probe6.rcs` (raw) | 1,112 | 416 | 60 | JPEG preview | TLV properties |

Block layout: export — `[3, 1, 11, 0]`, raw — `[3, 1, 11, 5, 0]`.
CONFIRMED: the block order in the file matches the directory record order.

---

## 4. Blocks

### 4.1 type 3 — tile tree

`u64 nodeCount` + exactly `nodeCount x 368` bytes. CONFIRMED by division:
probe6 `(1112-8)/3 = 368`, `golden/level_7` `(15,832-8)/43 = 368`,
`level_7_orig` `(116,296-8)/316 = 368`.

Layout of the 368-byte record:

| Offset | Type | Meaning | Status |
|---|---|---|---|
| `0..48` | 6xf64 | tile cube: min xyz, max xyz (edge 2.25 m in the references) | CONFIRMED |
| `48..96` | 6xf64 | real bbox of the tile points (min, max), **rounded to 1 mm** | CONFIRMED: on single-point probe6 tiles both triples equal the point coordinate |
| `96` | u32 | `1..12` (probe6 -> 1, level_7 -> 8..12) | NOT DECODED |
| `100` | u32 | number of tile points | CONFIRMED (sum = 315,264, see section 7 about the header difference) |
| `104` | u32 | tile payload size in the `type 11` block | CONFIRMED: the sum over tiles equals the `type 11` block size exactly |
| `108` | u32 | `0` in our references | NOT DECODED |
| `112`, `116` | u32 | `1..50` | NOT DECODED |
| `120..128` | 2xu32 | array | NOT DECODED |
| `128..368` | 60xu32 | array; on single-point tiles `tail[27] = 1`, the rest zeros | HYPOTHESIS: histogram/cells; confirmed for a single-point tile |

CONFIRMED: tile cubes do not overlap (`test_tile_cubes_are_disjoint`),
`min <= max`, the tile bbox lies inside the cube, and the union of cubes covers the
scene bbox.

### 4.2 type 11 — point bodies

`tile payload = header + count x 16 bytes`, header = `payload_size - 16*count`.

**Point body (16 bytes, LE), coordinates relative to the tile `cube_min`:**

| Offset | Type | Meaning | Status |
|---|---|---|---|
| `0:2` | u16 | `(x - cube_min.x)` in **1 mm** | CONFIRMED |
| `2:4` | u16 | `(y - cube_min.y)` in **1/4 mm** | CONFIRMED |
| `4:6` | u16 | `(z - cube_min.z)` in **1/16 mm** | CONFIRMED |
| `6:8` | u16 | `0` | NOT DECODED |
| `8` | u8 | "intensity" / colouring channel | CONFIRMED: probe `attr_intensity` (input 0..255 -> the same numbers), but the value depends on the importer — see below |
| `9:12` | 3xu8 | colour in **B, G, R** order | CONFIRMED: copied verbatim (100 % of reference points from the source palette) |
| `12:16` | 4 bytes | `0` in the probes; in the corpus `[12]` = 0..255, `[13]` = 0..4 | NOT DECODED |

CONFIRMED on the corpus (13 exports, about 1M points checked by the decoder, the
16-byte layout confirmed by the tile bbox match, error 0.0000):

| Input | Byte `[8]` |
|---|---|
| real intensity (probe `attr_intensity`, 0..255) | the same numbers |
| `.pts` with an intensity column, all zeros (corpus) | **255 for every** point |
| import from `.laz` (`HasIntensity=0`) | **linear gradient along height** (corr with z = 1.000) |
| `.pts` without an intensity column (`probe6/test1/test2`) | garbage (0/241/255) |

So `[8]` is not a copy of intensity but the channel ReCap uses to colour the
cloud (consistent with the `type 0` TLV `101/102`, the colouring attribute
bounds). Our writer puts the `.pts` value there (0 for the corpus) and Revit
accepts it: the colour comes from `[9:12]`, height colouring is not needed.

DISPROVED: "`12:16` is always zero" — true only for the three-point probes; in
corpus clouds `[12]` takes 0..255 and `[13]` takes 0..4 (meaning open).

Important: the delta is **first rounded to 1 mm** and only then multiplied by the
axis scale (otherwise `z = 1.194444` gives `19,111` instead of `19,104`).
CONFIRMED on `probe6`.

Without an intensity column in `.pts`, ReCap writes arbitrary values into byte
`[8]` (observed `0x00`, `0xFF`, `0xF1`) — that is importer behaviour, not format.

**Payload header:** an array of 4-byte records `(u16 index, u8 length, u8 kind)`.
CONFIRMED on 97 tiles (43 `golden/level_7`, 43 raw `level_7 Support`, 9
single-point, 2 `attr_intensity`):

- the header length is always a multiple of 4;
- `length` is a multiple of 8, `kind` in 0..8;
- records with `kind = 8` number exactly `count`, and their `index` is a
  permutation of `0..count-1` (checked including N = 25,385);
- the minimum header is 4 bytes `00 00 00 08` (single-point tiles);
- `header = 4*count + 4*M`, where `M` is the number of `kind != 8` records;
  `M/count` is about 0.68 on large tiles, but it is **not** a function of `count`
  alone (`N=39 -> M=40`, `N=88 -> M=70`, `N=90 -> M=60`).

HYPOTHESIS: `kind 0..7` records are spatial hierarchy descriptors inside a tile.
Disproved: a relation between `M` and `tail_u32`, "`length` = body bytes",
"contiguous body runs", "`index` = point body number".

CONFIRMED (Revit 2020): the header may be written with **`kind = 8` records only**
(`count` records `(index, 0, 8)`) — this is how the files accepted by Revit at
324,680 and 2,580,040 points were built. So the meaning of `kind 0..7` is not
required by the writer.

CONFIRMED (2026-10-30, ReCap exports `attic_south_ost` and `attic_south_west`):
with **`count > 65,536`** there are still exactly `count` `kind = 8` records, but
**`index` repeats** — exactly 65,536 unique values (0..65535) for 66,745 and for
192,402 records (2.9 repetitions). So `index` is not a point id but a 16-bit slot
that ReCap reuses. Our writer emits unique `index` values and is therefore limited
to `MAX_TILE_POINTS = 65536`; the tree splits deeper than ReCap does: at
1,086,140 points we get 186 tiles (depth 4) against 39 tiles (depth 3, max 66,745);
at 3,953,680 ReCap has 192,402 points per tile. Both variants are valid, Revit
opens ours.

### 4.3 type 1 — metadata

```
block = 128 bytes (32 x u32) + K records of 72 bytes
record = u32 0, u32 0, +DOUBLE_MAX x3, -DOUBLE_MAX x3, u32 0, UINT_MAX, UINT_MAX, 0
```

CONFIRMED (`tools/analyze_type1.py`, `MetadataHeaderTests`, 13 references — both
dialects, including `level_7_orig`):

- non-zero header words form a **prefix**: after the first zero only zeros follow;
- `words[L]` is the number of **occupied octree tiles at level `L`**: `words[0] = 1`
  (the root), the last word is the number of leaves = number of `type 3` records.
  Internal tiles are not stored in `type 3` but are counted here;
- `K = (size - 128) / 72 = sum(words)` — the **total number of octree tiles**;
- all `K` records are identical and equal to an "empty attribute range".

| File | leaf level | words | `K` | size |
|---|---|---|---|---|
| `probe6`, `test1`, `test2` | 1 | `(1, 3)` | 4 | 416 |
| `attr_intensity` | 2 | `(1, 2, 2)` | 5 | 488 |
| `golden/level_7` (and the raw copy) | 3 | `(1, 8, 13, 43)` | 65 | 4,808 |
| `level_7_orig` | 4 | `(1, 8, 17, 64, 316)` | 406 | 29,360 |

How it is checked: `type 3` leaves are projected upwards level by level
(`parent = cell >> (depth - L)`), the number of distinct parents at level `L` is
compared with `words[L]`, and `K` is compared with the word sum.

Consequence for the writer: `build_metadata_block(words)` derives the record count
from the words (`records=None -> sum(words)`), and a mismatch is rejected as an
error. That is why `probe6.pts` without any template produces a `type 1` block
that matches the reference byte for byte
(`test_generated_block_matches_golden_probe6`).

HYPOTHESIS: one record corresponds to one octree tile. There is no way to check —
in every available file the records are identical, so "one record per tile" cannot
be distinguished from "one record per level". The meaning of the record fields (an
attribute range?) is also open.

DISPROVED: "the words are scan/sector counters and `K` is unrelated to tiles
(43 tiles -> K = 65)". `K` is the total number of octree tiles, including
internal ones.

### 4.4 type 0 / type 5 — properties and preview

Export (v5): block `type 0`, exactly **134 bytes**:

```
u32 version = 10
TLV (id u32, len u32, payload):
  100  u8  = 1        attribute schema version
  101  f32           upper bound of the active attribute
  102  f32           lower bound
  201  u32 = 0
  202  u32 = 0
  203  f64 = +pi     204  f64 = -pi
  205  f64 = +pi/2   206  f64 = -pi/2
  301  u8  = 0
```

CONFIRMED: the block is byte-identical in `probe6/test1/test2/level_7`; `101/102`
are stubs `-999999.0 / +999999.0`; `build_properties_block()` reproduces it
exactly.

In raw (v2) it is the same 134 bytes, but `101/102` hold real values:
`probe6` -> `(-1.0, -3.1111)` (max/min along height), `attr_intensity` ->
`(255.0, 0.0)` (max/min intensity). HYPOTHESIS: the bounds of the attribute used
to colour the cloud (intensity, otherwise height).

`level_7_orig.rcs` is laid out differently: leading word `11`, `301 = 1`, an extra
TLV `302` of 256 zero bytes — a different import settings set.

In the raw dialect the TLV properties live in `type 5`, while `type 0` is a
**JPEG preview** of the scan (8,462 and 8,520 bytes, starting with
`FF D8 FF E0 ... JFIF`).

---

## 5. Coordinate system and quantisation

CONFIRMED on `golden/probe6.rcs` and `golden/test2.rcs`:

1. `center` in the header is the exact bbox centre of the source `.pts` (no rounding).
2. Local coordinate: `local = point - center`.
3. The root cube is centred at **zero of the local frame**: `root_min = -R`,
   `root_max = +R`. `R = 2.25*2^k`, where `k` is the smallest value with
   `R >= max|extent|` (`probe6`: `extent = 1.5` -> `R = 2.25`; `level_7`:
   `extent = 4.654` -> `R = 9.0`).
4. `extent` (bbox half-sizes) in the header is rounded to 1 mm.
5. The point body stores the delta to `cube_min` **rounded to 1 mm first**:

   ```
   delta_x = round(local.x - cube_min.x, 3) * 1000     (1 mm)
   delta_y = round(local.y - cube_min.y, 3) * 4000     (1/4 mm)
   delta_z = round(local.z - cube_min.z, 3) * 16000    (1/16 mm)
   ```

HYPOTHESIS: the per-axis scales (1 / 0.25 / 0.0625 mm) are a format property; the
rule holds on every checked point, the reason is unknown.

---

## 6. Building the tile tree

CONFIRMED on the reference files:

- the tile edge in **every** file is `2.25 m` (`golden/level_7`: `R = 9`, root edge
  `18`, depth `3` -> `18 / 2^3 = 2.25`);
- a tile is an octree cell of depth `d = log2(2R / 2.25)`;
- cell coordinates: `cube_min = -R + index * edge`, where `index` is the cell number
  along each axis (0..2^d-1);
- the tile order in the `type 3` block is an octree walk; at depth 1 it is the
  ascending key `x + 2y + 4z` (checked on `probe6` and `test2`). For depth 3 the
  order looks like a DFS over children `x + 2y + 4z` (HYPOTHESIS for deep levels —
  there is no byte-level comparison with ReCap).

The writer may choose a **greater** depth (smaller tiles): `pts2rcs` increases `d`
until no cell holds more than `--max-points-per-tile` points. Files built this way
(10,394 tiles instead of 43) were accepted by Revit.

---

## 7. Two dialects: raw (import) and export (Unified RCS)

ReCap writes two different files for one cloud: `<project> Support/<scan>.rcs`
(right after import) and the export result.

| | raw (`Support/*.rcs`) | export |
|---|---|---|
| `chunk_version` (@0xD4) | `2` | `5` |
| block types | `[3, 1, 11, 5, 0]` | `[3, 1, 11, 0]` |
| properties | `type 5` (TLV) | `type 0` (TLV) |
| `type 0` | JPEG preview, ~8.4 KB | — |
| bbox half-sizes | full precision (0.666667 / 1.5 / 1.055556) | rounded to 1 mm |
| `101/102` in properties | real attribute bounds | stubs `-/+999999` |
| `level_7`: count in the header | 324,648 of 324,680 | 317,548 |
| `level_7`: sum of tile counts | 323,353 | 317,538 |
| bytes/point (`level_7`, `type 11`) | 22.734 | 22.763 |

CONFIRMED: the same reader parses both dialects and returns the same points
(`Support/probe6.rcs` -> the same 3 points and colours, different tile order).

The schema version (@0xD4) depends on the import path: raw from `.laz` is `1`, raw
from a "non-text" import is `134` (section 2). The block set stays the same, so the
dialect is determined by the blocks, not by the version.

---

## 8. ReCap reproducibility

CONFIRMED (ReCap 2020 `6.0.0.110`):

- **the export is deterministic almost everywhere**: `golden/probe6.rcs` and
  `golden/reference/probe6_orig.rcs` have the same size (100,327) and differ only
  in the GUID string and the **least significant bit of double** in `center`
  (@0x10), the repeated centre (@0x58), the bbox half-sizes (@0xA0) and — through
  them — in the tile cubes/bboxes (`type 3`, the first 96 bytes of a record). The
  same holds for `test1`, `test2`.
  Example: `center.y` = `1.6111111111111112` (import 2026-10-29) versus
  `1.6111111111111098` (2026-11-02) for the same `.pts` — 1 ULP, but byte
  comparison sees it (test `ulp_prone_offsets`);
- importing the same `.pts` into ReCap gives the same file except the GUID when the
  settings match. The settings are visible in `.rcp` (`docs/RCP-FORMAT.md`), so old
  differences are explained by different `.rcp` files, not by non-determinism:
  `density="5"` (Decimation 5 mm) -> 19,088 points, no `density` (1 mm) -> 19,093 on
  `level_7_n19099`;
- **ReCap drops some points**:
  * on import — 0.03..0.08 % (collapsing inside a 1 mm cell); on the sparse
    `level_7_n19099` there is no loss;
  * on Unified export — the denser the cloud, the more: -2.2 % (324,680),
    -5.9 % (1.09M), -17.6 % (3.95M). It looks like decimation on a grid of about
    5 mm (`docs/CORPUS.md`), and it cannot be disabled by a setting;
- **our writer** writes **all** points from `.pts`: `count` = number of lines, no
  loss. The byte-level proof rests on `golden/reference/*_orig.rcs` (there the
  differences from our file are only the GUID and the intensity bytes).

---

## 9. Writer: assembling a file

Tools: `tools/rcs_write.py` (structures and serialisation), `tools/pts2rcs.py` (CLI).

CONFIRMED: `read -> write` is byte-identical on 13 reference files (both dialects),
`test_round_trip_is_byte_identical`. `pts2rcs` reproduces `golden/probe6.rcs` byte
for byte except the GUID and the intensity byte.

An `.rcs` template is **not needed** — every block is generated. Header constants:

| Field | Value |
|---|---|
| `word4 / word8 / word12` | `84 / 3 / 2` |
| `zeros28`, `unit` | `(0,0,0)`, `(1,1,1)` |
| `chunk_id` | `131` |
| `chunk_version` | `5` |
| `flags` | `01 01 00` |
| blocks | `[3, 1, 11, 0]` |

Build order:

1. `bounds(.pts)` -> `center` (exact), `extent` (1 mm), `R = 2.25*2^k`.
2. Depth `d`: starting from `log2(2R/2.25)`, increase until no cell holds more than
   `max_points_per_tile` points (1 by default).
3. For every occupied cell — a `type 3` record (368 B): cube, point bbox (1 mm),
   `header_u32 = (1, count, payload_size, 0, 0, 0, 0, 0)`,
   `tail_u32` all zeros except `tail[27] = count`.
4. Tile payload (`type 11`): header `count x (index, 0, 8)` + 16-byte bodies.
5. `type 1` — `build_metadata_block(words)`: `words` = occupied octree tiles per
   level, records = `sum(words)` (see section 4.3). `--metadata minimal` is the
   degenerate variant `(1,)` + 1 record (200 B; it does **not** satisfy the 4.3
   invariant and is only a fallback), `--metadata template` takes the block from a
   reference.
6. `type 0` — `build_properties_block()` (134 B).

Tree depth: a tile holds at most `count` bodies, and the payload header `index` is
a `u16` (section 4.2), so the limit is **65,536 points per tile**. Hence
`-m/--max-points-per-tile`: the default `auto` = 65536, and the tree only splits
when necessary. For `level_7` this gives depth 3, 43 tiles and a `type 1` block
**byte-identical to the ReCap reference** (`(1, 8, 13, 43)`, 65 records); a smaller
value (for example `-m 200`) gives a finer tree and a larger file.

### 9.1 Build algorithm (pseudocode, for another language)

The formulas are written so they can be ported to Rust/C++/Go without reading the
Python code. All values are metres, `round(x, 3)` rounds to 1 mm.

```
build(points, max_per_tile = 65536):
    # 1. extents and the local frame
    lo, hi    = min/max over the points (per axis)
    center    = (lo + hi) / 2
    half      = round((hi - lo) / 2, 3)             # bbox extent, 1 mm
    R         = 2.25 * 2^k, the smallest >= max(|half_x|,|half_y|,|half_z|)

    # 2. tree depth: edge 2.25 m at the "natural" depth, finer only when a cell
    #    holds more than max_per_tile points (the u16 index limit)
    d         = round(log2(2R / 2.25))
    repeat:
        edge  = 2R / 2^d
        cell(p)= ( clamp(floor((p.x - center.x + R) / edge), 0, 2^d - 1),
                   clamp(floor((p.y - center.y + R) / edge), 0, 2^d - 1),
                   clamp(floor((p.z - center.z + R) / edge), 0, 2^d - 1) )
        if max(points per cell) <= max_per_tile: break
        d += 1                                        # up to MAX_DEPTH = 20

    # 3. tile order — DFS over octants from the root: key = (x + 2y + 4z) at every
    #    level, from the most significant index bit to the least significant
    for cell in sort(occupied_cells, key = subtree_key(cell, d)):
        cube_min = -R + cell * edge
        cube_max = cube_min + edge
        local(p) = p - center                          # local frame of the file
        # 3.1 point bodies: coordinates relative to the TILE CORNER
        for p in cell:
            ix = round((local(p).x - cube_min.x) * 1000)   # 1 mm
            iy = round((local(p).y - cube_min.y) * 4000)   # 0.25 mm
            iz = round((local(p).z - cube_min.z) * 16000)  # 0.0625 mm
            assert ix,iy,iz in 0..65535                    # with edge 2.25 m: 2250/9000/36000
            body = u16(ix) u16(iy) u16(iz) u16(0) u8(intensity) u8(B) u8(G) u8(R) u32(0)
        # 3.2 tile record (368 B, section 4.1)
        record = f64[3](cube_min) f64[3](cube_max)
                 f64[3](min local) f64[3](max local)      # rounded to 1 mm
                 u32[8] = (1, count, payload_size, 0,0,0,0,0)
                 u32[60] = 0..0, tail[27] = count
        # 3.3 tile payload (section 4.2)
        payload = count * (u16(index=i), u8(0), u8(8)) + count * body

    # 4. file blocks, order and offsets — sections 1, 3
    header.count  = number of points
    header.center = center; root_min = -R; root_max = +R
    header.extent_min = -half; extent_max = +half
    directory + blocks [type 3, type 1, type 11, type 0] contiguously from 0x1812D

    # 5. metadata (section 4.3)
    words[L] = number of DISTINCT parents of the leaves at level L (L = 0..d),
               words[0] = 1, the last word = number of leaves
    K = sum(words)
    type_1 = u32[32] = words, the rest zeros
             + K records of 72 B: u32(0) u32(0) f64(+MAX)*3 f64(-MAX)*3
                                  u32(0) u32(MAX) u32(MAX) u32(0)
```

Every implementation must satisfy the invariants — Appendix C.

CLI (`tools/cli_paths.py`): the first argument is the input (file, directory or
mask), the output name is derived (`<name>.rcs` / `<name>.pts`), an existing
output is not overwritten without `-f`, and a swapped direction (`pts2rcs` with a
`.rcs` input) is rejected with a hint.
7. GUID: a random `uuid4` in the `{8_4_4_4_12}` format; `--guid` is for debugging
   only and is validated before reading `.pts`; hyphens are normalised to
   underscores.

Size guide: `20*N` (bodies + headers) `+ 368*tiles + ~98 KB` of constant part; with
200 points per tile that is about 32 B/point (`level_7` -> 10.4 MB), with 1 point
per tile about 390 B/point.

---

## 10. Acceptance

CONFIRMED (Revit 2020, the end consumer) — files built by `pts2rcs` opened and
showed a correct cloud:

| File | Points | Tiles | `type 1` | Size |
|---|---|---|---|---|
| `probe6_gen.rcs` | 3 | 3 | reference (4 records) | 100,327 |
| `attr_flat.rcs` | 6 | 6 | reference | 101,491 |
| `attr_multi.rcs` | 6 | 2 | reference | 100,019 |
| `level_7_multi.rcs` | 324,680 | 10,394 | reference | 10,422,147 |
| `level_7_no_tpl.rcs` | 324,680 | 10,394 | generated (1 record) | 10,417,539 |
| `level_7_tpl_small.rcs` | 324,680 | 10,394 | from `probe6` (4 records) | 10,417,755 |
| `entrance.rcs` (outside the repository) | 2,580,040 | 64,081 | — | 75,286,163 |

Consequence: the contents of `type 1` do not matter to Revit, and no template is
needed.

Re-acceptance after phase 6 (when `type 1` became derived from the octree):
`probe6_gen.rcs`, `level_7_gen.rcs`, `level_7_no_tpl.rcs` opened in **Revit 2023**
(23.0.10.18) — the cloud and the dimensions were correct. Revit 2020 is not
installed on the machine, so the earlier note "accepted in Revit 2020" refers to an
older run.

Acceptance criteria status:

| Criterion | Status |
|---|---|
| (a) the file opens in an Autodesk tool | OK: Revit 2023; ReCap 2020 was not checked directly |
| (b) bbox = source bbox +/-1e-3 | OK: `validate_pts` / `rcs_inspect --pts`; for ReCap references the tolerance is 1 cm (`INFO`) because they are thinned |
| (c) count = number of points | OK: `count` = number of `.pts` lines |
| (d) coordinates +/-1e-6 | OK: round-trip through our reader (`rcs2pts`); along x the quantisation step is 1 mm |
| (e) the cloud matches visually | OK: Revit 2023 |

Not verified: linking `ins.rvt` (a Revit model) with our file, and opening the file
in ReCap itself (only Revit is available).

---

## 11. Other observations (they do not affect the format)

- **Intensity.** Byte `[8]` of the body is intensity (probe `attr_intensity`:
  input 0/40/80/120/160/255 -> exactly the same values). Without an intensity
  column ReCap writes arbitrary values.
- **ReCap importer limits** (not format limits): a 2-point cloud is rejected, as
  are 6 points without intensity (flat bbox) and 6 points on a straight line (6
  columns); 3+ volumetric points and a cloud with an intensity column are
  accepted. These features were disproved as the sole cause — the ReCap log is
  empty.
- **`.rcp`** is a ZIP of three entries (`<guid>.jpg`, `<guid>.xml`,
  `rs_screenshot__.bmp`); there are no points inside, the XML stores import
  settings and links to `<name> Support/<name>.rcs`. Parsed in
  `docs/RCP-FORMAT.md`, read by `tools/rcp_info.py`.
  DISPROVED: "`.rcp` is not XML but a packed text-binary format".
- **Sentinels.** In `type 1` empty ranges are encoded as `+/-DOUBLE_MAX`; in raw
  properties the same values mean "attribute not set".

---

## 12. Open questions (backlog)

| Question | Value | How to check |
|---|---|---|
| Meaning of the `type 1` record fields (all records identical) | understanding | a cloud with real attribute ranges (several scans, different attributes) |
| Meaning of `kind 0..7` records in the tile payload header | not needed for Revit | a file with `kind 0..7` from ReCap; references with known geometry |
| Tile record fields `@96`, `@108`, `@112`, `@116`, `@120` | not needed (filled with constants) | tiles with the same geometry but different contents |
| Real meaning of `tail_u32` (60 words) | not needed (`tail[27] = count` is enough) | tiles with many points |
| Body bytes `[12]`, `[13]` (0..255 and 0..4) | understanding | correlation with colour/height/tile index on the corpus (13 exports) |
| Exact rule for byte `[8]`: 255 / height gradient / input value | understanding | a scan with **real** intensity (the corpus has zero) |
| Why the header `count` exceeds the sum over tiles | understanding the importer | observations: `level_7_n19099` 0, `level_7` 10, `attic_entrance` 87, `attic_south_ost` 91 |
| Why ReCap loses points on export | understanding | the main cause is `Decimation grid` on import (5 mm -> up to -22 %); the rest is explained by export decimation on a grid of about 5 mm: 1,021,979 versus 1,025,000 cells (+0.3 %), 317,548 versus 317,951 (+0.13 %), 3,253,181 versus 3,283,572 (+0.9 %). Find the step and grid origin — `docs/CORPUS.md` |
| TLV tail of `type 5` (loss of sync after several records) | raw diagnostics | parsing `Support/*.rcs` |
| `.rcp` format: `Color`, `MaxIntensity/MinIntensity`, `PointCloudSegmentList`, `bk1..bk3` | understanding the UI | `docs/RCP-FORMAT.md` section 5 |

Closed: "meaning of the `type 1` header words and `K`" — section 4.3; "the `.rcp`
format (container and model)" — `docs/RCP-FORMAT.md`; "import settings that changed
the counts" — `density` in the `.rcp` XML.

---

## 13. DISPROVED (anti-pitfalls)

- "`0x08` = number of points" — no: `level_7` has 324,680 points and `0x08` holds `3`.
- "the file has a 48-byte point array starting at `0x10`" — no: `0x10` is the bbox centre.
- "RCS = a flat point list, size = 16 + 48*N" — no: it is a container with a block directory.
- "padding the file with zeros up to 100,327 bytes is enough" — no: without valid blocks it will not open.
- "the first coordinates are (-0.7778, 1.6111, -2.0556)" — that is the bbox centre of `probe6.pts`.
- "`blockCount` is at `0x12D`" — no: `0x12D` holds the constant `2`, the counter is at `0x129`.
- "the payload header is `4*count` records `index:08` plus a service tail" — no: `kind = 8` records are interleaved with `kind 0..7` records across the whole header.
- "the last `count` header records are a permutation" — no.
- "`index` of a `kind = 8` record = point body number" — no: on `attr_intensity` the bodies go in the order `[3,1,4,2,0]`, while the `kind = 8` records give `[4,1,2,3,0]`.
- "`M` (number of `kind != 8` records) is related to `tail_u32`" — no.
- "`length` in a header record = body bytes" — no, the sums do not match.

### Why the 2025 attempt was doomed (short)

1. RCS was treated as "a file with points", while it is a container with a block directory and an octree.
2. The header was read incorrectly: `0x10` is the bbox centre, not the first point; `0x08` is not the point count.
3. Some fields were read as big-endian (the old report said "version 50331648, 33554432 points" — those are `0x03000000` / `0x02000000`).
4. Points were searched as consecutive `f64 XYZ` — they do not exist in the file (coordinates are quantised inside tiles).
5. The generator wrote 48-byte records and padded the file with zeros to 100,327 bytes: the size matched, the structure did not.
6. The tail (GUID, tables, sentinels) was copied from one dump — unguaranteed.
7. Success was defined as "the header and the size matched", not "Revit opened the file and the points are in place". Hence `.clinerules` 6: the criterion is Revit, bbox, count.
8. The artefacts of that fitting (`probe6.rcs`, `probe6_final.rcs`, `test1_gen.rsc`) were deleted.

Conclusion: the problem was not the "proprietary" format but the absence of a
verifiable model and the substitution of the acceptance criterion. The full
post-mortem (7 mistakes, why it looked like a "closed door", what worked, and what
is now fixed) is in [`docs/LESSONS.md`](LESSONS.md).
