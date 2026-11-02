# Формат `.rcp` (проект ReCap 2020, 6.0.0.110)

Статусы: **ПОДТВЕРЖДЕНО** — проверено на файлах корпуса (`golden/*.rcp`);
**ГИПОТЕЗА** — догадка, указано, чем проверять; **ОПРОВЕРГНУТО** — с указанием, что
именно это disproved.

Читать: `python tools/rcp_info.py golden/*.rcp` (`--json`, `--entries`,
`--dump-xml` в `out/`).

## 1. Контейнер

`.rcp` — обычный **ZIP** (сигнатура `50 4B 03 04`), ровно **3 записи**:

| Запись | Размер | Что это |
|---|---|---|
| `<guid>.jpg` | 17–28 КБ | превью проекта |
| `<guid>.xml` | 10–11 КБ | **модель проекта** (настройки и ссылки) |
| `rs_screenshot__.bmp` | **15 491 022 Б** во всех проектах | скриншот окна ReCap |

ПОДТВЕРЖДЕНО на 13 проектах корпуса (`.pts`-ветка, `.laz`-ветка, `probe6/test1/test2`).
Сжатие — deflate, порядок записей произвольный.

**Точек внутри `.rcp` нет.** Данные лежат рядом: `<имя> Support/<имя>.rcs`
(raw-диалект v2), а в XML — только пути.

## 2. XML: карта документа

```
<Autodesk Version="1.0">
  <ProjectId UUID="…"/>
  <Nodes>
    <VoxelTreeRunTime Id="{…}">
      <Translation x y z/> <Rotation/> <Scale/>
      <Color R G B r g b/>                 цвет скана в списке
      <Visible/> <Registered/> <Selected/>
      <NodeName Value/> <Group Value/>
      <Path Value="…\Support\<имя>.rcs"/>  абсолютный путь к raw-файлу
      <RelativePath Value=".\..."/>        то же относительно .rcp
      <NumPoints Value/>                   сколько точек принял импорт
      <HasRGB/> <HasNormals/> <HasIntensity/> <NormalizeIntensity/>
      <MaxIntensity/> <MinIntensity/> <RangeImageWidth/> <RangeImageHeight/>
      <IsLidarData/>
    </VoxelTreeRunTime>
  </Nodes>
  <Measurements/>
  <PointCloudSegmentList>  RegionManager (DefaultLayer + RegionArray), ClipManager
  <ViewStates/> <ViewStates25D/>
  <LimitBoxes>             габаритный ящик (6 граней)
  <ProjectNavigator>       регионы/сканы, видимость
  <ActiveViewport> <AutoCamCamera/> <AutoCamHomeCamera/>
  <GeoReference/> <GridOrigin/> <LastActiveGeoReference/>
  <CoordLabel X Y Z/> <UnitType Value="1" IsSurveyMode="0"/> <UnitPrecision Value="3"/>
  <ProjectMetaData MajorVersion="1" MinorVersion="6" PatchVersion="0" …>
      <SaveInformation Application="ReCap" ApplicationVersion="6.0.0.110"
                       Timestamp="20261102T145352" UserId="Jean" Count="1"/>
      <ScanImportSettings> … </ScanImportSettings>
  </ProjectMetaData>
  <Project ver="1.0.0" app="Autodesk ReCap" path=… cache=…>
      <ShotInfo name id PCPid HasIntensity path relativePath rawScanPath
                relativeRawScanPath type="UNSTRUCTURED">
          <OrientedBoundingBox><tform><T x y/><S x y z/></tform></OrientedBoundingBox>
          <ScanImportSettings> … </ScanImportSettings>
      </ShotInfo>
      <GroupInfo name id> <LaserRegInfo isReg/> <map id name><bounds …/></map> …
  </Project>
</Autodesk>
```

ПОДТВЕРЖДЕНО: имена и вложенность перечисленных элементов на всех 13 проектах;
`ProjectMetaData` хранит **историю** сохранений (`SaveInformation` на каждое
сохранение, `Count` = 0, 1, …).

