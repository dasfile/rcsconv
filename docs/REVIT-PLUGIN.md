# Revit plugin (C#): implementation and API facts

The add-in builds a ReCap `.rcs` **itself** — from `.pts` (and, next, `.las`) — and
links it into the active Revit document as a point cloud. It does not need ReCap,
a template or `rcsconv.exe`: the writer is a direct implementation of
`docs/RCS-FORMAT.md` section 9.1 in C#.

Source: [`revit/RcsConvRevit.cs`](../revit/RcsConvRevit.cs) (writer core +
`IExternalCommand`), [`revit/RcsConvProbe.cs`](../revit/RcsConvProbe.cs) (API
probe), [`revit/tests/RcsWriterTests.cs`](../revit/tests/RcsWriterTests.cs)
(console harness), [`revit/README.md`](../revit/README.md).

## 1. Environment and constraints (fixed by the owner)

| Item | Value |
|---|---|
| Revit | 2023 (.NET Framework 4.8), x64 |
| Compiler | `csc.exe` from `C:\Windows\Microsoft.NET\Framework64\v4.0.30319` → **C# 5** |
| Hot reload | `D:\dev\IvanTools\CsHotLoader.cs` — compiles a single `.cs` in memory and runs it |
| File layout | one `.cs` per command, UTF-8 **without BOM**, LF (IvanTools convention) |
| Entry point | exactly **one** `public class : IExternalCommand` per file (HotLoader runs the first) |
| External DLLs | **forbidden**: HotLoader references `RevitAPI.dll`, `RevitAPIUI.dll` and assemblies already loaded in the AppDomain |
| Attributes | `[Transaction(TransactionMode.Manual)]`, `[Regeneration(RegenerationOption.Manual)]` |
| UI | `TaskDialog`; `OpenFileDialog` for the input |

C# 5 restrictions: no string interpolation, no pattern matching, no `?.`,
`nameof`, `out var`, tuples, expression-bodied members or `default` literals.

## 2. Point cloud API of Revit 2023 (verified by reflection over RevitAPI.dll)

**CONFIRMED:** `Document.Link(...)` has **no** point cloud overload in 2023 (only
DWF, DGN, 3DM, STL, OBJ, SKP, SAT, DWG). The working path is:

```csharp
PointCloudType type = PointCloudType.Create(document, engineIdentifier, pathToRcs);
PointCloudInstance instance = PointCloudInstance.Create(document, type.Id, Transform.Identity);
```

- `engineIdentifier` is a registered point cloud engine; the list and the
  file-based flag come from `PointCloudEngineRegistry.GetSupportedEngines()` and
  `IsEngineFileBased(...)`. The `.rcs` engine ships with Revit
  (`AdskRcPointCloudEngine.dll`); **ReCap itself is not required**.
- `PointCloudType.Create` and `PointCloudInstance.Create` are element creations and
  run inside a `Transaction` (`TransactionMode.Manual`).
- The path of a type: `PointCloudType.GetPath()` (a `ModelPath`) is converted with
  `ModelPathUtils.ConvertModelPathToUserVisiblePath(...)`.
- The bounding box of the instance: `Element.get_BoundingBox(null)` (feet; the
  report converts to metres).
- `PointCloudType.FoundStatus` reports whether the external file is found.

`RcsConvProbe` prints the engine list and the identifier that accepted the `.rcs`,
then creates and (optionally) deletes a probe instance. Run it once in Revit and
record the identifier here.

## 3. Writer (C#) — verified without Revit

`RcsConv.Core.RcsWriter.Build(points, options, out stats)` implements the
specification: header, block directory at `0x135`, first block at `0x1812D`,
`type 3` (368-byte tiles, DFS order), `type 11` (16-byte bodies + `kind = 8`
payload header), `type 1` (level words, `K = sum(words)`), `type 0` (134 bytes),
GUID `{8_4_4_4_12}`.

Verification is byte-level against the accepted Rust converter
(`revit/run-tests.ps1` → `out/revit-tests`):

