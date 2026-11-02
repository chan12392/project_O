//! Runs every card of a plan in parallel, as far as the caps allow, and writes
//! the normalized events. Cards that cannot get a slot wait in a queue; a card
//! whose provider hits a limit mid-run stops its queued siblings on that
//! provider instead of retrying them.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;

use crate::usage::budget::{self, Pace};
use crate::run::events::{CardInfo, CardStatus, OEvent, StreamSignal};
use crate::usage::UsageWindow;
use crate::sys::js::{js_string, js_tail, now_iso, parse_time};
use crate::run::plan::{Card, Plan, ProviderId};
use crate::policy::{check_plan, LIMITS};
use crate::agents::clean;
use crate::sys::proc;
use crate::providers::{launch_spec_with, LaunchSpec, StreamReader, TeamChannel};
use crate::sys::resolve::resolve_cli;
use crate::run::store::{
    active_markers, append_event, clear_active, cooldown_times, is_clean, mark_active, policy_context, read_events, read_plan, record_start, run_dir,
    set_cooldown, started_on, stop_requested, ActiveMarker,
};

/// Builds the command for a card's CLI. The runner adds the environment, pipes,
/// the hidden window and the kill-on-close job; tests swap in a fake CLI here.
pub type Spawner = dyn Fn(&LaunchSpec, &str) -> io::Result<Command> + Send + Sync;

const WAIT_FOR_SLOT: Duration = Duration::from_millis(2_000);

