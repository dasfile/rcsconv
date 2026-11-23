# Release checklist for the Rust product

The target product is `rcsconv.exe` (CLI) plus `rcsconv-gui.exe` (Win32 GUI): two
static binaries (no Python, no DLLs, no installation). The Revit add-in additionally
loads `rcsconv.dll`, built from the same crates; the CLI and the GUI never load it.

## 1. Version

- The workspace version is the single source of truth: `rust/*/Cargo.toml`
  (all crates share it, currently `1.0.1`).
- `res/app.rc` must not hard-code the version: `build.rs` writes `version.h` from
  `CARGO_PKG_VERSION_MAJOR/MINOR/PATCH`, so `VERSIONINFO` cannot drift.
- `revit/build-addin.ps1` reads the same version from `rust/rcsconv/Cargo.toml`.
- The Python reference keeps its own version (`pyproject.toml`, `1.0.0`) - it is a
  separate artefact.

## 2. Build (Windows GNU, no MSVC)

```powershell
cd rust
cargo clean
cargo test --workspace --offline    # 73 tests
cargo build --release               # rcsconv.exe, rcsconv-gui.exe, rcsconv.dll
```

Expected sizes (1.0.x): `rcsconv.exe` about 0.84 MB, `rcsconv-gui.exe` about
0.87 MB, `rcsconv.dll` about 0.76 MB. `objdump -p` must list only system DLLs
(no `libgcc_s_seh-1.dll`, no `libwinpthread-1.dll`: the CRT is static).

## 3. Verification before publishing

| Check | Command / evidence |
|---|---|
| CLI | `rcsconv.exe --version` -> the release version; `--help` |
| conversion | `rcsconv.exe las2rcs golden\level_7.laz -o out` -> size and count match the reference |
| byte parity | the same with `-g <GUID>` equals the Python/Rust reference bytes |
| GUI | `rcsconv-gui.exe`: add a file, convert, open the folder (`docs/GUI.md` section 5) |
| GUI == CLI | test `gui_and_cli_produce_identical_bytes` |
| add-in | `revit/build-addin.ps1`, then link a cloud in Revit 2023 |

## 4. Packaging

Two archives are published.

```
rcsconv-<version>-x86_64-pc-windows-gnu.zip     (built by CI or manually)
    rcsconv.exe
    rcsconv-gui.exe
    rcsconv.dll
    LICENSE
    README.md
    SHA256SUMS.txt          (exe, gui, dll)
    revit/                  (RcsConvRevit.cs + README for HotLoader)

rcsconv-revit-<version>.zip                     (built locally)
    RcsConv.addin
    rcsconv/RcsConvRevit.dll
    rcsconv/rcsconv.dll
    rcsconv/README.md
    rcsconv/LICENSE
```

**The add-in archive is built locally**: the CI runner has no `RevitAPI.dll`, so
`cargo`/`csc` cannot compile the add-in there. Run `revit/build-addin.ps1` and
upload `out/dist/rcsconv-revit-<version>.zip` to the GitHub release by hand.

## 5. Release notes

The body of a GitHub release comes from `docs/releases/<tag>.md` (the workflow uses
`body_path`). Every release tag must have such a file; `v1.0.0` and `v1.0.1` are
included.

## 6. CI

- `tests.yml`: Python 3.11/3.12 and `cargo test --workspace` on Ubuntu. Actions are
  on Node 24 (`checkout@v5`, `setup-python@v6`).
- `release.yml`: on a `v*` tag, on `windows-latest` - stable Rust with the
  `x86_64-pc-windows-gnu` target, MinGW linker, `cargo test --target ...`, release
  build, zip with the three binaries and `revit/` sources, release body from
  `docs/releases/<tag>.md`. MSVC is not used.

## 7. Publishing

```powershell
git push origin main
git push origin v1.0.1            # tag created locally after the release commit
```

The workflow creates the CLI archive. Build the add-in archive locally, upload it
to the release page, and check that both assets are present.

## 8. After publishing

- Record the run in `PLAN.md` (tag, sizes, hashes, Revit acceptance).
- Keep the release zips locally in `out/dist/` for reproducibility.
