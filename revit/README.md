# revit/ — the Revit plugin (C#)

A thin Revit 2023 add-in that builds a ReCap `.rcs` **itself** (no ReCap, no
`rcsconv.exe`, no template) and links it into the active document as a point cloud.

| File | Purpose |
|---|---|
| `RcsConvRevit.cs` | the plugin: writer core (`RcsConv.Core`) + `IExternalCommand` (`IvanTools.RcsConvRevit`) |
| `RcsConvProbe.cs` | the API probe: lists point cloud engines, creates and removes a probe instance |
| `tests/RcsWriterTests.cs` | console harness: compares the C# bytes with `rcsconv.exe` (Rust) |
| `run-tests.ps1` | builds and runs the harness (no Revit application needed) |

## Current state

- The command converts through `rcsconv.dll` (`.pts`, `.las`, `.laz`); the DLL is
  built by `cargo build --release` and located once with "Locate rcsconv.dll...".
- The C# writer (`.pts` -> `.rcs`) is **deprecated** but verified byte for byte against the Rust converter on
  `probe6`, `test1`, `test2`, `level_7` (324,680 points), `level_7_n19099`,
  `attic_south_west` (3,953,680 points, 79.5 MB), `entrance`, `roof_1_stairwell`.
- `.las` and `.laz` are handled by the DLL (the Rust crates `las`/`laz`); no C#
  format code is involved.
- Linking uses `PointCloudType.Create` + `PointCloudInstance.Create` (Revit API
  2023); the engine identifier is chosen from
  `PointCloudEngineRegistry.GetSupportedEngines()`. `RcsConvProbe` reports the exact
  identifier — run it once in Revit and keep the value in `docs/REVIT-PLUGIN.md`.

## How to run through CsHotLoader

1. Copy `RcsConvRevit.cs` (or `RcsConvProbe.cs`) anywhere, for example into
   `C:\ProgramData\Autodesk\Revit\Addins\2023\IvanTools\`.
2. Revit: **Add-Ins -> External Tools -> IvanTools.HotLoader**, pick the file.
3. `RcsConvProbe` asks for a `.rcs` and reports engines, type/instance ids, path,
   `FoundStatus` and the bounding box. `RcsConvRevit` asks for `.pts`/`.las`,
   builds the `.rcs` into `%LOCALAPPDATA%\rcsconv\cache`, links it and reports the
   result.

The files are ASCII/UTF-8 without BOM, LF line endings, one `IExternalCommand`
class per file (the HotLoader rule).

## Tests without Revit

```powershell
cd C:\dev\rcsconv
cargo build --release --manifest-path rust\Cargo.toml   # rcsconv.exe is the oracle
powershell -NoProfile -ExecutionPolicy Bypass -File .\revit\run-tests.ps1
```

The harness writes `out\revit-tests\rust\*.rcs` (via `rcsconv.exe`) and
`out\revit-tests\csharp\*.rcs`, then compares them byte for byte and prints the
first differing offset. `-Cases probe6.pts` limits the run; the default set is
`probe6`, `test1`, `test2`, `level_7`, `level_7_n19099`.

## Compiling the add-in

```cmd
csc /target:library /out:RcsConvRevit.dll ^
    /reference:"C:\Program Files\Autodesk\Revit 2023\RevitAPI.dll" ^
    /reference:"C:\Program Files\Autodesk\Revit 2023\RevitAPIUI.dll" ^
    /reference:System.Windows.Forms.dll RcsConvRevit.cs
```

Then add an `<AddIn>` entry to `IvanTools.addin` (class `IvanTools.RcsConvRevit`)
and a row in `build.ps1`. The AddInId must stay stable between releases.

## Notes and limitations

- The cache file name contains the input size and timestamp, so a changed input
  produces a new `.rcs` and a locked file never blocks a rebuild.
- The `.rcs` stays referenced by the model: keep the cache folder in place (or
  relink after clearing it).
- The Revit point cloud engine for `.rcs` ships with Revit; ReCap itself is not
  required.
