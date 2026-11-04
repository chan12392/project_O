use std::{fs, io::Write, time::{SystemTime, UNIX_EPOCH}};

use o_core::{lead::{self, LeadEvent}, run::{runner::official_command, team::{self, Speaker}}};
use serde_json::json;

#[test]
#[ignore = "실제 Codex 구독과 실행 한도를 사용하는 수동 통합 검사"]
fn official_codex_reuses_default_model_for_card_questions() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("o-default-model-{}-{stamp}", std::process::id()));
    let home = root.join("state");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(home.join("runs/run-1")).unwrap();
    fs::write(home.join("runs/run-1/events.jsonl"), "").unwrap();
    let project = project.to_string_lossy().into_owned();
    fs::write(home.join("runs/run-1/plan.json"), json!({"title":"test","project":project,"cards":[{"id":"a","provider":"codex","task":"test","depends_on":[]}]}).to_string()).unwrap();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let mut events = fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap();
    writeln!(events, "{}", json!({"t":"lead.user","at":"now","text":"테스트 지시","provider":"codex","effort":"low","profile":"project-o"})).unwrap();
    writeln!(events, "{}", json!({"t":"lead.dispatch","at":"now","planId":"p1","runId":"run-1","clean":true})).unwrap();
    drop(events);
    let card = Speaker { run_id: "run-1".into(), me: "a".into(), cards: vec!["a".into()] };
    for (id, question) in [("first", "검증용 식별자는 DEFAULT-4392 입니다. 식별자가 무엇인가요?"), ("second", "이전 질문에서 알려준 검증용 식별자는 무엇인가요?")] {
        team::send_tracked(&home, &card, "lead", question, true, Some(id)).ok().unwrap();
        assert!(lead::wake_card(&home, &project, "run-1", &card.cards, &official_command).unwrap());
    }
    let events = lead::read_thread(&home, &thread);
    let sessions: Vec<_> = events.iter().filter_map(|event| match event { LeadEvent::WorkerSession { session_id, model, .. } if model.is_none() => Some(session_id), _ => None }).collect();
    assert_eq!(sessions.len(), 2, "기본 모델 세션이 두 질문에 기록되어야 합니다");
    assert_eq!(sessions[0], sessions[1], "두 번째 질문은 첫 세션을 재개해야 합니다");
    let answers: Vec<_> = events.iter().filter_map(|event| match event { LeadEvent::Reply { text, .. } => Some(text), _ => None }).collect();
    assert_eq!(answers.len(), 2);
    assert!(answers[1].contains("DEFAULT-4392"), "이전 질문의 식별자를 기억하지 못했습니다");
    let usage: Vec<_> = events.iter().filter_map(|event| match event { LeadEvent::Usage { input, cached, complete, .. } => Some((input, cached, complete)), _ => None }).collect();
    println!("카드 질문별 신규 입력·캐시 토큰: {usage:?}");
}
