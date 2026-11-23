# =============================================================================
# Build the installable Revit add-in: RcsConvRevit.dll + RcsConv.addin + rcsconv.dll
# =============================================================================
# Output (out/dist):
#   rcsconv-revit-<version>/RcsConv.addin
#   rcsconv-revit-<version>/rcsconv/RcsConvRevit.dll
#   rcsconv-revit-<version>/rcsconv/rcsconv.dll
#   rcsconv-revit-<version>/rcsconv/README.md
#   rcsconv-revit-<version>/rcsconv/LICENSE
#   rcsconv-revit-<version>.zip
#
# Usage:
#   .\revit\build-addin.ps1
#   .\revit\build-addin.ps1 -Version 1.0.0
# =============================================================================

[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$RevitDir = "C:\Program Files\Autodesk\Revit 2023",
    [string]$Dist = ""
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
if (-not $Version) {
    $Version = (Select-String -Path (Join-Path $root "rust\rcsconv\Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
}
if (-not $Dist) { $Dist = Join-Path $root "out\dist" }

$framework = "C:\Windows\Microsoft.NET\Framework64\v4.0.30319"
$csc = Join-Path $framework "csc.exe"
$stage = Join-Path $Dist "rcsconv-revit-$Version"
$payload = Join-Path $stage "rcsconv"
$rustDll = Join-Path $root "rust\target\release\rcsconv.dll"
$zip = Join-Path $Dist "rcsconv-revit-$Version.zip"

if (-not (Test-Path -LiteralPath $csc)) { throw "csc.exe not found: $csc" }
if (-not (Test-Path -LiteralPath $rustDll)) {
    throw "rcsconv.dll not found: build the Rust release first (cargo build --release)"
}

$references = @(
    (Join-Path $RevitDir "RevitAPI.dll"),
    (Join-Path $RevitDir "RevitAPIUI.dll"),
    (Join-Path $framework "System.dll"),
    (Join-Path $framework "System.Core.dll")
)
foreach ($reference in $references) {
    if (-not (Test-Path -LiteralPath $reference)) { throw "Missing reference: $reference" }
}

if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Path $payload -Force | Out-Null

$parts = @($Version.Split("."))
while ($parts.Count -lt 4) { $parts += "0" }
$assemblyVersion = $parts[0..3] -join "."
$versionFile = Join-Path $env:TEMP "RcsConvRevitVersion.cs"
@"
using System.Reflection;
[assembly: AssemblyTitle("rcsconv Revit add-in")]
[assembly: AssemblyProduct("rcsconv")]
[assembly: AssemblyCompany("Ivan Rodionov")]
[assembly: AssemblyVersion("$assemblyVersion")]
[assembly: AssemblyFileVersion("$assemblyVersion")]
"@ | Set-Content -LiteralPath $versionFile -Encoding ASCII

$output = Join-Path $payload "RcsConvRevit.dll"
$cscArgs = @("/nologo", "/warn:4", "/target:library", "/out:$output")
foreach ($reference in $references) { $cscArgs += "/reference:$reference" }
$cscArgs += (Join-Path $root "revit\RcsConvRevit.cs")
$cscArgs += $versionFile

Write-Host "Compiling RcsConvRevit.dll"
& $csc $cscArgs
if ($LASTEXITCODE -ne 0) { throw "Compilation failed" }

Copy-Item -LiteralPath $rustDll -Destination $payload -Force
Copy-Item -LiteralPath (Join-Path $root "revit\RcsConv.addin") -Destination $stage -Force
Copy-Item -LiteralPath (Join-Path $root "revit\README.md")    -Destination $payload -Force
Copy-Item -LiteralPath (Join-Path $root "LICENSE")             -Destination $payload -Force

if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip

$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLower()
Write-Host ""
Write-Host ("Package : {0}" -f $zip)
Write-Host ("Version : {0} ({1})" -f $Version, $assemblyVersion)
Write-Host ("SHA256  : {0}" -f $hash)
