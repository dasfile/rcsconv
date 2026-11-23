# =============================================================================
# Build and run the C# writer tests (no Revit application required)
# =============================================================================
# Compiles revit/RcsConvRevit.cs together with revit/tests/RcsWriterTests.cs,
# runs the harness and compares the C# bytes with rcsconv.exe (Rust) on the same
# inputs and the same GUID.
#
# Usage:
#   .\revit\run-tests.ps1
#   .\revit\run-tests.ps1 -Cases probe6.pts
# =============================================================================

[CmdletBinding()]
param(
    [string]$RevitDir = "C:\Program Files\Autodesk\Revit 2023",
    [string[]]$Cases = @()
)

$ErrorActionPreference = "Stop"

$root      = Split-Path -Parent $PSScriptRoot
$framework = "C:\Windows\Microsoft.NET\Framework64\v4.0.30319"
$csc       = Join-Path $framework "csc.exe"
$outDir    = Join-Path $root "out\revit-tests"
$exe       = Join-Path $outDir "RcsWriterTests.exe"

$references = @(
    (Join-Path $RevitDir "RevitAPI.dll"),
    (Join-Path $RevitDir "RevitAPIUI.dll"),
    (Join-Path $framework "System.dll"),
    (Join-Path $framework "System.Core.dll")
)

if (-not (Test-Path -LiteralPath $csc)) { throw "csc.exe not found: $csc" }
foreach ($reference in $references) {
    if (-not (Test-Path -LiteralPath $reference)) { throw "Missing reference: $reference" }
}
if (-not (Test-Path -LiteralPath (Join-Path $root "rust\target\release\rcsconv.exe"))) {
    throw "Build the Rust release first: cargo build --release (in rust/)"
}

New-Item -ItemType Directory -Path $outDir -Force | Out-Null

$arguments = @("/nologo", "/warn:4", "/target:exe", "/platform:x64", "/out:$exe")
foreach ($reference in $references) { $arguments += "/reference:$reference" }
$arguments += (Join-Path $root "revit\RcsConvRevit.cs")
$arguments += (Join-Path $root "revit\tests\RcsWriterTests.cs")

Write-Host "Compiling RcsWriterTests.exe"
& $csc $arguments
if ($LASTEXITCODE -ne 0) { throw "Compilation failed" }

# Revit assemblies are referenced by the command class; copy them so the harness
# can start outside Revit if the runtime decides to resolve them.
Copy-Item -LiteralPath (Join-Path $RevitDir "RevitAPI.dll")   -Destination $outDir -Force
Copy-Item -LiteralPath (Join-Path $RevitDir "RevitAPIUI.dll") -Destination $outDir -Force

Write-Host "Running tests"
& $exe @Cases
exit $LASTEXITCODE
