param(
    [string]$OutputDirectory = "src-tauri\target\pyembed"
)

$ErrorActionPreference = "Stop"

$pyoxidizer = Get-Command pyoxidizer -ErrorAction SilentlyContinue
if (-not $pyoxidizer) {
    throw "pyoxidizer was not found. Install it in your active Python environment with: python -m pip install pyoxidizer"
}

$resolvedOutputDirectory = Join-Path (Get-Location) $OutputDirectory
New-Item -ItemType Directory -Force -Path $resolvedOutputDirectory | Out-Null

& $pyoxidizer.Source generate-python-embedding-artifacts $resolvedOutputDirectory

$configFile = Join-Path $resolvedOutputDirectory "pyo3-build-config-file.txt"
if (-not (Test-Path $configFile)) {
    throw "PyOxidizer did not create the expected PyO3 config file: $configFile"
}

Write-Host "Generated PyO3 embedding artifacts at $resolvedOutputDirectory"
Write-Host "For an embedded build, set PYO3_CONFIG_FILE to: $configFile"
