// =============================================================================
// RcsConvRevit - build a ReCap .rcs from .pts (and later .las) inside Revit
// =============================================================================
// The writer follows the published specification (docs/RCS-FORMAT.md, section
// 9.1) and does not need ReCap, a template or any external converter. The file
// layout is the Unified RCS dialect v5 produced by ReCap 2020 (6.0.0.110).
//
// Environment : Autodesk Revit 2023 (.NET Framework 4.8), C# 5
// Constraints : one .cs file, no NuGet, no external DLLs (CsHotLoader compiles
//               the file in memory and references only Revit assemblies and the
//               assemblies already loaded in the Revit process).
//
// Structure:
//   namespace RcsConv.Core  - DEPRECATED pure .NET writer (no Revit types). The
//                             command now calls rcsconv.dll; the writer stays as the
//                             byte-for-byte reference for revit/tests/RcsWriterTests.cs
//   namespace IvanTools     - the IExternalCommand: pick a file, build the .rcs,
//                             create PointCloudType + PointCloudInstance
//
// Running:
//   1) via IvanTools.HotLoader - pick this file, it is compiled and executed;
//   2) or compile RcsConvRevit.dll and add an <AddIn> entry to IvanTools.addin.
//
// License: MIT
// =============================================================================

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
using Autodesk.Revit.Attributes;
using Autodesk.Revit.DB;
using Autodesk.Revit.UI;
using RcsConv.Core;

namespace RcsConv.Core
{
    /// <summary>A cloud point: metres, intensity and colour 0..255.</summary>
    public struct CloudPoint
    {
        public double X;
        public double Y;
        public double Z;
        public byte Intensity;
        public byte R;
        public byte G;
        public byte B;

        public CloudPoint(double x, double y, double z, byte intensity, byte r, byte g, byte b)
        {
            X = x;
            Y = y;
            Z = z;
            Intensity = intensity;
            R = r;
            G = g;
            B = b;
        }
    }

    /// <summary>How to build the .rcs.</summary>
    public sealed class BuildOptions
    {
        /// <summary>Points per tile limit (1..65536), the u16 index limit by default.</summary>
        public uint MaxPerTile = 65536;

        /// <summary>Fixed GUID like "{8_4_4_4_12}"; null means a random uuid4.</summary>
        public string Guid;

        /// <summary>Metadata mode: "generated" (default) or "minimal".</summary>
        public string Metadata = "generated";
    }

    /// <summary>What was produced (for the report).</summary>
    public sealed class BuildStats
    {
        public long Points;
        public int Tiles;
        public int Depth;
        public double TileEdge;
        public int MaxPointsPerTile;
        public int Size;
        public int MetadataWords;
        public int MetadataRecords;
    }

    /// <summary>Python-compatible rounding (banker's rounding, decimal digits).</summary>
    public static class PyMath
    {
        /// <summary>Round to the nearest integer, halves to even (Python round(x)).</summary>
        public static double RoundHalfEven(double value)
        {
            if (double.IsNaN(value) || double.IsInfinity(value))
                return value;
            double floor = Math.Floor(value);
            double diff = value - floor;
            if (diff > 0.5)
                return floor + 1.0;
            if (diff < 0.5)
                return floor;
            return (floor % 2.0 == 0.0) ? floor : floor + 1.0;
        }

        /// <summary>
        /// Round to a number of decimal places the way Python round(x, digits) does:
        /// the exact binary value of the double is rounded to the nearest decimal with
        /// halves to even. .NET Framework formatting is not usable here — it goes
        /// through 15 significant digits, so 3.6434999999999999 (a double just below
        /// x.xxx5) is printed as "3.6435" and rounded up, shifting the result by 1 mm.
        /// The decomposition below is exact: value = mantissa * 2^exponent.
        /// </summary>
        public static double RoundDigits(double value, int digits)
        {
            if (double.IsNaN(value) || double.IsInfinity(value))
                return value;
            if (digits < 0 || digits > 9)
                return value;
            if (value == 0.0)
                return value;

            long bits = BitConverter.DoubleToInt64Bits(value);
            bool negative = bits < 0;
            bits &= 0x7FFFFFFFFFFFFFFF;
            int exponentBits = (int)((bits >> 52) & 0x7FF);
            ulong mantissa = (ulong)(bits & 0xFFFFFFFFFFFFFL);
            int exponent;
            if (exponentBits == 0)
            {
                exponent = -1074;
            }
            else
            {
                mantissa |= 1UL << 52;
                exponent = exponentBits - 1075;
            }

            ulong scale = 1;
            for (int index = 0; index < digits; index++)
                scale *= 10;

            double result;
            if (exponent >= 0)
            {
                result = value; // already an integer, nothing to round
            }
            else
            {
                int shift = -exponent;
                if (mantissa > ulong.MaxValue / scale)
                    return value;
                ulong product = mantissa * scale;
                ulong rounded;
                if (shift > 63)
                {
                    rounded = product > (1UL << 63) ? 1UL : 0UL;
                }
                else
                {
                    rounded = product >> shift;
                    ulong mask = (1UL << shift) - 1;
                    ulong remainder = product & mask;
                    ulong half = 1UL << (shift - 1);
                    if (remainder > half || (remainder == half && (rounded & 1UL) == 1UL))
                        rounded++;
                }
                result = rounded / (double)scale;
            }
            return negative ? -result : result;
        }
    }
}

namespace RcsConv.Core
{
    /// <summary>Reading .pts (Leica Cyclone text): x y z [intensity] r g b.</summary>
    public static class PtsReader
    {
        /// <summary>Read every point of the file, keeping file order.</summary>
        public static CloudPoint[] Read(string path)
        {
            List<CloudPoint> points = new List<CloudPoint>(1 << 16);
            using (StreamReader reader = new StreamReader(path, Encoding.UTF8, true, 1 << 16))
            {
                string line;
                while ((line = reader.ReadLine()) != null)
                {
                    CloudPoint point;
                    if (TryParseLine(line, out point))
                        points.Add(point);
                }
            }
            return points.ToArray();
        }

