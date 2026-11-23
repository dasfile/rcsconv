# PLAN — RCS format reverse engineering (ADOCT / Autodesk ReCap)

## Handover (read this first)

**Status: the project is closed against the acceptance criteria.** The reader and
writer are ready, the specification is published, and acceptance passed in **Revit
2023** (23.0.10.18 — the only Revit on the machine) on **9 real clouds** (0.3–4.0
million points): every file built by `pts2rcs` opened and the dimensions matched a
ruler — in metres. On `probe6` the file is reproduced byte for byte (except the GUID
and 2 intensity bytes); on `level_7` the `type 1` block matches the ReCap reference
byte for byte.

| Layer | Done |
|---|---|
| Container: header, block directory, dialect v2 (raw) / v5 (export) | yes |
| Tile tree (`type 3`, 368-byte record) | yes |
| Point bodies (`type 11`, 16 B) and the tile payload header | yes |
| Metadata (`type 1`) and properties (`type 0`) — parsed and generated | yes |
| Reading `rcs2pts`, round-trip through ReCap | yes |
| Writer `tools/rcs_write.py` + `tools/pts2rcs.py` | yes, accepted in Revit 2023 |
| CLI (`cli_paths`), acceptance (`validate_pts`), corpus (`corpus_status`, `pts_subset`) | yes |
| LAS/LAZ import (`las2pts`, via `laspy`) | yes |
| `.rcp` project: ZIP + XML, import settings (`rcp_info`) | yes, `docs/RCP-FORMAT.md` |
| Corpus: `golden/` — `.pts` and `.laz` branches, raw+export+reference | yes, `docs/CORPUS.md` |
| Specification + a checklist for another implementation | `docs/RCS-FORMAT.md` section 9.1, Appendix C |
| Tests | 85 (`unittest`); without `golden/` some classes are skipped |
| Target product: `rcsconv.exe` in Rust (GNU, `crt-static`) | R0–R2, R4 done; GUI G1–G4 done; R3, R5 — `docs/ROADMAP.md` |

**There are no mandatory tasks.** Open questions (`docs/RCS-FORMAT.md` section 12)
are "for understanding" and do not affect compatibility: the meaning of `kind 0..7`
records, tile record fields `@96...` and `tail_u32`, the meaning of `type 1` record
fields, the exact decimation step on export (about 5 mm), header `count` versus the
sum over tiles, body bytes `[8]`/`[12]`/`[13]`. The `.rcp` format is parsed
(`docs/RCP-FORMAT.md`). The `.e57` branch is **deferred** until a valid file appears
(`docs/CORPUS.md`, "E57 — deferred").

**What not to do:** generate probes and import synthetic clouds into ReCap (the
importer rejects them without a clear reason, the log is empty — see the journal);
fit bytes to an expected size; hard-code dump fragments; modify `golden/**`; bring
back the writer's dependency on an `.rcs` template.

**Reading order:** `PLAN.md` -> `.clinerules` -> `docs/RCS-FORMAT.md` -> `README.md`
-> `tools/rcs_blocks.py`, `tools/rcs_points.py` -> `tools/rcs_write.py`,
`tools/pts2rcs.py` -> `tools/tests/test_container.py`.

**Disk and git:** only our code (`tools/`), the Rust port (`rust/`), documentation
and rules are in git. All data (`.laz`/`.pts` sources, `.rcp` projects, `.rcs`
references, `reference/*_orig.rcs`) lives locally in **`golden/`** and is excluded by
`.gitignore`; there are also `golden/MANIFEST.sha256` (SHA-256 of all data) and
`golden/README.md` (corpus passport). Run results go to `out/`, experiments to
`dumps/`. The reference ReCap version is **2020 (6.0.0.110)**. The repository is
English-only; the Russian mirror of this journal and the other documents is kept
locally in `docs-rus/` (git-ignored).

## Goal

A reversible **reader + writer** for the `.rcs` container (ReCap 6.0.0.110),
verifiable with Autodesk tools: ReCap opens the result, Revit links the cloud, and
coordinates/colours match the source `.pts`.

**Not a goal:** "a converter at any cost", size fitting, editing third-party files.

## Input data

Everything lives in **`golden/`** (not in git, about 1.6 GB; see
`golden/README.md`):

