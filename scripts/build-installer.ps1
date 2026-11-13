<#
.SYNOPSIS
  Builds the project_O Windows installer (NSIS, per-user) with the `o` CLI inside.

.DESCRIPTION
  1. signing setup, only when a certificate is configured (see SIGNING below)
  2. npm ci in app/
  3. release `o.exe` -> app/src-tauri/binaries/o-<triple>.exe (scripts/prepare-sidecar.mjs)
  4. tauri build with scripts/installer/tauri.installer.json merged in (adds the sidecar)
  5. signed builds: `signtool verify /pa` on project_O.exe, the o.exe sidecar and the setup
  6. prints the path of the produced *-setup.exe

  Needs Rust (stable) and Node 22.18+. The NSIS toolchain is downloaded by the Tauri CLI
  on first use. See docs/INSTALL.md.

  SIGNING (opt-in, ADR-0038). Set exactly one of these; with none the build is unsigned and
  the script says so and goes on:
    PROJECT_O_SIGN_PFX_BASE64 + PROJECT_O_SIGN_PFX_PASSWORD
        A PFX (base64) imported into Cert:\CurrentUser\My as non-exportable for this build and
        removed again afterwards. Used by release.yml from repository secrets of the same names.
    PROJECT_O_SIGN_THUMBPRINT
        SHA-1 thumbprint of a code-signing certificate already in Cert:\CurrentUser\My
        (e.g. one on a hardware token).
    PROJECT_O_SIGN_COMMAND
        Tauri's bundle.windows.signCommand, `%1` is the file (e.g. a cloud signing tool such as
        Azure Trusted Signing's). The tool and its credentials are the caller's.
  PROJECT_O_SIGN_TIMESTAMP_URL overrides the RFC 3161 timestamp server for the two certificate
  modes (default http://timestamp.digicert.com). Tauri signs project_O.exe, its other binaries,
  the sidecar, the installer and the uninstaller. Secret values are never printed.

.EXAMPLE
  pwsh scripts/build-installer.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $root 'app'
$inActions = $env:GITHUB_ACTIONS -eq 'true'

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

function Find-SignTool {
    $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem -LiteralPath $kits -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+(\.\d+){3}$' } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1
    if (-not $found) { throw 'signing is configured but signtool.exe was not found (install the Windows SDK)' }
    $found
}

# Returns $null for an unsigned build, else @{ Config = <tauri config path>; Added = <cert added to the store or $null> }.
function Enable-Signing {
    $modes = @('PROJECT_O_SIGN_PFX_BASE64', 'PROJECT_O_SIGN_THUMBPRINT', 'PROJECT_O_SIGN_COMMAND') |
        Where-Object { -not [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_)) }
    if (-not $modes) {
        $notice = '서명하지 않은 빌드입니다 (unsigned): PROJECT_O_SIGN_* 서명 설정이 없습니다. SmartScreen이 "알 수 없는 게시자"를 띄웁니다. docs/INSTALL.md 참고.'
        if ($inActions) { Write-Host "::warning::$notice" } else { Write-Warning $notice }
        return $null
    }
    if (@($modes).Count -gt 1) { throw "set only one signing mode, found: $($modes -join ', ')" }

    $windows = [ordered]@{}
    $added = $null
    $timestamp = if ($env:PROJECT_O_SIGN_TIMESTAMP_URL) { $env:PROJECT_O_SIGN_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }
    switch ($modes) {
        'PROJECT_O_SIGN_PFX_BASE64' {
            if ([string]::IsNullOrEmpty($env:PROJECT_O_SIGN_PFX_PASSWORD)) { throw 'PROJECT_O_SIGN_PFX_PASSWORD is not set' }
            $X509 = [Security.Cryptography.X509Certificates.X509Certificate2]
            try {
                $bytes = [Convert]::FromBase64String($env:PROJECT_O_SIGN_PFX_BASE64)
                # Read once without touching the key store, to see whether the certificate is already there.
                $cert = $X509::new($bytes, $env:PROJECT_O_SIGN_PFX_PASSWORD, [Security.Cryptography.X509Certificates.X509KeyStorageFlags]'EphemeralKeySet')
                if (-not $cert.HasPrivateKey) { throw 'no private key' }
                $store = [Security.Cryptography.X509Certificates.X509Store]::new('My', 'CurrentUser')
                $store.Open('ReadWrite')
                try {
                    if (-not $store.Certificates.Find('FindByThumbprint', $cert.Thumbprint, $false).Count) {
                        # Not Exportable: later build steps can ask Windows to sign, but cannot copy the key.
                        $added = $X509::new($bytes, $env:PROJECT_O_SIGN_PFX_PASSWORD, [Security.Cryptography.X509Certificates.X509KeyStorageFlags]'UserKeySet, PersistKeySet')
                        $store.Add($added)
                    }
                } finally { $store.Close() }
            } catch {
                throw 'could not read or import the PFX (check PROJECT_O_SIGN_PFX_BASE64 and PROJECT_O_SIGN_PFX_PASSWORD; it needs a private key)'
            } finally {
                $bytes = $null
            }
            $windows.certificateThumbprint = $cert.Thumbprint
            $windows.digestAlgorithm = 'sha256'
            $windows.timestampUrl = $timestamp
            $windows.tsp = $true  # RFC 3161: signtool /tr <url> /td sha256
            Write-Host "==> signing: PFX certificate $($cert.Thumbprint) ($($cert.Subject))" -ForegroundColor Cyan
        }
        'PROJECT_O_SIGN_THUMBPRINT' {
            $thumb = $env:PROJECT_O_SIGN_THUMBPRINT -replace '\s', ''
            if ($thumb -notmatch '^[0-9A-Fa-f]{40}$') { throw 'PROJECT_O_SIGN_THUMBPRINT must be a 40-digit hex SHA-1 thumbprint' }
            if (-not (Test-Path -LiteralPath "Cert:\CurrentUser\My\$thumb")) { throw "certificate $thumb is not in Cert:\CurrentUser\My" }
            $windows.certificateThumbprint = $thumb.ToUpperInvariant()
            $windows.digestAlgorithm = 'sha256'
            $windows.timestampUrl = $timestamp
            $windows.tsp = $true  # RFC 3161: signtool /tr <url> /td sha256
            Write-Host "==> signing: certificate $thumb from Cert:\CurrentUser\My" -ForegroundColor Cyan
        }
        'PROJECT_O_SIGN_COMMAND' {
            # The command itself is not printed: it may name accounts or endpoints.
            $windows.signCommand = $env:PROJECT_O_SIGN_COMMAND
            Write-Host '==> signing: PROJECT_O_SIGN_COMMAND (bundle.windows.signCommand)' -ForegroundColor Cyan
        }
    }
    # The secrets are no longer needed: npm and cargo build scripts that run next must not see them.
    foreach ($name in 'PROJECT_O_SIGN_PFX_BASE64', 'PROJECT_O_SIGN_PFX_PASSWORD') { [Environment]::SetEnvironmentVariable($name, $null) }

    $config = Join-Path ([IO.Path]::GetTempPath()) ("project_O-sign-{0}.json" -f [Guid]::NewGuid().ToString('N'))
    # No BOM: Tauri's JSON reader refuses one (Windows PowerShell's `-Encoding utf8` writes it).
    [IO.File]::WriteAllText($config, (@{ bundle = @{ windows = $windows } } | ConvertTo-Json -Depth 4), [Text.UTF8Encoding]::new($false))
    @{ Config = $config; Added = $added }
}