        /// <summary>Parse one .pts line; false when it is not a point (header, garbage).</summary>
        public static bool TryParseLine(string line, out CloudPoint point)
        {
            point = new CloudPoint();
            if (line == null)
                return false;
            string[] parts = line.Split(new char[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 3)
                return false;

            double[] xyz = new double[3];
            for (int axis = 0; axis < 3; axis++)
            {
                if (!double.TryParse(parts[axis], NumberStyles.Float, CultureInfo.InvariantCulture, out xyz[axis]))
                    return false;
            }

            long[] numbers = new long[Math.Max(0, parts.Length - 3)];
            for (int index = 0; index < numbers.Length; index++)
            {
                double value;
                if (double.TryParse(parts[index + 3], NumberStyles.Float, CultureInfo.InvariantCulture, out value))
                    numbers[index] = (long)PyMath.RoundHalfEven(value);
                else
                    numbers[index] = 0;
            }

            long intensity;
            long r;
            long g;
            long b;
            switch (numbers.Length)
            {
                case 0:
                    intensity = 0; r = 0; g = 0; b = 0;
                    break;
                case 1:
                case 2:
                    intensity = numbers[0]; r = 0; g = 0; b = 0;
                    break;
                case 3:
                    intensity = 0; r = numbers[0]; g = numbers[1]; b = numbers[2];
                    break;
                default:
                    intensity = numbers[0]; r = numbers[1]; g = numbers[2]; b = numbers[3];
                    break;
            }

            point = new CloudPoint(
                xyz[0], xyz[1], xyz[2],
                Clamp8(intensity), Clamp8(r), Clamp8(g), Clamp8(b));
            return true;
        }

        private static byte Clamp8(long value)
        {
            if (value < 0) return 0;
            if (value > 255) return 255;
            return (byte)value;
        }
    }
}
namespace RcsConv.Core
{
    /// <summary>Byte layout constants and little-endian writers.</summary>
    public static class RcsFormat
    {
        public const int HeaderEnd = 0x135;
        public const int GuidOffset = 0xE3;
        public const int GuidLength = 38;
        public const int BlockCountOffset = 0x129;
        public const int FirstBlockOffset = 0x1812D;
        public const uint ChunkId = 131;
        public const uint ChunkVersion = 5;
        public static readonly byte[] Magic = { 0x41, 0x44, 0x4F, 0x43, 0x54, 0x00, 0x00, 0x00 };

        public static void AddU16(List<byte> buffer, ushort value)
        {
            buffer.Add((byte)value);
            buffer.Add((byte)(value >> 8));
        }

        public static void AddU32(List<byte> buffer, uint value)
        {
            buffer.Add((byte)value);
            buffer.Add((byte)(value >> 8));
            buffer.Add((byte)(value >> 16));
            buffer.Add((byte)(value >> 24));
        }

        public static void AddU64(List<byte> buffer, ulong value)
        {
            for (int shift = 0; shift < 64; shift += 8)
                buffer.Add((byte)(value >> shift));
        }

        public static void AddF32(List<byte> buffer, float value)
        {
            byte[] bytes = BitConverter.GetBytes(value);
            if (!BitConverter.IsLittleEndian)
                Array.Reverse(bytes);
            buffer.AddRange(bytes);
        }

        public static void AddF64(List<byte> buffer, double value)
        {
            AddU64(buffer, (ulong)BitConverter.DoubleToInt64Bits(value));
        }

        internal static void PutU32(byte[] target, int offset, uint value)
        {
            target[offset] = (byte)value;
            target[offset + 1] = (byte)(value >> 8);
            target[offset + 2] = (byte)(value >> 16);
            target[offset + 3] = (byte)(value >> 24);
        }

        internal static void PutU64(byte[] target, int offset, ulong value)
        {
            for (int shift = 0; shift < 64; shift += 8)
                target[offset + shift / 8] = (byte)(value >> shift);
        }

        internal static void PutF64(byte[] target, int offset, double value)
        {
            PutU64(target, offset, (ulong)BitConverter.DoubleToInt64Bits(value));
        }

        internal static void PutF64x3(byte[] target, int offset, double[] value)
        {
            PutF64(target, offset, value[0]);
            PutF64(target, offset + 8, value[1]);
            PutF64(target, offset + 16, value[2]);
        }
    }

    /// <summary>GUID in the ReCap form: {8_4_4_4_12}, lower case.</summary>
    public static class RecapGuid
    {
        /// <summary>A random uuid4 GUID.</summary>
        public static string New()
        {
            string hex = Guid.NewGuid().ToString("N");
            return Format(hex);
        }

        /// <summary>Validate and normalise a user GUID (hyphens are accepted).</summary>
        public static string Normalize(string text)
        {
            if (text == null)
                throw new ArgumentNullException("text");
            string value = text.Trim();
            if (value.Length < 2 || value[0] != '{' || value[value.Length - 1] != '}')
                throw new FormatException("GUID must be in the {8_4_4_4_12} format: " + text);
            string body = value.Substring(1, value.Length - 2);
            string[] groups = body.Split(new char[] { '-', '_' });
            if (groups.Length != 5 || groups[0].Length != 8 || groups[1].Length != 4 ||
                groups[2].Length != 4 || groups[3].Length != 4 || groups[4].Length != 12)
                throw new FormatException("GUID must be in the {8_4_4_4_12} format: " + text);

            StringBuilder hex = new StringBuilder(32);
            foreach (string group in groups)
            {
                foreach (char symbol in group)
                {
                    if (!Uri.IsHexDigit(symbol))
                        throw new FormatException("GUID contains a non-hex character: " + text);
                    hex.Append(char.ToLowerInvariant(symbol));
                }
            }
            return Format(hex.ToString());
        }

        private static string Format(string hex)
        {
            return "{" + hex.Substring(0, 8) + "_" + hex.Substring(8, 4) + "_" +
                   hex.Substring(12, 4) + "_" + hex.Substring(16, 4) + "_" +
                   hex.Substring(20, 12) + "}";
        }
    }
}
namespace RcsConv.Core
{
    /// <summary>Fixed blocks: header, properties (type 0), metadata (type 1).</summary>
    public static class RcsBlocks
    {
        /// <summary>Compose the 0x135-byte header.</summary>
        public static byte[] BuildHeader(double[] center, double[] extent, double half,
                                         ulong count, string guid, int blockCount)
        {
            byte[] header = new byte[RcsFormat.HeaderEnd];
            Array.Copy(RcsFormat.Magic, header, 8);
            RcsFormat.PutU32(header, 0x04, 84);
            RcsFormat.PutU32(header, 0x08, 3);
            RcsFormat.PutU32(header, 0x0C, 2);
            RcsFormat.PutF64x3(header, 0x10, center);
            RcsFormat.PutF64x3(header, 0x28, new double[] { 0, 0, 0 });
            RcsFormat.PutF64x3(header, 0x40, new double[] { 1, 1, 1 });
            RcsFormat.PutF64x3(header, 0x58, center);
            RcsFormat.PutF64x3(header, 0x70, new double[] { -half, -half, -half });
            RcsFormat.PutF64x3(header, 0x88, new double[] { half, half, half });
            RcsFormat.PutF64x3(header, 0xA0, new double[] { -extent[0], -extent[1], -extent[2] });
            RcsFormat.PutF64x3(header, 0xB8, extent);
            RcsFormat.PutU32(header, 0xD0, RcsFormat.ChunkId);
            RcsFormat.PutU32(header, 0xD4, RcsFormat.ChunkVersion);
            RcsFormat.PutU64(header, 0xD8, count);
            header[0xE0] = 1;
            header[0xE1] = 1;
            header[0xE2] = 0;
            byte[] guidBytes = Encoding.ASCII.GetBytes(guid);
            if (guidBytes.Length != RcsFormat.GuidLength)
                throw new ArgumentException("GUID length must be 38 bytes");
            Array.Copy(guidBytes, 0, header, RcsFormat.GuidOffset, RcsFormat.GuidLength);
            RcsFormat.PutU32(header, RcsFormat.BlockCountOffset, (uint)blockCount);
            RcsFormat.PutU32(header, RcsFormat.BlockCountOffset + 4, 2);
            return header;
        }