/// `clean` is the profile root when the run uses clean agent profiles (`o dispatch --clean`).
pub fn run_plan(home: &Path, run_id: &str, plan: &Plan, clean: Option<&Path>, spawner: Arc<Spawner>) -> io::Result<CardStatus> {
    let emit = |event: OEvent| append_event(home, run_id, &event);
    emit(OEvent::RunStart {
        at: now_iso(),
        run_id: run_id.to_string(),
        title: plan.title.clone(),
        project: plan.project.clone(),
        cards: plan
            .cards
            .iter()
            .map(|c| CardInfo {
                id: c.id.clone(),
                provider: c.provider,
                label: Some(c.label()),
                role: c.role.clone(),
                model: c.model.clone(),
                effort: c.effort.clone(),
            })
            .collect(),
    })?;

    // What earlier work of the project each card gets (ADR-0005); empty with `--no-context`.
    let contexts = Arc::new(crate::context::prepare(home, run_id, plan));
    let shared_plan = Arc::new(plan.clone());
    let mut pending: Vec<Card> = plan.cards.clone();
    let mut queued: HashSet<String> = HashSet::new();
    let mut running = 0usize;
    let mut results: Vec<CardStatus> = Vec::new();
    let (done_tx, done_rx) = mpsc::channel::<io::Result<CardStatus>>();
    let ended = |card: &Card, status: CardStatus, result: Option<String>| OEvent::CardEnd {
        at: now_iso(),
        card_id: card.id.clone(),
        status,
        result,
        exit_code: None,
    };

    while !pending.is_empty() || running > 0 {
        if stop_requested(home, run_id) {
            for card in pending.drain(..) {
                emit(ended(&card, CardStatus::Stopped, None))?;
                results.push(CardStatus::Stopped);
            }
        }
        for card in pending.clone() {
            let take = |pending: &mut Vec<Card>| pending.retain(|c| c.id != card.id);
            if let Some(until) = cooldown_times(home)?.get(&card.provider) {
                if parse_time(until).is_some_and(|t| t > Utc::now()) {
                    take(&mut pending);
                    emit(ended(&card, CardStatus::Failed, Some(format!("{} 사용량 한도로 {until}까지 차단됨", card.provider))))?;
                    results.push(CardStatus::Failed);
                    continue;
                }
            }
            // Dispatch checked the day's cap, but plans dispatched back to back all see the same count.
            if started_on(home, Utc::now())? >= LIMITS.max_cards_per_day {
                take(&mut pending);
                emit(ended(&card, CardStatus::Failed, Some(format!("오늘 실행 상한 {}개에 닿아 시작하지 않음", LIMITS.max_cards_per_day))))?;
                results.push(CardStatus::Failed);
                continue;
            }
            if let Some(reason) = slot_blocker(home, card.provider) {
                if queued.insert(card.id.clone()) {
                    emit(OEvent::CardQueued { at: now_iso(), card_id: card.id.clone(), reason })?;
                }
                continue;
            }
            // Today's usage budget, read the moment the card would start (ADR-0004).
            let budget = budget::assess_now(home, card.provider, Utc::now())?;
            if budget.state == Pace::Over {
                take(&mut pending);
                emit(ended(&card, CardStatus::Failed, Some(budget.message)))?;
                results.push(CardStatus::Failed);
                continue;
            }
            take(&mut pending);
            let mut card = card.clone();
            if let Some(to) = budget::lower_effort(card.effort.as_deref(), budget.steps_down) {
                emit(OEvent::CardBudget {
                    at: now_iso(),
                    card_id: card.id.clone(),
                    provider: card.provider,
                    effort_from: card.effort.clone(),
                    effort_to: to.clone(),
                    spent: budget.spent.unwrap_or_default(),
                    allowance: budget.allowance,
                })?;
                card.effort = Some(to);
            }
            let (home, run_id, plan, clean, spawner, done) =
                (home.to_path_buf(), run_id.to_string(), Arc::clone(&shared_plan), clean.map(Path::to_path_buf), Arc::clone(&spawner), done_tx.clone());
            let contexts = Arc::clone(&contexts);
            thread::spawn(move || {
                let context = contexts.get(&card.id).map(String::as_str);
                let outcome = panic::catch_unwind(AssertUnwindSafe(|| run_card(&home, &run_id, &plan, &card, context, clean.as_deref(), spawner.as_ref())))
                    .unwrap_or_else(|_| Err(io::Error::other(format!("카드 {} 실행 중 내부 오류", card.id))));
                let _ = done.send(outcome);
            });
            running += 1;
        }
        if running > 0 {
            let first = done_rx.recv().map_err(io::Error::other)?;
            running -= 1;
            results.push(first?);
            while let Ok(next) = done_rx.try_recv() {
                running -= 1;
                results.push(next?);
            }
        } else if !pending.is_empty() {
            thread::sleep(WAIT_FOR_SLOT);
        }
    }

    let status = if results.contains(&CardStatus::Stopped) {
        CardStatus::Stopped
    } else if results.iter().all(|r| *r == CardStatus::Done) {
        CardStatus::Done
    } else {
        CardStatus::Failed
    };
    emit(OEvent::RunEnd { at: now_iso(), run_id: run_id.to_string(), status })?;
    Ok(status)
}

fn slot_blocker(home: &Path, provider: ProviderId) -> Option<String> {
    let active = active_markers(home);
    if active.len() >= LIMITS.max_active_total {
        return Some(format!("전체 동시 실행 {}개가 차서 대기", LIMITS.max_active_total));
    }
    let max = LIMITS.max_active_per_provider(provider);
    if active.iter().filter(|m| m.provider == provider).count() >= max {
        return Some(format!("{provider} 동시 실행 {max}개가 차서 대기"));
    }
    None
}

/// Clears the card's active marker however the card ends.
struct Slot<'a> {
    home: &'a Path,
    run_id: &'a str,
    card_id: &'a str,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        clear_active(self.home, self.run_id, self.card_id);
    }
}

fn append_bytes(path: &Path, bytes: &[u8]) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(bytes);
    }
}

