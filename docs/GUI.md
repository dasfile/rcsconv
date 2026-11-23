# GUI: `rcsconv-gui.exe`

A native Win32 interface (no framework) for converting `.las`/`.laz`/`.pts` files
into ReCap `.rcs`. The language of the interface is **English**.

The binary sits next to the CLI and links the same crates (`rcs-conv` and below), so
the conversion result is identical to `rcsconv.exe` — this is checked by the test
`gui_and_cli_produce_identical_bytes` in `rust/rcsconv-gui/src/job.rs`.

## 1. Window

```
+------------------------------------------------------------------+
| rcsconv 1.0.0 - .las/.laz/.pts -> ReCap .rcs                      |
| Drag .las/.laz/.pts into the window or press "Add files...".      |
| +--------------------------------------------------------------+ |
| | File            | Folder          | Size     | Status        | |
| | level_7.laz     | D:\scan         | 42.1 MB  | written       | |
| +--------------------------------------------------------------+ |
| Output folder: [ D:\out                  ] [ Browse... ]          |
| Scale:         [ 1     ]  Recenter: [ keep as is v ]              |
| Points per tile:[ 65536]  GUID: [                        ]        |
| [x] Colour  [x] Intensity  [ ] Overwrite .rcs  [ ] Include subfolders |
| [ Convert ] [ Open folder ]                                       |
| [################## progress ##################]                  |
| [Add files...] [Add folder...] [Remove] [Clear]      [Close]      |
+------------------------------------------------------------------+
```

DPI awareness is `PerMonitorV2`; the layout is built for 96 dpi and scaled
(`model::scale`). The minimum client size is 900 x 480 at 96 dpi
(`MIN_SIZE_96`); `WM_GETMINMAXINFO` enforces it.

## 2. Controls

| Control | Behaviour |
|---|---|
| **Add files...** | `GetOpenFileNameW` with `OFN_ALLOWMULTISELECT`; filter `*.las;*.laz;*.pts` |
| **Add folder...** | `SHBrowseForFolderW`; adds every matching file, walking subdirectories |
| drag and drop | `WM_DROPFILES`; directories are walked, files are filtered by extension |
| **Remove** | removes the selected row |
| **Clear** | empties the queue |
| **Convert** | starts the background conversion; the button turns into **Cancel** |
| **Cancel** | sets the cancellation flag; the worker stops at the next checkpoint and no partial `.rcs` is left (atomic `tmp` + `rename`) |
| **Open folder** | opens the output folder (or the folder of the first input) in Explorer |
| **Close** | saves the settings and closes the window; a running job is cancelled and joined |

The list is a `SysListView32` in report mode with columns File / Folder / Size /
Status. Status values: `queued`, `running...`, `written`, `skipped`, `failed`,
`cancelled`.

## 3. Parameters

| Field | CLI equivalent | Notes |
|---|---|---|
| Output folder | `-o` | empty means "next to the input" |
| Scale | `--units-scale` | must be a number > 0 |
| Recenter | `--recenter` | `keep as is` / `bbox center` / `bbox minimum` |
| Points per tile | `-m` | integer in 1..65536; default 65536 |
| GUID | `-g` | empty means random; validated before reading files |
| Colour | `--no-color` (inverse) | read LAS colour |
| Intensity | `--no-intensity` (inverse) | read LAS intensity |
| Overwrite .rcs | `-f` | an existing `.rcs` is skipped without it |
| Include subfolders | `-r` | walk subdirectories when adding folders |

Validation messages are the same strings the CLI prints, and they appear right in
the hint line. Settings are stored in the registry under
`HKCU\Software\rcsconv\gui` (values: `out_dir`, `units`, `recenter`,
`max_per_tile`, `guid`, `color`, `intensity`, `force`, `recursive`) and restored on
start.

## 4. Conversion

- The queue is converted in a **background thread**; the UI thread only polls it on
  a 120 ms timer (`WM_TIMER`) and takes finished results.
- Progress is reported per stage (`reading`, `octree`, `tiles`, `writing`) with
  `done/total` counters; the hint line shows `stage: done of total (N %)`.
- A panic inside the worker is caught (`catch_unwind`) and the job is marked
  finished — the window survives (`panic = "unwind"` in the release profile).
- Files are written atomically: `<name>.rcs.tmp<pid>` then rename, so a cancelled or
  failed conversion never leaves a truncated `.rcs`.
- The final hint summarises the run: `Done: N written, M skipped, K failed`
  (+ `, C cancelled`).

## 5. Acceptance checklist (G6)

1. `cargo build --release`, start `rust\target\release\rcsconv-gui.exe`; no console
   window appears.
2. Add `golden\level_7.laz` with **Add files...**; the row shows the size.
3. Set the output folder, press **Convert**; the progress bar moves through all four
   stages and the row ends as `written`.
4. Press **Open folder**; Explorer shows `level_7.rcs`.
5. Compare with the CLI: `rcsconv.exe las2rcs golden\level_7.laz -o out -g <same GUID>`
   — the files are byte-identical.
6. Press **Convert** again without **Overwrite .rcs**; the row ends as `skipped`.
7. Start a conversion of a large cloud and press **Cancel**: the row ends as
   `cancelled` and no `.rcs` appears in the output folder.
8. Resize the window, move it to a monitor with another DPI: the layout follows, no
   control is clipped.
9. Close the window and start it again: the parameters are restored.
10. Open the produced `.rcs` in Revit 2023: the cloud and its dimensions are correct.

## 6. Known limitations

- The interface is Windows-only; on other platforms `rcsconv-gui` prints a message
  and exits.
- Reading `.rcs` (an info panel and `.pts` export) is not implemented yet — it
  arrives with R3/G5 (`docs/ROADMAP.md`).
- The icon is the standard one; a custom `.ico` is planned.
- The linker may warn about "multiple non-default manifests" — this is expected (our
  manifest plus the CRT one); `FindResource(1, RT_MANIFEST)` returns ours.