        /// <summary>Properties block (type 0), 134 bytes, reference stubs.</summary>
        public static byte[] BuildProperties()
        {
            List<byte> block = new List<byte>(134);
            RcsFormat.AddU32(block, 10);
            AddTlv(block, 100, new byte[] { 1 });
            AddF32Tlv(block, 101, -999999.0f);
            AddF32Tlv(block, 102, 999999.0f);
            AddU32Tlv(block, 201, 0);
            AddU32Tlv(block, 202, 0);
            AddF64Tlv(block, 203, Math.PI);
            AddF64Tlv(block, 204, -Math.PI);
            AddF64Tlv(block, 205, Math.PI / 2.0);
            AddF64Tlv(block, 206, -Math.PI / 2.0);
            AddTlv(block, 301, new byte[] { 0 });
            return block.ToArray();
        }

        /// <summary>Metadata block (type 1) from the level words.</summary>
        public static byte[] BuildMetadata(int[] words)
        {
            if (words.Length > 32)
                throw new ArgumentException("The metadata header holds at most 32 words");
            int records = 0;
            foreach (int word in words)
                records += word;

            List<byte> block = new List<byte>(128 + 72 * records);
            for (int index = 0; index < 32; index++)
                RcsFormat.AddU32(block, index < words.Length ? (uint)words[index] : 0u);
            for (int index = 0; index < records; index++)
            {
                RcsFormat.AddU32(block, 0);
                RcsFormat.AddU32(block, 0);
                RcsFormat.AddF64(block, double.MaxValue);
                RcsFormat.AddF64(block, double.MaxValue);
                RcsFormat.AddF64(block, double.MaxValue);
                RcsFormat.AddF64(block, -double.MaxValue);
                RcsFormat.AddF64(block, -double.MaxValue);
                RcsFormat.AddF64(block, -double.MaxValue);
                RcsFormat.AddU32(block, 0);
                RcsFormat.AddU32(block, uint.MaxValue);
                RcsFormat.AddU32(block, uint.MaxValue);
                RcsFormat.AddU32(block, 0);
            }
            return block.ToArray();
        }

        private static void AddTlv(List<byte> block, uint id, byte[] payload)
        {
            RcsFormat.AddU32(block, id);
            RcsFormat.AddU32(block, (uint)payload.Length);
            block.AddRange(payload);
        }

        private static void AddU32Tlv(List<byte> block, uint id, uint value)
        {
            List<byte> payload = new List<byte>(4);
            RcsFormat.AddU32(payload, value);
            AddTlv(block, id, payload.ToArray());
        }

        private static void AddF32Tlv(List<byte> block, uint id, float value)
        {
            List<byte> payload = new List<byte>(4);
            RcsFormat.AddF32(payload, value);
            AddTlv(block, id, payload.ToArray());
        }

        private static void AddF64Tlv(List<byte> block, uint id, double value)
        {
            List<byte> payload = new List<byte>(8);
            RcsFormat.AddF64(payload, value);
            AddTlv(block, id, payload.ToArray());
        }
    }
}
namespace RcsConv.Core
{
    /// <summary>Octree cell: per-axis indices at a given depth.</summary>
    public struct Cell : IEquatable<Cell>
    {
        public long X;
        public long Y;
        public long Z;

        public Cell(long x, long y, long z)
        {
            X = x;
            Y = y;
            Z = z;
        }

        public bool Equals(Cell other)
        {
            return X == other.X && Y == other.Y && Z == other.Z;
        }

        public override bool Equals(object obj)
        {
            return obj is Cell && Equals((Cell)obj);
        }

        public override int GetHashCode()
        {
            unchecked
            {
                int hash = 17;
                hash = hash * 31 + X.GetHashCode();
                hash = hash * 31 + Y.GetHashCode();
                hash = hash * 31 + Z.GetHashCode();
                return hash;
            }
        }
    }

    /// <summary>Fixed-depth octree: the same rules as tools/rcs_build.py.</summary>
    public static class Octree
    {
        public const double TileEdge = 2.25;
        public const double BaseHalf = 2.25;
        public const int MaxDepth = 20;

        /// <summary>Smallest 2.25*2^k that covers the bbox half-extent.</summary>
        public static double RootHalf(double[] halfExtent)
        {
            double need = 0.0;
            foreach (double value in halfExtent)
                need = Math.Max(need, Math.Abs(value));
            double half = BaseHalf;
            while (half < need - 1e-9)
                half *= 2.0;
            return half;
        }

        /// <summary>Depth at which the tile edge is 2.25 m (the ReCap rule).</summary>
        public static int RecapDepth(double half)
        {
            double value = Math.Log(2.0 * half / TileEdge, 2.0);
            double rounded = PyMath.RoundHalfEven(value);
            if (rounded <= 0.0)
                return 0;
            return (int)rounded;
        }

        /// <summary>Cell index at the given depth.</summary>
        public static Cell CellOf(double[] local, double half, int depth)
        {
            long side = 1L << depth;
            double edge = (2.0 * half) / side;
            long[] cell = new long[3];
            for (int axis = 0; axis < 3; axis++)
            {
                double raw = Math.Floor((local[axis] + half) / edge);
                double clamped = Math.Max(0.0, Math.Min(side - 1, raw));
                cell[axis] = (long)clamped;
            }
            return new Cell(cell[0], cell[1], cell[2]);
        }

        /// <summary>DFS order key: octant indices from root to leaf (x + 2y + 4z).</summary>
        public static byte[] SubtreeKey(Cell cell, int depth)
        {
            byte[] key = new byte[depth];
            int position = 0;
            for (int level = depth; level >= 1; level--)
            {
                long mask = 1L << (level - 1);
                int x = (cell.X & mask) != 0 ? 1 : 0;
                int y = (cell.Y & mask) != 0 ? 1 : 0;
                int z = (cell.Z & mask) != 0 ? 1 : 0;
                key[position++] = (byte)(x + 2 * y + 4 * z);
            }
            return key;
        }

        /// <summary>Increase the depth until no cell holds more than maxPerTile points.</summary>
        public static int ChooseDepth(CloudPoint[] points, double[] center, double half, uint maxPerTile)
        {
            int depth = RecapDepth(half);
            while (depth <= MaxDepth)
            {
                Dictionary<Cell, int> seen = new Dictionary<Cell, int>(points.Length);
                bool collision = false;
                foreach (CloudPoint point in points)
                {
                    double[] local = { point.X - center[0], point.Y - center[1], point.Z - center[2] };
                    Cell cell = CellOf(local, half, depth);
                    int counter;
                    seen.TryGetValue(cell, out counter);
                    counter++;
                    seen[cell] = counter;
                    if (counter > maxPerTile)
                    {
                        collision = true;
                        break;
                    }
                }
                if (!collision)
                    return depth;
                depth++;
            }
            throw new InvalidOperationException(
                "Cannot split the points into cells without duplicates (depth limit " + MaxDepth + ")");
        }

