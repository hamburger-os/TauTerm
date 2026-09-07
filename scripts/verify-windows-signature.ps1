param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [string]$ExpectedThumbprint = $env:TAUTERM_WINDOWS_CERT_THUMBPRINT
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
    throw "Signed file was not found: $FilePath"
}

$signature = Get-AuthenticodeSignature -LiteralPath $FilePath
if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
    throw "Authenticode signature is not valid for $FilePath (status: $($signature.Status); message: $($signature.StatusMessage))."
}
if ($null -eq $signature.SignerCertificate) {
    throw "Authenticode signer certificate is missing for $FilePath."
}
if ($null -eq $signature.TimeStamperCertificate) {
    throw "Authenticode timestamp is missing for $FilePath."
}

if (-not [string]::IsNullOrWhiteSpace($ExpectedThumbprint)) {
    $actual = ($signature.SignerCertificate.Thumbprint -replace "\\s", "").ToUpperInvariant()
    $expected = ($ExpectedThumbprint -replace "\\s", "").ToUpperInvariant()
    if ($actual -ne $expected) {
        throw "Unexpected Authenticode signer for $FilePath."
    }
}

Write-Host "Valid Authenticode signature: $FilePath"
