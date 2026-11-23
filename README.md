# rcsconv — a reversible Autodesk ReCap `.rcs` converter

A reader and **writer** for the proprietary `.rcs` container (signature `ADOCT`,
reference version ReCap 2020 / 6.0.0.110). It builds `.rcs` **without ReCap and
without a template** — from `.pts`, `.las` or `.laz` — and the file opens in Revit.

The project grew out of reverse engineering: the format specification
(`docs/RCS-FORMAT.md`) and the `.rcp` project format (`docs/RCP-FORMAT.md`) are as
valuable as the code.

## What it does

| Input | Output | Command |
|---|---|---|
| `.pts` (Leica text, 3–7 columns) | `.rcs` (Unified RCS, v5) | `tools/pts2rcs.py` |
| `.las` / `.laz` | `.rcs` | `tools/las2rcs.py` |
| `.rcs` | `.pts` | `tools/rcs2pts.py` |
| `.rcs` | invariant check, bbox, `type 1` | `tools/rcs_inspect.py`, `tools/validate_pts.py` |
| `.rcp` (ReCap project) | import settings from XML | `tools/rcp_info.py` |

Also: `.pts` subsampling, LAS/LAZ → `.pts`, comparing two `.rcs` files, inspecting
tiles and metadata, and a corpus checklist.

**The standalone `rcsconv.exe` (Rust)** builds `.rcs` from `.las/.laz` and `.pts`:
one static file, no Python and no DLLs, byte parity with the Python version
(except GUID): [`docs/ROADMAP.md`](docs/ROADMAP.md). Reading `.rcs` in Rust is the
next step. Very large clouds: the writer now builds the file on disk (streaming), with a peak
of a few tens of megabytes regardless of the cloud size; measurements and the design
are in [`docs/STREAMING.md`](docs/STREAMING.md). `--in-memory` keeps the reference
writer for comparison.

## Quick start

```powershell
# .pts -> .rcs and back (three small probes live in golden/)
python tools/pts2rcs.py golden/probe6.pts -o out           # -> out/probe6.rcs
python tools/rcs2pts.py out/probe6.rcs -o out -c -i         # -> out/probe6.pts
python tools/rcs_write.py out/probe6.rcs out/copy.rcs       # read -> write, byte identical
python tools/validate_pts.py golden/probe6.pts out/probe6.rcs -s

# LAS/LAZ -> .rcs, without a text intermediate
python tools/las2rcs.py golden/level_7.laz -o out
python tools/las2rcs.py "golden/*.laz" -o out
python tools/las2rcs.py scan.las --units-scale 0.3048 --recenter center

# corpus and projects
python tools/corpus_status.py golden -g golden              # what was captured, count, tiles
python tools/validate_pts.py "golden/*.pts" -g golden       # summary over all pairs
python tools/rcp_info.py "golden/*.rcp"                     # import settings from XML
```

CLI rule: the first argument is the input (a file, directory or mask), the output
name is derived (`<name>.rcs` / `<name>.pts`), and an existing file is not
overwritten without `-f`. Write results to `out/`.

Requirements: Python 3.11 + `numpy`. `laspy` (with the `laszip` or `lazrs`
backend) is needed only for LAS/LAZ: `pip install -e ".[las]"`.

The Rust port builds without MSVC — it needs the GNU host
`x86_64-pc-windows-gnu` (toolchain installation is described below; `rustup` is
not required):


Ready-to-use `.exe` commands (same behaviour as the scripts: input is a file,
directory or mask, output `<name>.rcs`, no overwrite without `-f`):

```powershell
rust\target\release\rcsconv.exe las2rcs golden\level_7.laz -o out
rust\target\release\rcsconv.exe las2rcs "golden\*.laz" -o out -f
rust\target\release\rcsconv.exe las2rcs scan.las --units-scale 0.3048 --recenter center
rust\target\release\rcsconv.exe pts2rcs golden\probe6.pts -o out
rust\target\release\rcsconv.exe --help
```

### GUI: `rcsconv-gui.exe`

The interface is native Win32 (no framework), a separate binary next to the CLI:
`rcsconv.exe` stays a console program (`Subsystem = Windows CUI`),
`rcsconv-gui.exe` runs without a console window (`Windows GUI`) and does not spawn
the CLI as a child process — both link the same crates. The system boundary is the
`windows-sys` crate (the only new dependency), and `unsafe` is isolated in
`rust/rcsconv-gui/src/win32.rs`.