## 3. `ScanImportSettings` — фактические настройки импорта

```xml
<ScanImportTextSettings unitScale="1" skipLines="1" separator=" ">   <!-- .pts -->
    <TextImportColumn index="0"/> … <TextImportColumn index="6"/>     <!-- x y z i r g b -->
    <TextImportColumn index="11"/> ×7                                 <!-- «пусто» -->
</ScanImportTextSettings>
<ScanImportFilterSettings><IntensityClip max="100"/></ScanImportFilterSettings>
<ScanImportAdvancedSettings density="5" upDirection="Z"/>
```

| Атрибут | Смысл | ПОДТВЕРЖДЕНО |
|---|---|---|
| `unitScale` | `Text Options → Unit` (1 = метр) | все проекты `.pts` → `1` |
| `skipLines` | пропуск шапки-счётчика: `1` есть, **атрибута нет** = `0` | `probe6/test1/test2` — атрибута нет; остальные `1` |
| `separator` | разделитель текста (`" "` = пробел) | все `.pts` → пробел |
| `TextImportColumn index` | назначение колонок; `11` = «не использовать» | 7 колонок → `0…6`; у 6-колоночных `.pts` слот интенсивности пропущен: `0,1,2,4,5,6` (`probe6`) |
| `IntensityClip max` | `Clip points` в процентах: 100 = «0 % обрезки» | все проекты → `100` |
| `density` | **`Decimation grid` в мм**; нет атрибута = 1 мм (по умолчанию) | пара проектов `level_7_n19099`: `density="5"` → экспорт 19 088 точек, без атрибута → 19 093 |
| `upDirection` | «верх» сцены | все → `Z` |

При импорте из `.laz` блок текстовых настроек пуст: `<ScanImportTextSettings unitScale="1"/>`,
а `separator`/`skipLines`/колонок нет — и в `ShotInfo` появляется
`HasIntensity="0"` (в LAS интенсивность есть, но вся нулевая).

Следствие для `.rcs`: от `HasIntensity` зависит байт `[8]` тела точки
(`docs/RCS-FORMAT.md` §11): `.pts`-ветка → константа 255, `.laz`-ветка → градиент
по высоте. Цвет (BGR, `[9:12]`) в обеих ветках сохраняется дословно.

## 4. Что ещё в XML полезно знать

- `NumPoints` = число точек, принятых импортом (совпадает с `count` заголовка raw
  `.rcs`); сравнение с числом строк `.pts` показывает потери импорта
  (`docs/CORPUS.md`).
- `ShotInfo/@type` = `UNSTRUCTURED` для импортированного облака (у фотограмметрии
  ReCap бывают структурированные сканы — не проверялось).
- `<GeoReference>`/`<GridOrigin>` = `(0,0,0)`, `UnitType=1`, `UnitPrecision=3` —
  локальные метры, CRS не задана.
- `<GroupInfo>/<LaserRegInfo isReg="1" level="FINE_ALIGNED" status="OK"/>` —
  регистрация одного скана «сам с собой».

## 5. Открытые вопросы

| Вопрос | Ценность | Чем проверять |
|---|---|---|
| `VoxelTreeRunTime/Color`, `MaxIntensity`, `MinIntensity` (всегда 0) | понимание UI | проекты с реальной интенсивностью/несколькими сканами |
| `PointCloudSegmentList` (слои, регионы, клипы) | понимание | проект с обрезанным/разделённым облаком |
| `rs_screenshot__.bmp` — константа или рендер? | не нужно | сравнить SHA-256 двух проектов |
| ГИПОТЕЗА: `*.rcp.bk1…bk3` — предыдущие сохранения; `Temporary Cache Files/*.md,*.png` — кэш превью | понимание | сравнить `bk*` между сохранениями |
| Пишет ли ReCap `.rcp` иначе при `Save As`/сжатии | не нужно | два сохранения одного проекта |