        /// <summary>Number of occupied tiles on every level 0..depth.</summary>
        public static int[] LevelCounts(List<Cell> cells, int depth)
        {
            int[] counts = new int[depth + 1];
            for (int level = 0; level <= depth; level++)
            {
                int shift = depth - level;
                HashSet<Cell> parents = new HashSet<Cell>();
                foreach (Cell cell in cells)
                    parents.Add(new Cell(cell.X >> shift, cell.Y >> shift, cell.Z >> shift));
                counts[level] = parents.Count;
            }
            return counts;
        }
    }
}
namespace RcsConv.Core
{
    /// <summary>Builds a Unified RCS (.rcs) file from a point set — see docs/RCS-FORMAT.md 9.1.</summary>
    public static class RcsWriter
    {
        /// <summary>Build the file bytes; stats describe what was produced.</summary>
        public static byte[] Build(CloudPoint[] points, BuildOptions options, out BuildStats stats)
        {
            if (points == null || points.Length == 0)
                throw new InvalidOperationException("The source contains no points");
            if (options == null)
                options = new BuildOptions();
            if (options.MaxPerTile < 1 || options.MaxPerTile > 65536)
                throw new ArgumentOutOfRangeException("MaxPerTile",
                    "max points per tile must be in 1..65536");

            double[] low = { double.MaxValue, double.MaxValue, double.MaxValue };
            double[] high = { double.MinValue, double.MinValue, double.MinValue };
            foreach (CloudPoint point in points)
            {
                low[0] = Math.Min(low[0], point.X);
                low[1] = Math.Min(low[1], point.Y);
                low[2] = Math.Min(low[2], point.Z);
                high[0] = Math.Max(high[0], point.X);
                high[1] = Math.Max(high[1], point.Y);
                high[2] = Math.Max(high[2], point.Z);
            }

            double[] center =
            {
                (low[0] + high[0]) / 2.0,
                (low[1] + high[1]) / 2.0,
                (low[2] + high[2]) / 2.0
            };
            double[] extent = new double[3];
            for (int axis = 0; axis < 3; axis++)
                extent[axis] = PyMath.RoundDigits((high[axis] - low[axis]) / 2.0, 3);
            double half = Octree.RootHalf(extent);
            int depth = Octree.ChooseDepth(points, center, half, options.MaxPerTile);
            double edge = (2.0 * half) / (double)(1L << depth);

            // Grouping: cell -> point indices in file order.
            Dictionary<Cell, List<int>> cells = new Dictionary<Cell, List<int>>(points.Length);
            for (int index = 0; index < points.Length; index++)
            {
                CloudPoint point = points[index];
                double[] local = { point.X - center[0], point.Y - center[1], point.Z - center[2] };
                Cell cell = Octree.CellOf(local, half, depth);
                List<int> list;
                if (!cells.TryGetValue(cell, out list))
                {
                    list = new List<int>();
                    cells.Add(cell, list);
                }
                list.Add(index);
            }

            // DFS tile order.
            List<TileEntry> order = new List<TileEntry>(cells.Count);
            foreach (KeyValuePair<Cell, List<int>> pair in cells)
                order.Add(new TileEntry(Octree.SubtreeKey(pair.Key, depth), pair.Key));
            order.Sort(TileEntry.Compare);

            List<byte> records = new List<byte>(368 * order.Count);
            List<byte> payloads = new List<byte>(20 * points.Length);
            List<Cell> cellList = new List<Cell>(order.Count);
            int maxPointsPerTile = 0;

            foreach (TileEntry entry in order)
            {
                List<int> indices = cells[entry.Cell];
                cellList.Add(entry.Cell);
                maxPointsPerTile = Math.Max(maxPointsPerTile, indices.Count);
                double[] cubeMin =
                {
                    -half + entry.Cell.X * edge,
                    -half + entry.Cell.Y * edge,
                    -half + entry.Cell.Z * edge
                };
                double[] cubeMax = { cubeMin[0] + edge, cubeMin[1] + edge, cubeMin[2] + edge };
                WriteTile(records, payloads, points, indices, center, cubeMin, cubeMax);
            }

            List<byte> block3 = new List<byte>(8 + records.Count);
            RcsFormat.AddU64(block3, (ulong)order.Count);
            block3.AddRange(records);

            int[] words = Octree.LevelCounts(cellList, depth);
            byte[] metadata = RcsBlocks.BuildMetadata(words);
            byte[] properties = RcsBlocks.BuildProperties();

            string guid = string.IsNullOrEmpty(options.Guid)
                ? RecapGuid.New()
                : RecapGuid.Normalize(options.Guid);

            List<BlockEntry> blocks = new List<BlockEntry>(4);
            blocks.Add(new BlockEntry(3, block3.ToArray()));
            blocks.Add(new BlockEntry(1, metadata));
            blocks.Add(new BlockEntry(11, payloads.ToArray()));
            blocks.Add(new BlockEntry(0, properties));

            byte[] header = RcsBlocks.BuildHeader(center, extent, half, (ulong)points.Length, guid, blocks.Count);
            byte[] file = Assemble(header, blocks);

            int metadataRecords = 0;
            foreach (int word in words)
                metadataRecords += word;

            stats = new BuildStats();
            stats.Points = points.Length;
            stats.Tiles = order.Count;
            stats.Depth = depth;
            stats.TileEdge = edge;
            stats.MaxPointsPerTile = maxPointsPerTile;
            stats.Size = file.Length;
            stats.MetadataWords = words.Length;
            stats.MetadataRecords = metadataRecords;
            return file;
        }

