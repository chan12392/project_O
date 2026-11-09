#Requires -Version 7.2
<#
.SYNOPSIS
  Real-agent release gate: the owner runs this on their own PC before a `main` release (ADR-0038).

.DESCRIPTION
  Uses the real, logged-in Claude Code and Codex CLIs through `o`, so it spends subscription
  usage (a few small cards). CI cannot do this: it has no logged-in agents.

  Automatic checks
    1. preconditions: not inside an agent (O_AGENT_ROLE / O_CARD_ID), `o about`, git/gh on PATH,
       claude and codex logged in (`o agents doctor --json`, project-o profile)
    2. a throwaway git project OUTSIDE the home folder (default <system drive>\project_O-release-gate)
    3. `o dispatch` of a 2-card plan (claude + codex) -> both cards and the run end `done`
    4. `o dispatch` of a longer card, `o stop` once it started -> card and run end `stopped`
    5. `o followup` on card `a` of run 3 -> the follow-up run ends `done`
    6. `o resources --json` -> no resource lease left for these runs (recovery)
    7. update path, read-only: the release the app's updater would offer (same rules as
       app/src-tauri/src/commands/update.rs), its commit is in test_1, and the downloaded setup
       matches GitHub's size and SHA-256. Nothing is installed; the Authenticode state is shown.
  Manual checks (a person must do these; `o lead decide` is refused inside agents on purpose)
    M1-M3 lead approval, lead plan "실행", the update screen — asked as y/n at the end.

  Prints a PASS/FAIL table. Exit 0 = PASS, 1 = a check failed, 2 = automatic checks passed but
  manual checks were skipped (-SkipManual or a non-interactive console).
  The throwaway project, O's run records and the downloaded setup are kept for review.

.PARAMETER O
  o.exe to test. Default: the installed app's o.exe (%LOCALAPPDATA%\Programs\project_O\o.exe),
  else `o` on PATH.

.PARAMETER Root
  Parent folder for the throwaway project. Must be outside the home folder and the O state folder.

.PARAMETER AgentProfile
  project-o (default, the clean profiles) or own (your usual CLI settings): `o dispatch --profile`.

.EXAMPLE
  pwsh scripts/release-gate.ps1
.EXAMPLE
  pwsh scripts/release-gate.ps1 -O .\target\release\o.exe -TimeoutMinutes 30
#>
[CmdletBinding()]
param(
    [string]$O,
    [string]$Root = (Join-Path $env:SystemDrive 'project_O-release-gate'),
    [ValidateSet('project-o', 'own')][string]$AgentProfile = 'project-o',
    [ValidateRange(1, 120)][int]$TimeoutMinutes = 20,
    [switch]$SkipManual
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

$Repository = 'chan12392/project_O'
$results = [System.Collections.Generic.List[object]]::new()
$state = @{}

function Add-Result([string]$Name, [string]$Status, [string]$Detail) {
    $results.Add([pscustomobject]@{ Check = $Name; Status = $Status; Detail = $Detail })
    $color = switch ($Status) { 'PASS' { 'Green' } 'FAIL' { 'Red' } default { 'Yellow' } }
    Write-Host ("[{0}] {1} — {2}" -f $Status, $Name, $Detail) -ForegroundColor $color
}

# Runs one automatic check: PASS with the returned text, FAIL with the thrown message.
function Invoke-Check([string]$Name, [scriptblock]$Body) {
    Write-Host "==> $Name" -ForegroundColor Cyan
    try {
        $detail = (& $Body | Out-String).Trim()
        Add-Result $Name 'PASS' $detail
        $true
    } catch {
        Add-Result $Name 'FAIL' $_.Exception.Message
        $false
    }
}

function Invoke-O {
    param([string[]]$Arguments, [string]$InputText)
    $all = if ($PSBoundParameters.ContainsKey('InputText')) { $InputText | & $script:OExe @Arguments 2>&1 } else { & $script:OExe @Arguments 2>&1 }
    $code = $LASTEXITCODE
    $out = @($all | Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] }) -join "`n"
    $err = @($all | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] } | ForEach-Object { $_.ToString() }) -join "`n"
    [pscustomobject]@{ Code = $code; Out = $out; Err = $err }
}