| File | What it is | How to get it |
|---|---|---|
| `golden/<name>.laz` | iPhone (Polycam) source, 10 clouds | capture / no `las2pts` needed |
| `golden/<name>.pts` | text representation: count header + 7 columns | `tools/las2pts.py --precision 4` |
| `golden/<name>.rcp` | ReCap project = ZIP (XML with settings + preview) | ReCap 2020, `Import` (settings — `docs/CORPUS.md`) |
| `golden/<name>.rcs` | Unified RCS export (v5) — **reference** | ReCap 2020, `Export -> Unified RCS` |
| `golden/<name> Support/<name>.rcs` | raw dialect v2 (right after import) | created by ReCap itself |
| `golden/probe6|test1|test2.pts` | 3 points, 6 columns, **no header** | manual probes (needed by tests) |
| `golden/probe6|test1|test2.rcs` | their exports (100,327 B) + `Support` raw | ReCap 2020 |
| `golden/lazimport/` | second branch: import **directly from `.laz`** (2 clouds) | ReCap 2020, `Import -> .laz` |
| `golden/reference/*_orig.rcs` | imports of 2026-10-29 (4 files) | ReCap 2020; they carry the byte-level proof |
| `golden/level_7_n19099.pts/.rcs` | `level_7` subsample (every 17th point) | `tools/pts_subset.py -n 20000` |
| `golden/MANIFEST.sha256` | SHA-256 of all data | regenerated as needed |
| `ins.rvt` | the Revit model — the end consumer | — (outside the repository) |

`probe6/test1/test2` are manual, so they differ qualitatively from the corpus: 6
columns (no intensity channel) and no count header (no `skipLines` in `.rcp`). They
are valid fixtures, but "how ReCap handles intensity" cannot be studied with them.

Note: `golden/level_7.rcs` (317,548 points, 43 tiles) was captured with
`Decimation 1 mm`; `golden/reference/level_7_orig.rcs` is an import with different
settings (316,890 points, 316 tiles). The pair for reverse engineering is
`golden/level_7.rcs`, see section 4 and `docs/CORPUS.md`.

## Phases

### Phase 0 — hygiene + golden set  done
- [x] directory clean-up (72 files of the old approach deleted, 9 reference files kept)
- [x] directory structure, git, hash manifest
- [x] ReCap 2020: `data/pts/*.pts -> golden/*.rcs` (probe6, test1, test2, level_7)
- [x] learned: export is deterministic (only the GUID, 30 bytes), import too (28 GUID bytes)
- **Acceptance:** for every reference file the pair ".pts <-> .rcs bytes" is known — done.

### Phase 1 — container parser  done
- [x] `tools/rcs_inspect.py`: header, block directory, TLV
- [x] invariant check on all 4 reference files (8 tests, all pass)
- **Acceptance:** blocks are contiguous and cover the file to the last byte; the
  header bbox = the `.pts` bbox; `count` = the number of points on 3-point files.

### Phase 2 — tile tree (type 3 block)  done
- [x] established: `u64 nodeCount` + `nodeCount x 368` bytes
- [x] record layout: tile cube (0..48), real point bbox (48..96, rounded to mm),
      `count` (@100), `payload_size` (@104); the payload sum = the `type 11` block size
- [x] `TileInfo`/`iter_tiles`/`tile_payload` — the data bounds of every tile are known
- [ ] fields `@96`, `@112`, `@116` and arrays `@120`, `@244` — not decoded
- **Acceptance:** the union of cubes is about the scene bbox; the payload sum = the
  `type 11` block — done.

### Phase 3 — point decoder (type 11 block)  done (core)
- [x] 16-byte point body: x (1 mm), y (1/4 mm), z (1/16 mm) from `cube_min`, colour B, G, R
- [x] `tools/rcs_points.py`: `decode_body`, `decode_tile`, `decode_all`
- [x] `tools/rcs2pts.py`: export to `.pts` (`level_7` — about 2 s)
- [x] verified: 9 single-point reference files (coordinates and colours), `level_7` —
      bbox +/-0.3 mm
- [x] both dialects (v2 raw and v5 export) are read by the same code
- [x] body byte `[8]` is **intensity** (confirmed by the `attr_intensity` probe)
- [x] the tile payload header is decoded: `4*count + 4*M` records
      `(u16 index, u8 length, u8 kind)`; `kind=8` records are per point (a permutation)
- [ ] the meaning of `kind 0..7` records and the `index` field is not decoded; **not
      needed by the writer** (Revit accepted files whose header had only `kind = 8`)
- [ ] body bytes `[6:8]` and `[12:16]` are not decoded (zero in the reference files)
- [ ] count difference: 315,264 (tiles) versus 315,270 (header)

### Phases 4-7 — writer, import branches, `.rcp`, acceptance  done
- [x] `tools/rcs_write.py`: `read -> write` byte-identical on 13 reference files
- [x] `tools/rcs_build.py`: the octree and assembly from scratch; `type 1` derived
      from the tree (43 tiles, depth 3 on `level_7`)
- [x] `tools/pts2rcs.py`: `.pts -> .rcs`; `probe6` byte for byte except GUID and 2
      intensity bytes
- [x] two import branches (`.pts`, `.laz`) and `reference/*_orig.rcs` — the writer's
      byte proof
- [x] `.rcp` parsed as ZIP + XML (`tools/rcp_info.py`, `docs/RCP-FORMAT.md`)
- [x] acceptance in Revit 2023 on 9 real clouds; `type 1`/`type 0` byte-identical
- [x] `.e57` closed for lack of a valid file (`docs/CORPUS.md`)

