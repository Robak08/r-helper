$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

# package-release.ps1 copies explicitly after build; skip the build.rs hook.
$env:RHELPER_SKIP_RELEASE_PACKAGING = '1'

cargo build --release
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

& (Join-Path $PSScriptRoot 'copy-release.ps1')
