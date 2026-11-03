//! Local findings and read-only GitHub issue discovery (ADR-0027).
use std::fs;
use std::path::Path;
use std::time::Duration;

use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::review::Gh;
use crate::run::{events::{CardStatus, OEvent}, store::{is_run_id, plan_card_ids, read_events, read_plan}};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Finding {
    pub repo: String,
    pub run_id: String,
    pub card_id: String,
    pub sha: String,
    pub summary: String,
    pub reproduction: String,
    pub evidence: Vec<String>,
    pub classification: String,
    pub issue: Option<IssueRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resolution {
    pub run_id: String,
    pub card_id: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueRef {
    pub number: u64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub issues: Vec<Issue>,
    pub possibly_truncated: bool,
}

fn valid_repo(repo: &str) -> bool {
    Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").unwrap().is_match(repo)
        && !repo.split('/').any(|part| part.starts_with('-') || part == "." || part == "..")
}

fn args(parts: &[&str]) -> Vec<String> { parts.iter().map(|s| (*s).to_string()).collect() }

/// Returns candidates only; a text match never decides whether issues are duplicates.
pub fn search(gh: &dyn Gh, cwd: &Path, repo: &str, query: &str) -> Result<SearchResult, String> {
    if !valid_repo(repo) || query.trim().is_empty() || query.len() > 1000 {
        return Err("저장소(owner/repo)와 검색어가 필요합니다".into());
    }
    let output = gh.run(cwd, &args(&["issue", "list", "--repo", repo, "--state", "all", "--limit", "100", "--search", query, "--json", "number,title,state,url,updatedAt"]))?;
    let issues: Vec<Issue> = serde_json::from_str(&output).map_err(|e| format!("이슈 검색 결과 오류: {e}"))?;
    Ok(SearchResult { possibly_truncated: issues.len() >= 100, issues })
}

fn open(home: &Path) -> Result<Connection, String> {
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let db = Connection::open(home.join("issues.db")).map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS findings (id INTEGER PRIMARY KEY, repo TEXT NOT NULL, run_id TEXT NOT NULL, card_id TEXT NOT NULL, sha TEXT NOT NULL, summary TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(repo, run_id, card_id, sha, summary));").map_err(|e| e.to_string())?;
    Ok(db)
}

/// Stores a finding, never writing to GitHub. A linked issue must still match the observed version.
fn validate(home: &Path, gh: &dyn Gh, cwd: &Path, finding: &Finding) -> Result<(), String> {
    if !valid_repo(&finding.repo) || !is_run_id(&finding.run_id)
        || !Regex::new(r"^(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})$").unwrap().is_match(&finding.sha)
        || finding.summary.trim().is_empty() || finding.reproduction.trim().is_empty()
        || finding.evidence.is_empty() || finding.evidence.iter().any(|e| e.trim().is_empty()) {
        return Err("저장소, 실행, 전체 발견 SHA, 요약, 재현 및 증거 참조가 필요합니다".into());
    }
    let cards = plan_card_ids(&read_plan(home, &finding.run_id).map_err(|e| format!("실행을 읽지 못했습니다: {e}"))?).map_err(|e| e.to_string())?;
    if !cards.contains(&finding.card_id) { return Err("해당 실행에 카드가 없습니다".into()); }
    match (finding.classification.as_str(), &finding.issue) {
        ("unclassified" | "external" | "contention", None) => {}
        ("related" | "duplicate", Some(issue)) if issue.number > 0 && !issue.updated_at.is_empty() => {
            let number = issue.number.to_string();
            let output = gh.run(cwd, &args(&["issue", "view", &number, "--repo", &finding.repo, "--json", "number,title,state,url,updatedAt"]))?;
            let current: Issue = serde_json::from_str(&output).map_err(|e| format!("이슈 조회 결과 오류: {e}"))?;
            if current.number != issue.number || current.updated_at != issue.updated_at {
                return Err("이슈가 조회 이후 바뀌었습니다. 다시 조회하고 판단하세요".into());
            }
        }
        _ => return Err("분류와 이슈 참조가 맞지 않습니다".into()),
    }
    Ok(())
}

pub fn record(home: &Path, gh: &dyn Gh, cwd: &Path, finding: &Finding) -> Result<i64, String> {
    if finding.resolution.is_some() { return Err("발견은 미종결 상태로 기록하세요".into()); }
    validate(home, gh, cwd, finding)?;
    let data = serde_json::to_string(finding).map_err(|e| e.to_string())?;
    let db = open(home)?;
    db.execute("INSERT INTO findings(repo,run_id,card_id,sha,summary,data) VALUES (?1,?2,?3,?4,?5,?6)", params![finding.repo.to_ascii_lowercase(), finding.run_id, finding.card_id, finding.sha.to_ascii_lowercase(), finding.summary, data])
        .map_err(|e| format!("발견을 저장하지 못했습니다 (중복 또는 저장소 오류): {e}"))?;
    Ok(db.last_insert_rowid())
}

/// Record a person's verification; successful card completion is not proof of a fix.
pub fn resolve(home: &Path, id: i64, resolution: Option<Resolution>) -> Result<(), String> {
    let db = open(home)?;
    let old: String = db.query_row("SELECT data FROM findings WHERE id = ?1", [id], |row| row.get(0)).map_err(|e| e.to_string())?;
    let mut finding: Finding = serde_json::from_str(&old).map_err(|e| e.to_string())?;
    if let Some(r) = &resolution {
        if !is_run_id(&r.run_id) || r.evidence.trim().is_empty() || r.evidence.len() > 4000 {
            return Err("검증 실행, 카드와 증거가 필요합니다".into());
        }
        let cards = plan_card_ids(&read_plan(home, &r.run_id).map_err(|e| format!("검증 실행을 읽지 못했습니다: {e}"))?).map_err(|e| e.to_string())?;
        let status = read_events(home, &r.run_id).iter().rev().find_map(|e| match e {
            OEvent::CardEnd { card_id, status, .. } if card_id == &r.card_id => Some(*status),
            _ => None,
        });
        if !cards.contains(&r.card_id) || status != Some(CardStatus::Done) {
            return Err("성공한 검증 카드의 종료 기록이 필요합니다".into());
        }
    }
    finding.resolution = resolution;
    let data = serde_json::to_string(&finding).map_err(|e| e.to_string())?;
    if db.execute("UPDATE findings SET data = ?1 WHERE id = ?2 AND data = ?3", params![data, id, old]).map_err(|e| e.to_string())? != 1 {
        return Err("다른 작업이 발견을 변경했습니다. 다시 확인하세요".into());
    }
    Ok(())
}

/// Optimistic update: a simultaneous classification cannot silently overwrite this one.
pub fn classify(home: &Path, gh: &dyn Gh, cwd: &Path, id: i64, classification: &str, issue: Option<IssueRef>) -> Result<(), String> {
    let db = open(home)?;
    let old: String = db.query_row("SELECT data FROM findings WHERE id = ?1", [id], |row| row.get(0)).map_err(|e| e.to_string())?;
    let mut finding: Finding = serde_json::from_str(&old).map_err(|e| e.to_string())?;
    finding.classification = classification.to_string();
    finding.issue = issue;
    validate(home, gh, cwd, &finding)?;
    let data = serde_json::to_string(&finding).map_err(|e| e.to_string())?;
    if db.execute("UPDATE findings SET data = ?1 WHERE id = ?2 AND data = ?3", params![data, id, old]).map_err(|e| e.to_string())? != 1 {
        return Err("다른 작업이 발견을 변경했습니다. 다시 확인하세요".into());
    }
    Ok(())
}

pub fn list(home: &Path, repo: &str) -> Result<Vec<serde_json::Value>, String> {
    if !valid_repo(repo) { return Err("저장소는 owner/repo 형식이어야 합니다".into()); }
    let db = open(home)?;
    let mut stmt = db.prepare("SELECT id, data FROM findings WHERE repo = ?1 ORDER BY id DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([repo.to_ascii_lowercase()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
    rows.map(|row| {
        let (id, data) = row.map_err(|e| e.to_string())?;
        let finding: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"id": id, "finding": finding}))
    }).collect()
}
