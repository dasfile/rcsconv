# Project handover

A short entry point for a new conversation or agent. Details are in `PLAN.md`,
`docs/RCS-FORMAT.md` and `docs/RCP-FORMAT.md`.

## What this project is

Reverse engineering of Autodesk ReCap formats: the `.rcs` container (signature
`ADOCT`, reference version ReCap **2020 / 6.0.0.110**) and the `.rcp` project. The
goal is a reversible `.rcs` reader and writer that Revit accepts. Python 3.11
(stdlib + `numpy`) is the reference implementation; `laspy` is needed only for
LAS/LAZ. The target product is the standalone `rcsconv.exe` (and `rcsconv-gui.exe`)
in Rust (`docs/ROADMAP.md`); Python remains the reference and the oracle.

## State

| Layer | Done |
|---|---|
| `.rcs` container: header, directory, dialect v2 (raw) / v5 (export) | yes |
| Tile tree (`type 3`, 368 B) and point bodies (`type 11`, 16 B) | yes |
| Metadata (`type 1`) and properties (`type 0`) — parsed and generated | yes |
| Reader (`rcs2pts`), writer (`rcs_build` + `pts2rcs`) — without ReCap and without a template | yes, accepted in Revit 2023 |
| Inputs: `.pts` (`pts2rcs`) and `.las/.laz` (`las2rcs`) | yes |
| `.rcp` = ZIP + XML, import settings (`rcp_info`) | yes, `docs/RCP-FORMAT.md` |
| Corpus and reference files: `corpus_status`, `validate_pts`, `pts_subset`, `las2pts` | yes, `docs/CORPUS.md` |
| Tests | 85 Python (`python -m unittest discover -s tools/tests`), 71 Rust (`cargo test --workspace`) |
| Rust `rust/` (R0): GNU toolchain, static `rcsconv.exe` | yes |
| Rust `rcs-format` (R1): container, tiles, points, `type 1`/`type 0`, writer | yes, parity on 9 reference files |
| Rust `rcs-cloud` + `cloud-pts`/`cloud-las` (R2/R4): `.laz`/`.las`/`.pts` -> `.rcs` | yes, parity with Python (except GUID) |
| Rust GUI `rcsconv-gui` (G1-G4): Win32, file queue, parameters, background conversion | yes, `docs/GUI.md` |

## Next task

**R3: reading `.rcs` in Rust** (`docs/ROADMAP.md`) — the `rcs2pts` and `inspect`
subcommands on top of the finished `rcs-format` (container, tiles and point bodies
are already ported and verified). Acceptance: the same points, indices and bbox as
Python (`tools/rcs2pts.py`, `tools/rcs_inspect.py`). Then R5: final measurements and
the `.exe` release (see `docs/RELEASE.md`).

Done before that: `.las/.laz` -> `.rcs` (`las2rcs`) and `.pts` -> `.rcs`
(`pts2rcs`) in Rust — parity with Python on `level_7` (324,680 points, 43 tiles)
**byte for byte except the GUID**; 0.7 s against 11.2 s in Python, the `.exe` is
static.

The Python implementation is closed against acceptance and removed from the
critical path: it is the reference and oracle for parity tests, and the research
environment for section 12. Optimising Python (`numpy`, speed) is **not planned** —
the goal is set in Rust.

Open questions "for understanding" — `docs/RCS-FORMAT.md` section 12: export
decimation (grid of about 5 mm), body bytes `[8]`/`[12]`/`[13]`, `kind 0..7`, tile
record fields `@96...`, header `count` versus the sum over tiles. The `.e57` branch
is deferred (`docs/CORPUS.md`); the Revit plugin waits for the owner's materials.

Repository language: **English**. The Russian mirror of the documentation is kept
locally in `docs-rus/` (git-ignored) and is not part of the repository.

## About the data

The data is not in git: the reference files were created by Autodesk and are
captured locally (`golden/`, `docs/CORPUS.md`). `golden/` contains `.laz/.pts`
sources, `.rcp` projects, `.rcs` references, raw `Support/*.rcs`, the second branch
`lazimport/` and `reference/*_orig.rcs`. Run results go to `out/`, experiments to
`dumps/`.

## What not to do

- Do not modify `golden/**` or `golden/reference/*_orig.rcs`.
- Do not fit bytes to an expected size and do not hard-code dump fragments.
- Do not bring back the writer's dependency on an `.rcs` template (`.clinerules` 14).
- Do not import synthetic clouds into ReCap: that direction is closed (`PLAN.md`).

## Reading order

1. `PLAN.md` — status, phases, journal.
2. `.clinerules` — rules and acceptance criteria.
3. `docs/RCS-FORMAT.md`, `docs/RCP-FORMAT.md` — specifications.
4. `README.md`, `docs/CORPUS.md` — how to run things and on what data.
5. `docs/GUI.md` — the GUI reference and its acceptance checklist.
6. `tools/rcs_blocks.py` -> `tools/rcs_points.py` -> `tools/rcs_write.py` -> `tools/rcs_build.py`.
7. `tools/tests/test_container.py` — what the tests guarantee.
8. `docs/LESSONS.md` — why the first 2025 attempt failed (read it before the Rust port,
   not after).

## Housekeeping notes

- Branch `main`; `.gitignore` excludes `golden/`, `out/`, `dumps/`, `docs-rus/`,
  `__pycache__`, `rust/target/`.
- Reference ReCap version — 2020 (6.0.0.110); acceptance — Revit 2023 in the owner's
  environment.
- Reverse-engineering tools (Ghidra, Kaitai) are not required: `.rcp` was parsed with
  the standard library, and the section 12 questions are solved by differential
  analysis of the corpus.
