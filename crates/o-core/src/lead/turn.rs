//! Runs a lead conversation turn and collects provider output.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn send_turn(home: &Path, req: &SendRequest, spawner: &Spawner, limit: Duration, start: Option<&StartRun<'_>>, worker: Option<WorkerTurn<'_>>, ask: bool, origin: Option<Origin<'_>>) -> Result<Vec<LeadEvent>, String> {
    let turn_started = Utc::now();
    let overall = worker.is_none() && overall::is_overall(home, req.project);
    let review = match origin { Some(Origin::Review(target)) => Some(target), _ => None };
    check_request(home, req)?;
    let thread_id = match &worker {
        Some(w) => w.thread_id.to_string(),
        None => select_thread(home, req.project, req.thread, req.fresh)?,
    };
    let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
    current.lock().map_err(|e| e.to_string())?;
    if let Some(Origin::Forwarded { source, at, index }) = origin {
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, source).join("dispatch.lock")).map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let project = thread_project(&read_thread(home, source)).ok_or("원본 총괄 대화를 확인할 수 없습니다")?;
        let at = chrono::DateTime::parse_from_rfc3339(at).map_err(|e| e.to_string())?;
        let stopped = match fs::read_to_string(thread_dir(home, source).join("stop-all-at")) {
            Ok(stopped) => chrono::DateTime::parse_from_rfc3339(&stopped).map_err(|e| e.to_string())? >= at,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.to_string()),
        };
        if stopped || !is_open(home, &project, source) {
            return Err("원본 총괄이 중지되었거나 닫혀 요청을 넘기지 않았습니다".into());
        }
        let marks = thread_dir(home, source).join("forwarded");
        fs::create_dir_all(&marks).map_err(|e| e.to_string())?;
        OpenOptions::new().write(true).create_new(true).open(marks.join(index.to_string())).map_err(|_| "이미 넘긴 요청입니다".to_string())?;
        drop(lock);
    }
    if !is_open(home, req.project, &thread_id) {
        return Err("총괄 대화가 닫혀 시작하지 않았습니다".into());
    }
    let writer = !overall && tabs::is_writer(home, req.project, &thread_id)?;
    let mut slot = TurnSlot::take(home, &thread_id, req.provider)?;
    drop(current);
    // Writer selection and acquiring this turn are atomic; transfers reject a live turn.
    let read_only = SendRequest { yolo: false, ..*req };
    let req = if writer { req } else { &read_only };
    let ask = ask && writer;
    if let Some(w) = &worker {
        if req.yolo || store::stop_requested(home, &w.speaker.run_id) || !is_open(home, req.project, &thread_id)
            || run_thread(home, req.project, &w.speaker.run_id).as_deref() != Some(&thread_id) {
            return Err("실행과 열린 총괄 대화가 일치하지 않습니다".into());
        }
        read_complete_thread(home, &thread_id)?;
        let claimed = team::claim(home, w.speaker).map_err(|e| e.to_string())?;
        if claimed.as_ref().is_none_or(|m| m.id != w.message.id) { return Err("다른 메시지가 먼저 접수되어 상태 확인이 필요합니다".into()); }
    }
    let pending: Vec<_> = if worker.is_some() { Vec::new() } else { notes::read(home, &thread_id).map_err(|e| format!("설계 주석을 읽지 못했습니다: {e}"))?.into_iter().filter(|note| !note.sent).collect() };
    let mut user_text = req.text.trim().to_string();
    if !pending.is_empty() {
        user_text.push_str("\n\n[설계에 남긴 주석]\n");
        for note in &pending { user_text.push_str(&format!("- [{}] “{}”: {}\n", note.tag, note.quote, note.text)); }
        if js_len(&user_text) > LIMITS.max_task_chars { return Err("메시지와 주석을 합친 길이가 너무 깁니다. 주석을 줄인 뒤 다시 보내세요".into()); }
    }
    let progress_path = thread_dir(home, &thread_id).join("progress.json");
    let _ = fs::write(&progress_path, "[]");
    let before = if worker.is_some() { read_complete_thread(home, &thread_id)? } else { read_thread(home, &thread_id) };
    let attachments = req.attach.iter().map(|path| files::import(home, &thread_id, path)).collect::<Result<Vec<_>, _>>()?;

    let mut added = Vec::new();
    let mut emit = |event: LeadEvent| -> Result<(), String> {
        append(home, &thread_id, &event).map_err(|e| e.to_string())?;
        added.push(event);
        Ok(())
    };
    if let Some(w) = &worker {
        emit(LeadEvent::Worker { at: now_iso(), run_id: w.speaker.run_id.clone(), card_id: w.message.from.clone(), message_id: w.message.id.clone(), text: user_text.clone() })?;
    } else { emit(LeadEvent::User {
        at: now_iso(),
        text: user_text.clone(),
        attachments: attachments.clone(),
        provider: req.provider,
        model: req.model.filter(|m| !m.is_empty()).map(str::to_string),
        effort: req.effort.filter(|e| !e.is_empty()).map(str::to_string),
        profile: Some(Profile::of_clean(req.clean.is_some())),
        yolo: req.yolo,
        origin: origin.map(|origin| match origin { Origin::Forwarded { .. } => ORIGIN_OVERALL, Origin::Review(_) => ORIGIN_REVIEW }.to_string()),
    })?; }

    let app_server = req.provider == ProviderId::Codex
        && !req.yolo
        && spawner(&providers::codex::app_server_probe(), req.project).is_ok_and(|launch| providers::codex::supports_app_server(launch.command()));
    // Normal mode with someone to ask: Codex app-server asks O itself, Claude asks O's approval server.
    let wanted = ask && !req.yolo && worker.is_none() && matches!(req.provider, ProviderId::Claude | ProviderId::Codex);
    let program = if wanted && req.provider == ProviderId::Claude { approval_program() } else { None };
    let ask = wanted && (if req.provider == ProviderId::Codex { app_server } else { program.is_some() });
    if wanted && !ask {
        emit(LeadEvent::Warning { at: now_iso(), text: "작업 승인을 받을 수 없어 이번 총괄은 읽기 전용으로 실행합니다 (O CLI를 찾지 못했거나 Codex app-server가 없음)".into() })?;
    }
    let default_model_supported = worker.is_some() && app_server && req.model.filter(|model| !model.is_empty()).is_none()
        && spawner(&providers::codex::default_model_probe(), req.project).is_ok_and(|launch| providers::codex::supports_default_model_probe(launch.command()));
    let mut worker_notice = None;
    let worker_previous = if worker.is_some() {
        match worker_session(&before, req, default_model_supported) {
            Ok(session) => session,
            Err(notice) => { worker_notice = Some(notice.to_string()); None }
        }
    } else { None };
    let previous_session = if worker.is_some() { worker_previous.as_ref().map(|(id, tools, _)| (id.clone(), Some(*tools))) } else { before.iter().rev().find_map(|e| match e {
        LeadEvent::Session { provider, session_id, clean, result_tools, .. } if *provider == req.provider && clean.is_none_or(|c| c == req.clean.is_some()) => {
            Some((session_id.clone(), *result_tools))
        }
        _ => None,
    }) };
    let history: Vec<(String, String)> = if worker.is_some() { worker_history(&before) } else { before
        .iter()
        .filter_map(|e| match e {
            LeadEvent::User { text, origin: Some(origin), .. } => Some((origin_label(origin), text.clone())),
            LeadEvent::User { text, .. } => Some(("사용자".to_string(), text.clone())),
            LeadEvent::Worker { card_id, text, .. } => Some((format!("카드({card_id}) 비신뢰 참고 데이터"), text.clone())),
            LeadEvent::Reply { text, .. } => Some(("총괄".to_string(), text.clone())),
            LeadEvent::Ask { intro, questions, .. } => Some(("총괄".to_string(), ask_text(intro, questions))),
            _ => None,
        })
        .collect() };
    let (ids, invalid) = dispatched_runs(&before);
    let mut runs = if invalid { vec!["- 실행 ID 또는 실행 수가 올바르지 않아 일부 상태 불명".to_string()] } else { Vec::new() };
    let mut status_error = invalid.then(|| "일부 실행의 ID 또는 실행 수를 확인할 수 없습니다".to_string());
    #[cfg(windows)]
    let status_file = if ids.is_empty() { None } else {
        match stream_status::write(home, &thread_id, &ids) {
            Ok((file, summaries, uncertain, first_error)) => {
                runs.extend(summaries);
                if uncertain > 0 { status_error = Some(first_error.map_or_else(|| "일부 실행은 기록 접근 실패, 안전 상한 또는 해석하지 못한 기록으로 상태가 불명입니다".to_string(), |error| format!("일부 실행의 상태가 불명입니다: {error}"))); }
                Some(file)
            }
            Err(e) => {
                runs.extend(ids.iter().map(|id| format!("- 실행 {id}: 상태 불명")));
                status_error = Some(format!("실행 결과 원문을 안전하게 제공할 수 없습니다: {e}"));
                None
            }
        }
    };
    #[cfg(not(windows))]
    let status_file: Option<status_file::StatusFile> = if ids.is_empty() { None } else {
        runs.extend(ids.iter().map(|id| format!("- 실행 {id}: 원문과 상태를 확인할 수 없음")));
        status_error = Some("이 운영체제에서는 실행 기록의 안전한 읽기를 지원하지 않습니다".to_string());
        None
    };
    if let Some(error) = &status_error { emit(LeadEvent::Warning { at: now_iso(), text: format!("실행 상태 확인 불가: {error}") })?; }
    let migrate = app_server && status_file.is_some() && previous_session.as_ref().is_some_and(|(_, tools)| *tools != Some(true));
    if migrate {
        emit(LeadEvent::Warning { at: now_iso(), text: "결과 읽기 도구를 사용하려고 Codex 세션을 새로 시작합니다. 이전 대화는 최근 일부만 전달되며 캐시가 초기화될 수 있습니다.".into() })?;
        if worker.is_some() { worker_notice = Some("결과 읽기 도구를 사용하려고 카드 질문용 새 세션을 시작했습니다".into()); }
    }
    let result_tools = cfg!(windows) && app_server && (previous_session.is_none() || migrate || previous_session.as_ref().is_some_and(|(_, tools)| *tools == Some(true)));
    let session = if migrate { None } else { previous_session.map(|(id, _)| id) };
    let resolve_default_model = default_model_supported && session.is_some();
    let missed = if let Some((_, _, checkpoint)) = worker_previous.as_ref().filter(|_| session.is_some()) {
        worker_history(&before[checkpoint + 1..])
    } else {
        session.as_deref().map(|id| missed_turns(&before, req.provider, id)).unwrap_or_default()
    };
    if worker.is_some() && ((session.is_none() && (history.len() > 6 || history.iter().any(|(_, text)| js_len(text.trim()) > 500)))
        || (session.is_some() && (missed.len() > 12 || missed.iter().any(|(_, text)| js_len(text.trim()) > 500)))) {
        worker_notice = Some(format!("{}대화 이력 일부가 생략되어 빠진 내용은 확인할 수 없습니다", worker_notice.map_or(String::new(), |notice| format!("{notice}. "))));
    }
    let status_path = status_file.as_ref().map(|f| f.path.to_string_lossy().into_owned());
    let budget = budget_lines(home);
    let design = before.iter().rev().find_map(|event| match event { LeadEvent::Design { design, .. } => serde_json::to_string(design).ok(), _ => None });
    let overview = overall.then(|| overall::overview(home));
    let input = prompt::PromptInput {
        project: req.project,
        text: &user_text,
        attachments: &attachments,
        budget: &budget,
        runs: &runs,
        runs_file: status_path.as_deref(),
        runs_error: status_error.as_deref(),
        result_tools,
        resumed: session.is_some(),
        history: &history,
        missed: &missed,
        design: design.as_deref(),
        yolo: req.yolo,
        approvals: ask,
        overview: overview.as_deref(),
        forwarded: matches!(origin, Some(Origin::Forwarded { .. })),
        review: review.map(|target| target.data.as_str()),
    };
    let text = if worker.is_some() { prompt::build_worker(&input) } else { prompt::build(&input) };
    let files_dir = status_file.as_ref().and_then(|f| f.path.parent()).map(Path::to_path_buf).unwrap_or_else(|| files::thread_files(home, &thread_id));
    let files_dir = files_dir.exists().then(|| files_dir.to_string_lossy().into_owned());
    let images: Vec<String> = attachments.iter().filter(|a| a.image).map(|a| a.path.clone()).collect();
    let turn = LeadTurn {
        project: req.project,
        prompt: text,
        session: session.as_deref(),
        model: req.model.filter(|m| !m.is_empty()),
        effort: req.effort.filter(|e| !e.is_empty()),
        files_dir: files_dir.as_deref(),
        images: &images,
        app_server,
        approver: program.as_ref().filter(|_| ask).map(|program| approvals::server_command(program, home, &thread_id, &slot.marker.token, req.project)),
    };
    let Some(spec) = providers::lead_spec(req.provider, &turn, req.yolo) else {
        return Err(format!("{}는 총괄로 쓸 수 없습니다", req.provider));
    };

    let dir = thread_dir(home, &thread_id);
    let stopped = || LeadEvent::Error { at: now_iso(), text: "총괄을 중지했습니다".into() };
    let command = spawner(&spec, req.project).and_then(|mut command| {
        if let Some(root) = req.clean {
            // ADR-0015: a lead on the project_O profile needs that profile signed in.
            if let Some(why) = clean::not_ready(root, &[req.provider]).remove(&req.provider) {
                return Err(io::Error::other(why));
            }
            let env = clean::environment(root, req.provider)?;
            command.env_clear().envs(env);
        }
        command.envs(&spec.env);
        command.env("O_AGENT_ROLE", "lead");
        Ok(command)
    });
    // A stop that came while the turn set up (attachments, budget, the command) ends it here,
    // before the CLI can spend anything.
    if slot.stop_asked() {
        emit(stopped())?;
        return Ok(added);
    }
    let spawned = command.and_then(|command| {
        #[cfg(all(test, any(windows, target_os = "macos")))]
        if turn_process::NO_JOB.get() {
            let mut command = command.command();
            slot.marker.unresolved = Some(true);
            slot.keep_marker = true;
            if let Err(error) = slot.write() { slot.keep_marker = false; return Err(error); }
            let child = match proc::hide_window(&mut command).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn() {
                Ok(child) => child,
                Err(error) => { slot.keep_marker = false; return Err(error); }
            };
            return Ok((crate::sys::worker::Child::standard(child), None));
        }
        #[cfg(windows)]
        {
            let native = command.native(std::env::vars_os())?;
            let executable = match command.handle_owner_executable {
                Some(path) => path,
                None => {
                    let current = std::env::current_exe()?;
                    if current.file_name().is_some_and(|name| name.eq_ignore_ascii_case("o.exe")) { current }
                    else { current.parent().ok_or_else(|| io::Error::other("총괄 도우미 위치를 찾지 못했습니다"))?.join("o.exe") }
                }
            };
            let owner = crate::sys::win_launch::HandleOwner::start(executable, "--o-handle-owner")?;
            if slot.stop_asked() { return Err(io::Error::new(io::ErrorKind::Interrupted, "총괄이 중지되었습니다")); }
            slot.marker.unresolved = Some(true);
            slot.keep_marker = true;
            if let Err(error) = slot.write() { slot.keep_marker = false; return Err(error); }
            match native.spawn(&owner) {
                Ok((child, tree)) => Ok((crate::sys::worker::Child::native(child), Some(Arc::new(Mutex::new(tree))))),
                Err(error) => { slot.keep_marker = false; Err(error) }
            }
        }
        #[cfg(target_os = "macos")]
        {
            slot.marker.unresolved = Some(true);
            slot.keep_marker = true;
            if let Err(error) = slot.write() { slot.keep_marker = false; return Err(error); }
            match crate::sys::macos_worker::spawn(&command) {
                Ok((child, tree)) => Ok((crate::sys::worker::Child::mac(child), Some(tree))),
                Err(error) => { slot.keep_marker = false; Err(error) }
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let mut command = command.command();
            slot.marker.unresolved = Some(true);
            slot.keep_marker = true;
            if let Err(error) = slot.write() { slot.keep_marker = false; return Err(error); }
            let child = match proc::hide_window(&mut command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
                Ok(child) => child,
                Err(error) => { slot.keep_marker = false; return Err(error); }
            };
            let tree = proc::contain(&child).map(|tree| Arc::new(Mutex::new(tree)));
            Ok((crate::sys::worker::Child::standard(child), tree))
        }
    });
    let (mut child, tree) = match spawned {
        Ok(child) => child,
        Err(error) => {
            if slot.stop_asked() { emit(stopped())?; return Ok(added); }
            emit(LeadEvent::Error { at: now_iso(), text: format!("총괄 CLI를 시작하지 못했습니다: {error}") })?;
            return Ok(added);
        }
    };
    let contained = tree.is_some();
    if let Err(error) = slot.set_child(child.id()) {
        slot.keep_marker = true;
        slot.marker.unresolved = Some(true);
        let _ = slot.write();
        let _ = child.kill();
        return Err(format!("총괄 프로세스 상태를 기록하지 못했습니다: {error}"));
    }
    #[cfg(any(windows, target_os = "macos"))]
    if slot.stop_asked() {
        let confirmed = tree.as_ref().is_some_and(|tree| tree.lock().is_ok_and(|mut tree| tree.stop_and_confirm())) && child.wait(&tree).is_ok();
        if confirmed { slot.keep_marker = false; }
        emit(if confirmed { stopped() } else { LeadEvent::Error { at: now_iso(), text: "총괄 중지 후 자손 종료 상태가 불명입니다".into() } })?;
        return Ok(added);
    }
    #[cfg(any(windows, target_os = "macos"))]
    if contained { if let Err(error) = child.resume() { return Err(format!("총괄 시작 상태가 불명입니다: {error}")); } }
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let token = slot.marker.token.clone();
    let process = turn_process::TurnProcess::start(child, tree, slot.stop.clone(), slot.marker.token.clone(), limit, ask.then(|| approvals::folder(home, &thread_id)));
    let mut stdin_worker = None;
    let mut server_stdin = None;
    if app_server {
        if let Some(stdin) = stdin.take() {
            let (writer, worker) = process.writer(stdin);
            server_stdin = Some(writer);
            stdin_worker = Some(worker);
        }
    } else if let Some(mut stdin) = stdin.take() {
        let text = spec.stdin.clone().unwrap_or_default();
        stdin_worker = Some(thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        }));
    }
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    let stderr = stderr_pipe.map(|mut stderr| {
        let tail = Arc::clone(&stderr_tail);
        thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut tail = tail.lock().unwrap();
                tail.push_str(&String::from_utf8_lossy(&chunk[..n]));
                *tail = js_tail(&tail, 400).to_string();
            }
        })
    });
    let (stdout, stdout_worker) = match stdout {
        Some(stdout) => { let (reader, worker) = process.reader(stdout); (Some(reader), Some(worker)) }
        None => (None, None),
    };

    let mut reader = StreamReader::new(req.provider);
    // The session this turn ran in: the resumed one, or the one the stream names.
    let mut used_session = if app_server { None } else { session.clone() };
    let (mut input, mut cached, mut output) = (0.0, 0.0, 0.0);
    let mut used = false;
    let mut usage_complete = true;
    let mut last_message = String::new();
    let mut last: Option<(bool, String)> = None;
    let mut limited = false;
    let mut progress = Vec::<Value>::new();
    let mut counts = [0u64; 4];
    let mut partial = String::new();
    let mut partial_written = None::<Instant>;
    let mut reported_files = BTreeSet::new();
    let mut tracker = steps::Tracker::default();
    // An error that ends the turn after cleanup, and whether app-server finished the turn successfully.
    let mut fatal = None::<String>;
    let mut finished = false;
    let mut provider_finished = false;
    let mut raw_issue = None::<String>;
    if let Some(stdout) = stdout {
        let mut raw = match OpenOptions::new().create(true).append(true).open(dir.join("turn.raw.jsonl")) {
            Ok(file) => Some(file),
            Err(error) => { raw_issue = Some(error.to_string()); None }
        };
        let mut process_line = |line: &str| -> Result<(), String> {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                if value["type"] == "o.delta" {
                    if let Some(delta) = value["delta"].as_str() {
                        // shortcut: previews stop at 64 KiB; upgrade if longer live previews are needed.
                        for ch in delta.chars() {
                            if partial.len() + ch.len_utf8() > 65_536 { break; }
                            partial.push(ch);
                        }
                        if partial_written.is_none_or(|at| at.elapsed() >= Duration::from_millis(100)) {
                            let state = progress_state(&progress, &counts, partial_prose(&partial), tracker.steps());
                            if let Ok(text) = serde_json::to_string(&state) { let _ = fs::write(&progress_path, text); }
                            partial_written = Some(Instant::now());
                        }
                    }
                    return Ok(());
                }
                if value["type"] == "item.completed" && value["item"]["type"] == "agent_message" {
                    partial.clear();
                    partial_written = None;
                }
                let before = tracker.total;
                let finished = tracker.read(&value);
                // A card question's turn keeps to its reply: its steps are not part of the person's chat.
                if worker.is_none() { for step in finished.iter().cloned() { emit(LeadEvent::step(step))?; } }
                if tracker.total != before || !finished.is_empty() {
                    let state = progress_state(&progress, &counts, partial_prose(&partial), tracker.steps());
                    if let Ok(text) = serde_json::to_string(&state) { let _ = fs::write(&progress_path, text); }
                }
                if req.provider == ProviderId::Codex && value["type"] == "item.completed" && value["item"]["type"] == "file_change" && value["item"]["status"] != "failed" && value["item"]["status"] != "declined" {
                    if let Some(changes) = value["item"]["changes"].as_array() {
                        for path in changes.iter().filter_map(|change| change["path"].as_str()).take(100) {
                            if reported_files.len() < 100 && !path.is_empty() { reported_files.insert(path.chars().take(1024).collect::<String>()); }
                        }
                    }
                }
            }
            if let Some(file) = raw.as_mut() {
                if let Err(error) = file.write_all(format!("{line}\n").as_bytes()) {
                    raw_issue = Some(error.to_string());
                    raw = None;
                }
            }
            for signal in reader.read(line) {
                match signal {
                    StreamSignal::Session { session_id } => {
                        if worker.is_none() && session.as_deref() != Some(session_id.as_str()) {
                            emit(LeadEvent::Session { at: now_iso(), provider: req.provider, session_id: session_id.clone(), clean: Some(req.clean.is_some()), result_tools: result_tools.then_some(true) })?;
                            index_session(home, &thread_id, &session_id);
                        }
                        if worker.is_some() && session.as_deref() != Some(session_id.as_str()) { index_session(home, &thread_id, &session_id); }
                        used_session = Some(session_id);
                    }
                    StreamSignal::Message { text } => last_message = text,
                    StreamSignal::Usage { input: i, cached: c, output: o, complete } => {
                        (input, cached, output, used) = (input + i, cached + c, output + o, true);
                        usage_complete &= complete;
                    }
                    StreamSignal::Limit { until } => {
                        limited = true;
                        let _ = store::set_cooldown(home, req.provider, &until, Utc::now());
                    }
                    StreamSignal::Quota { window, used: share, resets_at } => {
                        let reading = UsageWindow { provider: req.provider, window, group: None, used: share, resets_at };
                        let _ = budget::record(home, &[reading], Utc::now());
                    }
                    StreamSignal::Final { ok, text } => last = Some((ok, text)),
                    StreamSignal::Tool { name, summary } => {
                        match name.to_ascii_lowercase().as_str() {
                            "read" | "read_file" => counts[0] += 1,
                            "glob" | "grep" | "search" | "web_search" => counts[1] += 1,
                            "edit" | "write" | "multiedit" | "edit_file" | "write_file" | "apply_patch" => counts[2] += 1,
                            "bash" | "shell" | "command" | "execute_command" => counts[3] += 1,
                            _ => {}
                        }
                        progress.push(json!({ "name": name, "summary": summary }));
                        if progress.len() > 3 { progress.remove(0); }
                        let state = progress_state(&progress, &counts, partial_prose(&partial), tracker.steps());
                        if let Ok(text) = serde_json::to_string(&state) { let _ = fs::write(&progress_path, text); }
                    }
                }
            }
            Ok(())
        };
        let mut protocol_error = None;
        if app_server {
            match server_stdin.take() {
                None => fatal = Some("Codex app-server 입력을 열지 못했습니다".into()),
                Some(stdin) => {
                    let stdin = BufWriter::new(stdin);
                    let on_event = |event| {
                        if fatal.is_none() {
                            provider_finished |= matches!(event, providers::codex_app_server::Event::Finished { .. });
                            finished |= matches!(event, providers::codex_app_server::Event::Finished { ok: true, .. });
                            if let Some(line) = providers::codex_app_server::stream_line(event) {
                                fatal = process_line(&line).err();
                            }
                        }
                    };
                    #[cfg(windows)]
                    let mut read_result = |request: providers::codex_app_server::CardResultRequest| {
                            let guard = status_file.as_ref().ok_or("현재 실행 결과가 없습니다")?;
                            let result = if request.run_id == "*" && request.card_id == "*" && request.line.is_none() {
                                stream_status::list_results(guard, request.offset.unwrap_or(0))
                            } else {
                                stream_status::read_card_result(guard, &request.run_id, &request.card_id, request.line, request.offset.unwrap_or(0))
                            };
                            result
                                .map(|value| value.to_string()).map_err(|error| error.to_string())
                    };
                    #[cfg(windows)]
                    let reader = result_tools.then_some(&mut read_result as &mut dyn FnMut(_) -> _);
                    #[cfg(not(windows))]
                    let reader = None;
                    // Each command or file change outside the read-only sandbox waits for the person.
                    let mut approve = |request: providers::codex_app_server::Approval| -> &'static str {
                        let request = match &request {
                            providers::codex_app_server::Approval::Command(params) => approvals::from_codex_command(params),
                            providers::codex_app_server::Approval::Files(params, changes) => approvals::from_codex_files(params, changes),
                        };
                        match approvals::ask(home, &thread_id, &token, req.project, request, approvals::WAIT, &mut || {}).decision {
                            "allow" => "accept",
                            "stopped" => "cancel",
                            _ => "decline",
                        }
                    };
                    let approver = ask.then_some(&mut approve as providers::codex_app_server::Approver<'_>);
                    let result = providers::codex_app_server::run_turn(&turn, BufReader::new(stdout), stdin, on_event, reader, approver, resolve_default_model);
                    protocol_error = result.err();
                }
            }
        } else {
            let mut lines = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match lines.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Err(error) => { fatal = Some(format!("총괄 출력을 끝까지 읽지 못했습니다: {error}")); break; }
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&buf);
                if let Err(error) = process_line(text.trim_end_matches(['\r', '\n'])) {
                    fatal = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = protocol_error { last = Some((false, error)); }
    }
    drop(server_stdin);
    let _ = fs::write(&progress_path, "[]");
    // Steps the turn never finished (stopped, timed out, cut off) stay on record as incomplete.
    if worker.is_none() { for step in tracker.unfinished() { emit(LeadEvent::step(step))?; } }
    // The turn ends with an error below; the CLI must not outlive it.
    if fatal.is_some() { process.kill(); }
    // app-server is a long-lived process; do not let a stuck shutdown keep the turn open forever.
    // A turn it already finished keeps its result: only the process is ended.
    if app_server && fatal.is_none() {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if process.poll().is_some() || process.unknown() || process.cancelled() { break; }
            if Instant::now() >= deadline {
                process.kill();
                if !finished {
                    last = Some((false, "Codex app-server가 종료되지 않았습니다. 대화 상태를 확인하세요.".into()));
                }
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    let mut grace = None;
    let status = loop {
        let status = process.poll();
        let pipes_done = stdout_worker.as_ref().is_none_or(|h| h.is_finished()) && stderr.as_ref().is_none_or(|h| h.is_finished())
            && stdin_worker.as_ref().is_none_or(|h| h.is_finished());
        if status.is_some() && pipes_done { break status; }
        if process.unknown() { process.kill(); break None; }
        if process.cancelled() && Instant::now() >= *grace.get_or_insert_with(|| Instant::now() + Duration::from_secs(1)) {
            break status;
        }
        thread::sleep(Duration::from_millis(25));
    };
    let incomplete = status.is_none() || stdout_worker.as_ref().is_some_and(|h| !h.is_finished()) || stderr.as_ref().is_some_and(|h| !h.is_finished())
        || stdin_worker.as_ref().is_some_and(|h| !h.is_finished());
    let stop_asked = slot.stop_asked();
    let (reason, reap, tree_confirmed) = process.finish();
    // Requests nobody answered are closed: the turn that asked them is over.
    if ask { approvals::close_left(home, &thread_id); }
    let stderr_tail = stderr_tail.lock().unwrap().clone();
    let mut unknown_saved = true;
    // Without a job, closed pipes cannot prove that detached descendants have exited.
    let unresolved = incomplete || (!contained && reason != turn_process::End::Complete) || (cfg!(any(windows, target_os = "macos")) && (!contained || !tree_confirmed));
    if unresolved {
        slot.marker.unresolved = Some(true);
        unknown_saved = slot.write().is_ok();
        slot.keep_marker = true;
    } else {
        slot.keep_marker = false;
    }
    if incomplete {
        // Keep the turn locked until the process and inherited pipes actually finish.
        thread::spawn(move || {
            reap.wait();
            if let Some(worker) = stdout_worker { let _ = worker.join(); }
            if let Some(worker) = stderr { let _ = worker.join(); }
            if let Some(worker) = stdin_worker { let _ = worker.join(); }
            if !unknown_saved { loop { thread::park(); } }
            drop(slot);
        });
    } else if unresolved && !unknown_saved {
        thread::spawn(move || { let _slot = slot; loop { thread::park(); } });
    }
    if used {
        emit(LeadEvent::Usage { at: now_iso(), provider: req.provider, input, cached, output, complete: Some(usage_complete && (!app_server || provider_finished) && !incomplete) })?;
    }
    if let Some(error) = raw_issue {
        emit(LeadEvent::Error { at: now_iso(), text: format!("총괄 원시 출력 기록이 불완전합니다: {error}") })?;
    }
    if incomplete {
        let text = match reason {
            turn_process::End::Timeout => "총괄이 제한 시간을 넘겼고 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요.",
            turn_process::End::Stop => "총괄 중지를 요청했지만 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요.",
            _ => "총괄의 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요.",
        };
        let mut text = if unknown_saved { text.to_string() } else { format!("{text} 불명 상태를 저장하지 못했습니다.") };
        if !stderr_tail.trim().is_empty() { text.push_str(&format!("\n최근 오류 출력: {}", stderr_tail.trim())); }
        emit(LeadEvent::Error { at: now_iso(), text })?;
        return Ok(added);
    }
    let timed_out = reason == turn_process::End::Timeout;
    let exit = status.and_then(proc::exit_code);
    if let Some(error) = fatal {
        return Err(if unresolved { format!("{error} 총괄 자손의 종료 상태도 불명입니다.") } else { error });
    }
    // The CLI got this turn's message: its session has seen the thread through it.
    let session_seen = used_session.is_some();
    if let Some(session_id) = used_session.as_ref().filter(|_| worker.is_none()) {
        emit(LeadEvent::Seen { at: now_iso(), provider: req.provider, session_id: session_id.clone(), through: before.len() })?;
        if !pending.is_empty() && notes::change(home, &thread_id, notes::Change::Sent { notes: pending.clone() }).is_err() {
            emit(LeadEvent::Error { at: now_iso(), text: "주석은 전달했지만 전달 상태를 저장하지 못했습니다. 다시 보내기 전에 주석을 확인하세요".into() })?;
        }
    }
    if stop_asked || reason == turn_process::End::Stop {
        let text = if unknown_saved { "총괄 중지를 요청했지만 자손 프로세스 종료 여부는 불명입니다." }
            else { "총괄 중지를 요청했지만 자손 종료 여부가 불명이며 이 상태를 저장하지 못했습니다." };
        emit(if contained && !unresolved { stopped() } else { LeadEvent::Error { at: now_iso(), text: text.into() } })?;
        return Ok(added);
    }
    if timed_out {
        let minutes = limit.as_secs().div_ceil(60);
        let text = if contained && !unresolved { format!("총괄이 제한 시간({minutes}분) 안에 답하지 않아 멈췄습니다. 요청을 나눠 다시 보내세요") }
            else if unknown_saved { format!("총괄이 제한 시간({minutes}분)을 넘겨 종료를 요청했지만 자손 프로세스 종료 여부는 불명입니다.") }
            else { format!("총괄이 제한 시간({minutes}분)을 넘겼고 자손 종료 여부가 불명이며 이 상태를 저장하지 못했습니다.") };
        emit(LeadEvent::Error { at: now_iso(), text })?;
        return Ok(added);
    }
    if unresolved {
        let text = if unknown_saved { "총괄 자손의 종료 상태가 불명입니다." } else { "총괄 자손의 종료 상태가 불명이며 이 상태를 저장하지 못했습니다." };
        emit(LeadEvent::Warning { at: now_iso(), text: text.into() })?;
    }
    // A turn app-server finished needs no clean exit: a slow shutdown was ended above.
    let ok = last.as_ref().is_some_and(|(ok, _)| *ok) && (exit == Some(0) || finished);
    let answer = last.map(|(_, text)| text).filter(|t| !t.trim().is_empty()).unwrap_or(last_message);
    if !ok {
        let why = if limited {
            format!("{} 사용량 한도에 걸렸습니다. 다른 에이전트를 고르거나 한도가 풀린 뒤 다시 보내세요", req.provider)
        } else {
            [answer.trim(), stderr_tail.trim()].into_iter().find(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| format!("종료 코드 {exit:?}"))
        };
        emit(LeadEvent::Error { at: now_iso(), text: format!("총괄이 답하지 못했습니다: {why}") })?;
        return Ok(added);
    }

    if worker.is_some() {
        worker_reply::reply(home, req, &thread_id, &worker, answer, worker_notice, &session, &used_session, &before, default_model_supported, result_tools, &mut emit)?;
        return Ok(added);
    }

    if !session_seen && !pending.is_empty() && notes::change(home, &thread_id, notes::Change::Sent { notes: pending }).is_err() {
        emit(LeadEvent::Error { at: now_iso(), text: "주석은 전달했지만 전달 상태를 저장하지 못했습니다. 다시 보내기 전에 주석을 확인하세요".into() })?;
    }

    answer::process_answer(home, req, &thread_id, &answer, &before, reported_files, overall, review, migrate, unresolved, turn_started, start, &mut emit)?;
    Ok(added)
}
