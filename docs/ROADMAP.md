# Roadmap

Status: the Python implementation is closed against acceptance (version 1.0.0, see
`PLAN.md`) and **remains the reference**. The target product of the project is the
standalone `rcsconv.exe`.

Roles are fixed:

| What | Why |
|---|---|
| **Python (`tools/`)** | reference implementation and "oracle" for parity tests, format research environment (section 12), corpus analysis |
| **Rust (`rust/`)** | target deliverable: `rcsconv.exe` — one self-contained `.exe` without Python or external DLLs; the same crates also build `rcsconv-gui.exe` and `rcsconv.dll` (C ABI for the add-in) |

## 1. Goal: `rcsconv.exe`

The owner's product requirements:

- **one file** for the CLI, static linking (`crt-static`): no Python, no external DLLs,
  no installation (the GUI is a second static binary; the add-in additionally loads
  `rcsconv.dll`, built from the same crates);
- the same command set as the Python version: `pts2rcs`, `las2rcs`, `rcs2pts`, `inspect`;
- the result is **byte-identical** to the Python version on the reference files.

### Release acceptance criteria

| Criterion | Threshold |
|---|---|
| binary size | <= 6 MB, static linking (no external DLLs, no Python runtime) |
| parity with Python on `probe6/test1/test2` | byte for byte |
| parity on `level_7` (`.pts` and `.laz`) | byte for byte on the `type 1`/`type 0` blocks, 43 tiles, depth 3 |
| speed: 4 M points | <= 30 s; goal <= 10 s on 20 cores |
| acceptance by the consumer | the file opens in Revit 2023, bbox/count/coordinates per `.clinerules` 6 |

Preliminary measurements (release, GNU, `crt-static`; the reference is Python on the
same machine):

| Metric | Python | Rust |
|---|---|---|
| `level_7.pts` (324,680 points) -> `.rcs` | 11.2 s | **0.7 s** |
| `level_7.laz` (324,680 points) -> `.rcs` | 19 s | **0.7 s** |
| `rcsconv.exe` size (with LAZ) | — | **604 KB**, no external DLLs |

## 2. Why Rust, not C

Rust was chosen by the owner; the reasoning is recorded so the decision is not
revisited:

- **LAZ in Rust is a ready pure crate** (`laz`, plus `las` if needed): no external
  DLLs. In C one would have to depend on a foreign `laszip.dll` (breaking the
  single-file CLI) or write a decompressor; `rcsconv.dll` is our own crate, not an
  external dependency.
- **Parity with Python out of the box**: `cargo test` + the same reference files;
  `serde`, `clap`, `zip`, `quick-xml` cover JSON, the CLI and `.rcp` without manual
  parsing.
- **No manual memory management** in the hot loop (millions of points, 20 threads);
  in C the cost of a mistake is crashes and corrupted data at acceptance.
- **A static GNU binary** is produced with the `crt-static` flag (2–5 MB);
  PyInstaller would have given 30–50 MB plus an external Python runtime.

## 3. Toolchain: GNU, no MSVC

- Toolchain `stable-x86_64-pc-windows-gnu` (rustc 1.99.1); the linker is the
  installed MinGW (WinLibs, gcc 16.2, UCRT).
- It is installed in two equivalent ways: `rustup` (CI, other machines) or
  **portably** — unpack the toolchain archive into `C:\rust` and set the machine
  `PATH` + `CARGO_HOME` (this is the owner's machine; `rustup` is not needed;
  README, section "Toolchain: portable installation").
- **MSVC is not used** — the owner's decision (and it is not needed).
- Release profile: `lto = "fat"`, `codegen-units = 1`, `strip = true`,
  `panic = "unwind"`, `+crt-static`. `unwind` rather than `abort` so the GUI can
  catch a worker panic with `catch_unwind` and show an error box.

## 4. Phases (one session each, each with a parity test)

