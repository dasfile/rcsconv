using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;
using RcsConv.Core;

namespace RcsWriterTests
{
    /// <summary>
    /// Console harness for the C# writer: compares its bytes with rcsconv.exe (Rust)
    /// on the same input and the same GUID. No Revit application is needed.
    /// </summary>
    internal static class Program
    {
        private const string FixedGuid = "{00112233-4455-6677-8899-aabbccddeeff}";

        private static int Main(string[] args)
        {
            string root = FindRoot();
            if (root == null)
            {
                Console.Error.WriteLine("Repository root not found (rust/ and golden/ are required).");
                return 2;
            }
            string exe = Path.Combine(root, "rust", "target", "release", "rcsconv.exe");
            if (!File.Exists(exe))
            {
                Console.Error.WriteLine("rcsconv.exe not found: " + exe);
                return 2;
            }

            string output = Path.Combine(root, "out", "revit-tests");
            string rustOut = Path.Combine(output, "rust");
            string csOut = Path.Combine(output, "csharp");
            Directory.CreateDirectory(rustOut);
            Directory.CreateDirectory(csOut);

            string[] cases = args.Length > 0
                ? args
                : new string[] { "probe6.pts", "test1.pts", "test2.pts", "level_7.pts", "level_7_n19099.pts" };

            int failures = 0;
            int checkedCount = 0;
            foreach (string name in cases)
            {
                string source = Path.Combine(root, "golden", name);
                if (!File.Exists(source))
                {
                    Console.WriteLine("skipped: no " + source);
                    continue;
                }

                string stem = Path.GetFileNameWithoutExtension(name);
                string rustFile = Path.Combine(rustOut, stem + ".rcs");
                string csFile = Path.Combine(csOut, stem + ".rcs");

                if (!RunRust(exe, source, rustOut, rustFile))
                {
                    failures++;
                    continue;
                }

                CloudPoint[] points = PtsReader.Read(source);
                BuildStats stats;
                byte[] bytes = RcsWriter.Build(points, new BuildOptions { Guid = FixedGuid }, out stats);
                File.WriteAllBytes(csFile, bytes);

                byte[] expected = File.ReadAllBytes(rustFile);
                checkedCount++;
                if (bytes.Length != expected.Length)
                {
                    Console.WriteLine("FAIL {0}: size {1} vs {2} (Rust)", name, bytes.Length, expected.Length);
                    failures++;
                    continue;
                }

                int differences = 0;
                int firstDifference = -1;
                int firstLength = 0;
                for (int index = 0; index < bytes.Length; index++)
                {
                    if (bytes[index] == expected[index])
                        continue;
                    differences++;
                    if (firstDifference < 0)
                    {
                        firstDifference = index;
                        firstLength = Math.Min(16, bytes.Length - index);
                    }
                }

                if (differences == 0)
                {
                    Console.WriteLine("OK   {0}: {1} bytes, {2} points, {3} tiles, depth {4}",
                        name, bytes.Length, stats.Points, stats.Tiles, stats.Depth);
                }
                else
                {
                    Console.WriteLine("FAIL {0}: {1} differing bytes, first at 0x{2:X}",
                        name, differences, firstDifference);
                    Console.WriteLine("  C#  : " + Hex(bytes, firstDifference, firstLength));
                    Console.WriteLine("  Rust: " + Hex(expected, firstDifference, firstLength));
                    failures++;
                }
            }

            Console.WriteLine("---");
            Console.WriteLine("{0} checked, {1} failed", checkedCount, failures);
            return failures == 0 ? 0 : 1;
        }
        private static bool RunRust(string exe, string source, string outDir, string expectedPath)
        {
            if (File.Exists(expectedPath))
                File.Delete(expectedPath);
            ProcessStartInfo info = new ProcessStartInfo(exe);
            info.Arguments = string.Format(CultureInfo.InvariantCulture,
                "pts2rcs \"{0}\" -o \"{1}\" -f -g \"{2}\"", source, outDir, FixedGuid);
            info.UseShellExecute = false;
            info.CreateNoWindow = true;
            info.RedirectStandardOutput = true;
            info.RedirectStandardError = true;
            using (Process process = Process.Start(info))
            {
                string stdout = process.StandardOutput.ReadToEnd();
                string stderr = process.StandardError.ReadToEnd();
                process.WaitForExit();
                if (process.ExitCode != 0)
                {
                    Console.Error.WriteLine("rcsconv failed for " + source + ": " + stderr);
                    return false;
                }
                if (!string.IsNullOrEmpty(stdout))
                    Console.WriteLine("  rust: " + stdout.Trim());
            }
            if (!File.Exists(expectedPath))
            {
                Console.Error.WriteLine("rcsconv did not produce " + expectedPath);
                return false;
            }
            return true;
        }

        private static string FindRoot()
        {
            string current = AppDomain.CurrentDomain.BaseDirectory;
            for (int depth = 0; depth < 8 && current != null; depth++)
            {
                if (Directory.Exists(Path.Combine(current, "rust")) &&
                    Directory.Exists(Path.Combine(current, "golden")))
                    return current;
                DirectoryInfo parent = Directory.GetParent(current);
                current = parent == null ? null : parent.FullName;
            }
            return null;
        }

        private static string Hex(byte[] data, int offset, int length)
        {
            StringBuilder text = new StringBuilder();
            for (int index = 0; index < length; index++)
                text.AppendFormat(CultureInfo.InvariantCulture, "{0:X2} ", data[offset + index]);
            return text.ToString().Trim();
        }
    }
}
