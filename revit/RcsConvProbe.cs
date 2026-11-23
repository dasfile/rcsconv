// =============================================================================
// RcsConvProbe - verify the point cloud API of Revit 2023 (run via HotLoader)
// =============================================================================
// The probe answers two questions before the real plugin relies on them:
//   1) which point cloud engines are registered, and which of them are file-based;
//   2) which engine identifier accepts a .rcs file, and does
//      PointCloudType.Create + PointCloudInstance.Create work inside a transaction.
//
// It creates the point cloud, reports id / path / FoundStatus / bbox, and then
// deletes what it created (the model stays clean).
//
// License: MIT
// =============================================================================

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using Autodesk.Revit.Attributes;
using Autodesk.Revit.DB;
using Autodesk.Revit.UI;

namespace IvanTools
{
    [Transaction(TransactionMode.Manual)]
    [Regeneration(RegenerationOption.Manual)]
    public class RcsConvProbe : IExternalCommand
    {
        private const string Caption = "rcsconv - Point Cloud Probe";

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

            StringBuilder report = new StringBuilder();
            report.AppendLine("Registered point cloud engines:");
            IList<string> engines = Autodesk.Revit.DB.PointClouds.PointCloudEngineRegistry.GetSupportedEngines();
            List<string> fileBased = new List<string>();
            foreach (string identifier in engines)
            {
                bool isFileBased =
                    Autodesk.Revit.DB.PointClouds.PointCloudEngineRegistry.IsEngineFileBased(identifier);
                report.AppendFormat(CultureInfo.InvariantCulture, "  {0}  (file based: {1}){2}",
                    identifier, isFileBased, Environment.NewLine);
                if (isFileBased)
                    fileBased.Add(identifier);
            }
            if (fileBased.Count == 0)
            {
                TaskDialog.Show(Caption, report + Environment.NewLine + "No file-based engine: cannot link .rcs.");
                return Result.Failed;
            }

            string file = PickRcs();
            if (file == null)
                return Result.Cancelled;
            string fullPath = Path.GetFullPath(file);
            report.AppendLine();
            report.AppendLine("File: " + fullPath);

            // Try the engines in the order Revit reports them; the first success wins.
            PointCloudType type = null;
            string usedEngine = null;
            List<string> failures = new List<string>();
            using (Transaction transaction = new Transaction(uidoc.Document, "Point cloud probe"))
            {
                transaction.Start();
                foreach (string identifier in fileBased)
                {
                    try
                    {
                        type = PointCloudType.Create(uidoc.Document, identifier, fullPath);
                        usedEngine = identifier;
                        break;
                    }
                    catch (Exception error)
                    {
                        failures.Add(identifier + ": " + error.Message);
                    }
                }

                if (type == null)
                {
                    transaction.RollBack();
                    report.AppendLine("PointCloudType.Create failed for every file-based engine:");
                    foreach (string failure in failures)
                        report.AppendLine("  " + failure);
                    TaskDialog.Show(Caption, report.ToString());
                    return Result.Failed;
                }

                report.AppendLine("Engine identifier: " + usedEngine);
                report.AppendLine("Type id: " + type.Id.IntegerValue);
                report.AppendLine("GetPath(): " +
                    ModelPathUtils.ConvertModelPathToUserVisiblePath(type.GetPath()));
                report.AppendLine("FoundStatus: " + type.FoundStatus);

                PointCloudInstance instance = PointCloudInstance.Create(
                    uidoc.Document, type.Id, Transform.Identity);
                report.AppendLine("Instance id: " + instance.Id.IntegerValue);

                BoundingBoxXYZ box = instance.get_BoundingBox(null);
                if (box != null)
                {
                    report.AppendFormat(CultureInfo.InvariantCulture,
                        "BBox (m): ({0:0.###}, {1:0.###}, {2:0.###}) - ({3:0.###}, {4:0.###}, {5:0.###}){6}",
                        box.Min.X * 0.3048, box.Min.Y * 0.3048, box.Min.Z * 0.3048,
                        box.Max.X * 0.3048, box.Max.Y * 0.3048, box.Max.Z * 0.3048,
                        Environment.NewLine);
                }
                IList<string> scans = instance.GetScans();
                report.AppendLine("Scans: " + (scans == null ? 0 : scans.Count));

                transaction.Commit();

                TaskDialog dialog = new TaskDialog(Caption);
                dialog.MainInstruction = "Point cloud created";
                dialog.MainContent = report.ToString();
                dialog.AddCommandLink(TaskDialogCommandLinkId.CommandLink1, "Delete the probe elements");
                dialog.CommonButtons = TaskDialogCommonButtons.Close;
                bool delete = dialog.Show() == TaskDialogResult.CommandLink1;

                if (delete)
                {
                    using (Transaction cleanup = new Transaction(uidoc.Document, "Point cloud probe cleanup"))
                    {
                        cleanup.Start();
                        uidoc.Document.Delete(instance.Id);
                        uidoc.Document.Delete(type.Id);
                        cleanup.Commit();
                    }
                }
            }
            return Result.Succeeded;
        }

        private static string PickRcs()
        {
            using (System.Windows.Forms.OpenFileDialog dialog = new System.Windows.Forms.OpenFileDialog())
            {
                dialog.Title = "Select a .rcs file for the probe";
                dialog.Filter = "ReCap point cloud (*.rcs)|*.rcs|All files (*.*)|*.*";
                dialog.CheckFileExists = true;
                return dialog.ShowDialog() == System.Windows.Forms.DialogResult.OK
                    ? dialog.FileName
                    : null;
            }
        }
    }
}