Done: **R0** (toolchain, static skeleton), **R1** (container, tile tree, point
bodies, `type 1`, `type 0`, writer — byte parity on 9 reference files), **R2** and
**R4** (octree, `.pts` -> `.rcs`, `.las/.laz` -> `.rcs`; parity with Python on
`level_7` — only the GUID differs, as with Python itself), **G0** (GUI preparation:
the `rcs-conv` crate, progress and cancellation in the libraries), **G1** (the
`rcsconv-gui.exe` skeleton on `windows-sys`: window, comctl32 v6 manifest + DPI),
**G2** (file queue), **G3** (parameters), **G4** (background conversion).

The order was changed by the owner's decision: **LAZ matters more than `.pts`**
(capture is done in `.laz`), so R4 was done together with R2 and `.pts` stayed the
second input.

| Phase | What | Acceptance |
|---|---|---|
| **R0** done | install the GNU toolchain (`rustup`, later moved portably to `C:\rust` — see the `PLAN.md` journal), check `cargo build` | a "hello" `rcsconv.exe` builds, no MSVC needed |
| **R1** done | workspace `rust/` + crate `rcs-format`: header, directory, blocks, point body, `type 1`, `type 0` | byte for byte with Python on `probe6/test1/test2` |
| **R2** done | `rcs-cloud` (octree) + `.pts` parser -> the `pts2rcs` command | byte for byte on `level_7.pts` (43 tiles) |
| **R4** done | `cloud-las` (crates `las` + `laz`) -> `las2rcs` | byte for byte on `level_7.laz` |
| **R5** | release profile (`lto`, `strip`, `crt-static`), measurements | size and speed per section 1 |
| **R6** | Win32 GUI: `rcsconv-gui.exe` (`windows-sys`, no framework) | `level_7.laz` from the GUI equals the CLI file byte for byte; see section 4.1 |

Structure (done): `rust/` is the workspace; crates `rcs-format` (container),
`rcs-cloud` (octree and assembly), `cloud-pts`, `cloud-las`, `rcs-conv`
(orchestration: parameters, input walking, conversion — shared by the CLI and the
GUI), `rcsconv` (CLI), `rcsconv-gui` (GUI). Remaining: `R3` — reading `.rcs`
(`rcs2pts`, `inspect`) — and `R5` — final measurements and release.

### 4.1 R6 — Win32 GUI: phases

The owner's decision: the interface is **native Win32** (not a framework), because a
familiar look and "one light `.exe`" are required. The system boundary is the
**`windows-sys`** crate (thin declarations, no proc-macros or build.rs) plus our own
wrappers; `unsafe` is isolated in one or two modules. A separate binary
**`rcsconv-gui.exe`** (`windows_subsystem = "windows"`) next to `rcsconv.exe`: the
CLI keeps its console and exit codes, the GUI does not depend on it (both link the
same crates).

| Phase | What | Acceptance |
|---|---|---|
| **G0** done | `rcs-conv` (shared orchestration), `Progress`/`Cancel`, `Error::Cancelled`, atomic write `tmp`+`rename` | CLI files and texts unchanged (only GUID differences), 53 tests |
| **G1** done | crate `rcsconv-gui`: window, message loop, manifest (comctl32 v6, `PerMonitorV2`), `build.rs` via `windres`, `panic = "unwind"` + `catch_unwind` in the worker | the window opens without a console, `objdump` dependencies are system DLLs only, size measured, `cargo check --workspace` passes on Linux |
| **G2** done | file queue: `GetOpenFileNameW`, `SHBrowseForFolderW`, drag and drop (`WM_DROPFILES`), `SysListView32`, remove/clear, duplicates | the file set matches `expand_sources`; queue model tests |
| **G3** done | parameters (output folder, `--units-scale`, recenter, colour/intensity flags, `-m`, GUID), validation, storage in `HKCU\Software\rcsconv\gui` | test "GUI state -> `Options` == `Options::parse` for the equivalent string" |
| **G4** done | worker thread, progress bar per stage and point (`WM_TIMER` + atomic counters), "Cancel", log, "open folder", summary | `level_7.laz` from the GUI with `-g` equals the CLI file; cancellation leaves no file; the UI does not freeze |
| **G5** | R3: `inspect`/`rcs2pts` in the CLI + an "Info" panel and `.pts` export in the GUI | parity with `tools/rcs2pts.py` (+/-1e-6), `inspect` == `rcs_inspect.py` |
| **G6** | polish (hotkeys, window geometry, taskbar progress), `docs/GUI.md`, acceptance in Revit 2023 | the `docs/GUI.md` checklist can be reproduced by another person |