function Assert-O([pscustomobject]$Result, [string]$What) {
    if ($Result.Code -ne 0) { throw "$What failed (exit $($Result.Code)): $($Result.Err) $($Result.Out)".Trim() }
}

function ConvertFrom-JsonLine([string]$Text) {
    $line = $Text -split "`r?`n" | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1
    if (-not $line) { throw "no JSON in: $Text" }
    $line | ConvertFrom-Json
}

function Get-RunEvents([string]$RunId) {
    $r = Invoke-O @('events', $RunId)
    Assert-O $r "o events $RunId"
    @($r.Out -split "`r?`n" | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
}

# Polls `o events` until run.end (or $Until is true); returns @{ Status; Cards = @{ id = status }; Events }.
function Wait-Run([string]$RunId, [int]$Seconds, [scriptblock]$Until) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ($true) {
        $events = Get-RunEvents $RunId
        $cards = @{}
        foreach ($e in $events | Where-Object { $_.t -eq 'card.end' }) { $cards[$e.cardId] = $e.status }
        $end = $events | Where-Object { $_.t -eq 'run.end' } | Select-Object -Last 1
        $view = @{ Status = $end.status; Cards = $cards; Events = $events }
        if ($end -or ($Until -and (& $Until $events))) { return $view }
        if ((Get-Date) -gt $deadline) {
            $what = if ($Until) { 'did not reach the expected point (e.g. card start; it may be queued)' } else { 'did not finish' }
            throw "run $RunId $what within $Seconds s (o status $RunId)"
        }
        Start-Sleep -Seconds 5
    }
}