```powershell
rust\target\release\rcsconv-gui.exe        # window
```

State: phases **G1** (window skeleton with layout and `PerMonitorV2` DPI, comctl32
v6 manifest via `windres`), **G2** (file queue: "Add files..." with multi-select,
"Add folder..." walking subdirectories, drag and drop, a list with
File / Folder / Size / Status columns, "Remove" and "Clear"), **G3** (parameters:
output folder, `--units-scale`, recentring `-m`, points per tile, GUID,
colour/intensity/`-f`/`-r` flags; input validation is visible right in the hint,
values are stored in `HKCU\Software\rcsconv\gui`) and **G4** (conversion in a
background thread: progress bar per stage, "Convert"/"Cancel", row states, "Open
folder"; a file from the GUI is **byte-identical** to a file from the CLI, test
`gui_and_cli_produce_identical_bytes`). The interface language is English.
Remaining: G5 (reading `.rcs` — `inspect`/`rcs2pts`) and G6 (polish, `docs/GUI.md`,
acceptance in Revit) — `docs/ROADMAP.md` §4.1.

The linker may warn `.rsrc merge failure: multiple non-default manifests` — that is
normal: the exe has two manifests (ours and the built-in CRT one), but
`FindResource(1, RT_MANIFEST)` returns ours (verified, `out/diag/pe_resources.py`).

### Revit add-in: `revit/RcsConvRevit.cs`

The Revit 2023 add-in builds the `.rcs` itself in C# (a direct implementation of
`docs/RCS-FORMAT.md` section 9.1, no ReCap, no template, no `rcsconv.exe`) and links
it into the model as a point cloud. Source, the API probe and the byte-level test
harness are in `revit/`; the details are in [`docs/REVIT-PLUGIN.md`](docs/REVIT-PLUGIN.md).

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\revit\run-tests.ps1
```

The C# writer is verified byte for byte against the Rust converter on
`probe6`, `test1`, `test2`, `level_7` (324,680 points), `level_7_n19099`,
`attic_south_west` (3,953,680 points, 79.5 MB), `entrance` and `roof_1_stairwell`.
`.pts` is supported, `.las` is next; `.laz` is not supported by the C# build — use `rcsconv.exe las2rcs` for those files. The command asks for the target file (Save as) and saves under a free name when Revit keeps the old `.rcs` open.

### Toolchain: portable installation, no `rustup`

`rustup` is not required: the toolchain is installed **by copying** an unpacked
archive, and the user profile stays clean (no `.rustup`/`.cargo` under
`C:\Users\...`).

| What | Where on the owner's machine |
|---|---|
| Toolchain: `rustc`, `cargo`, `std`, `rustc_driver-*.dll`, `libgcc_s_seh-1.dll`, `libwinpthread-1.dll` | `C:\rust` (`bin\`, `lib\`, `libexec\`, `share\`, `etc\`) |
| Crate cache (`registry\`, `.global-cache`) | `C:\rust\cargo-home` — machine variable `CARGO_HOME` |
| Linker | MinGW/WinLibs (gcc 16.2) — `C:\mingw64\bin`, already in the system `PATH` |

Machine variables (they apply to all users and to new processes):

```powershell
# PATH: ... C:\mingw64\bin\;C:\rust\bin; ...  <- rust\bin right after mingw64\bin
[Environment]::SetEnvironmentVariable('PATH', $newPath, 'Machine')
[Environment]::SetEnvironmentVariable('CARGO_HOME', 'C:\rust\cargo-home', 'Machine')
```

Check after installation: `rustc -vV` -> `host: x86_64-pc-windows-gnu`,
`rustc --print sysroot` -> `C:\rust`, `cargo test --workspace --offline` -> 71 passed
(offline works thanks to the copied cache). `rust-toolchain.toml`
(`channel = "stable"`) is read only by `rustup` — without it the file is inert, but
CI needs it (Ubuntu installs `rustup` itself).

**Updating Rust:** download `rust-<version>-x86_64-pc-windows-gnu.tar.gz`
(about 533 MB) and unpack it **over** `C:\rust`; `cargo-home` is not touched.
The same can be done with a temporary `rustup` installation.

On another machine the classic `rustup` works too: `rustup-init.exe --profile minimal`
from [rustup.rs](https://rustup.rs/), then `rustup set default-host x86_64-pc-windows-gnu`
— otherwise `cargo` picks the MSVC toolchain. `rust-toolchain.toml` deliberately
does not pin the triple so the build also works on Linux (CI).


## How it was verified

The main point of the project is not "the file size matched" but verifiability:

- **85 tests** (`python -m unittest discover -s tools/tests`); without the data some
  classes are skipped instead of failing;
- `read -> write` produces a **byte-identical** file on the reference files;
- files built by `pts2rcs` were **opened in Revit 2023** on 9 real clouds
  (0.3–4.0 million points) and the dimensions were checked;
- on `level_7` the `type 1` (metadata) and `type 0` (properties) blocks match the
  ReCap reference **byte for byte**; from `.laz` the same tree is produced (43
  tiles, depth 3);
- `export ⊂ raw` — the Unified export only thins the imported cloud (verified on
  1.07 million points);
- what is **not** verified is listed honestly: Revit model linking, `.e57`, the
  exact decimation mechanism during export (`docs/RCS-FORMAT.md` §12).

## How it was made

The project is a "human + AI agent" pair: task setting, data capture and acceptance
in Revit belong to the project owner; the code, format analysis and documentation
text are produced by the Cline agent in VS Code.

That is why every format fact is labelled **CONFIRMED / HYPOTHESIS / DISPROVED** and
rests on a test or a script (`docs/RCS-FORMAT.md` §0). The DISPROVED section (§13)
also collects the mistakes of the early 2025 approach so nobody repeats them; a
short post-mortem is in [`docs/LESSONS.md`](docs/LESSONS.md).

## Repository layout

```
tools/                 parser, writer, converters, CLI (Python reference)
  cloud.py              point and the "point source" protocol
  pts_read.py           .pts            -> pts2rcs.py
  las_read.py           .las/.laz       -> las2rcs.py
  rcs_build.py          octree and .rcs assembly (knows nothing about the input format)
  rcs_cli.py            shared arguments and the per-file loop
  rcs_blocks.py         container: header, directory, blocks
  rcs_points.py         point bodies (16 B) and the node payload header
  rcs_write.py          serialisation: build a file from blocks
  analyze_*.py          type 1, header and payload analysis
