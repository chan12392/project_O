//! Card execution and resource reservation lifecycle.
use super::*;

/// The card's reservation remains held until the process tree's exit is confirmed.
pub(super) struct Slot<'a> {
    pub(super) home: &'a Path,
    pub(super) reservation: Reservation,
    pub(super) started: bool,
    pub(super) marked: bool,
    pub(super) exited: Arc<AtomicBool>,
    pub(super) best_effort: bool,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let r = &self.reservation;
        if self.started && !self.exited.load(Ordering::Acquire) {
            return;
        }
        if self.started {
            if self.best_effort {
                let warning = OEvent::CardTool { at: now_iso(), card_id: r.card_id.clone(), name: "자원 예약".into(), summary: "자손 종료를 확인할 수 없어 예약을 해제합니다. 자손이 남아 자원이 겹칠 수 있습니다.".into() };
                if append_event(self.home, &r.run_id, &warning).is_err() { return; }
            } else if crate::run::store::mark_resource_exited(self.home, r).is_err() {
                return;
            }
        }
        if self.marked {
            clear_active(self.home, &r.run_id, &r.card_id);
        }
        // A failed database write leaves the durable resource reservation in place.
        let _ = release_slot(self.home, r, self.started);
    }
}

/// An early I/O error must not leave a worker running after its slot is released.
struct RunningChild {
    child: crate::sys::worker::Child,
    _tree: Option<Arc<Mutex<proc::KillOnClose>>>,
    stop_watch: Option<(mpsc::Sender<()>, thread::JoinHandle<()>)>,
    exited: Arc<AtomicBool>,
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        if let Some((done, watcher)) = self.stop_watch.take() {
            let _ = done.send(());
            let _ = watcher.join();
        }
        let tree_exited = self._tree.as_ref().is_some_and(|tree| tree.lock().is_ok_and(|mut tree| tree.stop_and_confirm()));
        if !matches!(self.child.try_wait(&self._tree), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let exited = {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match self.child.try_wait(&self._tree) {
                    Ok(Some(_)) => break true,
                    Err(_) => break false,
                    Ok(None) if Instant::now() >= deadline => break false,
                    Ok(None) => thread::sleep(Duration::from_millis(25)),
                }
            }
        };
        self.exited.store(exited && (tree_exited || BEST_EFFORT_EXIT), Ordering::Release);
    }
}

/// What a card starts with besides its plan: the slot it holds, the checked folder, its
/// context section, the clean profile root and the spawner.
pub(super) struct Start<'a> {
    pub(super) slot: Slot<'a>,
    pub(super) workdir: &'a Workdir,
    pub(super) workspace: Option<&'a str>,
    pub(super) context: Option<&'a str>,
    pub(super) clean: Option<&'a Path>,
    pub(super) spawner: &'a Spawner,
    pub(super) yolo: bool,
    pub(super) dependencies: &'a [(String, String)],
}

