# Streaming writer (R9): measurements and design

Status: R9.0 (measurements), R9.1 (design) and R9.2 (implementation) are complete.

## 1. Why

The current writer builds the whole `.rcs` in memory: it keeps every point, the
payload buffer and the final file. This is fine for 4 million points and fails for
large clouds. The same shape exists in the Python reference and in the C# plugin,
so the language is not the cause: the architecture is.

## 2. Measurements (release build, current in-memory writer)

Method: `Start-Process` of `rust/target/release/rcsconv.exe`, `WorkingSet64` sampled
every 25 ms plus `PeakWorkingSet64` after exit; output goes to `out/mem`.

| Input | Points | Output | Peak RAM | Time | Bytes/point |
|---|---|---|---|---|---|
| `attic_south_west.laz` | 3,953,680 | 79.52 MB | **640.9 MB** | 9.09 s | ~170 |
| `entrance.laz` | 2,580,040 | 51.83 MB | **420.9 MB** | 5.74 s | ~170 |
| `attic_south_west.pts` | 3,953,680 | 79.52 MB | **372.5 MB** | 7.28 s | ~99 |

The LAZ path costs about 270 MB more than `.pts` on the same cloud: the decoded
`las::Point` vector plus the decoder buffers.

### Extrapolation (same ratio)

| Points | `.pts` peak | `.laz` peak |
|---|---|---|
| 4 M | 0.37 GB | 0.64 GB |
| 10 M | 1.0 GB | 1.7 GB |
| 20 M | 2.0 GB | 3.4 GB |
| 50 M | 5.0 GB | 8.5 GB |
| 100 M | 10 GB | 17 GB |

A 1 GB LAZ (tens of millions of points) is a crash risk today.

### Results after R9.2 (streaming is the default)

Same method and inputs, `rcsconv.exe` 1.0.0 built with the streaming writer:

| Input | Points | Output | Peak RAM before | Peak RAM after | Time before | Time after |
|---|---|---|---|---|---|---|
| `attic_south_west.laz` | 3,953,680 | 79.52 MB | 640.9 MB | **19.7 MB** | 9.09 s | 22.48 s |
| `entrance.laz` | 2,580,040 | 51.83 MB | 420.9 MB | **17.9 MB** | 5.74 s | 12.01 s |

Memory no longer depends on the cloud size. The time grows because a LAZ file is
decompressed two or three times (maxima pass, count pass, partition pass). A known
optimisation (R9.3) is to compute the colour/intensity maxima during the count pass
and reuse them, which removes one decompression.

The in-memory reference writer is still available: `rcsconv ... --in-memory`.

### Results after R9.3/R9.4 (adaptive budget, rayon, parallel batches)

The writer now chooses the path from the memory budget (`--memory-budget`,
`RCS_CONV_MEMORY_MB`, default **2048 MB**): if `points * 110 B + 64 MB` fits, the
in-memory writer is used, otherwise the streaming writer. `--in-memory` forces the
reference path and `--memory-budget 0` forces streaming.

Parallelism: `rayon` is used for the octree counts, grouping and tile encoding;
`las` uses `laz-parallel`, so LAZ is decompressed on all cores; the CLI converts
batches of files concurrently (memory budget is split between workers).

| Scenario | Before | After |
|---|---|---|
| `attic_north.pts` (1,411,760 points, 47 MB) | 3.03 s / 137 MB | **1.35 s / 140 MB** |
| `attic_south_west.laz` (3,953,680 points), default budget | 8.35 s / 644 MB | **4.45 s / 643 MB** |
| the same, `--memory-budget 0` (streaming) | 22.5 s / 19.7 MB | **18.25 s / 67 MB** |

Byte parity is unchanged: `level_7.laz` still produces 6,612,979 bytes with the same
SHA-256, and `streaming_matches_in_memory` compares both writers on every reference
file.

## 3. Where the memory goes (from `rcs-cloud` and `rcs-format`)

