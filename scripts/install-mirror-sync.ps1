# Registers a hidden scheduled task that keeps a mirror checkout up to date
# every few minutes with `o sync-mirror`. The team's mirror follows test_2,
# where merged work is tried together (test_1 and main are allowed too).
# Run once per PC, after installing o (cargo install --path crates/o-cli):
#   pwsh scripts/install-mirror-sync.ps1 -Mirror C:/Users/me/orca/workspaces/project_O/test_2
param(
  [Parameter(Mandatory = $true)][string]$Mirror,
  [int]$Minutes = 5
)

$branch = (git -C $Mirror branch --show-current).Trim()
# Same list as MIRROR_BRANCHES in crates/o-core/src/sys/mirror.rs (o sync-mirror refuses others).
if ($branch -notin @('test_1', 'test_1_hotfix', 'test_2', 'main_hotfix', 'main')) { throw "보호 브랜치 거울이 아닙니다: $branch" }
$origin = (git -C $Mirror remote get-url origin).Trim()
if ($LASTEXITCODE -ne 0 -or $origin -notmatch '^(https://github\.com/chan12392/project_O|git@github\.com:chan12392/project_O|ssh://git@github\.com/chan12392/project_O)(\.git)?/?$') { throw "origin이 공식 project_O 저장소가 아닙니다: $origin" }

# The installed o.exe lives outside the repo, so a mirror that is behind still syncs.
$o = (Get-Command o.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
# conhost --headless keeps the console process from opening a window.
$action = New-ScheduledTaskAction -Execute "$env:WINDIR\System32\conhost.exe" `
  -Argument "--headless `"$o`" sync-mirror `"$Mirror`""
$trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) `
  -RepetitionInterval (New-TimeSpan -Minutes $Minutes)
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -DontStopIfGoingOnBatteries `
  -AllowStartIfOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 2) -MultipleInstances IgnoreNew

$name = "project_O mirror sync ($branch)"
Register-ScheduledTask -TaskName $name -Action $action -Trigger $trigger -Settings $settings `
  -Description "project_O $branch 거울 폴더를 origin/$branch로 fast-forward" -Force | Out-Null
Write-Output "등록: $name → $Mirror (${Minutes}분마다, $o). 로그: $env:LOCALAPPDATA\project_O\mirror-sync.log"