        private static void WriteTile(List<byte> records, List<byte> payloads, CloudPoint[] points,
                                      List<int> indices, double[] center, double[] cubeMin, double[] cubeMax)
        {
            double[] low = { double.MaxValue, double.MaxValue, double.MaxValue };
            double[] high = { double.MinValue, double.MinValue, double.MinValue };
            foreach (int index in indices)
            {
                CloudPoint point = points[index];
                for (int axis = 0; axis < 3; axis++)
                {
                    double local = Axis(point, axis) - center[axis];
                    low[axis] = Math.Min(low[axis], local);
                    high[axis] = Math.Max(high[axis], local);
                }
            }

            double[] bboxMin = new double[3];
            double[] bboxMax = new double[3];
            for (int axis = 0; axis < 3; axis++)
            {
                bboxMin[axis] = PyMath.RoundDigits(low[axis], 3);
                bboxMax[axis] = PyMath.RoundDigits(high[axis], 3);
            }

            int count = indices.Count;
            int payloadLength = 4 * count + 16 * count;

            for (int number = 0; number < count; number++)
            {
                RcsFormat.AddU16(payloads, (ushort)number);
                payloads.Add(0);
                payloads.Add(8);
            }

            foreach (int index in indices)
            {
                CloudPoint point = points[index];
                for (int axis = 0; axis < 3; axis++)
                {
                    double delta = PyMath.RoundDigits(Axis(point, axis) - center[axis] - cubeMin[axis], 3);
                    double scale = axis == 0 ? 1000.0 : (axis == 1 ? 4000.0 : 16000.0);
                    double quantized = PyMath.RoundHalfEven(delta * scale);
                    if (!(quantized >= 0.0 && quantized <= 65535.0))
                        throw new InvalidOperationException(string.Format(
                            CultureInfo.InvariantCulture,
                            "Point {0} on axis {1} ({2}) does not fit into the tile cube",
                            index, axis, Axis(point, axis)));
                    RcsFormat.AddU16(payloads, (ushort)quantized);
                }
                payloads.Add(0);
                payloads.Add(0);
                payloads.Add(point.Intensity);
                payloads.Add(point.B);
                payloads.Add(point.G);
                payloads.Add(point.R);
                payloads.Add(0);
                payloads.Add(0);
                payloads.Add(0);
                payloads.Add(0);
            }

            AddTriple(records, cubeMin);
            AddTriple(records, cubeMax);
            AddTriple(records, bboxMin);
            AddTriple(records, bboxMax);
            RcsFormat.AddU32(records, 1);
            RcsFormat.AddU32(records, (uint)count);
            RcsFormat.AddU32(records, (uint)payloadLength);
            for (int word = 0; word < 5; word++)
                RcsFormat.AddU32(records, 0);
            for (int word = 0; word < 60; word++)
                RcsFormat.AddU32(records, word == 27 ? (uint)count : 0u);
        }

        private static byte[] Assemble(byte[] header, List<BlockEntry> blocks)
        {
            int directoryEnd = RcsFormat.HeaderEnd + 24 * blocks.Count;
            if (directoryEnd > RcsFormat.FirstBlockOffset)
                throw new InvalidOperationException("The block directory does not fit before the first block");

            List<byte> file = new List<byte>(RcsFormat.FirstBlockOffset + 32);
            file.AddRange(header);
            ulong offset = RcsFormat.FirstBlockOffset;
            foreach (BlockEntry block in blocks)
            {
                RcsFormat.AddU64(file, offset);
                RcsFormat.AddU64(file, (ulong)block.Data.Length);
                RcsFormat.AddU64(file, block.Type);
                offset += (ulong)block.Data.Length;
            }
            while (file.Count < RcsFormat.FirstBlockOffset)
                file.Add(0);
            foreach (BlockEntry block in blocks)
                file.AddRange(block.Data);
            return file.ToArray();
        }

        private static double Axis(CloudPoint point, int axis)
        {
            return axis == 0 ? point.X : (axis == 1 ? point.Y : point.Z);
        }

        private static void AddTriple(List<byte> buffer, double[] value)
        {
            RcsFormat.AddF64(buffer, value[0]);
            RcsFormat.AddF64(buffer, value[1]);
            RcsFormat.AddF64(buffer, value[2]);
        }

        private sealed class BlockEntry
        {
            public readonly ulong Type;
            public readonly byte[] Data;

            public BlockEntry(ulong type, byte[] data)
            {
                Type = type;
                Data = data;
            }
        }

        private sealed class TileEntry
        {
            public readonly byte[] Key;
            public readonly Cell Cell;

            public TileEntry(byte[] key, Cell cell)
            {
                Key = key;
                Cell = cell;
            }

            public static int Compare(TileEntry left, TileEntry right)
            {
                int length = Math.Min(left.Key.Length, right.Key.Length);
                for (int index = 0; index < length; index++)
                {
                    if (left.Key[index] != right.Key[index])
                        return left.Key[index] < right.Key[index] ? -1 : 1;
                }
                return left.Key.Length.CompareTo(right.Key.Length);
            }
        }
    }
}

namespace IvanTools
{
    /// <summary>P/Invoke facade over rcsconv.dll (C ABI, UTF-8, panic-safe).</summary>
    internal static class RustDll
    {
        private const string EnvName = "RCS_CONV_DLL";

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr ConvertToFn(IntPtr input, IntPtr target, IntPtr guid,
                                            uint maxPerTile, uint flags);

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void FreeFn(IntPtr text);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryW(string path);

        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern IntPtr GetProcAddress(IntPtr module, string name);

        private static IntPtr _module;
        private static ConvertToFn _convertTo;
        private static FreeFn _free;
        private static string _path;

        public static string Path { get { return _path; } }
        public static string LastError { get; private set; }
        public static bool Loaded { get { return _module != IntPtr.Zero && _convertTo != null; } }

        private const string SettingsKey = @"Software\rcsconv\revit";

        public static string LoadDllPath()
        {
            try
            {
                using (Microsoft.Win32.RegistryKey key =
                    Microsoft.Win32.Registry.CurrentUser.OpenSubKey(SettingsKey))
                {
                    if (key != null)
                        return key.GetValue("dll_path") as string;
                }
            }
            catch (Exception)
            {
            }
            return null;
        }

        public static void SaveDllPath(string path)
        {
            try
            {
                using (Microsoft.Win32.RegistryKey key =
                    Microsoft.Win32.Registry.CurrentUser.CreateSubKey(SettingsKey))
                {
                    if (key != null)
                        key.SetValue("dll_path", path);
                }
            }
            catch (Exception)
            {
            }
        }

        public static string Find()
        {
            string[] candidates = new string[]
            {
                LoadDllPath(),
                Environment.GetEnvironmentVariable(EnvName),
                System.IO.Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "rcsconv.dll"),
                System.IO.Path.Combine(Directory.GetCurrentDirectory(), "rcsconv.dll")
            };
            foreach (string candidate in candidates)
            {
                if (!string.IsNullOrEmpty(candidate) && File.Exists(candidate))
                    return candidate;
            }
            return null;
        }

        public static bool TryLoad(string path)
        {
            if (Loaded)
                return true;
            if (string.IsNullOrEmpty(path) || !File.Exists(path))
            {
                LastError = "rcsconv.dll not found: " + path;
                return false;
            }
            IntPtr module = LoadLibraryW(path);
            if (module == IntPtr.Zero)
            {
                LastError = "LoadLibrary failed (" + Marshal.GetLastWin32Error() + "): " + path;
                return false;
            }
            IntPtr convert = GetProcAddress(module, "rcsconv_convert_to");
            IntPtr free = GetProcAddress(module, "rcsconv_free");
            if (convert == IntPtr.Zero || free == IntPtr.Zero)
            {
                LastError = "rcsconv.dll does not export rcsconv_convert_to / rcsconv_free";
                return false;
            }
            _convertTo = (ConvertToFn)Marshal.GetDelegateForFunctionPointer(convert, typeof(ConvertToFn));
            _free = (FreeFn)Marshal.GetDelegateForFunctionPointer(free, typeof(FreeFn));
            _module = module;
            _path = path;
            LastError = null;
            return true;
        }