| Buffer | Size |
|---|---|
| `Vec<CloudPoint>` - every point (`f64` x3 + colour) | ~32 B/point |
| `payloads: Vec<u8>` - point bodies and tile headers | 20 B/point |
| `block3` - copy of the `type 3` records | 368 x tiles |
| `blocks` -> `file: Vec<u8>` - the whole output file | ~20 B/point |
| LAS/LAZ: `Vec<las::Point>` while reading | ~40 B/point |

The final file is assembled by copying the blocks, so at least two copies of the
point data exist at the peak.

## 4. Design of the streaming writer

### 4.1 Sizes are known in advance

Our writer emits payload headers made only of `kind = 8` records, so:

- `type 11` = `sum(4 * count + 16 * count)` = `20 * points`;
- `type 3` = `8 + 368 * tiles`;
- `type 1` = `128 + 72 * sum(words)`;
- `type 0` = 134 bytes.

Therefore the header, the block directory, `type 3` and `type 1` can be written
first, and the directory offsets are known without a second pass over the data.

### 4.2 Three passes

1. **Count pass.** bbox and `HashMap<Cell, u32>` of occupied cells; choose the
   depth. If a cell exceeds `max_per_tile` at the natural depth, repeat the pass at
   `d + 1` (usually zero or one extra pass).
2. **Partition pass.** Points are distributed into N bucket files keyed by the
   tile index (`(tile u32, body[16])` records). Buckets keep the number of open
   files and the buffer size bounded. Per-tile `min/max` are collected here for
   `type 3`.
3. **Assembly pass.** Write header, directory, `type 3`, `type 1`; then for every
   bucket (in tile order) read it, sort its records by tile index, and append the
   bodies in DFS tile order, with the payload header in front of each tile.

### 4.3 Memory target

Boolean: the cell map (kilobytes), per-tile statistics (tens of kilobytes) and one
bucket buffer (bounded, for example 32 MB). Independent of the cloud size.

### 4.4 Safety and cleanup

- Everything is written to `<name>.rcs.tmp<pid>` in the target folder; rename on
  success, so a failed or cancelled run never leaves a truncated `.rcs`.
- Bucket files live in a temporary folder next to the output (same volume) and are
  deleted in `finally`; cancellation is checked between passes and between buckets.
- The existing in-memory writer stays available (`--in-memory`) as the parity
  reference and for small inputs.

## 5. Invariants (must not change)

- The output is byte-identical to the current writer on every reference file
  (`probe6`, `test1`, `test2`, `level_7`, `level_7_n19099`, `attic_south_west`,
  `entrance`, `roof_1_stairwell`).
- DFS tile order, quantisation (1 mm / 1/4 mm / 1/16 mm), Python-compatible
  rounding, `tail[27] = count`, `type 1` words and `K = sum(words)` are unchanged.
- The same 65,536 points-per-tile limit and the same error messages.

## 6. Risks and open questions

| Question | Plan |
|---|---|
| Bucket count and buffer size | tune on the corpus; target peak below 100 MB |
| Temporary disk space | about the size of `type 11` (16 B/point); document it |
| LAZ is decoded twice | measure; expected ~2x the current read time (still seconds) |
| `.pts` is read twice or three times | I/O-bound; acceptable for large text files |
| Duplicate points in one cell | unchanged: the depth increases or the run fails as today |
| Progress reporting | stage + tile counters already exist; add a "partition" stage |

## 7. Implementation plan (R9.2)

1. Refactor the writer so point bodies can be pulled per tile (keep the in-memory
   path as the reference).
2. Count pass and bucket partition; per-tile statistics.
3. Assembly with precomputed offsets and atomic rename.
4. Switch the CLI to the streaming path by default; keep `--in-memory` for debug.
5. Parity tests and a memory measurement on the same three inputs.
6. Documentation: this file, `docs/ROADMAP.md`, `PLAN.md`, `README.md`.

## 8. Relationship to the DLL (R8)

The DLL is a packaging step, not a memory fix: it runs the same streaming core
inside Revit. It makes sense after R9.2, when the core no longer depends on the
cloud size.
