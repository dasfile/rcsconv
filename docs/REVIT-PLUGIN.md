# Revit plugin (C#) — scope and constraints (not started)

Goal: bring `.laz`/`.pts` (later `.e57`) into a Revit model without leaving Revit.
The plugin is a **thin client** of `rcsconv.exe`: it converts to a cached `.rcs` and
links that file, instead of re-implementing the format in C#.

Status: **waiting** — no C# code in this repository yet. The owner tests `.cs` files
with `CsHotLoader` before they become a compiled add-in.

## 1. Development environment (fixed by the owner)

| Item | Value |
|---|---|
| Revit | 2023 (.NET Framework 4.8), x64 |
| Compiler | `csc.exe` from `C:\Windows\Microsoft.NET\Framework64\v4.0.30319` → **C# 5** |
| Hot reload | `D:\dev\IvanTools\CsHotLoader.cs` — compiles a single `.cs` in memory and runs it |
| File layout | one `.cs`, UTF-8 **with BOM** |
| Entry point | exactly **one** `public class : IExternalCommand` (HotLoader runs the first one it finds) |
| External DLLs | **forbidden**: HotLoader references only `RevitAPI.dll`, `RevitAPIUI.dll` and assemblies already loaded in the AppDomain |
| Attributes | `[Transaction(TransactionMode.Manual)]`, `[Regeneration(RegenerationOption.Manual)]` |
| UI | `TaskDialog` for messages; `SaveFileDialog`/`OpenFileDialog` where a file is needed |

C# 5 restrictions: no string interpolation (use `string.Format` or concatenation),
no pattern matching (`as` + null check), no `?.`, `nameof`, `out var`, tuples,
expression-bodied members or `default` literals. Instead of tuples use small classes.

Not available on the machine (checked): .NET SDK (`dotnet build`), the .NET Framework
4.8 targeting pack (`MSBuild` for net48), the `netstandard.dll` facade. That is why
NuGet-based libraries (SharpGLTF and similar) are excluded.

## 2. Integration contract with `rcsconv.exe`

```
rcsconv.exe las2rcs <input.laz> -o <cache-dir> -f [-g {GUID}]
rcsconv.exe pts2rcs <input.pts> -o <cache-dir> -f [-g {GUID}]
```

- exit codes: `0` — everything written; `1` — at least one file failed; `2` — bad
  arguments or no inputs;
- stdout lines: `<input> -> <output>: N points, M tiles (depth D, edge E m), ...`;
- stderr lines: `<path>: ERROR: <reason>`;
- the cache directory is chosen by the plugin (for example
  `<project dir>\rcs-cache\`), the `.rcs` name is derived from the input name;
- conversion runs **outside** the Revit API context (the command may be modal, but
  the process must not run inside a transaction).

## 3. Planned flow

1. `IExternalCommand`: pick an input file (or use a configured folder).
2. Hash or timestamp the input; if the cached `.rcs` is fresh, skip the conversion.
3. Start `rcsconv.exe` with `UseShellExecute = false`, `CreateNoWindow = true`;
   show a `TaskDialog` with the exit code and the tail of stderr on failure.
4. Link the `.rcs` into the active document (`Document.Link` is not available for
   point clouds in all versions — the fallback is `Import`/`Link` through
   `RevitLinkType`; to be verified on Revit 2023 first).
5. Report the bounding box and the point count from the header to the user for a
   quick sanity check.

## 4. Open questions

| Question | How to resolve |
|---|---|
| Which API call links a `.rcs` in Revit 2023 | a minimal probe command through HotLoader |
| How Revit treats a `.rcs` GUID on relink | convert with a fixed `-g` and compare |
| Is a modal external process allowed inside a command | test; if not, run the conversion before opening the transaction |
| `.e57` support | deferred with the format branch (`docs/CORPUS.md`) |
