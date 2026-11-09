//! Delivers a completed lead answer to its waiting card.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn reply(home: &Path, req: &SendRequest, thread_id: &str, worker: &Option<WorkerTurn<'_>>, mut answer: String, mut worker_notice: Option<String>, session: &Option<String>, used_session: &Option<String>, before: &[LeadEvent], default_model_supported: bool, result_tools: bool, emit: &mut dyn FnMut(LeadEvent) -> Result<(), String>) -> Result<(), String> {
    if let Some(w) = worker {
        if answer.trim().is_empty() { emit(LeadEvent::Error { at: now_iso(), text: "카드 메시지에 대한 총괄 답변이 비어 있습니다".into() })?; return Ok(()); }
        if session.as_ref().is_some_and(|expected| used_session.as_ref() != Some(expected))
            || used_session.as_ref().is_some_and(|id| before.iter().any(|event| match event {
                LeadEvent::Session { session_id, .. } => session_id == id,
                LeadEvent::WorkerSession { session_id, .. } if session.is_none() => session_id == id,
                _ => false,
            })) {
            emit(LeadEvent::Error { at: now_iso(), text: "카드 질문의 읽기 전용 세션을 확인할 수 없어 답변을 전달하지 않았습니다".into() })?;
            return Ok(());
        }
        let reusable_session = used_session.as_ref().filter(|_| req.model.filter(|model| !model.is_empty()).is_some() || default_model_supported);
        if reusable_session.is_none() {
            let notice = "카드 질문용 세션 또는 모델을 확인할 수 없어 다음 질문에 재사용하지 않습니다";
            worker_notice = Some(format!("{}{}", worker_notice.map_or(String::new(), |previous| format!("{previous}. ")), notice));
        }
        if let Some(notice) = &worker_notice {
            emit(LeadEvent::Warning { at: now_iso(), text: notice.clone() })?;
            answer = format!("[{notice}]\n\n{answer}");
        }
        emit(LeadEvent::Reply { at: now_iso(), provider: req.provider, text: answer.clone(), reported_files: Vec::new() })?;
        let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
        current.lock().map_err(|e| e.to_string())?;
        if store::stop_requested(home, &w.speaker.run_id) || !is_open(home, req.project, thread_id) {
            emit(LeadEvent::Error { at: now_iso(), text: "실행이 중지되었거나 총괄 대화가 닫혀 카드에 답을 보내지 않았습니다".into() })?;
            return Ok(());
        }
        if w.message.kind == "question" {
            let key = crate::run::result::digest(&format!("{}\0{}", w.speaker.run_id, w.message.id));
            let text = if js_len(&answer) > LIMITS.max_message_chars {
                let violations = crate::policy::check_message(&answer, &crate::policy::Parties { from: "lead", to: &w.message.from, known: &w.speaker.cards });
                let supports = store::read_plan(home, &w.speaker.run_id).ok().and_then(|v| serde_json::from_value::<crate::run::plan::Plan>(v).ok())
                    .and_then(|plan| plan.cards.into_iter().find(|card| card.id == w.message.from))
                    .is_some_and(|card| providers::get(card.provider).searches_context());
                if !supports || violations.iter().any(|v| v.rule != "msg-size") {
                    emit(LeadEvent::Error { at: now_iso(), text: "긴 답변의 원문 전달을 지원하지 않거나 정책 검사에 걸렸습니다. 대화에서 원문을 확인하세요".into() })?;
                    return Ok(());
                }
                match crate::run::result::store_lead(home, &w.speaker.run_id, &answer) {
                    Ok(hash) => crate::run::result::lead_reference(&w.message.id, &hash),
                    Err(error) => { emit(LeadEvent::Error { at: now_iso(), text: format!("긴 답변을 저장하지 못했습니다: {error}") })?; return Ok(()); }
                }
            } else { answer.clone() };
            // The person may have answered while the model was at work: their answer stands.
            if team::send_reply(home, w.speaker, &w.message.from, &text, false, Some(&key), Some(&w.message.id)).is_err() && !answered(home, w.speaker, w.message)? {
                emit(LeadEvent::Error { at: now_iso(), text: "카드에 답을 보내지 못했습니다. 답변과 메시지 상태를 확인하세요".into() })?;
                return Ok(());
            }
        }
        team::transition(home, w.speaker, &w.message.id, "acknowledged").map_err(|e| e.to_string())?;
        if let Some(session_id) = reusable_session {
            let events = read_complete_thread(home, thread_id)?;
            if !matches!(events.get(before.len()), Some(LeadEvent::Worker { message_id, .. }) if message_id == &w.message.id) {
                return Err("카드 질문의 대화 위치를 확인할 수 없어 세션을 저장하지 않았습니다".into());
            }
            let history = serde_json::to_string(&events).map_err(|e| e.to_string())?;
            emit(LeadEvent::WorkerSession { at: now_iso(), provider: req.provider, session_id: session_id.clone(),
                model: req.model.filter(|s| !s.is_empty()).map(str::to_string), effort: req.effort.filter(|s| !s.is_empty()).map(str::to_string),
                profile: Profile::of_clean(req.clean.is_some()), result_tools, through: before.len(), history_hash: crate::run::result::digest(&history) })?;
        }
        return Ok(());
    }

    Ok(())
}
