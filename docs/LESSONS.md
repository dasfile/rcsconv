# Lessons of 2025: why the first attempt failed

A short post-mortem. The full list of disproved statements with evidence is in
[`RCS-FORMAT.md` section 13](RCS-FORMAT.md); the rules that grew out of it are in
[`.clinerules`](../.clinerules).

## 1. Conclusion

The 2025 failure was caused **not** by the format being proprietary and not by a
lack of tools. The cause was the absence of a verifiable model of the file and a
substituted acceptance criterion: "the size and the header matched" instead of
"Revit opened the file and the points are in place". The format was never closed;
the methodology made it look closed.

## 2. Seven mistakes

1. **Wrong model.** RCS was treated as "a file with points", although it is a
   container with a block directory and an octree of tiles.
2. **Wrong header.** `0x10` was taken for the first point (it is the bbox centre)
   and `0x08` for the point count (it is `3` with 324,680 points).
3. **Wrong byte interpretation.** Some fields were read as big-endian: the report
   showed "version 50331648" and "33554432 points" — those are `0x03000000` and
   `0x02000000`.
4. **Searching for something that is not there.** Coordinates were sought as
   consecutive `f64 XYZ`; in the file they are quantised per tile, not stored as a
   flat array.
5. **Fitting instead of structure.** The generator wrote 48-byte records and padded
   the file with zeros up to 100,327 bytes — the size matched, the structure did not
   exist.
6. **Copying a dump.** The tail (GUID, tables, sentinels) was copied from one file —
   unguaranteed and non-reproducible.
7. **Wrong acceptance criterion.** What was easy to check was checked, not what the
   consumer needs (`Revit`).

## 3. Why it looked like a "closed door"

Each mistake looked harmless on its own, but together they produced a
self-confirming dead end: wrong model -> wrong parser -> "the file does not open" ->
conclusion "the format is encrypted/proprietary" -> fitting attempts that confirmed
the uselessness of the approach. There were no external signs of the substitution,
so the dead end was never diagnosed.

## 4. What worked on the second attempt

- **Reader first, writer second.** `read -> write -> byte-identical` on 13 reference
  files before the first generation from `.pts` ([`PLAN.md`](../PLAN.md), phases 1-7).
- **Differential analysis of the corpus** instead of a single file: two import
  branches (`.pts` / `.laz`), raw and export, `reference/*_orig.rcs`.
- **Separating "fact / hypothesis / disproved"** in the specification; every fact is
  tied to a test or a script — [`RCS-FORMAT.md` section 0](RCS-FORMAT.md).
- **Acceptance by the end consumer.** Files built by the writer were opened in Revit
  2023 on 9 real clouds (0.3–4.0 million points) and the dimensions were checked.
- **Small probes and exact counters** (`probe6` — 3 points, `attr_intensity` — 6
  points with intensity) gave byte-level proof where large clouds cannot.
- **No synthetic clouds in ReCap**: that direction is closed, no time is wasted
  ([`PLAN.md`](../PLAN.md), "Probes: direction closed").

## 5. What is now fixed

- The criterion "the file is correct" is Revit + bbox +/-1e-3 + count + coordinates
  + visual match ([`.clinerules`](../.clinerules) 6), not the file size.
- Fitting bytes to an expected size and hard-coding dump fragments are forbidden (1),
  and the writer must not depend on an `.rcs` template (14).
- Any statement about the format enters the specification only with a verification (5).
- Changing a confirmed fact requires a new test that disproves it (4).
- The Python implementation is the reference and oracle; the target product is the
  standalone `rcsconv.exe` in Rust ([`ROADMAP.md`](ROADMAP.md)).
