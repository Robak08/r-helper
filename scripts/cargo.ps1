param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = 'Stop'

$isReleaseBuild = $false
for ($i = 0; $i -lt $CargoArgs.Count; $i++) {
  if ($CargoArgs[$i] -eq 'build' -and ($CargoArgs -contains '--release')) {
    $isReleaseBuild = $true
    break
  }
}

& cargo @CargoArgs
if ($LASTEXITCODE -ne 0) {
  exit $LASTEXITCODE
}

if ($isReleaseBuild) {
  & (Join-Path $PSScriptRoot 'copy-release.ps1')
}