        /// <summary>Build a .rcs into an explicit path; flags: bit 0 = overwrite.</summary>
        public static DllResult Convert(string input, string target, string guid, uint maxPerTile, uint flags)
        {
            if (!Loaded)
            {
                DllResult missing = new DllResult();
                missing.Message = LastError == null ? "rcsconv.dll is not loaded" : LastError;
                return missing;
            }
            IntPtr inputPtr = ToUtf8(input);
            IntPtr targetPtr = ToUtf8(target);
            IntPtr guidPtr = ToUtf8(guid);
            try
            {
                IntPtr response = _convertTo(inputPtr, targetPtr, guidPtr, maxPerTile, flags);
                string text = FromUtf8(response);
                _free(response);
                return DllResult.Parse(text);
            }
            finally
            {
                Free(inputPtr);
                Free(targetPtr);
                Free(guidPtr);
            }
        }

        private static IntPtr ToUtf8(string text)
        {
            if (text == null)
                return IntPtr.Zero;
            byte[] bytes = Encoding.UTF8.GetBytes(text);
            IntPtr pointer = Marshal.AllocHGlobal(bytes.Length + 1);
            Marshal.Copy(bytes, 0, pointer, bytes.Length);
            Marshal.WriteByte(pointer, bytes.Length, 0);
            return pointer;
        }

        private static string FromUtf8(IntPtr pointer)
        {
            if (pointer == IntPtr.Zero)
                return string.Empty;
            int length = 0;
            while (Marshal.ReadByte(pointer, length) != 0)
                length++;
            byte[] bytes = new byte[length];
            Marshal.Copy(pointer, bytes, 0, length);
            return Encoding.UTF8.GetString(bytes);
        }

        private static void Free(IntPtr pointer)
        {
            if (pointer != IntPtr.Zero)
                Marshal.FreeHGlobal(pointer);
        }
    }

    /// <summary>Line-based response of rcsconv_convert_to.</summary>
    internal sealed class DllResult
    {
        public bool Ok;
        public string Path;
        public long Points;
        public int Tiles;
        public int Depth;
        public string Message;

        public static DllResult Parse(string text)
        {
            DllResult result = new DllResult();
            string[] lines = (text ?? string.Empty).Replace("\r", string.Empty).Split('\n');
            if (lines.Length > 0 && lines[0] == "ok")
            {
                result.Ok = true;
                result.Path = lines.Length > 1 ? lines[1] : null;
                if (lines.Length > 2) long.TryParse(lines[2], out result.Points);
                if (lines.Length > 3) int.TryParse(lines[3], out result.Tiles);
                if (lines.Length > 4) int.TryParse(lines[4], out result.Depth);
                return result;
            }
            result.Message = lines.Length > 1 ? lines[1] : (text ?? string.Empty);
            return result;
        }
    }

    /// <summary>
    /// Build a .rcs from .pts/.las with the in-process writer and link it into the
    /// active document as a point cloud.
    /// </summary>
    [Transaction(TransactionMode.Manual)]
    [Regeneration(RegenerationOption.Manual)]
    public class RcsConvRevit : IExternalCommand
    {
        private const string Caption = "rcsconv - Point Cloud";

        public Result Execute(ExternalCommandData commandData, ref string message, ElementSet elements)
        {
            if (commandData == null || commandData.Application == null)
                return Result.Cancelled;
            UIDocument uidoc = commandData.Application.ActiveUIDocument;
            if (uidoc == null || uidoc.Document == null)
            {
                TaskDialog.Show(Caption, "No active document.");
                return Result.Cancelled;
            }
            Document document = uidoc.Document;

            string input = PickInput();
            if (input == null)
                return Result.Cancelled;

            string extension = Path.GetExtension(input).ToLowerInvariant();
            if (extension != ".pts" && extension != ".las" && extension != ".laz")
            {
                TaskDialog.Show(Caption, "Unsupported input: " + input);
                return Result.Cancelled;
            }

            string target = PickTarget(document, input);
            if (string.IsNullOrEmpty(target))
                return Result.Cancelled;
            SaveLastDir(Path.GetDirectoryName(target));
            Log("input: " + input);
            Log("target: " + target);

            bool reuse = false;
            while (File.Exists(target))
            {
                bool newer = File.GetLastWriteTimeUtc(target) >= File.GetLastWriteTimeUtc(input);
                ExistingChoice choice = AskExisting(target, newer);
                if (choice == ExistingChoice.Cancel)
                    return Result.Cancelled;
                if (choice == ExistingChoice.NewName)
                {
                    target = PickTarget(document, input);
                    if (string.IsNullOrEmpty(target))
                        return Result.Cancelled;
                    SaveLastDir(Path.GetDirectoryName(target));
                    Log("target: " + target);
                    continue;
                }
                reuse = choice == ExistingChoice.Reuse;
                break;
            }

            if (!EnsureRustDll())
                return Result.Failed;

            DllResult built = null;
            if (!reuse)
            {
                built = RustDll.Convert(input, target, null, 65536, 1);
                if (!built.Ok && IsFileBusy(target))
                {
                    string alternative = UniqueTarget(target);
                    DllResult retry = RustDll.Convert(input, alternative, null, 65536, 1);
                    if (retry.Ok)
                    {
                        Log("locked '" + target + "', saved as '" + alternative + "'");
                        target = alternative;
                        built = retry;
                    }
                }
                if (!built.Ok)
                {
                    Log("ffi failed: " + built.Message);
                    TaskDialog.Show(Caption, "Cannot build the .rcs:\n\n" + built.Message);
                    return Result.Failed;
                }
                target = built.Path;
            }

            ElementId instanceId;
            string engine;
            try
            {
                instanceId = LinkPointCloud(document, target, out engine);
            }
            catch (Exception error)
            {
                Log("link failed: " + error);
                TaskDialog.Show(Caption, "Cannot link the point cloud:\n\n" + error.Message);
                return Result.Failed;
            }

            Log("linked with engine " + engine + ", element " + instanceId.IntegerValue);
            ShowReport(document, target, built, instanceId, engine, reuse);
            return Result.Succeeded;
        }