pub(super) fn run_card(home: &Path, run_id: &str, plan: &Plan, card: &Card, start: Start) -> io::Result<CardStatus> {
    let Start { mut slot, workdir, workspace, context, clean, spawner, yolo, dependencies } = start;
    let emit = |event: OEvent| append_event(home, run_id, &event);
    let dir = run_dir(home, run_id);
    let cards_dir = dir.join("cards");
    let dir_text = dir.to_string_lossy().into_owned();
    let end =|status: CardStatus, result: Option<String>, exit_code: Option<i64>| OEvent::CardEnd {
        at: now_iso(),
        card_id: card.id.clone(),
        status,
        result,
        exit_code,
    };

    // Stopped while waiting for its thread: never start it.
    if stop_requested(home, run_id) {
        drop(slot);
        emit(end(CardStatus::Stopped, None, None))?;
        return Ok(CardStatus::Stopped);
    }
    // The project is resolved again right before the start: a link or junction pointed
    // elsewhere since the policy check would take the card out of the folder it allowed.
    let now = (workdir.resolve)(&plan.project);
    if !same_folder(&now, &workdir.checked) {
        drop(slot);
        let reason = format!("작업 폴더 {}가 정책 검사 뒤 다른 곳({now})을 가리켜 시작하지 않음 (검사한 폴더: {})", plan.project, workdir.checked);
        emit(end(CardStatus::Failed, Some(reason), None))?;
        return Ok(CardStatus::Failed);
    }
    let followup_path = dir.join("followup.json");
    let mut followup_lock = None;
    let followup = if followup_path.exists() {
        let reference: (String, String) = serde_json::from_slice(&fs::read(&followup_path)?).map_err(io::Error::other)?;
        let target = crate::run::followup::target(home, &reference.0, &reference.1).map_err(io::Error::other)?;
        if plan.cards.len() != 1 || target.card.id != card.id || target.card.provider != card.provider || target.card.model != card.model || target.project != plan.project || target.clean != clean.is_some() || yolo {
            return Err(io::Error::other("이어갈 작업 정보가 바뀌어 시작하지 않았습니다"));
        }
        let mut locks = home.join("session-locks");
        // Separate, fixed-size hashes keep fresh locks distinct and below Windows' filename limit.
        let key = match &target.session {
            Some(session) => session.clone(),
            None => {
                locks.push("fresh");
                use sha2::Digest;
                let hash = |text: &str| format!("{:x}", sha2::Sha256::digest(text));
                format!("{}.{}", hash(&reference.0), hash(&reference.1))
            }
        };
        fs::create_dir_all(&locks)?;
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(locks.join(format!("{}-{}-{}.lock", card.provider, target.clean, key)))?;
        lock.try_lock().map_err(|_| io::Error::other("이 에이전트의 후속 작업이 이미 실행 중입니다"))?;
        followup_lock = Some(lock);
        Some(target)
    } else { None };
    let resumed_workspace = followup.as_ref().map(|target| target.workspace.to_string_lossy().into_owned());
    let cwd = resumed_workspace.as_deref().or(workspace).unwrap_or(&workdir.checked);
    if resumed_workspace.is_some() && (!same_folder(&resolved_path(cwd), cwd) || !Path::new(cwd).is_dir()) {
        return Err(io::Error::other("기존 작업 폴더가 변경되어 시작하지 않았습니다"));
    }
    if let Some(path) = workspace {
        if !same_folder(&resolved_path(path), path) || !Path::new(path).is_dir() {
            drop(slot);
            emit(end(CardStatus::Failed, Some("분리된 작업 폴더가 변경되어 시작하지 않았습니다".into()), None))?;
            return Ok(CardStatus::Failed);
        }
        emit(OEvent::CardTool { at: now_iso(), card_id: card.id.clone(), name: "O 작업 폴더".into(), summary: path.to_string() })?;
    }
    // #37: without YOLO an OpenCode card must not run project-provided config or plugins, and
    // OpenCode has no switch to skip them, so a folder that has some (or a parent of it) is refused.
    if card.provider == ProviderId::OpenCode && !yolo {
        if let Some(found) = crate::providers::opencode::project_config(Path::new(cwd)) {
            drop(slot);
            emit(end(CardStatus::Failed, Some(crate::providers::opencode::project_config_refusal(&found)), None))?;
            return Ok(CardStatus::Failed);
        }
    }
    let checked_plan = Plan { project: cwd.to_string(), ..plan.clone() };
    let mut spec = launch_spec_for(card, &checked_plan, &TeamChannel { run_id, run_dir: &dir_text }, context, yolo);
    if let Some(target) = &followup {
        if card.provider == ProviderId::Agy { return Err(io::Error::other(crate::policy::AGY_UNAVAILABLE)); }
        // No session: the card's last session was cut off by a dead runner, so this follow-up
        // starts a fresh one in the same workspace (ADR-0039) and reports it like any new card.
        if let Some(session) = &target.session {
            match card.provider {
                ProviderId::Codex => { spec.args.pop(); spec.args.extend(["resume".into(), session.clone(), "-".into()]); }
                ProviderId::Claude => spec.args.extend(["--resume".into(), session.clone()]),
                ProviderId::OpenCode => {
                    crate::providers::opencode::check_session(clean, Path::new(cwd), session).map_err(io::Error::other)?;
                    spec.args.extend(["--session".into(), session.clone()]);
                }
                ProviderId::Devin => {
                    let at = spec.args.iter().position(|arg| arg == "--").ok_or_else(|| io::Error::other("Devin 인자 형식이 올바르지 않습니다"))?;
                    spec.args.splice(at..at, ["--resume".into(), session.clone()]);
                }
                ProviderId::Agy => return Err(io::Error::other(crate::policy::AGY_UNAVAILABLE)),
            }
        }
        emit(OEvent::CardTool { at: now_iso(), card_id: card.id.clone(), name: "O 작업 폴더".into(), summary: cwd.to_string() })?;
        if let Some(session) = &target.session {
            emit(OEvent::CardSession { at: now_iso(), card_id: card.id.clone(), session_id: session.clone() })?;
        }
    }
    let _prompt = if card.provider == ProviderId::Devin && spec.args.last().is_some_and(|text| text.len() > 12_000) {
        let text = spec.args.pop().ok_or_else(|| io::Error::other("Devin 프롬프트가 없습니다"))?;
        if spec.args.pop().as_deref() != Some("--") { return Err(io::Error::other("Devin 인자 형식이 올바르지 않습니다")); }
        let prompt = crate::run::result::prompt_file(home, run_id, &text).map_err(io::Error::other)?;
        let path = prompt.path().to_str().ok_or_else(|| io::Error::other("Devin 프롬프트 경로를 표현할 수 없습니다"))?;
        spec.args.extend(["--prompt-file".into(), path.into()]);
        Some(prompt)
    } else { None };
    // A program that never started (not found, refused) gives its slot and start back (`slot` drops).
    #[cfg(windows)]
    let mut watcher_executable = None;
    let spawned = spawner(&spec, cwd).and_then(|mut command| {
        // A clean card sees nothing of this process's environment but the kept system variables.
        if let Some(root) = clean {
            let env = clean::environment(root, card.provider)?;
            command.env_clear().envs(env);
        }
        command.envs(&spec.env);
        crate::agents::billing::check(home, card.provider, &command).map_err(io::Error::other)?;
        #[cfg(windows)]
        {
            let native = command.native(std::env::vars_os())?;
            let owner_executable = match command.handle_owner_executable { Some(path) => path, None => std::env::current_exe()? };
            let owner = crate::sys::win_launch::HandleOwner::start(owner_executable.clone(), "--o-handle-owner")?;
            watcher_executable = Some(owner_executable);
            if stop_requested(home, run_id) { return Err(io::Error::new(io::ErrorKind::Interrupted, "작업이 중지되었습니다")); }
            crate::run::store::mark_spawning(home, &slot.reservation)?;
            // A panic after the durable spawn marker must keep the reservation unknown.
            slot.started = true;
            // A named job lets a reader prove the card's tree gone after this runner dies (ADR-0039).
            match native.spawn_in_named_job(&owner, &format!("{run_id}-{}", card.id)) {
                Ok((child, tree)) => Ok((crate::sys::worker::Child::native(child), Some(Arc::new(Mutex::new(tree))))),
                Err(error) => { slot.started = false; Err(error) }
            }
        }
        #[cfg(target_os = "macos")]
        {
            crate::run::store::mark_spawning(home, &slot.reservation)?;
            slot.started = true;
            match crate::sys::macos_worker::spawn(&command) {
                Ok((child, tree)) => Ok((crate::sys::worker::Child::mac(child), Some(tree))),
                Err(error) => { slot.started = false; Err(error) }
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            crate::run::store::mark_spawning(home, &slot.reservation)?;
            let mut command = command.command();
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            slot.started = true;
            let child = if dependencies.is_empty() { proc::spawn_worker(&mut command) }
            else {
                crate::run::store::with_locked_event_text(home, run_id, |events| {
                    for (source, hash) in dependencies {
                        crate::run::result::verify_event_result(&events, source, hash).map_err(io::Error::other)?;
                    }
                    proc::spawn_worker(&mut command)
                }).and_then(|result| result)
            };
            match child {
                Ok(child) => { let tree = proc::contain(&child).map(|tree| Arc::new(Mutex::new(tree))); Ok((crate::sys::worker::Child::standard(child), tree)) }
                Err(error) => { slot.started = false; Err(error) }
            }
        }
    });
    let (child, tree) = match spawned {
        Ok(child) => child,
        Err(error) => {
            drop(slot);
            if stop_requested(home, run_id) { emit(end(CardStatus::Stopped, None, None))?; return Ok(CardStatus::Stopped); }
            emit(end(CardStatus::Failed, Some(format!("실행 실패: {error}")), None))?;
            return Ok(CardStatus::Failed);
        }
    };
    slot.started = true;
    #[cfg(windows)]
    let exit_watch = child.watch_handle()?;
    #[cfg(windows)]
    let ending = Arc::new(AtomicBool::new(false));
    #[cfg(all(test, windows))]
    let paused_watcher = card.id == "stop-race-test";
    #[cfg(not(all(test, windows)))]
    let paused_watcher = false;
    let stop_watch = tree.as_ref().filter(|_| !paused_watcher).map(|tree| {
        let tree = Arc::clone(tree);
        let stop = run_dir(home, run_id).join("stop");
        let (done, receiver) = mpsc::channel();
        #[cfg(windows)]
        let ending = Arc::clone(&ending);
        let watcher = thread::spawn(move || loop {
            #[cfg(windows)]
            let parent_exited = {
                use std::os::windows::io::AsRawHandle;
                use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
                use windows_sys::Win32::System::Threading::WaitForSingleObject;
                unsafe { WaitForSingleObject(exit_watch.as_raw_handle(), 0) != WAIT_TIMEOUT }
            };
            #[cfg(not(windows))]
            let parent_exited = false;
            if stop.exists() || parent_exited {
                #[cfg(windows)]
                ending.store(true, Ordering::Release);
                if let Ok(mut tree) = tree.lock() { tree.stop_and_confirm(); }
                break;
            }
            if !matches!(receiver.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Timeout)) { break; }
        });
        (done, watcher)
    });
    let mut running_child = RunningChild { child, _tree: tree, stop_watch, exited: Arc::clone(&slot.exited) };
    // Set only when the worker is confirmed in that named job (`win_launch::spawn_in_named_job`).
    let job_name = running_child._tree.as_ref().and_then(|tree| tree.lock().ok().and_then(|tree| tree.job_name().map(str::to_owned)));
    let child = &mut running_child.child;
    let pid = child.id();

    // The reservation already counts the card; the slot now lives as long as the CLI, and the
    // marker lets `o stop` find its process.
    let linked = link_slot(home, &slot.reservation, pid, job_name.as_deref()).and_then(|()| {
        slot.marked = true;
        mark_active(home, &ActiveMarker { run_id: run_id.to_string(), card_id: card.id.clone(), provider: card.provider, pid })
    });
    if let Err(error) = linked {
        drop(running_child);
        #[cfg(windows)]
        if slot.exited.load(Ordering::Acquire) { slot.started = false; }
        return Err(error);
    }
    #[cfg(windows)]
    if let Some(executable) = watcher_executable {
        // Best effort: a watcher that cannot start/open the job leaves no exit evidence.
        let _ = crate::run::job_watch::spawn(home, slot.reservation.slot_row, &executable);
    }
    #[cfg(windows)]
    if running_child._tree.is_none() { return Err(io::Error::other("프로세스 격리를 확인하지 못해 시작하지 않았습니다")); }
    #[cfg(all(test, windows))]
    if paused_watcher {
        let hook = BEFORE_TEST_RESUME.lock().unwrap().take();
        if let Some(hook) = hook { hook(); }
    }
    let start_event = OEvent::CardStart { at: now_iso(), card_id: card.id.clone(), pid: Some(pid) };
    #[cfg(any(windows, target_os = "macos"))]
    let mut resume_attempted = false;
    #[cfg(any(windows, target_os = "macos"))]
    let resumed = crate::run::store::with_events(home, run_id, |file| {
        if !dependencies.is_empty() {
            let events = crate::run::store::event_text(file)?;
            for (source, hash) in dependencies {
                crate::run::result::verify_event_result(&events, source, hash).map_err(io::Error::other)?;
            }
        }
        crate::run::store::with_start_gate(home, run_id, || {
            if run_dir(home, run_id).join("stop").try_exists()? {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "작업이 중지되었습니다"));
            }
            crate::run::store::append_value(file, &serde_json::to_value(&start_event).map_err(io::Error::other)?)?;
            resume_attempted = true;
            child.resume()
        })
    });
    #[cfg(not(any(windows, target_os = "macos")))]
    let resumed = { emit(start_event)?; child.resume() };
    if let Err(error) = resumed {
        #[cfg(any(windows, target_os = "macos"))]
        if !resume_attempted {
            drop(running_child);
            if slot.exited.load(Ordering::Acquire) { slot.started = false; }
            if stop_requested(home, run_id) {
                drop(slot);
                emit(end(CardStatus::Stopped, None, None))?;
                return Ok(CardStatus::Stopped);
            }
            return Err(error);
        }
        if stop_requested(home, run_id) {
            drop(running_child);
            drop(slot);
            emit(end(CardStatus::Stopped, None, None))?;
            return Ok(CardStatus::Stopped);
        }
        return Err(error);
    }
    if let Some(mut stdin) = child.stdin.take() {
        let text = spec.stdin.clone().unwrap_or_default();
        thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }

    let err_log: PathBuf = cards_dir.join(format!("{}.err.log", card.id));
    let mut err_file = OpenOptions::new().create(true).append(true).open(err_log)?;
    let stderr = child.stderr.take().map(|mut stderr| {
        thread::spawn(move || {
            let mut tail = String::new();
            let mut chunk = [0u8; 8192];
            let mut failure = None;
            loop {
                let n = match stderr.read(&mut chunk) {
                    Ok(n) => n,
                    Err(error) => { failure.get_or_insert(error); break; }
                };
                if n == 0 {
                    break;
                }
                if let Err(error) = err_file.write_all(&chunk[..n]) { failure.get_or_insert(error); }
                tail.push_str(&String::from_utf8_lossy(&chunk[..n]));
                tail = js_tail(&tail, 400).to_string();
            }
            failure.map_or(Ok(tail), Err)
        })
    });

    let mut reader = StreamReader::new(card.provider);
    let mut last: Option<(bool, String)> = None;
    let mut last_message = String::new();
    let mut take = |signal: StreamSignal| -> io::Result<()> {
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
            StreamSignal::Usage { input, cached, output, complete } => emit(OEvent::CardUsage { at, card_id, input, cached, output, complete: Some(complete) })?,
            StreamSignal::Quota { window, used, resets_at } => {
                // A snapshot for the daily budget; the event stays the record either way.
                let reading = UsageWindow { provider: card.provider, window: window.clone(), group: None, used, resets_at: resets_at.clone() };
                let _ = budget::record(home, &[reading], Utc::now());
                emit(OEvent::Quota { at, provider: card.provider, window, used, resets_at })?
            }
            StreamSignal::Final { ok, text } => last = Some((ok, text)),
        }
        Ok(())
    };
    let raw_log = cards_dir.join(format!("{}.raw.jsonl", card.id));
    let mut raw_file = OpenOptions::new().create(true).append(true).open(raw_log)?;
    let mut pipes_incomplete = false;
    let mut ending_deadline = None;
    if let Some(stdout) = child.stdout.take() {
        {
            let (send, receive) = mpsc::sync_channel(32);
            let output_worker = thread::spawn(move || {
                let mut lines = BufReader::new(stdout);
                loop {
                    let mut buf = Vec::new();
                    match lines.read_until(b'\n', &mut buf) {
                        Ok(0) => break,
                        Ok(_) => if send.send(Ok(buf)).is_err() { break; },
                        Err(error) => { let _ = send.send(Err(error)); break; }
                    }
                }
            });
            loop {
                match receive.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(buf)) => {
                        let text = String::from_utf8_lossy(&buf);
                        let line = text.strip_suffix('\n').unwrap_or(&text);
                        let line = line.strip_suffix('\r').unwrap_or(line);
                        raw_file.write_all(&buf)?;
                        for signal in reader.read(line) { take(signal)?; }
                    }
                    Ok(Err(error)) => return Err(error),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                #[cfg(windows)]
                let ended = ending.load(Ordering::Acquire);
                #[cfg(not(windows))]
                let ended = stop_requested(home, run_id) || child.try_wait(&running_child._tree)?.is_some();
                if ended && Instant::now() >= *ending_deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(7)) { pipes_incomplete = true; break; }
            }
            drop(receive);
            if !pipes_incomplete { output_worker.join().map_err(|_| io::Error::other("표준 출력 기록 중 내부 오류"))?; }
        }
    }
    // Like Node's "close": stdout has ended and the process has exited.
    let stderr_tail = match stderr {
        Some(handle) => {
            {
                while !handle.is_finished() {
                    #[cfg(windows)]
                    let ended = ending.load(Ordering::Acquire);
                    #[cfg(not(windows))]
                    let ended = stop_requested(home, run_id) || child.try_wait(&running_child._tree)?.is_some();
                    if ended && Instant::now() >= *ending_deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(7)) { pipes_incomplete = true; break; }
                    thread::sleep(Duration::from_millis(25));
                }
                if handle.is_finished() { handle.join().map_err(|_| io::Error::other("표준 오류 기록 중 내부 오류"))?? }
                else { pipes_incomplete = true; String::new() }
            }
        }
        None => String::new(),
    };
    let status = loop {
        match child.try_wait(&running_child._tree) {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(error),
            Ok(None) => {
                #[cfg(windows)]
                let ended = ending.load(Ordering::Acquire);
                #[cfg(not(windows))]
                let ended = stop_requested(home, run_id);
                if pipes_incomplete || ended && Instant::now() >= *ending_deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(7)) {
                    break Err(io::Error::other("작업 종료를 확인하지 못했습니다"));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    };
    let (exit_code, error) = match status {
        Ok(status) => (proc::exit_code(status), None),
        Err(error) => (None, Some(format!("실행 실패: {error}"))),
    };
    drop(running_child);
    // A plain-text CLI (Devin) is read now that it has exited: its whole answer and its transcript.
    for signal in reader.finish(card, &TeamChannel { run_id, run_dir: &dir_text }, &stderr_tail) {
        take(signal)?;
    }
    let mut status = if stop_requested(home, run_id) {
        CardStatus::Stopped
    } else if (!pipes_incomplete || BEST_EFFORT_EXIT) && last.as_ref().is_some_and(|(ok, _)| *ok) && exit_code == Some(0) {
        CardStatus::Done
    } else {
        CardStatus::Failed
    };
    drop(followup_lock);
    // A failure with nothing on stdout usually explains itself on stderr.
    let mut result = error.or_else(|| {
        [
            last.map(|(_, text)| text).unwrap_or_default(),
            last_message,
            if status == CardStatus::Failed { stderr_tail.trim().to_string() } else { String::new() },
        ]
        .into_iter()
        .find(|text| !text.is_empty())
    });
    if pipes_incomplete {
        let warning = if BEST_EFFORT_EXIT { "작업 출력이 끝났는지 확인하지 못했습니다." } else { "작업 출력이 끝났는지 확인하지 못해 후속 작업을 시작하지 않았습니다." };
        result = Some(match result { Some(text) if !text.is_empty() => format!("{warning}\n\n{text}"), _ => warning.into() });
    }
    if pipes_incomplete && !BEST_EFFORT_EXIT { slot.exited.store(false, Ordering::Release); }
    let exited = slot.exited.load(Ordering::Acquire);
    drop(slot);
    let held = crate::run::store::resource_held(home, run_id, &card.id).unwrap_or(true);
    if status == CardStatus::Done && (!exited || held) {
        status = CardStatus::Failed;
        let warning = "작업 자손의 종료 또는 예약 해제를 확인하지 못해 후속 작업을 시작하지 않았습니다. o resources --json으로 확인하세요.";
        result = Some(match result { Some(text) if !text.is_empty() => format!("{warning}\n\n{text}"), _ => warning.into() });
    }
    if held {
        emit(OEvent::CardTool { at: now_iso(), card_id: card.id.clone(), name: "자원 예약".into(), summary: "작업 자손의 종료를 확인하지 못해 예약이 남았습니다. o resources --json으로 확인하세요.".into() })?;
    }
    emit(end(status, result, exit_code))?;
    Ok(status)
}