function Format-Cards([hashtable]$Cards) { ($Cards.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ', ' }

function New-Plan([string]$Title, [object[]]$Cards) {
    $path = Join-Path $state.Work ("plan-{0}.json" -f ($Title -replace '[^a-z0-9]+', '-'))
    @{ title = $Title; project = $state.Project; cards = $Cards } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path -Encoding utf8
    $path
}

function Start-Plan([string]$Plan) {
    $r = Invoke-O @('dispatch', '--profile', $AgentProfile, '--mode', 'normal', $Plan)
    Assert-O $r 'o dispatch'
    (ConvertFrom-JsonLine $r.Out).runId
}

$timeout = $TimeoutMinutes * 60
Write-Host "project_O release gate — real Claude and Codex cards (spends subscription usage)" -ForegroundColor Cyan

# 1. Preconditions. A failure here ends the gate: nothing below could run.
$ready = Invoke-Check '사전 조건' {
    if ($env:O_AGENT_ROLE -or $env:O_CARD_ID) { throw 'an agent environment (O_AGENT_ROLE/O_CARD_ID) — run this yourself in a normal terminal' }
    $script:OExe = if ($O) { (Resolve-Path -LiteralPath $O).Path }
        elseif (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs\project_O\o.exe')) { Join-Path $env:LOCALAPPDATA 'Programs\project_O\o.exe' }
        else { (Get-Command o -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source }
    foreach ($tool in 'git', 'gh') { if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is not on PATH" } }
    $about = Invoke-O @('about', '--json')
    Assert-O $about 'o about --json'
    $info = ConvertFrom-JsonLine $about.Out
    $state.Version = $info.version
    if ($AgentProfile -eq 'project-o') {
        $doctor = Invoke-O @('agents', 'doctor', '--json')  # exit 1 only means profile issues; read the JSON
        $report = ConvertFrom-JsonLine $doctor.Out
        foreach ($provider in 'claude', 'codex') {
            $p = $report.profiles | Where-Object { $_.provider -eq $provider }
            if ($p.loggedIn -ne $true) { throw "$provider is not logged in for the project-o profile: $($p.detail) (o agents login $provider)" }
            foreach ($issue in $p.issues) { Write-Warning "$provider profile: $issue" }
            if ($p.apiKeyEnv) { Write-Warning "${provider}: API key environment variables set ($($p.apiKeyEnv -join ', ')) — usage may bill the API" }
        }
    } else {
        foreach ($tool in 'claude', 'codex') { if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is not on PATH" } }
    }
    "o $($info.version) at $($script:OExe), profile $AgentProfile"
}
if (-not $ready) {
    $results | Format-Table -AutoSize -Wrap | Out-Host
    Write-Host 'RESULT: FAIL (preconditions)' -ForegroundColor Red
    exit 1
}

# 2. A throwaway git project outside the home folder (Claude reads instruction files above its start folder).
$ready = Invoke-Check '임시 프로젝트 (홈 폴더 밖)' {
    $full = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Root).TrimEnd('\')
    $userHome = [IO.Path]::GetFullPath($HOME).TrimEnd('\')
    $oState = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'project_O')).TrimEnd('\')
    foreach ($forbidden in $userHome, $oState) {
        if ($full -eq $forbidden -or $full.StartsWith("$forbidden\", [StringComparison]::OrdinalIgnoreCase) -or $forbidden.StartsWith("$full\", [StringComparison]::OrdinalIgnoreCase)) {
            throw "-Root $full overlaps $forbidden; pick a folder outside it (e.g. C:\project_O-release-gate)"
        }
    }
    $state.Work = Join-Path $full (Get-Date -Format 'yyyyMMdd-HHmmss')
    $state.Project = Join-Path $state.Work 'project'
    New-Item -ItemType Directory -Path $state.Project -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $state.Project 'README.md') -Value '# release gate scratch project' -Encoding utf8
    & git -C $state.Project init --quiet | Out-Null
    if ($LASTEXITCODE) { throw 'git init failed' }
    & git -C $state.Project add README.md | Out-Null
    & git -C $state.Project -c user.name=release-gate -c user.email=release-gate@example.invalid commit --quiet -m 'init' | Out-Null
    if ($LASTEXITCODE) { throw 'git commit failed' }
    Push-Location $state.Project
    try { $ancestors = (ConvertFrom-JsonLine (Invoke-O @('agents', 'doctor', '--json')).Out).ancestorInstructions } finally { Pop-Location }
    if ($ancestors) { Write-Warning "instruction files above the project are read by Claude: $($ancestors -join ', ')" }
    $state.Project
}

if ($ready) {
    # 3. Two real cards side by side.
    $state.Run1Done = Invoke-Check 'dispatch: claude + codex 카드 done' {
        $plan = New-Plan 'release gate pair' @(
            @{ id = 'a'; provider = 'claude'; role = 'gate'; task = 'Create a file named hello-claude.txt in the project root containing exactly the line: ok. Do nothing else.'; resources = @(@{ kind = 'project-path'; key = 'hello-claude.txt'; mode = 'write' }) }
            @{ id = 'b'; provider = 'codex'; role = 'gate'; effort = 'low'; task = 'Create a file named hello-codex.txt in the project root containing exactly the line: ok. Do nothing else.'; resources = @(@{ kind = 'project-path'; key = 'hello-codex.txt'; mode = 'write' }) }
        )
        $state.Run1 = Start-Plan $plan
        $run = Wait-Run $state.Run1 $timeout
        if ($run.Status -ne 'done' -or $run.Cards['a'] -ne 'done' -or $run.Cards['b'] -ne 'done') {
            throw "run $($state.Run1) ended $($run.Status) ($(Format-Cards $run.Cards)); see o status $($state.Run1)"
        }
        "run $($state.Run1): $(Format-Cards $run.Cards)"
    }

    # 4. Stop a card that is working.
    Invoke-Check 'stop: 실행 중 카드 stopped' {
        $plan = New-Plan 'release gate stop' @(
            @{ id = 'slow'; provider = 'claude'; role = 'gate'; task = 'Create twenty files named part-01.md to part-20.md, one at a time. Each holds a different 400-word paragraph about a different city.'; resources = @(@{ kind = 'project-path'; key = 'parts'; mode = 'write' }) }
        )
        $state.Run2 = Start-Plan $plan
        $started = Wait-Run $state.Run2 300 { param($events) [bool]($events | Where-Object { $_.t -eq 'card.start' -and $_.cardId -eq 'slow' }) }
        if ($started.Status) { throw "the card ended ($($started.Status)) before it could be stopped; run $($state.Run2)" }
        Start-Sleep -Seconds 10
        Assert-O (Invoke-O @('stop', $state.Run2)) "o stop $($state.Run2)"
        $run = Wait-Run $state.Run2 180
        if ($run.Status -ne 'stopped' -or $run.Cards['slow'] -ne 'stopped') {
            throw "run $($state.Run2) ended $($run.Status) ($(Format-Cards $run.Cards)) instead of stopped"
        }
        "run $($state.Run2): $(Format-Cards $run.Cards)"
    } | Out-Null

    # 5. A follow-up to the finished claude card (its session is resumed).
    Invoke-Check 'followup: 후속 지시 done' {
        if (-not $state.Run1Done) { throw 'not tried: the dispatch check did not pass' }
        $r = Invoke-O @('followup', $state.Run1, 'a') -InputText 'Append a second line to hello-claude.txt: followup ok. Do nothing else.'
        Assert-O $r "o followup $($state.Run1) a"
        $state.Run3 = (ConvertFrom-JsonLine $r.Out).runId
        $run = Wait-Run $state.Run3 $timeout
        if (-not ($run.Events | Where-Object { $_.t -eq 'run.followup' -and $_.sourceRunId -eq $state.Run1 })) { throw "run $($state.Run3) is not a follow-up of $($state.Run1)" }
        if ($run.Status -ne 'done' -or $run.Cards['a'] -ne 'done') { throw "follow-up run $($state.Run3) ended $($run.Status) ($(Format-Cards $run.Cards))" }
        "run $($state.Run3) (from $($state.Run1)): $(Format-Cards $run.Cards)"
    } | Out-Null

    # 6. Recovery: no lease may outlive its run.
    Invoke-Check 'resources: 남은 자원 예약 없음' {
        $r = Invoke-O @('resources', '--json')
        Assert-O $r 'o resources --json'
        $leases = @((ConvertFrom-JsonLine $r.Out).leases)
        $ours = @($state.Run1, $state.Run2, $state.Run3) | Where-Object { $_ }
        $left = @($leases | Where-Object { $_.runId -in $ours })
        if ($left) { throw "leases left: $(($left | ForEach-Object { "$($_.runId)/$($_.cardId) ($($_.state))" }) -join ', ') — o resources" }
        $others = $leases.Count
        if ($others) { "none for this gate's runs; $others lease(s) from other runs (o resources)" } else { 'no leases' }
    } | Out-Null
}

# 7. Update path, read-only: what the updater would offer, and that the asset matches its digest.
Invoke-Check '업데이트 경로 (읽기 전용)' {
    $env:GH_HOST = 'github.com'; $env:GH_PROMPT_DISABLED = '1'
    $json = & gh api --hostname github.com "repos/$Repository/releases?per_page=20"
    if ($LASTEXITCODE) { throw 'gh api releases failed (gh auth login --hostname github.com)' }
    $candidates = foreach ($rel in ($json | Out-String | ConvertFrom-Json)) {
        if (-not $rel.prerelease -or $rel.draft -or $rel.tag_name -cnotmatch '^test_1-[0-9]{8}-[0-9a-f]{7}$') { continue }
        if ($rel.author.login -ne 'github-actions[bot]') { continue }
        if ($rel.target_commitish -cnotmatch '^[0-9a-f]{40}$' -or -not $rel.target_commitish.StartsWith($rel.tag_name.Substring(16))) { continue }
        $asset = $rel.assets | Where-Object {
            $_.name -cmatch '^project_O_[0-9]+\.[0-9]+\.[0-9]+_x64-setup\.exe$' -and $_.state -eq 'uploaded' -and $_.size -ge 1 -and $_.size -le 1GB -and $_.digest -cmatch '^sha256:[0-9a-f]{64}$'
        } | Select-Object -First 1
        if ($asset) { [pscustomobject]@{ Tag = $rel.tag_name; Published = [datetimeoffset]$rel.published_at; Target = $rel.target_commitish; Asset = $asset } }
    }
    $pick = $candidates | Sort-Object Published -Descending | Select-Object -First 1
    if (-not $pick) { throw 'no release the updater would offer (prerelease by the release workflow with a setup and a sha256 digest)' }
    $status = (& gh api --hostname github.com "repos/$Repository/compare/test_1...$($pick.Target)" --jq .status | Out-String).Trim()
    if ($status -notin 'identical', 'behind') { throw "release $($pick.Tag) commit is not in test_1 (compare: $status)" }
    $dir = Join-Path ([IO.Path]::GetTempPath()) ("project_O-gate-update-{0}" -f [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $dir | Out-Null
    & gh release download $pick.Tag --repo $Repository --pattern $pick.Asset.name --dir $dir | Out-Null
    if ($LASTEXITCODE) { throw "gh release download $($pick.Tag) failed" }
    $file = Get-Item -LiteralPath (Join-Path $dir $pick.Asset.name)
    if ($file.Length -ne $pick.Asset.size) { throw "size $($file.Length) differs from the release ($($pick.Asset.size))" }
    $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ("sha256:$hash" -ne $pick.Asset.digest) { throw "SHA-256 $hash differs from the release digest" }
    $sig = (Get-AuthenticodeSignature -LiteralPath $file.FullName).Status
    $signed = if ($sig -eq 'Valid') { 'signed' } else { "unsigned ($sig) — SmartScreen warns, see docs/INSTALL.md" }
    $state.UpdateTag = $pick.Tag
    "$($pick.Tag) $($pick.Asset.name): size and SHA-256 match, test_1 $status, $signed; kept at $($file.FullName) (not installed)"
} | Out-Null

# A failed or timed-out check must not leave a real agent spending usage.
foreach ($id in @($state.Run1, $state.Run2, $state.Run3) | Where-Object { $_ }) {
    try {
        if (-not (Get-RunEvents $id | Where-Object { $_.t -eq 'run.end' })) {
            Invoke-O @('stop', $id) | Out-Null
            Write-Warning "stopped unfinished run $id"
        }
    } catch {
        Write-Warning "could not check or stop run ${id}: $($_.Exception.Message) — o stop $id"
    }
}

# Manual checks: approvals are a person's decision; scripts and agents cannot give them.
$manual = @(
    @{ Name = 'M1 총괄 승인'; Ask = "앱에서 YOLO를 끄고 총괄에게 '$($state.Project)에 notes.txt를 만들어 줘'처럼 파일 수정이 필요한 작은 요청을 보낸다. 입력창 위 승인 카드가 뜨고, '거부'하면 수정되지 않고 '허용'하면 진행되었나?" }
    @{ Name = 'M2 총괄 계획 실행'; Ask = "총괄이 만든 계획 카드에서 '실행'을 눌러 카드가 끝까지(완료) 갔나? 계획은 '실행'을 누르기 전에는 전달되지 않았나?" }
    @{ Name = 'M3 업데이트 화면'; Ask = "설정 → 정보 → 업데이트 확인이 $(if ($state.UpdateTag) { $state.UpdateTag } else { '최신 test_1 프리릴리스' })를 보이거나 최신이라고 알리나? ('지금 업데이트'는 누르지 않아도 된다)" }
)
Write-Host ''
Write-Host '사람이 할 확인 (o lead decide는 에이전트 안에서 거부되며, 승인은 사람이 앱 버튼으로 한다):' -ForegroundColor Cyan
foreach ($m in $manual) { Write-Host "  - $($m.Name): $($m.Ask)" }
foreach ($m in $manual) {
    if ($SkipManual) { Add-Result $m.Name 'SKIP' '사람 확인 필요 (-SkipManual)'; continue }
    try { $answer = Read-Host "$($m.Name) — 확인했나? (y/n)" } catch { $answer = $null }
    switch -Regex ($answer) {
        '^(y|yes|예|ㅇ)$' { Add-Result $m.Name 'PASS' '사람이 확인함' }
        '^(n|no|아니오|ㄴ)$' { Add-Result $m.Name 'FAIL' '사람이 문제를 보고함' }
        default { Add-Result $m.Name 'SKIP' '응답 없음 — 사람 확인 필요' }
    }
}

Write-Host ''
$results | Format-Table -AutoSize -Wrap | Out-Host
if ($state.Work) { Write-Host "임시 프로젝트: $($state.Project) (지우지 않았다)" }
$failed = @($results | Where-Object Status -eq 'FAIL').Count
$skipped = @($results | Where-Object Status -eq 'SKIP').Count
if ($failed) {
    Write-Host "RESULT: FAIL ($failed failed)" -ForegroundColor Red
    exit 1
}
if ($skipped) {
    Write-Host "RESULT: INCOMPLETE (automatic checks passed; $skipped manual check(s) not confirmed)" -ForegroundColor Yellow
    exit 2
}
Write-Host 'RESULT: PASS' -ForegroundColor Green
exit 0