fn run_card(home: &Path, run_id: &str, plan: &Plan, card: &Card, context: Option<&str>, clean: Option<&Path>, spawner: &Spawner) -> io::Result<CardStatus> {
    let emit = |event: OEvent| append_event(home, run_id, &event);
    let dir = run_dir(home, run_id);
    let cards_dir = dir.join("cards");
    let dir_text = dir.to_string_lossy().into_owned();
    let spec = launch_spec_with(card, plan, &TeamChannel { run_id, run_dir: &dir_text }, context);
    let end = |status: CardStatus, result: Option<String>, exit_code: Option<i64>| OEvent::CardEnd {
        at: now_iso(),
        card_id: card.id.clone(),
        status,
        result,
        exit_code,
    };

    // A program that never started (not found, refused) holds no slot and counts nothing.
    let spawned = spawner(&spec, &plan.project).and_then(|mut command| {
        // A clean card sees nothing of this process's environment but the kept system variables.
        if let Some(root) = clean {
            command.env_clear().envs(clean::environment(root, card.provider)?);
        }
        command.envs(&spec.env);
        proc::hide_window(&mut command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
    });
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            emit(end(CardStatus::Failed, Some(format!("실행 실패: {error}")), None))?;
            return Ok(CardStatus::Failed);
        }
    };
    let _tree = proc::contain(&child);
    let pid = child.id();

    // Count the card the moment it holds a slot, so a parallel dispatch sees it.
    let _slot = Slot { home, run_id, card_id: &card.id };
    mark_active(home, &ActiveMarker { run_id: run_id.to_string(), card_id: card.id.clone(), provider: card.provider, pid })?;
    record_start(home, run_id, &card.id, card.provider, Utc::now())?;
    emit(OEvent::CardStart { at: now_iso(), card_id: card.id.clone(), pid: Some(pid) })?;

    if let Some(mut stdin) = child.stdin.take() {
        let text = spec.stdin.clone().unwrap_or_default();
        thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }

    let err_log: PathBuf = cards_dir.join(format!("{}.err.log", card.id));
    let stderr = child.stderr.take().map(|mut stderr| {
        thread::spawn(move || {
            let mut tail = String::new();
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                append_bytes(&err_log, &chunk[..n]);
                tail.push_str(&String::from_utf8_lossy(&chunk[..n]));
                tail = js_tail(&tail, 400).to_string();
            }
            tail
        })
    });

    let mut reader = StreamReader::new(card.provider);
    let mut last: Option<(bool, String)> = None;
    let mut last_message = String::new();
    let raw_log = cards_dir.join(format!("{}.raw.jsonl", card.id));
    if let Some(stdout) = child.stdout.take() {
        let mut lines = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            if lines.read_until(b'\n', &mut buf)? == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buf);
            let line = text.strip_suffix('\n').unwrap_or(&text);
            let line = line.strip_suffix('\r').unwrap_or(line);
            append_bytes(&raw_log, format!("{line}\n").as_bytes());
            for signal in reader.read(line) {
                let at = now_iso();
                let card_id = card.id.clone();
                match signal {
                    StreamSignal::Session { session_id } => emit(OEvent::CardSession { at, card_id, session_id })?,
                    StreamSignal::Message { text } => {
                        last_message = text.clone();
                        emit(OEvent::CardMessage { at, card_id, text })?;
                    }
                    StreamSignal::Tool { name, summary } => emit(OEvent::CardTool { at, card_id, name, summary })?,
                    StreamSignal::Limit { until } => {
                        let until = set_cooldown(home, card.provider, &until, Utc::now())?;
                        emit(OEvent::CardLimit { at: now_iso(), card_id, until })?;
                    }
                    StreamSignal::Usage { input, cached, output } => emit(OEvent::CardUsage { at, card_id, input, cached, output })?,
                    StreamSignal::Quota { window, used, resets_at } => {
                        // A snapshot for the daily budget; the event stays the record either way.
                        let reading = UsageWindow { provider: card.provider, window: window.clone(), group: None, used, resets_at: resets_at.clone() };
                        let _ = budget::record(home, &[reading], Utc::now());
                        emit(OEvent::Quota { at, provider: card.provider, window, used, resets_at })?
                    }
                    StreamSignal::Final { ok, text } => last = Some((ok, text)),
                }
            }
        }
    }
    // Like Node's "close": stdout has ended and the process has exited.
    let stderr_tail = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    let (exit_code, error) = match child.wait() {
        Ok(status) => (proc::exit_code(status), None),
        Err(error) => (None, Some(format!("실행 실패: {error}"))),
    };

    let status = if stop_requested(home, run_id) {
        CardStatus::Stopped
    } else if last.as_ref().is_some_and(|(ok, _)| *ok) && exit_code == Some(0) {
        CardStatus::Done
    } else {
        CardStatus::Failed
    };
    // A failure with nothing on stdout usually explains itself on stderr.
    let result = error.or_else(|| {
        [
            last.map(|(_, text)| text).unwrap_or_default(),
            last_message,
            if status == CardStatus::Failed { stderr_tail.trim().to_string() } else { String::new() },
        ]
        .into_iter()
        .find(|text| !text.is_empty())
    });
    drop(_slot);
    emit(end(status, result, exit_code))?;
    Ok(status)
}