tools/tests/           85 tests (unittest)
rust/                  Rust port: rcs-format, rcs-cloud, cloud-pts, cloud-las,
                       rcs-conv, rcsconv (CLI), rcsconv-gui (Win32 GUI)
revit/                 Revit 2023 add-in in C#: writer + linking + API probe + tests
docs/                  RCS-FORMAT.md, RCP-FORMAT.md, CORPUS.md, HANDOVER.md,
                       ROADMAP.md, LESSONS.md, GIT.md, GUI.md, RELEASE.md, STREAMING.md
docs-rus/              Russian mirror of the docs, local only (git-ignored)
golden/                data and reference files (NOT in git)
out/                   run results (NOT in git)
```

Documentation: **[PLAN.md](PLAN.md)** (status, phases, journal) ·
**[docs/RCS-FORMAT.md](docs/RCS-FORMAT.md)** (`.rcs` specification) ·
**[docs/RCP-FORMAT.md](docs/RCP-FORMAT.md)** (`.rcp` format) ·
**[docs/CORPUS.md](docs/CORPUS.md)** (corpus and reference files) ·
**[docs/HANDOVER.md](docs/HANDOVER.md)** (entry point) ·
**[docs/GUI.md](docs/GUI.md)** (GUI) ·
**[docs/RELEASE.md](docs/RELEASE.md)** (release checklist) ·
**[docs/REVIT-PLUGIN.md](docs/REVIT-PLUGIN.md)** (planned Revit add-in) ·
**[docs/ROADMAP.md](docs/ROADMAP.md)** · **[docs/GIT.md](docs/GIT.md)**.

## Data

The repository contains **no** Autodesk-created files and no heavy clouds: the
reference files are captured locally into `golden/` (see `golden/README.md` and
`docs/CORPUS.md`). `golden/`, `out/` and `dumps/` are listed in `.gitignore`.

## License

MIT — see [LICENSE](LICENSE). The project is not affiliated with Autodesk; ReCap,
Revit and the corresponding trademarks belong to Autodesk.

`rust/target/` (build artefacts, about 300 MB) is a working folder on C:, not in
git, and is cleaned with `cargo clean`.

```powershell
cd rust
cargo build --release      # -> rcsconv.exe, rcsconv-gui.exe, rcsconv.dll (static CRT)
cargo test                 # crate and GUI-logic tests (71)
```