        /// <summary>Load rcsconv.dll, asking for its path once when it is not found.</summary>
        private static bool EnsureRustDll()
        {
            string found = RustDll.Find();
            if (!string.IsNullOrEmpty(found) && RustDll.TryLoad(found))
            {
                Log("dll: " + found);
                return true;
            }

            TaskDialog dialog = new TaskDialog(Caption);
            dialog.MainInstruction = "rcsconv.dll is not loaded";
            dialog.MainContent =
                (RustDll.LastError == null ? "rcsconv.dll was not found." : RustDll.LastError) +
                Environment.NewLine + Environment.NewLine +
                "Build it with: cargo build --release -p rcsconv-ffi";
            dialog.AddCommandLink(TaskDialogCommandLinkId.CommandLink1, "Locate rcsconv.dll...");
            dialog.CommonButtons = TaskDialogCommonButtons.Cancel;
            if (dialog.Show() != TaskDialogResult.CommandLink1)
                return false;

            using (FileOpenDialog picker = new FileOpenDialog("rcsconv.dll (*.dll)|*.dll"))
            {
                picker.Title = "Locate rcsconv.dll";
                if (picker.Show() != ItemSelectionDialogResult.Confirmed)
                    return false;
                ModelPath path = picker.GetSelectedModelPath();
                if (path == null)
                    return false;
                string dll = ModelPathUtils.ConvertModelPathToUserVisiblePath(path);
                if (!RustDll.TryLoad(dll))
                {
                    TaskDialog.Show(Caption, "Cannot load the DLL:\n\n" + RustDll.LastError);
                    return false;
                }
                RustDll.SaveDllPath(dll);
                Log("dll: " + dll);
                return true;
            }
        }

        private static bool IsFileBusy(string path)
        {
            if (!File.Exists(path))
                return false;
            try
            {
                using (FileStream stream = new FileStream(path, FileMode.Open,
                    FileAccess.ReadWrite, FileShare.None))
                {
                }
                return false;
            }
            catch (IOException)
            {
                return true;
            }
            catch (UnauthorizedAccessException)
            {
                return true;
            }
        }

        private static string PickInput()
        {
            // Revit-native dialog: no System.Windows.Forms dependency, which matters for
            // CsHotLoader (it references only Revit assemblies and already loaded ones).
            using (FileOpenDialog dialog = new FileOpenDialog(
                "Point cloud sources (*.pts;*.las)|*.pts;*.las"))
            {
                dialog.Title = "Select a point cloud source";
                if (dialog.Show() != ItemSelectionDialogResult.Confirmed)
                    return null;
                ModelPath path = dialog.GetSelectedModelPath();
                return path == null ? null : ModelPathUtils.ConvertModelPathToUserVisiblePath(path);
            }
        }

        /// <summary>
        /// Create the point cloud instance, trying every file-based engine until one
        /// accepts the file. The identifier is reported back to the caller.
        /// </summary>
        private static ElementId LinkPointCloud(Document document, string path, out string engine)
        {
            string fullPath = Path.GetFullPath(path);
            List<string> candidates = CandidateEngines();
            List<string> failures = new List<string>();

            // An existing type already carries its working engine identifier.
            PointCloudType existing = FindType(document, fullPath);
            if (existing != null)
            {
                using (Transaction transaction = new Transaction(document, "Link point cloud"))
                {
                    transaction.Start();
                    PointCloudInstance instance =
                        PointCloudInstance.Create(document, existing.Id, Transform.Identity);
                    transaction.Commit();
                    engine = existing.EngineIdentifier;
                    return instance.Id;
                }
            }

            foreach (string identifier in candidates)
            {
                Transaction transaction = new Transaction(document, "Link point cloud");
                try
                {
                    transaction.Start();
                    PointCloudType type = PointCloudType.Create(document, identifier, fullPath);
                    PointCloudInstance instance =
                        PointCloudInstance.Create(document, type.Id, Transform.Identity);
                    transaction.Commit();
                    engine = identifier;
                    return instance.Id;
                }
                catch (Exception error)
                {
                    failures.Add(identifier + ": " + error.Message);
                    Log("engine '" + identifier + "' failed: " + error.Message);
                    if (transaction.GetStatus() == TransactionStatus.Started)
                        transaction.RollBack();
                }
                finally
                {
                    transaction.Dispose();
                }
            }

            engine = null;
            throw new InvalidOperationException(BuildEngineReport(candidates, failures));
        }

