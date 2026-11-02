<#
.SYNOPSIS
  Builds the project_O Windows installer (NSIS, per-user) with the `o` CLI inside.

.DESCRIPTION
  1. npm ci in app/
  2. release `o.exe` -> app/src-tauri/binaries/o-<triple>.exe (scripts/prepare-sidecar.mjs)
  3. tauri build with scripts/installer/tauri.installer.json merged in (adds the sidecar)
  4. prints the path of the produced *-setup.exe

  Needs Rust (stable) and Node 22.18+. The NSIS toolchain is downloaded by the Tauri CLI
  on first use. See docs/INSTALL.md.

.EXAMPLE
  pwsh scripts/build-installer.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $root 'app'

function Invoke-Step {
    param([string]$Title, [string]$Exe, [string[]]$Arguments, [string]$Dir)
    Write-Host "==> $Title" -ForegroundColor Cyan
    Push-Location $Dir
    try {
        & $Exe @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Title failed (exit $LASTEXITCODE)" }
    } finally {
        Pop-Location
    }
}

$started = Get-Date
Invoke-Step 'npm ci (app)' 'npm' @('ci') $app
Invoke-Step 'release o.exe -> sidecar' 'npm' @('run', 'sidecar') $app
# beforeBuildCommand in the installer config prepares the sidecar again; cargo makes that a no-op.
Invoke-Step 'tauri build (nsis)' 'npm' @('run', 'tauri', '--', 'build', '--config', '../scripts/installer/tauri.installer.json', '--bundles', 'nsis') $app

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
$setup = Get-ChildItem (Join-Path $targetDir 'release/bundle/nsis') -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -ge $started } |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $setup) { throw "no *-setup.exe was produced under $targetDir/release/bundle/nsis" }

Write-Host ("==> installer: {0} ({1:N1} MB)" -f $setup.FullName, ($setup.Length / 1MB)) -ForegroundColor Green
$setup.FullName
