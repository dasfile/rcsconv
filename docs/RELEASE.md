# Release checklist for the Rust product

The target product is `rcsconv.exe` (CLI) plus `rcsconv-gui.exe` (Win32 GUI): two
static binaries, no Python, no DLLs, no installation.

## 1. Version

- The workspace version is the single source of truth: `rust/Cargo.toml`
  (`[workspace.package]` or the crate versions, currently `1.0.0`).
- `res/app.rc` must not hard-code the version: `build.rs` passes
  `CARGO_PKG_VERSION_MAJOR/MINOR/PATCH` to `windres` with `-D` defines, so
  `VERSIONINFO` and the crate version cannot drift apart.
- The Python reference keeps its own version (`pyproject.toml`, `1.0.0`) — it is a
  separate artefact.

## 2. Build (Windows GNU, no MSVC)

```powershell
cd rust
cargo clean
cargo test --workspace --offline
cargo build --release
```

Expected: 71 tests pass; `target\release\rcsconv.exe` and `rcsconv-gui.exe` are
produced; `objdump -p` lists only system DLLs (no `libgcc_s_seh-1.dll`, no
`libwinpthread-1.dll` — the CRT is static).

## 3. Verification before publishing

| Check | Command / evidence |
|---|---|
| CLI smoke test | `rcsconv.exe --version`, `--help` |
| conversion | `rcsconv.exe las2rcs golden\level_7.laz -o out` -> size and `count` match the reference |
| byte parity | `rcsconv.exe las2rcs golden\level_7.laz -o out -g <GUID>` equals the Python build with the same GUID |
| GUI smoke test | `rcsconv-gui.exe`: add a file, convert, open the folder (see `docs/GUI.md` section 5) |
| GUI == CLI | test `gui_and_cli_produce_identical_bytes` |
| acceptance | open the produced `.rcs` in Revit 2023; bbox, count, coordinates, visual match |
| binary size | `rcsconv.exe` about 600 KB, `rcsconv-gui.exe` about 660 KB |

## 4. Packaging

The release archive contains:

```
rcsconv-<version>-x86_64-pc-windows-gnu.zip
    rcsconv.exe
    rcsconv-gui.exe
    LICENSE
    README.md
    SHA256SUMS.txt
```

Generate `SHA256SUMS.txt` from the two binaries and the archive itself.

## 5. CI

- `.github/workflows/tests.yml`: Python tests (3.11/3.12) and `cargo test --workspace`
  on Ubuntu.
- `.github/workflows/release.yml` (planned): on a `v*` tag, on `windows-latest` —
  install stable Rust with the `x86_64-pc-windows-gnu` target, `cargo test
  --workspace`, `cargo build --release`, create the zip above and attach it to the
  GitHub Release. MSVC must not be used.

## 6. After publishing

- Update the version in `docs/ROADMAP.md` measurements if the numbers changed.
- Record the run in `PLAN.md` (build id, sizes, hashes, Revit acceptance result).
- Keep the old release zip locally in `out/` for reproducibility.