function Assert-Signed {
    param([string]$SignTool, [string]$Path, [switch]$WarnOnly)
    Write-Host "==> signtool verify /pa $Path" -ForegroundColor Cyan
    & $SignTool verify /pa $Path | Out-Host
    if ($LASTEXITCODE -eq 0) { return }
    if ($WarnOnly) { Write-Warning "not signed or not trusted: $Path"; $global:LASTEXITCODE = 0; return }
    throw "signature check failed: $Path"
}

$started = Get-Date
$signing = $null
try {
    $signing = Enable-Signing
    $signTool = if ($signing) { Find-SignTool } else { $null }

    # Like the CI checks: no package install scripts, and cargo takes only what Cargo.lock names.
    Invoke-Step 'npm ci (app)' 'npm' @('ci', '--ignore-scripts') $app
    $env:PROJECT_O_LOCKED = '1'
    Invoke-Step 'release o.exe -> sidecar' 'npm' @('run', 'sidecar') $app
    # beforeBuildCommand in the installer config prepares the sidecar again; cargo makes that a no-op.
    $tauriArgs = @('run', 'tauri', '--', 'build', '--config', '../scripts/installer/tauri.installer.json')
    if ($signing) { $tauriArgs += @('--config', $signing.Config) }
    Invoke-Step 'tauri build (nsis)' 'npm' ($tauriArgs + @('--bundles', 'nsis', '--', '--locked')) $app

    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $setup = Get-ChildItem (Join-Path $targetDir 'release/bundle/nsis') -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -ge $started } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $setup) { throw "no *-setup.exe was produced under $targetDir/release/bundle/nsis" }

    if ($signing) {
        $sidecar = Get-ChildItem (Join-Path $app 'src-tauri/binaries') -Filter 'o-*.exe' |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $sidecar) { throw 'the o.exe sidecar was not found under app/src-tauri/binaries' }
        Assert-Signed $signTool (Join-Path $targetDir 'release/project_O.exe')
        Assert-Signed $signTool $sidecar.FullName
        Assert-Signed $signTool $setup.FullName
        Assert-Signed $signTool (Join-Path $targetDir 'release/o-uninstall-data.exe') -WarnOnly
        Write-Host '==> signed: project_O.exe, o.exe, setup' -ForegroundColor Green
    }

    Write-Host ("==> installer: {0} ({1:N1} MB)" -f $setup.FullName, ($setup.Length / 1MB)) -ForegroundColor Green
    $setup.FullName
} finally {
    if ($signing) {
        Remove-Item -LiteralPath $signing.Config -Force -ErrorAction SilentlyContinue
        if ($signing.Added) {
            $store = [Security.Cryptography.X509Certificates.X509Store]::new('My', 'CurrentUser')
            $store.Open('ReadWrite')
            try { $store.Remove($signing.Added) } finally { $store.Close() }
            # The imported key container stays on disk unless it is removed too.
            try {
                $key = $signing.Added.GetRSAPrivateKey()
                if (-not $key) { $key = $signing.Added.GetECDsaPrivateKey() }
                if ($key -is [Security.Cryptography.RSACng] -or $key -is [Security.Cryptography.ECDsaCng]) { $key.Key.Delete() }
                elseif ($key -is [Security.Cryptography.RSACryptoServiceProvider]) { $key.PersistKeyInCsp = $false; $key.Clear() }
            } catch {
                Write-Warning "could not remove the imported signing key: $($_.Exception.Message)"
            }
        }
    }
}