/// Launches the official CLI found on PATH, never through a shell (see `resolve`).
pub fn official_command(spec: &LaunchSpec, cwd: &str) -> io::Result<Command> {
    let cli = resolve_cli(&spec.bin)
        .ok_or_else(|| io::Error::other(format!("{} CLI를 PATH에서 찾지 못했거나 셸 없이 실행할 수 없습니다", spec.bin)))?;
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).args(&spec.args).current_dir(cwd);
    Ok(command)
}

/// `o stop`: asks the runner to stop and ends the run's live card processes
/// with their trees. `false` when there is no such run.
pub fn stop_run(home: &Path, run_id: &str) -> io::Result<bool> {
    if !run_dir(home, run_id).exists() {
        return Ok(false);
    }
    crate::run::store::request_stop(home, run_id)?;
    for marker in active_markers(home).into_iter().filter(|m| m.run_id == run_id) {
        proc::kill_tree(marker.pid);
    }
    Ok(true)
}

/// The background half of `o dispatch`. It checks the plan again, since anyone
/// can start it with a hand-written plan.json, and a crash still ends the run
/// on record so watchers (`o events --follow`, the app) are not left waiting.
pub fn run_detached(home: &Path, run_id: &str) -> Result<(), String> {
    let mut raw: Option<Value> = None;
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let plan = read_plan(home, run_id).map_err(|e| e.to_string())?;
        raw = Some(plan.clone());
        // Codex and Antigravity report usage only when asked: a stale reading is read again before the budget check.
        let probed = |c: &Value| matches!(c.get("provider").and_then(Value::as_str), Some("codex" | "agy"));
        if plan.get("cards").and_then(Value::as_array).is_some_and(|cards| cards.iter().any(probed)) {
            budget::refresh_if_stale(home, Utc::now());
        }
        let ctx = policy_context(home, Utc::now()).map_err(|e| e.to_string())?;
        let checked = check_plan(&plan, &ctx);
        let Some(plan) = checked.plan else {
            let reasons: Vec<&str> = checked.violations.iter().map(|v| v.message.as_str()).collect();
            return Err(format!("정책 위반: {}", reasons.join(" / ")));
        };
        let clean = is_clean(home, run_id).then(clean::default_root);
        run_plan(home, run_id, &plan, clean.as_deref(), Arc::new(official_command)).map_err(|e| e.to_string())?;
        Ok(())
    }))
    .unwrap_or_else(|panic| {
        let text = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()));
        Err(format!("내부 오류: {}", text.unwrap_or_default()))
    });
    let Err(message) = outcome else { return Ok(()) };

    let _ = fs::write(run_dir(home, run_id).join("runner.err.log"), format!("{} {message}\n", now_iso()));
    let started = read_events(home, run_id).iter().any(|e| matches!(e, OEvent::RunStart { .. }));
    if !started {
        if let Some(raw) = &raw {
            let text = |key: &str| raw.get(key).map(js_string).unwrap_or_else(|| "undefined".into());
            let _ = append_event(
                home,
                run_id,
                &OEvent::RunStart { at: now_iso(), run_id: run_id.to_string(), title: text("title"), project: text("project"), cards: Vec::new() },
            );
        }
    }
    if started || raw.is_some() {
        let _ = append_event(home, run_id, &OEvent::RunEnd { at: now_iso(), run_id: run_id.to_string(), status: CardStatus::Failed });
    }
    Err(message)
}