Requirements: Windows 10 1809+ (the owner's machine is 10 Pro 22H2, build 19045);
the icon is the standard one (a custom `.ico` comes later); the interface language
is **English**, identifiers are English too (`.clinerules` 11).

What must be transferred is the specification, not the code: `docs/RCS-FORMAT.md`
section 9.1 (the build algorithm) and Appendix C (a checklist for an implementation
in another language). The Python version is the oracle: a difference is a Rust bug
until a test proves otherwise.

## 5. Revit plugin (C#) — waiting for the owner's materials

Task: insert `.laz`/`.e57` directly into a Revit model. The owner has prior work;
once it is handed over, a separate document with the architecture will appear (what
calls the converter, how `.rcs` is cached, what happens on upgrade). Not started
yet: without the materials neither the API (Revit API vs an external process) nor
the delivery form (add-in + CLI) can be chosen. The natural integration point is
the finished `rcsconv.exe` (R5). The constraints and the integration contract are
recorded in `docs/REVIT-PLUGIN.md` (later; no C# code yet).

## 6. `.e57` — deferred

Reason: there is no valid `.e57` — both the libe57.org files and the CloudCompare
2.14.beta export contain corrupted XML (details in `docs/CORPUS.md`). The practical
path today is `e57 -> CloudCompare -> .laz -> las2rcs`. When a readable file
appears: `e57_read.py` + `e572rcs.py` (see `PLAN.md`, F2).

## 7. Backlog section 12 of the `.rcs` format

Ordered by value (all are "for understanding", they do not affect compatibility):

1. the decimation mechanism during Unified export (grid of about 5 mm);
2. body bytes `[12]`/`[13]`;
3. tile record fields `@96/@112/@116/@120/tail`;
4. `kind 0..7` records in the payload header;
5. header `count` versus the sum over tiles.

Understanding section 12 is needed by the Rust port only for a complete `inspect`:
generating `.rcs` does not depend on these questions.

## 8. Small items

- `--project` for the converters: build an `.rcp` wrapper (ZIP + XML + our `.rcs`) —
  an experiment, verified by opening in ReCap.
- A format schema in Kaitai Struct / ImHex as independent documentation (the Kaitai
  compiler is installed on the owner's machine).
- CI: the `cargo test` job exists; publishing `.exe` files to releases happens after
  R5 (`docs/RELEASE.md`).
- Repository language: English only. The Russian mirror of the documentation lives
  in `docs-rus/` (git-ignored) on the owner's machine.

## 9. Streaming writer (R9)

The current writer builds the whole `.rcs` in memory (about 100 B/point for
`.pts` and 170 B/point for `.laz`). Measurements and the design of a streaming
writer are in [`docs/STREAMING.md`](STREAMING.md): three passes (count, bucket
partition, assembly), precomputed block sizes, temporary bucket files and atomic
rename. The target is a constant peak of tens of megabytes regardless of the
cloud size; byte parity with the current writer is mandatory.

R9.2 is done: the CLI uses the streaming writer by default (`--in-memory` keeps the
reference path). Peak memory on `attic_south_west.laz` (3.95 M points) fell from
640.9 MB to 19.7 MB; the LAZ file is decompressed two or three times, so the time
grew from 9.1 s to 22.5 s. R9.3 is the optimisation of the passes.

The DLL (R8) is a packaging step and comes after R9: it runs the same core inside
Revit.
