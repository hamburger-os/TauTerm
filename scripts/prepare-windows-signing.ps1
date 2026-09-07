param(
    [Parameter(Mandatory = $true)][string]$PfxBase64,
    [Parameter(Mandatory = $true)][string]$PfxPassword,
    [Parameter(Mandatory = $true)][string]$TimestampUrl
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($PfxBase64)) {
    throw "WINDOWS_CODE_SIGNING_PFX_BASE64 is required for a Windows release."
}
if ([string]::IsNullOrWhiteSpace($PfxPassword)) {
    throw "WINDOWS_CODE_SIGNING_PFX_PASSWORD is required for a Windows release."
}
if ([string]::IsNullOrWhiteSpace($TimestampUrl)) {
    throw "WINDOWS_CODE_SIGNING_TIMESTAMP_URL is required for a Windows release."
}

$pfxPath = Join-Path $env:RUNNER_TEMP "tauterm-code-signing.pfx"
$configPath = Join-Path $env:RUNNER_TEMP "tauterm-windows-signing.json"

try {
    $normalized = ($PfxBase64 -replace "\\s", "")
    $bytes = [Convert]::FromBase64String($normalized)
    [IO.File]::WriteAllBytes($pfxPath, $bytes)

    $securePassword = ConvertTo-SecureString -String $PfxPassword -AsPlainText -Force
    $imported = @(Import-PfxCertificate -FilePath $pfxPath -CertStoreLocation "Cert:\\CurrentUser\\My" -Password $securePassword)
    $certificate = $imported | Where-Object { $_.HasPrivateKey } | Select-Object -First 1
    if ($null -eq $certificate) {
        throw "The imported Windows code-signing certificate does not contain a private key."
    }

    $thumbprint = ($certificate.Thumbprint -replace "\\s", "").ToUpperInvariant()
    $signingConfig = @{
        bundle = @{
            windows = @{
                certificateThumbprint = $thumbprint
                digestAlgorithm = "sha256"
                timestampUrl = $TimestampUrl
            }
        }
    }
    $json = $signingConfig | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText($configPath, $json, [Text.UTF8Encoding]::new($false))

    Add-Content -Path $env:GITHUB_ENV -Value "TAUTERM_REQUIRE_WINDOWS_CODE_SIGNING=1"
    Add-Content -Path $env:GITHUB_ENV -Value "TAUTERM_WINDOWS_CERT_THUMBPRINT=$thumbprint"
    Add-Content -Path $env:GITHUB_ENV -Value "TAUTERM_WINDOWS_TIMESTAMP_URL=$TimestampUrl"
    Add-Content -Path $env:GITHUB_ENV -Value "TAUTERM_TAURI_SIGNING_CONFIG=$configPath"

    Write-Host "Windows Authenticode certificate imported and release signing config prepared."
}
finally {
    if (Test-Path -LiteralPath $pfxPath) {
        Remove-Item -LiteralPath $pfxPath -Force
    }
}
