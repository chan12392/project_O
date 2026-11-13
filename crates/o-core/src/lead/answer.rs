//! Interprets a lead answer and records its requested actions.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn process_answer(home: &Path, req: &SendRequest, thread_id: &str, answer: &str, before: &[LeadEvent], reported_files: BTreeSet<String>, overall: bool, review: Option<&overall::ReviewTarget>, migrate: bool, unresolved: bool, turn_started: DateTime<Utc>, start: Option<&StartRun<'_>>, emit: &mut dyn FnMut(LeadEvent) -> Result<(), String>) -> Result<(), String> {
    let (prose, plan) = prompt::split_plan(answer);
    let (prose, design) = prompt::split_design(&prose);
    let (prose, followups) = prompt::split_followups(&prose);
    let (prose, forwards) = if overall { prompt::split_forwards(&prose) } else { (prose, Ok(None)) };
    let (prose, verdict) = if overall { prompt::split_review(&prose) } else { (prose, Ok(None)) };
    let prose = if migrate {
        format!("결과 읽기 도구를 사용하려고 Codex 세션을 새로 시작했습니다. 이전 대화는 최근 일부만 전달되었으며 캐시가 초기화될 수 있습니다.\n\n{prose}")
    } else { prose };
    let (prose, ask) = prompt::split_ask(&prose);
    match ask {
        Ok(None) => {}
        // Questions stand in for the answer: a plan, design or follow-up written beside them
        // is not taken, since the person has not chosen yet (the lead is told not to mix them).
        Ok(Some(questions)) => {
            let asks = before.iter().filter(|e| matches!(e, LeadEvent::Ask { .. })).count();
            emit(LeadEvent::Ask { at: now_iso(), ask_id: format!("{thread_id}-q{}", asks + 1), provider: req.provider, intro: prose, questions })?;
            return Ok(());
        }
        Err(message) => {
            if !prose.is_empty() {
                emit(LeadEvent::Reply { at: now_iso(), provider: req.provider, text: prose, reported_files: reported_files.into_iter().collect() })?;
            }
            emit(LeadEvent::Error { at: now_iso(), text: message })?;
            return Ok(());
        }
    }
    if !prose.is_empty() {
        emit(LeadEvent::Reply { at: now_iso(), provider: req.provider, text: prose, reported_files: reported_files.into_iter().collect() })?;
    }
    if overall {
        if !matches!(plan, Ok(None)) || !matches!(followups, Ok(None)) {
            emit(LeadEvent::Error { at: now_iso(), text: "전체 총괄은 계획·후속 지시를 내지 않습니다. 일은 o-forward로 프로젝트 총괄에게 넘길 요청을 제안합니다".into() })?;
            return Ok(());
        }
        // A review turn's answer is its verdict alone: anything else beside it, and the plan waits for the person.
        if let Some(target) = review {
            let verdict = match verdict {
                Ok(Some(verdict)) if matches!(design, Ok(None)) && matches!(forwards, Ok(None)) => verdict,
                _ => {
                    emit(LeadEvent::Error { at: now_iso(), text: "검토 답에 o-review 블록 하나만 있어야 합니다. 계획은 사람의 실행을 기다립니다".into() })?;
                    return Ok(());
                }
            };
            if unresolved { emit(LeadEvent::Error { at: now_iso(), text: "총괄 종료 상태가 불명이라 검토 결과를 쓰지 않았습니다".into() })?; return Ok(()); }
            emit(LeadEvent::Review { at: now_iso(), plan_id: target.plan_id.clone(), project: target.project.clone(), verdict: verdict.verdict, notes: verdict.notes.trim().to_string() })?;
            return Ok(());
        }
        if !matches!(verdict, Ok(None)) {
            emit(LeadEvent::Error { at: now_iso(), text: "검토 블록은 O가 검토를 요청한 턴에만 씁니다".into() })?;
            return Ok(());
        }
        match design {
            Ok(Some(design)) => emit(LeadEvent::Design { at: now_iso(), design })?,
            Ok(None) => {},
            Err(message) => { emit(LeadEvent::Error { at: now_iso(), text: message })?; return Ok(()); }
        }
        let items = match forwards {
            Ok(None) => return Ok(()),
            Err(message) => { emit(LeadEvent::Error { at: now_iso(), text: message })?; return Ok(()); }
            Ok(Some(items)) => items,
        };
        // A proposal only: it goes on when the person presses 요청 전달 (`overall::send_on`).
        match overall::check(home, items) {
            Err(message) => emit(LeadEvent::Error { at: now_iso(), text: message })?,
            Ok(items) => {
                let proposals = before.iter().filter(|e| matches!(e, LeadEvent::ForwardPlan { .. })).count();
                emit(LeadEvent::ForwardPlan {
                    at: now_iso(),
                    proposal_id: format!("{thread_id}-f{}", proposals + 1),
                    items,
                    provider: req.provider,
                    model: req.model.filter(|m| !m.is_empty()).map(str::to_string),
                    effort: req.effort.filter(|e| !e.is_empty()).map(str::to_string),
                    profile: Profile::of_clean(req.clean.is_some()),
                })?;
            }
        }
        return Ok(());
    }
    match design {
        Ok(Some(design)) => emit(LeadEvent::Design { at: now_iso(), design })?,
        Ok(None) => {},
        Err(message) => {
            emit(LeadEvent::Error { at: now_iso(), text: message })?;
            return Ok(());
        }
    }
    match followups {
        Ok(None) => {},
        Err(message) => { emit(LeadEvent::Error { at: now_iso(), text: message })?; return Ok(()); }
        Ok(Some(items)) => {
            if unresolved { emit(LeadEvent::Error { at: now_iso(), text: "총괄 종료 상태가 불명이라 후속 지시를 보내지 않았습니다".into() })?; return Ok(()); }
            let Some(start) = start else { emit(LeadEvent::Error { at: now_iso(), text: "이 실행 환경에서는 후속 지시를 자동 전달할 수 없습니다".into() })?; return Ok(()); };
            if !matches!(plan, Ok(None)) { emit(LeadEvent::Error { at: now_iso(), text: "새 설계와 후속 지시가 함께 있어 실행하지 않았습니다".into() })?; return Ok(()); }
            // Closing holds current.lock too, so follow-ups only start from an open thread.
            // Same order as `dispatch` (current.lock, then dispatch.lock). This turn already holds
            // turn.lock; that is safe because no current.lock holder waits on turn.lock (`dispatch`
            // only tries it, `thread_for` never takes it).
            let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
            current.lock().map_err(|e| e.to_string())?;
            let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread_id).join("dispatch.lock")).map_err(|e| e.to_string())?;
            lock.lock().map_err(|e| e.to_string())?;
            let stopped = fs::read_to_string(thread_dir(home, thread_id).join("stop-all-at")).ok().and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok()).is_some_and(|at| at.with_timezone(&Utc) >= turn_started);
            if stopped || !is_open(home, req.project, thread_id) {
                emit(LeadEvent::Error { at: now_iso(), text: "중지되었거나 총괄 대화가 닫혀 후속 지시를 보내지 않았습니다".into() })?;
                return Ok(());
            }
            let allowed: BTreeSet<_> = before.iter().filter_map(|e| match e { LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } => Some(run_id.as_str()), _ => None }).collect();
            let mut seen = BTreeSet::new();
            if items.iter().any(|item| !allowed.contains(item.run.as_str()) || !seen.insert((&item.run, &item.card))) {
                emit(LeadEvent::Error { at: now_iso(), text: "이 대화에서 분배한 서로 다른 작업만 이어갈 수 있습니다".into() })?;
                return Ok(());
            }
            for item in items {
                match crate::run::followup::start(home, &item.run, &item.card, &item.task, start) {
                    Ok(run_id) => emit(LeadEvent::Followup { at: now_iso(), source_run_id: item.run, card_id: item.card, run_id })?,
                    Err(error) => emit(LeadEvent::Error { at: now_iso(), text: format!("{} 후속 지시를 보내지 못했습니다: {error}", item.card) })?,
                }
            }
        }
    }
    match plan {
        Ok(None) => {}
        Ok(Some(plan)) => {
            let plans = before.iter().filter(|e| matches!(e, LeadEvent::Plan { .. })).count();
            let (plan, violations, notices) = check_lead_plan(home, req.project, plan, RunMode { profile: Profile::of_clean(req.clean.is_some()), context: true, yolo: req.yolo });
            emit(LeadEvent::Plan { at: now_iso(), plan_id: format!("{thread_id}-p{}", plans + 1), plan, ok: violations.is_empty(), violations, notices })?;
        }
        Err(message) => emit(LeadEvent::Error { at: now_iso(), text: message })?,
    }
    Ok(())
}