| Input | Points | Tiles | Depth | Result |
|---|---|---|---|---|
| `probe6.pts`, `test1.pts`, `test2.pts` | 3 | 3 | 1 | byte identical (100,327 B) |
| `level_7.pts` | 324,680 | 43 | 3 | byte identical (6,612,979 B) |
| `level_7_n19099.pts` | 19,099 | 42 | 3 | byte identical |
| `attic_south_west.pts` | 3,953,680 | 761 | 5 | byte identical (79,524,955 B) |
| `entrance.pts` | 2,580,040 | 282 | 4 | byte identical |
| `roof_1_stairwell.pts` | 3,130,480 | 386 | 4 | byte identical |

**CONFIRMED:** rounding must be implemented exactly like Python
(`round(x, digits)` on the binary value, halves to even). .NET Framework formatting
goes through 15 significant digits and turns `3.6434999999999999` into `"3.6435"`,
which shifts the value by 1 mm — the writer therefore decomposes the double into
mantissa and exponent and rounds with 64-bit integer arithmetic.

## 4. Behaviour of the command

1. Pick `.pts` or `.las` (`.laz` is rejected with an explanation).
2. Read the points (`PtsReader`: 3–7 columns, count header skipped, intensity
   optional, values clamped to 0..255).
3. Build the `.rcs` into
   `%LOCALAPPDATA%\rcsconv\cache\<name>-<size>-<mtime>.rcs`. The name includes the
   input size and timestamp, so a changed input produces a new file and a file
   locked by Revit never blocks a rebuild.
4. Create or reuse a `PointCloudType` for that path (compared via
   `Path.GetFullPath`, case-insensitive) and create a `PointCloudInstance` with
   `Transform.Identity` inside a transaction.
5. Report the element id, the point/tile statistics and the bbox in metres; a
   command link opens the cache folder.

The `.rcs` stays referenced by the model: the cache folder must stay in place, or
the cloud has to be relinked after clearing it.

## 5. Scope

| Feature | State |
|---|---|
| `.pts` -> `.rcs`, byte parity with Rust | done |
| Linking into the model (`PointCloudType` + `PointCloudInstance`) | done (verify in Revit with the probe) |
| `.las` reader (binary LAS 1.2/1.4, formats 0–3) | next |
| `.laz` | **not planned** in C#: a LASzip decoder is out of scope for one `.cs` without libraries; use `.pts`/`.las` or `rcsconv.exe` |
| `.e57` | deferred with the format branch (`docs/CORPUS.md`) |
| Batch conversion of a folder | later |

## 6. Acceptance checklist

1. `revit/RcsConvProbe.cs` through HotLoader: the dialog lists the engines; the
   selected `.rcs` is created, `FoundStatus` is `Found`, the bbox is plausible; the
   probe elements are deleted afterwards.
2. `revit/RcsConvRevit.cs` through HotLoader with `golden/probe6.pts`: a point cloud
   appears at the origin, the report shows 3 points / 3 tiles.
3. `golden/level_7.pts` (324,680 points): the cloud is visible in a 3D view, the
   bbox measured in Revit matches the source within 1 mm.
4. Repeat the run: no duplicate `PointCloudType` is created; a second instance is
   allowed and reported.
5. Compile with `csc` (0 errors, 0 warnings) and add an `<AddIn>` entry
   (class `IvanTools.RcsConvRevit`) plus a row in `build.ps1` for the release.

## 7. Open questions

| Question | How to resolve |
|---|---|
| Exact `engineIdentifier` for `.rcs` in this Revit build | `RcsConvProbe` output; record it in section 2 |
| How Revit treats a regenerated `.rcs` at the same path | the cache name changes per input; a manual "Reload" in Manage Links is the fallback |
| Does `PointCloudInstance.Create` need a specific view or level | checked in the probe; the API takes no view |
| `.las` point formats actually used by the owner's pipeline | test with `.las` produced from `golden/*.laz` |
