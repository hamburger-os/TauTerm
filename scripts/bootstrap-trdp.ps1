$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Vendor = Join-Path $Root "src-tauri\vendor\tcnopen"
$Native = Join-Path $Root "src-tauri\native"
$Build = Join-Path $Root ".cache\trdp-native-build"
$Out = Join-Path $Root "src-tauri\binaries"
$ToolsOut = Join-Path $Root "tools\trdp-test-peer\bin"

function Resolve-CMake {
  $Command = Get-Command cmake -ErrorAction SilentlyContinue
  if ($Command) { return $Command.Source }

  $Candidates = @()
  if ($env:ProgramFiles) {
    $Candidates += Join-Path $env:ProgramFiles "CMake\bin\cmake.exe"
  }
  if (${env:ProgramFiles(x86)}) {
    $Candidates += Join-Path ${env:ProgramFiles(x86)} "CMake\bin\cmake.exe"
  }

  $VsWhere = if (${env:ProgramFiles(x86)}) {
    Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  } else {
    $null
  }
  if ($VsWhere -and (Test-Path $VsWhere)) {
    $VsInstall = & $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.CMake.Project -property installationPath 2>$null
    if ($VsInstall) {
      $Candidates += Join-Path $VsInstall "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
    }
  }

  return ($Candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1)
}

$CMake = Resolve-CMake
if (-not $CMake) {
  throw @"
CMake 3.20+ is required for the complete TRDP development build.
Install it, then open a new PowerShell:
  winget install --id Kitware.CMake -e --source winget
Also ensure the Windows C++ build tools are installed (Desktop development with C++).
"@
}

Write-Host "Using CMake: $CMake"
$VersionText = & $CMake --version | Select-Object -First 1
if ($VersionText -notmatch 'cmake version (\d+)\.(\d+)') {
  throw "Unable to determine CMake version from: $VersionText"
}
$CMakeMajor = [int]$Matches[1]
$CMakeMinor = [int]$Matches[2]
if ($CMakeMajor -lt 3 -or ($CMakeMajor -eq 3 -and $CMakeMinor -lt 20)) {
  throw "CMake 3.20+ is required; found $CMakeMajor.$CMakeMinor at $CMake"
}

$VsWhere = if (${env:ProgramFiles(x86)}) {
  Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
} else {
  $null
}
if (-not $VsWhere -or -not (Test-Path $VsWhere)) {
  throw @"
Windows C++ Build Tools are required for the complete TRDP development build.
Install Visual Studio Build Tools with the 'Desktop development with C++' workload.
"@
}
$VsInstall = & $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
if ($VsInstall) { Write-Host "Using Windows C++ Build Tools: $VsInstall" }
if (-not $VsInstall) {
  throw @"
MSVC x64/x86 build tools were not found.
Open Visual Studio Installer and add the 'Desktop development with C++' workload,
or install Build Tools with Microsoft.VisualStudio.Workload.VCTools.
"@
}

if (-not (Test-Path (Join-Path $Vendor "src\api\trdp_if_light.h")) -or
    -not (Test-Path (Join-Path $Vendor "src\common\trdp_private.h"))) {
  throw "Vendored TCNOpen 3.0.0.0 source is incomplete under $Vendor"
}

New-Item -ItemType Directory -Force -Path $Build, $Out, $ToolsOut | Out-Null

Write-Host "Configuring vendored TCNOpen 3.0.0.0 + TauTerm TRDP native helpers..."
& $CMake -S $Native -B $Build -A x64
if ($LASTEXITCODE -ne 0) { throw "TRDP CMake configure failed with exit code $LASTEXITCODE" }

& $CMake --build $Build --config Release --parallel
if ($LASTEXITCODE -ne 0) { throw "TRDP native build failed with exit code $LASTEXITCODE" }

$BridgeBuilt = Get-ChildItem -Path $Build -Recurse -File -Filter "tauterm-trdp-bridge.exe" | Select-Object -First 1
$PeerBuilt = Get-ChildItem -Path $Build -Recurse -File -Filter "trdp-test-peer.exe" | Select-Object -First 1
$BridgeExe = Join-Path $Out "tauterm-trdp-bridge.exe"
$PeerExe = Join-Path $ToolsOut "trdp-test-peer.exe"

if (-not $BridgeBuilt) { throw "TRDP bridge executable was not produced under $Build" }
if (-not $PeerBuilt) { throw "TRDP reference peer executable was not produced under $Build" }

Copy-Item -Force $BridgeBuilt.FullName $BridgeExe
Copy-Item -Force $PeerBuilt.FullName $PeerExe

$SmokeInput = "{`"command`":`"monitor_open`",`"params`":{`"mode`":`"monitor`"}}`n{`"command`":`"shutdown`"}`n"
$SmokeOutput = $SmokeInput | & $BridgeExe
if ($LASTEXITCODE -ne 0 -or ($SmokeOutput -join "`n") -notmatch '"command":"shutdown"') {
  throw "TRDP bridge smoke test failed"
}

& $PeerExe *> $null
if ($LASTEXITCODE -ne 2) { throw "TRDP reference peer usage smoke test failed" }

Write-Host "TRDP bridge ready: $BridgeExe"
Write-Host "Reference peer ready: $PeerExe"
Write-Host "TCNOpen source: $Vendor (MPL-2.0, vendored 3.0.0.0 snapshot)"
exit 0