### Rust port (R0-R4, G0-G4)  done
- [x] R0: GNU toolchain, `rcsconv.exe` skeleton, static CRT
- [x] R1: `rcs-format` — container, tiles, point bodies, `type 1`/`type 0`, writer
- [x] R2/R4: `rcs-cloud` + `cloud-pts` + `cloud-las` — `.pts`/`.las`/`.laz` -> `.rcs`
- [x] G0: `rcs-conv` (shared orchestration), progress/cancel, atomic write
- [x] G1-G4: `rcsconv-gui.exe` on `windows-sys`: window, file queue, parameters,
      background conversion; a GUI file equals a CLI file byte for byte
- [ ] R3: reading `.rcs` in Rust (`rcs2pts`, `inspect`)
- [ ] R5: final measurements and release

### Phase E — English-only repository (this iteration)  done
- [x] E0: `docs-rus/` isolated in `.gitignore`/`.clineignore`; the Russian originals of
      all documents (`README.md`, `PLAN.md`, `.clinerules`, `docs/*`) were copied there
      with a local `MANIFEST.sha256`
- [x] E1: all user-facing Rust strings (GUI, CLI, errors, progress stages) translated to
      English; tests updated; 71 Rust tests green
- [x] E2: all Rust comments/rustdoc, `Cargo.toml`, `rust-toolchain.toml`,
      `.cargo/config.toml`, `res/app.rc`, `build.rs` translated; the double-encoded
      Cyrillic damage in `rust/Cargo.toml` and `rcsconv-gui/src/win32.rs` was repaired
      and translated (0 Cyrillic characters left in `rust/`)
- [x] E4: `README.md`, `.clinerules`, `docs/RCS-FORMAT.md`, `docs/RCP-FORMAT.md`,
      `docs/CORPUS.md`, `docs/ROADMAP.md`, `docs/HANDOVER.md`, `docs/LESSONS.md`,
      `docs/GIT.md`, `PLAN.md` rewritten in English; `docs/GUI.md`, `docs/RELEASE.md`,
      `docs/REVIT-PLUGIN.md` added
- [x] stale facts fixed: the toolchain lives in `C:\rust` (`CARGO_HOME=C:\rust\cargo-home`),
      not `C:\mingw64\rust`; `panic = "unwind"` instead of `abort`; test counts 85/71;
      GUI phases G1-G4 done
- [ ] E3: the Python reference tools (`tools/`) still carry Russian comments, docstrings
      and messages. The user-facing behaviour is unchanged; this is the remaining
      translation task (planned next, it does not affect the release).

### Phase R5 — release (in progress)
- [x] version 1.0.0 for every crate; `VERSIONINFO` is generated by `build.rs` into
      `version.h` from `CARGO_PKG_VERSION_*` (checked: the GUI exe reports 1.0.0.0)
- [x] `docs/RELEASE.md` checklist, `.github/workflows/release.yml` (tag `v*` -> zip with
      both `.exe` files, `LICENSE`, `README.md`, `SHA256SUMS.txt`)
- [x] release build, `cargo test --workspace` (71 passed), CLI/GUI smoke tests,
      `probe6.pts -> out/probe6.rcs` = 100,327 bytes (reference size)
- [ ] final measurements table and Revit acceptance run for the 1.0.0 binaries

## Journal

Entries up to 2026-11-03 (in Russian) are preserved in `docs-rus/PLAN.md`. From the
English-only phase onward the journal is kept here in English.

- **E0** — `docs-rus/` contains byte-identical Russian copies of `README.md`,
  `PLAN.md`, `.clinerules` and all `docs/*` files; its SHA-256 manifest is local.
  `.gitignore` and `.clineignore` exclude the folder.
- **E1** — Rust strings: GUI labels/hints/messages, `Stage::title`,
  `EntryStatus::label`, `RecenterChoice::label`, `describe()`, all `Display` errors of
  `rcs-format`/`rcs-cloud`/`cloud-pts`/`cloud-las`, CLI reports. Message separators
  use ASCII only for console safety. `cargo test --workspace`: 71 passed.
- **E2** — comments/rustdoc translated across the workspace. Two files had
  double-encoded Cyrillic (`rust/Cargo.toml`, `rcsconv-gui/src/win32.rs`); the text
  was decoded back and translated. Cyrillic search in `rust/` returns nothing; tests
  still 71 passed.
- **E4** — documentation rewritten in English. `RCS-FORMAT.md` keeps every confirmed
  fact, table and number; the prose is tightened. `README.md` describes the Rust
  product first, the GUI second and the Python reference third.
- **Not done in this iteration** — E3 (Python tool translation). Deferred on purpose:
  the release path (Rust + GUI + docs) has priority; the Python tools stay the frozen
  oracle. Tracked as an open item above and in `docs/ROADMAP.md`.
