# The `.rcp` format (ReCap 2020 project, 6.0.0.110)

Statuses: **CONFIRMED** — checked on the corpus files (`golden/*.rcp`);
**HYPOTHESIS** — a guess with a note on how to check it; **DISPROVED** — with a
note on what disproved it.

Inspect with: `python tools/rcp_info.py golden/*.rcp` (`--json`, `--entries`,
`--dump-xml` to `out/`).

## 1. Container

`.rcp` is an ordinary **ZIP** (signature `50 4B 03 04`) with exactly **3 entries**:

| Entry | Size | What it is |
|---|---|---|
| `<guid>.jpg` | 17–28 KB | project preview |
| `<guid>.xml` | 10–11 KB | **project model** (settings and links) |
| `rs_screenshot__.bmp` | **15,491,022 B** in every project | ReCap window screenshot |

CONFIRMED on 13 corpus projects (the `.pts` branch, the `.laz` branch,
`probe6/test1/test2`). Compression is deflate, entry order is arbitrary.

**There are no points inside `.rcp`.** The data lives next to it:
`<name> Support/<name>.rcs` (raw dialect v2), and the XML only stores paths.

## 2. XML: document map

```
<Autodesk Version="1.0">
  <ProjectId UUID="..."/>
  <Nodes>
    <VoxelTreeRunTime Id="{...}">
      <Translation x y z/> <Rotation/> <Scale/>
      <Color R G B r g b/>                 scan colour in the list
      <Visible/> <Registered/> <Selected/>
      <NodeName Value/> <Group Value/>
      <Path Value="...\Support\<name>.rcs"/>  absolute path to the raw file
      <RelativePath Value=".\..."/>        the same relative to the .rcp
      <NumPoints Value/>                   points accepted by the import
      <HasRGB/> <HasNormals/> <HasIntensity/> <NormalizeIntensity/>
      <MaxIntensity/> <MinIntensity/> <RangeImageWidth/> <RangeImageHeight/>
      <IsLidarData/>
    </VoxelTreeRunTime>
  </Nodes>
  <Measurements/>
  <PointCloudSegmentList>  RegionManager (DefaultLayer + RegionArray), ClipManager
  <ViewStates/> <ViewStates25D/>
  <LimitBoxes>             bounding box (6 faces)
  <ProjectNavigator>       regions/scans, visibility
  <ActiveViewport> <AutoCamCamera/> <AutoCamHomeCamera/>
  <GeoReference/> <GridOrigin/> <LastActiveGeoReference/>
  <CoordLabel X Y Z/> <UnitType Value="1" IsSurveyMode="0"/> <UnitPrecision Value="3"/>
  <ProjectMetaData MajorVersion="1" MinorVersion="6" PatchVersion="0" ...>
      <SaveInformation Application="ReCap" ApplicationVersion="6.0.0.110"
                       Timestamp="20261102T145352" UserId="user" Count="1"/>
      <ScanImportSettings> ... </ScanImportSettings>
  </ProjectMetaData>
  <Project ver="1.0.0" app="Autodesk ReCap" path=... cache=...>
      <ShotInfo name id PCPid HasIntensity path relativePath rawScanPath
                relativeRawScanPath type="UNSTRUCTURED">
          <OrientedBoundingBox><tform><T x y/><S x y z/></tform></OrientedBoundingBox>
          <ScanImportSettings> ... </ScanImportSettings>
      </ShotInfo>
      <GroupInfo name id> <LaserRegInfo isReg/> <map id name><bounds .../></map> ...
  </Project>
</Autodesk>
```

CONFIRMED: the names and nesting of these elements in all 13 projects;
`ProjectMetaData` keeps a **history** of saves (`SaveInformation` per save,
`Count` = 0, 1, ...).

## 3. `ScanImportSettings` — the actual import settings

```xml
<ScanImportTextSettings unitScale="1" skipLines="1" separator=" ">   <!-- .pts -->
    <TextImportColumn index="0"/> ... <TextImportColumn index="6"/>     <!-- x y z i r g b -->
    <TextImportColumn index="11"/> x7                                 <!-- "unused" -->
</ScanImportTextSettings>
<ScanImportFilterSettings><IntensityClip max="100"/></ScanImportFilterSettings>
<ScanImportAdvancedSettings density="5" upDirection="Z"/>
```

| Attribute | Meaning | CONFIRMED |
|---|---|---|
| `unitScale` | `Text Options -> Unit` (1 = metre) | every `.pts` project -> `1` |
| `skipLines` | skip the count header: `1` when present, **no attribute** = `0` | `probe6/test1/test2` have no attribute; the rest have `1` |
| `separator` | text delimiter (`" "` = space) | every `.pts` -> space |
| `TextImportColumn index` | column assignment; `11` = "unused" | 7 columns -> `0..6`; for 6-column `.pts` the intensity slot is skipped: `0,1,2,4,5,6` (`probe6`) |
| `IntensityClip max` | `Clip points` in percent: 100 = "0 % clipping" | every project -> `100` |
| `density` | **`Decimation grid` in mm**; absent = 1 mm (the default) | the `level_7_n19099` project pair: `density="5"` -> export 19,088 points, absent -> 19,093 |
| `upDirection` | scene "up" | every project -> `Z` |

When importing from `.laz`, the text settings block is empty:
`<ScanImportTextSettings unitScale="1"/>`, with no `separator`/`skipLines`/columns —
and `HasIntensity="0"` appears in `ShotInfo` (LAS has intensity, but it is all
zeros).

Consequence for `.rcs`: `HasIntensity` affects byte `[8]` of the point body
(`docs/RCS-FORMAT.md` 11): the `.pts` branch gives the constant 255, the `.laz`
branch gives a height gradient. The colour (BGR, `[9:12]`) is preserved verbatim in
both branches.

## 4. Other useful XML facts

- `NumPoints` = the number of points accepted by the import (matches the `count` in
  the header of the raw `.rcs`); comparing it with the `.pts` line count shows the
  import loss (`docs/CORPUS.md`).
- `ShotInfo/@type` = `UNSTRUCTURED` for an imported cloud (ReCap photogrammetry may
  have structured scans — not checked).
- `<GeoReference>`/`<GridOrigin>` = `(0,0,0)`, `UnitType=1`, `UnitPrecision=3` —
  local metres, no CRS.
- `<GroupInfo>/<LaserRegInfo isReg="1" level="FINE_ALIGNED" status="OK"/>` —
  registration of a single scan "with itself".

## 5. Open questions

| Question | Value | How to check |
|---|---|---|
| `VoxelTreeRunTime/Color`, `MaxIntensity`, `MinIntensity` (always 0) | understanding the UI | projects with real intensity/several scans |
| `PointCloudSegmentList` (layers, regions, clips) | understanding | a project with a cropped/split cloud |
| `rs_screenshot__.bmp` — a constant or a render? | not needed | compare SHA-256 of two projects |
| HYPOTHESIS: `*.rcp.bk1..bk3` are previous saves; `Temporary Cache Files/*.md,*.png` is a preview cache | understanding | compare `bk*` between saves |
| Does ReCap write `.rcp` differently on `Save As`/compression | not needed | two saves of one project |
