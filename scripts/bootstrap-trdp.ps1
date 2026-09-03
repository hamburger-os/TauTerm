$ErrorActionPreference = "Stop"

$Version = "3.0.0.0"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Cache = Join-Path $Root ".cache\tcnopen-$Version"
$Zip = Join-Path $Cache "$Version.zip"
$Src = Join-Path $Cache "src"
$Out = Join-Path $Root "src-tauri\binaries"
$Url = "https://sourceforge.net/projects/tcnopen/files/TRDP/$Version/$Version.zip/download"

New-Item -ItemType Directory -Force -Path $Cache, $Out | Out-Null
if (-not (Test-Path $Zip)) {
  Write-Host "Downloading TCNOpen $Version from SourceForge..."
  Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing
}
if (-not (Test-Path $Src)) {
  New-Item -ItemType Directory -Force -Path $Src | Out-Null
  Expand-Archive -LiteralPath $Zip -DestinationPath $Src -Force
}

$Project = Get-ChildItem -Path $Src -Recurse -Filter TRDP.vcxproj |
  Where-Object { $_.FullName -match '[\\/]VSExpress2015[\\/]TRDP[\\/]TRDP\.vcxproj$' } |
  Select-Object -First 1
if (-not $Project) { throw "Could not locate TCNOpen TRDP.vcxproj" }
$TrdpDir = (Resolve-Path (Join-Path $Project.Directory.FullName "..\.." )).Path

$VsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $VsWhere)) { throw "Visual Studio Build Tools 2022 with Desktop C++ workload is required" }
$VsRoot = & $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $VsRoot) { throw "Visual Studio C++ build tools were not found" }
$DevCmd = Join-Path $VsRoot "Common7\Tools\VsDevCmd.bat"
if (-not (Test-Path $DevCmd)) { throw "VsDevCmd.bat not found" }

# The upstream project targets an old toolset/SDK. Override both at build time so the
# fixed TCNOpen 3.0.0.0 source can be built with current Visual Studio Build Tools.
$Build = Join-Path $Cache "build-bridge.cmd"
$Native = Join-Path $Root "src-tauri\native\trdp_bridge.c"
$Exe = Join-Path $Out "tauterm-trdp-bridge.exe"
$ProjectPath = $Project.FullName
$Cmd = @"
@echo off
call "$DevCmd" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%
msbuild "$ProjectPath" /m /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 /p:WindowsTargetPlatformVersion=10.0
if errorlevel 1 exit /b %errorlevel%
for /r "$Src" %%F in (TRDP.lib) do set TRDP_LIB=%%F
if not defined TRDP_LIB exit /b 20
cl /nologo /O2 /std:c11 /DWIN64 /DMD_SUPPORT=1 /DL_ENDIAN /D_CRT_SECURE_NO_WARNINGS /D_WINSOCK_DEPRECATED_NO_WARNINGS `
  /I"$TrdpDir\src\api" /I"$TrdpDir\src\common" /I"$TrdpDir\src\vos\api" /I"$TrdpDir\src\vos\windows" `
  "$Native" "%TRDP_LIB%" Ws2_32.lib Iphlpapi.lib winmm.lib /Fe:"$Exe"
exit /b %errorlevel%
"@
Set-Content -LiteralPath $Build -Value $Cmd -Encoding ASCII

Write-Host "Building TCNOpen $Version and TauTerm TRDP bridge..."
& cmd.exe /d /c $Build
if ($LASTEXITCODE -ne 0) { throw "TRDP native build failed with exit code $LASTEXITCODE" }
if (-not (Test-Path $Exe)) { throw "TRDP bridge executable was not produced" }

Write-Host "TRDP bridge ready: $Exe"
Write-Host "TCNOpen source remains in: $Src"
Write-Host "TCNOpen TRDP is MPL-2.0; see THIRD_PARTY_LICENSES.md."