        /// <summary>
        /// Engines in the order they should be tried: the ReCap engine first, then
        /// anything that looks like it, then the rest. Every identifier reported by the
        /// registry is kept: filtering with IsEngineFileBased is NOT safe here - for
        /// some identifiers that very call throws "This engine type has not been
        /// registered", so the flag is only used for reporting, never for filtering.
        /// </summary>
        private static List<string> CandidateEngines()
        {
            IList<string> all =
                Autodesk.Revit.DB.PointClouds.PointCloudEngineRegistry.GetSupportedEngines();
            List<string> ordered = new List<string>();
            AddExact(ordered, all, "rcs");
            AddExact(ordered, all, "recap");
            foreach (string identifier in all)
            {
                if (identifier.IndexOf("rcap", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    identifier.IndexOf("rcs", StringComparison.OrdinalIgnoreCase) >= 0)
                    AddUnique(ordered, identifier);
            }
            foreach (string identifier in all)
                AddUnique(ordered, identifier);
            string[] names = new string[ordered.Count];
            ordered.CopyTo(names);
            Log("engines: " + string.Join(", ", names));
            return ordered;
        }

        private static void AddExact(List<string> ordered, IList<string> engines, string wanted)
        {
            foreach (string identifier in engines)
            {
                if (string.Equals(identifier, wanted, StringComparison.OrdinalIgnoreCase))
                    AddUnique(ordered, identifier);
            }
        }

        private static void AddUnique(List<string> ordered, string identifier)
        {
            foreach (string existing in ordered)
            {
                if (string.Equals(existing, identifier, StringComparison.OrdinalIgnoreCase))
                    return;
            }
            ordered.Add(identifier);
        }

        /// <summary>Failure text with the engine list, so the next run needs no guessing.</summary>
        private static string BuildEngineReport(List<string> candidates, List<string> failures)
        {
            StringBuilder text = new StringBuilder();
            text.AppendLine("No point cloud engine accepted the .rcs file.");
            text.AppendLine();
            text.AppendLine("Registered engines:");
            IList<string> all =
                Autodesk.Revit.DB.PointClouds.PointCloudEngineRegistry.GetSupportedEngines();
            foreach (string identifier in all)
            {
                string fileBased;
                try
                {
                    fileBased = Autodesk.Revit.DB.PointClouds.PointCloudEngineRegistry
                        .IsEngineFileBased(identifier) ? "true" : "false";
                }
                catch (Exception error)
                {
                    fileBased = "unknown (" + error.Message + ")";
                }
                text.AppendFormat(CultureInfo.InvariantCulture, "  {0} (file based: {1}){2}",
                    identifier, fileBased, Environment.NewLine);
            }
            text.AppendLine();
            if (candidates.Count == 0)
            {
                text.AppendLine("The registry reported no engines, so a .rcs cannot be linked.");
            }
            else
            {
                text.AppendLine("Attempts:");
                foreach (string failure in failures)
                    text.AppendLine("  " + failure);
            }
            return text.ToString();
        }

        /// <summary>Append a line to %TEMP%\RcsConvRevit.log; never throws.</summary>
        private static void Log(string message)
        {
            try
            {
                string path = Path.Combine(Path.GetTempPath(), "RcsConvRevit.log");
                using (StreamWriter writer = new StreamWriter(path, true, Encoding.UTF8))
                {
                    writer.WriteLine(string.Format(CultureInfo.InvariantCulture,
                        "{0:yyyy-MM-dd HH:mm:ss} {1}", DateTime.Now, message));
                }
            }
            catch (Exception)
            {
                // diagnostics must never break the command
            }
        }

        private const string SettingsKey = @"Software\rcsconv\revit";

        private static string LoadLastDir()
        {
            try
            {
                using (Microsoft.Win32.RegistryKey key =
                    Microsoft.Win32.Registry.CurrentUser.OpenSubKey(SettingsKey))
                {
                    if (key != null)
                        return key.GetValue("last_dir") as string;
                }
            }
            catch (Exception)
            {
            }
            return null;
        }

        private static void SaveLastDir(string folder)
        {
            try
            {
                using (Microsoft.Win32.RegistryKey key =
                    Microsoft.Win32.Registry.CurrentUser.CreateSubKey(SettingsKey))
                {
                    if (key != null)
                        key.SetValue("last_dir", folder);
                }
            }
            catch (Exception)
            {
            }
        }

        /// <summary>Last used folder, else the model folder, else the input folder.</summary>
        private static string SuggestFolder(string input, Document document)
        {
            string last = LoadLastDir();
            if (!string.IsNullOrEmpty(last) && Directory.Exists(last))
                return last;
            string model = document.PathName;
            if (!string.IsNullOrEmpty(model))
            {
                string dir = Path.GetDirectoryName(model);
                if (!string.IsNullOrEmpty(dir) && Directory.Exists(dir))
                    return dir;
            }
            return Path.GetDirectoryName(Path.GetFullPath(input));
        }

        private enum ExistingChoice { Reuse, Rebuild, NewName, Cancel }

        private static ExistingChoice AskExisting(string target, bool newerThanInput)
        {
            TaskDialog dialog = new TaskDialog(Caption);
            dialog.MainInstruction = "The .rcs file already exists";
            dialog.MainContent = target + Environment.NewLine + Environment.NewLine +
                (newerThanInput
                    ? "It is newer than the input, so it can be reused."
                    : "It is older than the input, so rebuilding is recommended.") +
                Environment.NewLine + Environment.NewLine +
                "Revit keeps linked .rcs files open, so saving under a new name is the safe choice.";
            dialog.AddCommandLink(TaskDialogCommandLinkId.CommandLink1, "Use the existing file");
            dialog.AddCommandLink(TaskDialogCommandLinkId.CommandLink2, "Rebuild (overwrite)");
            dialog.AddCommandLink(TaskDialogCommandLinkId.CommandLink3, "Save under a new name...");
            dialog.CommonButtons = TaskDialogCommonButtons.Cancel;
            TaskDialogResult result = dialog.Show();
            if (result == TaskDialogResult.CommandLink1)
                return ExistingChoice.Reuse;
            if (result == TaskDialogResult.CommandLink2)
                return ExistingChoice.Rebuild;
            if (result == TaskDialogResult.CommandLink3)
                return ExistingChoice.NewName;
            return ExistingChoice.Cancel;
        }


        /// <summary>"Save as": the file name is adjustable because Revit locks linked files.</summary>
        private static string PickTarget(Document document, string input)
        {
            string folder = SuggestFolder(input, document);
            string suggested = Path.Combine(folder,
                Path.GetFileNameWithoutExtension(input) + ".rcs");
            using (FileSaveDialog dialog = new FileSaveDialog("ReCap point cloud (*.rcs)|*.rcs"))
            {
                dialog.Title = "Save the .rcs file";
                dialog.InitialFileName = suggested;
                if (dialog.Show() != ItemSelectionDialogResult.Confirmed)
                    return null;
                ModelPath path = dialog.GetSelectedModelPath();
                if (path == null)
                    return null;
                string target = ModelPathUtils.ConvertModelPathToUserVisiblePath(path);
                if (!target.EndsWith(".rcs", StringComparison.OrdinalIgnoreCase))
                    target += ".rcs";
                return target;
            }
        }

        /// <summary>cloud.rcs -> cloud-2.rcs, cloud-3.rcs, ... (first free name).</summary>
        private static string UniqueTarget(string target)
        {
            string dir = Path.GetDirectoryName(target);
            string stem = Path.GetFileNameWithoutExtension(target);
            for (int index = 2; index < 1000; index++)
            {
                string candidate = Path.Combine(dir, stem + "-" +
                    index.ToString(CultureInfo.InvariantCulture) + ".rcs");
                if (!File.Exists(candidate))
                    return candidate;
            }
            throw new IOException("No free .rcs name next to " + target);
        }

        private static PointCloudType FindType(Document document, string fullPath)
        {
            FilteredElementCollector collector = new FilteredElementCollector(document);
            foreach (Element element in collector.OfClass(typeof(PointCloudType)))
            {
                PointCloudType type = element as PointCloudType;
                if (type == null || type.GetPath() == null)
                    continue;
                string existing = ModelPathUtils.ConvertModelPathToUserVisiblePath(type.GetPath());
                if (string.Equals(Path.GetFullPath(existing), fullPath, StringComparison.OrdinalIgnoreCase))
                    return type;
            }
            return null;
        }

        private static void ShowReport(Document document, string path, DllResult built,
                                       ElementId instanceId, string engine, bool reused)
        {
            string text = "File: " + path + Environment.NewLine +
                          "Element id: " + instanceId.IntegerValue + Environment.NewLine +
                          "Engine: " + engine;
            if (reused)
            {
                text += Environment.NewLine + "Reused the existing file (no rebuild).";
            }
            else if (built != null)
            {
                text += string.Format(CultureInfo.InvariantCulture,
                    Environment.NewLine + "Points: {0}; tiles: {1}; depth: {2}",
                    built.Points, built.Tiles, built.Depth);
            }
            Element instance = document.GetElement(instanceId);
            BoundingBoxXYZ box = instance == null ? null : instance.get_BoundingBox(null);
            if (box != null)
            {
                text += string.Format(CultureInfo.InvariantCulture,
                    Environment.NewLine + "BBox (m): ({0:0.###}, {1:0.###}, {2:0.###}) - ({3:0.###}, {4:0.###}, {5:0.###})",
                    box.Min.X * 0.3048, box.Min.Y * 0.3048, box.Min.Z * 0.3048,
                    box.Max.X * 0.3048, box.Max.Y * 0.3048, box.Max.Z * 0.3048);
            }
            text += Environment.NewLine + Environment.NewLine +
                    "Manage Links shows the point cloud; the .rcs file must stay in place.";

            TaskDialog dialog = new TaskDialog(Caption);
            dialog.MainInstruction = "Point cloud linked";
            dialog.MainContent = text;
            dialog.CommonButtons = TaskDialogCommonButtons.Close;
            dialog.Show();
        }
    }
}
