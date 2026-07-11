param(
    [int]$WaitForBuildScriptPid = 0
)

$ErrorActionPreference = 'Stop'

function Wait-ForCargoBuild {
    param([int]$StartPid)

    if ($StartPid -le 0) {
        return
    }

    $current = $StartPid
    $cargoPid = $null

    for ($i = 0; $i -lt 32; $i++) {
        $proc = Get-CimInstance Win32_Process -Filter "ProcessId=$current" -ErrorAction SilentlyContinue
        if (-not $proc) {
            break
        }

        if ($proc.Name -ieq 'cargo.exe') {
            $cargoPid = $current
            break
        }

        if ($proc.ParentProcessId -le 0) {
            break
        }

        $current = $proc.ParentProcessId
    }

    if ($cargoPid) {
        Wait-Process -Id $cargoPid -ErrorAction SilentlyContinue
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

Wait-ForCargoBuild -StartPid $WaitForBuildScriptPid
Start-Sleep -Milliseconds 300

$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$pkg = $metadata.packages | Where-Object { $_.name -eq 'r-helper' } | Select-Object -First 1
if (-not $pkg) {
    throw "Could not find r-helper package in cargo metadata"
}

$versionSlug = $pkg.version -replace '\.', '_'
$destName = "rhelper-$versionSlug.exe"
$distDir = Join-Path $repoRoot 'dist'
$source = Join-Path $repoRoot 'target\release\rhelper.exe'
$dest = Join-Path $distDir $destName

if (-not (Test-Path $source)) {
    throw "Release binary not found: $source"
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
Copy-Item $source $dest -Force

Write-Host "Packaged: $dest"
