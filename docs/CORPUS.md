# Real-scan corpus: passport and what it told us

All data lives in one folder, `golden/` (`golden/README.md`). Reference files come
**only** from ReCap 2020 (6.0.0.110), **Unified RCS** export (dialect v5) — the
format our writer produces. Sources were captured on an iPhone (Polycam) in local
metres (CRS = none).

## What is where

```
golden/                        the "from .pts" branch
    <name>.laz                 primary source (iPhone / Polycam)
    <name>.pts                 las2pts.py (count header + 7 columns)
    <name>.rcp                 ReCap project (ZIP: settings XML + preview)
    <name>.rcs                 Export -> Unified RCS — REFERENCE
    <name> Support/<name>.rcs  raw dialect v2 (created by the import)
golden/lazimport/              the "from .laz" branch (same two clouds, no .pts)
golden/reference/              *_orig.rcs — imports of 2026-10-29 (byte proof)
```

## Origin of the `.pts` files (important)

| What | How it was produced | Check |
|---|---|---|
| 10 corpus `.pts` + `level_7` | `tools/las2pts.py --precision 4` from `.laz` | x/y/z, intensity and RGB match the `.laz` **exactly** (0 differences over 324,680 points) |
| `level_7_n19099.pts` | `tools/pts_subset.py -n 20000` from `level_7.pts` (every 17th) | the header is recalculated |
| `probe6/test1/test2.pts` | **manual** probes: 3 points, 6 columns, **no count header**, 15 digits | ReCap accepted them (`NumPoints=3`), but they have no intensity channel |

Why `.pts` does not corrupt the data: in `.laz` the colour is already 8-bit
(`max 255`) and coordinates are multiples of 0.1 mm, so rounding to 4 decimals
loses nothing (the `.laz` header scale is a "fictitious" precision of about 1e-9).
RCS itself quantises to 1 mm / 1/4 mm / 1/16 mm, so the format never stores
anything finer than 0.0625 mm.

## Clouds and counters

`import` is `NumPoints` from the project XML, `export` is the reference header
`count`.

| Cloud | `.pts` | import | export | export loss | tiles / depth |
|---|---|---|---|---|---|
| attic_center_south_west | 1,644,120 | 1,643,488 | 1,509,808 | -8.1 % | 36 / 3 |
| attic_entrance | 962,000 | 961,763 | 908,244 | -5.6 % | 45 / 3 |
| attic_north | 1,411,760 | 1,411,301 | 1,305,132 | -7.5 % | — |
| attic_north_ost | 1,412,320 | 1,411,939 | 1,322,122 | -6.4 % | — |
| attic_south_ost | 1,086,140 | 1,085,855 | 1,021,979 | -5.9 % | 39 / 3 |
| attic_south_west | 3,953,680 | 3,950,433 | 3,253,181 | -17.6 % | 38 / 3 |
| entrance | 2,580,040 | 2,578,690 | 2,314,856 | -10.2 % | — |
| level_7 | 324,680 | 324,648 | 317,548 | -2.2 % | 43 / 3 |
| level_7_n19099 | 19,099 | 19,099 | 19,093 | -0.03 % | 42 / 3 |
| roof_1_stairwell | 3,130,480 | 3,129,017 | 2,845,656 | -9.1 % | — |

The `.laz` branch (A/B control of the same two clouds):

| Cloud | import | export | tiles / depth | delta vs `.pts` branch |
|---|---|---|---|---|
| attic_center_south_west | 1,643,470 | 1,512,729 | 267 / 4 | import -18, export +2,921 (+0.19 %) |
| attic_entrance | 961,759 | 908,038 | 153 / 4 | import -4, export -206 (-0.02 %) |

So the `.pts` intermediate changes the counters by 0.02–0.2 % — that is rounding to
0.1 mm before ReCap decimation. The structure (`type 1`, number of tiles) does not
depend on the branch: the `(1, 8, ...)` words match the tree, but the depth of the
`.laz` branch is one higher (267 tiles at depth 4 against 36 tiles at depth 3),
while the largest tile in the `.pts` branch holds 102,504 points (ReCap allows
> 65,536, see `docs/RCS-FORMAT.md` 4.2) and in the `.laz` branch 20,992.

## What ReCap loses and why (hypothesis, partly confirmed)

1. **Import.** Loss 0.03–0.08 % (on the sparse `level_7_n19099` — **zero**): points
   collapse inside a 1 mm cell. The numbers are the "import" column against `.pts`.
