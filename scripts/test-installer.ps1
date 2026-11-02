# No GUI launch, no real app-data deletion. Keep the fixture and evidence for review.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Setup)
$ErrorActionPreference = 'Stop'

function Assert-That([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
$setupPath = (Resolve-Path -LiteralPath $Setup).Path
Assert-That ((Split-Path -Leaf $setupPath) -match '^project_O_\d+\.\d+\.\d+_x64-setup\.exe$') 'Unexpected installer name'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O'
Assert-That (-not (Test-Path -LiteralPath $uninstallKey)) 'An existing installation is registered; leave it intact'
$shortcuts = @(
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'project_O.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) 'project_O.lnk')
)
foreach ($shortcut in $shortcuts) { Assert-That (-not (Test-Path -LiteralPath $shortcut)) 'An existing shortcut must be preserved' }

$id = [Guid]::NewGuid().ToString('N')
$fixture = Join-Path $env:LOCALAPPDATA "project_O-uninstall-tests\$id"
$installDir = Join-Path $env:TEMP "project_O-installer-$id\Installed app"
Assert-That ([IO.Path]::GetFullPath($fixture).StartsWith([IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'project_O-uninstall-tests')) + '\')) 'Fixture containment'
Assert-That ([IO.Path]::GetFullPath($installDir).StartsWith([IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\project_O-installer-')) 'Install containment'
$data = @('project_O', 'project_O-agents', 'local.project-o.desktop') | ForEach-Object { Join-Path $fixture "Local\$_" }
$data += Join-Path $fixture 'Roaming\local.project-o.desktop'
$realData = @('project_O', 'project_O-agents', 'local.project-o.desktop') | ForEach-Object { Join-Path $env:LOCALAPPDATA $_ }
$realData += Join-Path $env:APPDATA 'local.project-o.desktop'
$existingRealData = @($realData | Where-Object { Test-Path -LiteralPath $_ })
foreach ($path in $data) {
    New-Item -ItemType Directory -Path (Join-Path $path 'attachments') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $path 'attachments\fake.txt'), 'fake')
}
$preserved = @('.claude', '.codex', '.gemini', '.agents', 'repositories', 'projects') | ForEach-Object { Join-Path $fixture "Local\$_" }
foreach ($path in $preserved) {
    New-Item -ItemType Directory -Path $path -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $path 'keep.txt'), 'keep')
}
New-Item -ItemType Junction -Path (Join-Path $data[0] 'linked-project') -Target $preserved[-1] | Out-Null
$remembered = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Project O\project_O')
$hadMemory = $null -ne $remembered
$oldLocation = if ($remembered) { $remembered.GetValue('') } else { $null }
if ($remembered) { $remembered.Close() }

function Install-Fixture {
    $process = Start-Process -FilePath $setupPath -ArgumentList "/S /D=$installDir" -WindowStyle Hidden -Wait -PassThru
    Assert-That ($process.ExitCode -eq 0) "Install exit: $($process.ExitCode)"
    foreach ($name in @('project_O.exe', 'o.exe', 'o-uninstall-data.exe', 'uninstall.exe')) {
        Assert-That (Test-Path -LiteralPath (Join-Path $installDir $name)) "Missing installed file: $name"
    }
    foreach ($shortcut in $shortcuts) { Assert-That (Test-Path -LiteralPath $shortcut) 'Missing shortcut' }
    $entry = Get-ItemProperty -LiteralPath $uninstallKey
    Assert-That ($entry.UninstallString.Trim('"') -eq (Join-Path $installDir 'uninstall.exe')) 'Uninstaller registry path'
}
function Assert-Uninstalled {
    Assert-That (-not (Test-Path -LiteralPath $installDir)) 'App folder remains'
    Assert-That (-not (Test-Path -LiteralPath $uninstallKey)) 'Uninstall registry key remains'
    foreach ($shortcut in $shortcuts) { Assert-That (-not (Test-Path -LiteralPath $shortcut)) 'Shortcut remains' }
    foreach ($path in $existingRealData) { Assert-That (Test-Path -LiteralPath $path) 'Real app data was removed' }
}

try {
    Install-Fixture
    $helper = Join-Path $installDir 'o-uninstall-data.exe'
    $unsafeId = [Guid]::NewGuid().ToString('N')
    $unsafeRoot = Join-Path $env:LOCALAPPDATA "project_O-uninstall-tests\$unsafeId\Local"
    New-Item -ItemType Directory -Path $unsafeRoot -Force | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $unsafeRoot 'project_O') -Target $preserved[-1] | Out-Null
    $rejected = Start-Process -FilePath $helper -ArgumentList "--test-id $unsafeId" -WindowStyle Hidden -Wait -PassThru -RedirectStandardError (Join-Path $fixture 'link-rejection.txt')
    Assert-That ($rejected.ExitCode -ne 0) 'A linked data root must be refused'
    Assert-That ([IO.File]::ReadAllText((Join-Path $preserved[-1] 'keep.txt')) -eq 'keep') 'Linked project was modified'
    $holdScript = Join-Path $fixture 'hold-binary.ps1'
    @'
param([string]$Binary, [string]$Ready)
$handle = [IO.File]::Open($Binary, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
[IO.File]::WriteAllText($Ready, 'ready')
try { Start-Sleep -Seconds 120 } finally { $handle.Dispose() }
'@ | Set-Content -LiteralPath $holdScript -Encoding utf8
    $ready = Join-Path $fixture 'lock-ready.txt'
    $lockedBinary = Join-Path $installDir 'o.exe'
    $holder = Start-Process -FilePath (Get-Command pwsh).Source -ArgumentList "-NoProfile -File `"$holdScript`" `"$lockedBinary`" `"$ready`"" -WindowStyle Hidden -PassThru
    $deadline = (Get-Date).AddSeconds(15)
    while (-not (Test-Path -LiteralPath $ready) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
    Assert-That (Test-Path -LiteralPath $ready) 'Lock test did not start'
    # Exact Settings command plus PassThru, to assert the exit code.
    $keep = Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
    Assert-That ($keep.ExitCode -eq 0) "Keep-data exit: $($keep.ExitCode)"
    Assert-That ($holder.WaitForExit(5000)) 'The process locking bundled o.exe is still running'
    Assert-Uninstalled
    foreach ($path in $data) { Assert-That (Test-Path -LiteralPath (Join-Path $path 'attachments\fake.txt')) 'Keep-data removed a fixture' }
    Write-Output 'PASS keep data: app folder, shortcuts and uninstall key removed; data retained'

    Install-Fixture
    $purge = Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList "/S /PURGE /TESTDATA=$id" -WindowStyle Hidden -Wait -PassThru
    Assert-That ($purge.ExitCode -eq 0) "Purge exit: $($purge.ExitCode)"
    Assert-Uninstalled
    foreach ($path in $data) { Assert-That (-not (Test-Path -LiteralPath $path)) 'Purge left a fixture folder' }
    foreach ($path in $preserved) { Assert-That ([IO.File]::ReadAllText((Join-Path $path 'keep.txt')) -eq 'keep') 'Purge touched an unrelated folder or followed a junction' }
    Write-Output 'PASS purge: only four fixed fixture folders removed; normal CLI/project/repo sentinels and junction target preserved'

    Install-Fixture
    $beforePlans = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'project_O-purge-plan-*' | Select-Object -ExpandProperty FullName)
    $dry = Start-Process -FilePath ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\project_O').UninstallString.Trim('"')) -ArgumentList '/S /PURGE /DATADRYRUN' -WindowStyle Hidden -Wait -PassThru
    Assert-That ($dry.ExitCode -eq 0) "Dry-run exit: $($dry.ExitCode)"
    Assert-Uninstalled
    $plans = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'project_O-purge-plan-*' | Where-Object { $_.FullName -notin $beforePlans })
    Assert-That ($plans.Count -eq 1) 'Expected exactly one dry-run plan'
    $plan = Get-Content -LiteralPath (Join-Path $plans[0].FullName 'plan.json') -Raw | ConvertFrom-Json
    Assert-That ($plan.dryRun -eq $true) 'Dry-run marker'
    Assert-That ((Compare-Object @($plan.folders | Sort-Object) @($realData | Sort-Object)).Count -eq 0) 'Dry-run paths differ from the four known folders'
    $evidence = [ordered]@{ setup = $setupPath; install = $installDir; fixture = $fixture; keepExit = $keep.ExitCode; purgeExit = $purge.ExitCode; dryRunExit = $dry.ExitCode; dataPlan = $plan; checks = @('installed files', 'quoted registry path', 'keep data', 'purge fake data', 'shortcut cleanup', 'uninstall registry cleanup', 'locked sidecar released', 'linked root refused', 'junction target retained', 'normal CLI and repositories retained', 'real data untouched') }
    $report = Join-Path $fixture 'evidence.json'
    $evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $report -Encoding utf8
    Write-Output "PASS dry-run: real data untouched; evidence=$report"
} finally {
    # Restore only the installation-location memory changed by these test installs.
    if ($hadMemory) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Project O\project_O')
        if ($null -ne $oldLocation) { $key.SetValue('', $oldLocation) } else { $key.DeleteValue('', $false) }
        $key.Close()
    } elseif (-not (Test-Path -LiteralPath $uninstallKey)) {
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKey('Software\Project O\project_O', $false)
    }
}
