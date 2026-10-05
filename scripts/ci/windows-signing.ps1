$ErrorActionPreference = 'Stop'

if (-not $env:WINDOWS_CERTIFICATE -and -not $env:WINDOWS_CERTIFICATE_PASSWORD) {
    Write-Output 'No Windows certificate configured; building unsigned installers.'
    exit 0
}
if (-not $env:WINDOWS_CERTIFICATE -or -not $env:WINDOWS_CERTIFICATE_PASSWORD) {
    throw 'Configure both WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD.'
}

$certificatePath = Join-Path $env:RUNNER_TEMP 'cultivator-signing.pfx'
[System.IO.File]::WriteAllBytes($certificatePath, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
try {
    $password = ConvertTo-SecureString $env:WINDOWS_CERTIFICATE_PASSWORD -AsPlainText -Force
    $certificate = Import-PfxCertificate -FilePath $certificatePath -CertStoreLocation 'Cert:\CurrentUser\My' -Password $password
    $signingCertificate = $certificate | Where-Object { $_.HasPrivateKey } | Select-Object -First 1
    if (-not $signingCertificate) {
        throw 'The Windows signing certificate has no private key.'
    }
    $configuration = @{
        bundle = @{
            windows = @{
                certificateThumbprint = $signingCertificate.Thumbprint
                digestAlgorithm = 'sha256'
                timestampUrl = 'http://timestamp.digicert.com'
            }
        }
    }
    $configuration | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath '.cache/release-signing.json' -Encoding utf8
} finally {
    Remove-Item -LiteralPath $certificatePath -Force
}