2. **Export.** Loss grows with density: -2.2 % at 324K, -17.6 % at 3.95M, -0.03 % at
   19K. It looks like **decimation on a grid of about 5 mm** (it cannot be disabled
   by a setting). Checked against raw `.pts` (unique cells):

   | Cloud | export | 5.0 mm cells | difference |
   |---|---|---|---|
   | attic_south_ost | 1,021,979 | 1,025,000 | +0.3 % |
   | level_7 | 317,548 | 317,951 | +0.13 % |
   | attic_south_west | 3,253,181 | 3,283,572 | +0.9 % |

   For 1 mm cells there is no match (1,083,683 versus 1,021,979), so the "1 mm
   grid" hypothesis is disproved. The exact step and grid origin are an open item
   in `docs/RCS-FORMAT.md` section 12.
3. **Older numbers** (`315,270`, `19,088`) belonged to projects with `density="5"`
   (Decimation 5 mm) — visible in the XML of the old `.rcp` files. The current
   corpus has no `density`, i.e. 1 mm.

## Import protocol (ReCap 2020, manual)

1. `File -> New Project`, folder `golden/`, project name = cloud name.
2. `Import -> Point Cloud` -> choose `.pts` (or `.laz`).
3. Settings (all matter):

   | Field | Value | Why |
   |---|---|---|
   | `Filtering -> Clip points` | **0 %** | no clipping needed (`IntensityClip max="100"`) |
   | `Text Options -> Delimiter` | `space` | our `.pts` files use spaces |
   | `Text Options -> Unit` | `1 Meter` | local metres |
   | `Text Options -> Skip` | **1** with a count header, otherwise **0** | `probe6/test1/test2` have no header |
   | `Advanced -> Decimation grid` | **1 mm** (do not change) | 5 mm cuts up to -22 % |
   | `Advanced -> Coordinate System` | `none` | a wrong CRS reprojects |
4. `Export -> Unified RCS` next to the `.pts`, name = cloud name.

You can check the result from the `.rcp` itself (it is a ZIP):
`python tools/rcp_info.py golden/*.rcp` prints `skipLines`, `separator`,
`IntensityClip`, `density`, `NumPoints`, `HasIntensity`.

## How to check

```powershell
# checklist: what was captured, raw/export count, tiles/depth
python tools/corpus_status.py golden -g golden

# our conversion and comparison with the reference (the reference is <name>.rcs)
python tools/pts2rcs.py "golden/*.pts" -o out
python tools/validate_pts.py "golden/*.pts" -g golden

# sanity report for a single .pts
python tools/validate_pts.py golden/probe6.pts
```

The reference `count` is an **observation** (`INFO`): the criterion "count = number
of points" is applied only to our files; the reference bbox drifts by up to 2–3 mm
because of decimation, so references use a 1 cm tolerance (`validate_pts` without
`--strict`).

## E57 — deferred (decision of 2026-11-03)

The `.e57` branch was stopped: **no valid `.e57` could be obtained**, and without
one a reader cannot be developed.

- `golden/e57_example/` — reference files from libe57.org (`bunnyFloat`,
  `bunnyDouble`, `pump*`), written by `InteliSum-LD3-Studio`. Their XML is
  **corrupted** (garbage bytes inside tag names) and cannot be read by standard
  libraries.
- `golden/e57_example_2/` — CloudCompare 2.14.beta export (`E57Format-3.4.1`): the
  E57 header is correct, the data block (29.7 MB) is byte-identical between saves,
  but the XML of **every** file has 3–9 garbage insertions
  (`<isAtomicClock...Referenced>`, `<xMinimum type="Fl...oat">`), and
  `xmlPhysicalLength` is 9 bytes shorter than the real length. ReCap 2020 does not
  import such files ("it does not see the links in the project tree") — the reason
  is the file, not ReCap.
- Decision: `.e57` is **in the backlog**; the practical path is
  `e57 -> CloudCompare -> .laz -> tools/las2rcs.py`. Our own `e57_read.py` will be
  written when a valid file appears.
- Side result: `golden/reference/raw_v134.rcs` (72 points, `@0xD4 = 134`) showed
  that `chunk_version` depends on the import path (`docs/RCS-FORMAT.md` section 2).
- The e57 files themselves were deleted by the owner as junk; they are not in git
  or in `MANIFEST.sha256`.
