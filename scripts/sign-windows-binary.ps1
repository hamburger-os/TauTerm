param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [string]$Thumbprint = $env:TAUTERM_WINDOWS_CERT_THUMBPRINT,
    [string]$TimestampUrl = $env:TAUTERM_WINDOWS_TIMESTAMP_URL
)

$ErrorActionPreference = "Stop"
$required = $env:TAUTERM_REQUIRE_WINDOWS_CODE_SIGNING -eq "1"

if ([string]::IsNullOrWhiteSpace($Thumbprint) -or [string]::IsNullOrWhiteSpace($TimestampUrl)) {
    if ($required) {
        throw "Windows release signing is required but certificate thumbprint/timestamp URL is unavailable."
    }
    Write-Host "Windows code signing is not configured; leaving local binary unsigned: $FilePath"
    exit 0
}

if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
    throw "Windows binary to sign was not found: $FilePath"
}

$signToolCommand = Get-Command "signtool.exe" -ErrorAction SilentlyContinue
if ($null -ne $signToolCommand) {
    $signTool = $signToolCommand.Source
}
else {
    $kitsRoots = @(
        (Join-Path $env:ProgramFiles "Windows Kits\10\bin"),
        (Join-Path ([Environment]::GetEnvironmentVariable("ProgramFiles(x86)")) "Windows Kits\10\bin")
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and (Test-Path -LiteralPath $_) }

    $signTool = $kitsRoots |
        ForEach-Object { Get-ChildItem -Path $_ -Filter "signtool.exe" -Recurse -ErrorAction SilentlyContinue } |
        Where-Object { $_.FullName -match "\\\\x64\\\\signtool\\.exe$" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if ([string]::IsNullOrWhiteSpace($signTool)) {
    throw "signtool.exe was not found in PATH or the Windows SDK."
}

& $signTool sign /sha1 $Thumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 /d "TauTerm" $FilePath
if ($LASTEXITCODE -ne 0) {
    throw "signtool failed to sign $FilePath (exit $LASTEXITCODE)."
}

& $signTool verify /pa /v $FilePath
if ($LASTEXITCODE -ne 0) {
    throw "signtool verification failed after signing $FilePath (exit $LASTEXITCODE)."
}

Write-Host "Authenticode signed: $FilePath"
