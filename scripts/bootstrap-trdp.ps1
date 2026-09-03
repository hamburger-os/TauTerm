$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Vendor = Join-Path $Root "src-tauri\vendor\tcnopen"
$Native = Join-Path $Root "src-tauri\native"
$Build = Join-Path $Root ".cache\trdp-native-build"
$Out = Join-Path $Root "src-tauri\binaries"
$ToolsOut = Join-Path $Root "tools\trdp-test-peer\bin"

if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
  throw "CMake 3.20+ is required"
}
if (-not (Test-Path (Join-Path $Vendor "src\api\trdp_if_light.h")) -or
    -not (Test-Path (Join-Path $Vendor "src\common\trdp_private.h"))) {
  throw "Vendored TCNOpen 3.0.0.0 source is incomplete under $Vendor"
}

New-Item -ItemType Directory -Force -Path $Build, $Out, $ToolsOut | Out-Null

Write-Host "Configuring vendored TCNOpen 3.0.0.0 + TauTerm TRDP native helpers..."
cmake -S $Native -B $Build -A x64
if ($LASTEXITCODE -ne 0) { throw "TRDP CMake configure failed with exit code $LASTEXITCODE" }

cmake --build $Build --config Release --parallel
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
