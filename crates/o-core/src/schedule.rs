//! User-created Windows scheduled lead requests. Scheduler arguments contain only an O job ID.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use chrono::{Local, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::run::store;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub name: String,
    pub project: String,
    pub provider: String,
    pub text: String,
    pub start: String,
    pub repeat: String,
    pub clean: bool,
    pub auto_dispatch: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(flatten)]
    pub request: Request,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastRun>,
}
fn default_enabled() -> bool { true }

#[derive(Serialize, Deserialize)]
pub struct LastRun { pub at: String, pub ok: bool, pub error: Option<String> }

fn dir(home: &Path) -> PathBuf { home.join("schedules") }
fn job_path(home: &Path, id: &str) -> Result<PathBuf, String> {
    if !store::is_run_id(id) { return Err("예약 ID가 올바르지 않습니다".into()); }
    Ok(dir(home).join(format!("{id}.json")))
}

pub fn get(home: &Path, id: &str) -> Result<Job, String> {
    let mut job: Job = serde_json::from_slice(&fs::read(job_path(home, id)?).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if job.id != id { return Err("예약 ID가 일치하지 않습니다".into()); }
    let last = dir(home).join(format!("{id}.last"));
    job.last = fs::read(last).ok().and_then(|data| serde_json::from_slice(&data).ok());
    Ok(job)
}

pub fn list(home: &Path) -> Result<Vec<Job>, String> {
    let entries = match fs::read_dir(dir(home)) { Ok(v) => v, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()), Err(e) => return Err(e.to_string()) };
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        jobs.push(get(home, id)?);
    }
    jobs.sort_by(|a, b| a.request.start.cmp(&b.request.start));
    Ok(jobs)
}

fn validate(home: &Path, user: &Path, request: &Request) -> Result<(), String> {
    crate::projects::check(home, user, &request.project)?;
    if request.name.trim().is_empty() || request.name.chars().count() > 80 { return Err("예약 이름은 1–80자로 입력하세요".into()); }
    if request.text.trim().is_empty() || request.text.len() > 32_000 { return Err("요청은 1–32000바이트로 입력하세요".into()); }
    if !matches!(request.provider.as_str(), "claude" | "codex") { return Err("예약 총괄은 Claude 또는 Codex를 선택하세요".into()); }
    if !matches!(request.repeat.as_str(), "once" | "daily" | "weekly") { return Err("반복 주기를 확인하세요".into()); }
    let date = NaiveDateTime::parse_from_str(&request.start, "%Y-%m-%dT%H:%M").map_err(|_| "예약 날짜와 시간을 확인하세요")?;
    if date <= Local::now().naive_local() { return Err("현재보다 나중 시간을 선택하세요".into()); }
    Ok(())
}

fn xml_escape(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }
fn task_xml(exe: &Path, job: &Job) -> Result<String, String> {
    let exe = exe.to_str().ok_or("실행 파일 경로를 읽지 못했습니다")?;
    let start = format!("{}:00", job.request.start);
    let calendar = match job.request.repeat.as_str() {
        "daily" => "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>".to_string(),
        "weekly" => {
            use chrono::Datelike;
            let date = NaiveDateTime::parse_from_str(&job.request.start, "%Y-%m-%dT%H:%M").map_err(|e| e.to_string())?;
            let day = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][date.weekday().num_days_from_monday() as usize];
            format!("<ScheduleByWeek><DaysOfWeek><{day}/></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>")
        },
        _ => String::new(),
    };
    let trigger = if job.request.repeat == "once" { format!("<TimeTrigger><StartBoundary>{start}</StartBoundary><Enabled>true</Enabled></TimeTrigger>") } else { format!("<CalendarTrigger><StartBoundary>{start}</StartBoundary><Enabled>true</Enabled>{calendar}</CalendarTrigger>") };
    Ok(format!(r#"<?xml version="1.0" encoding="UTF-16"?><Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Triggers>{trigger}</Triggers><Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals><Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><StartWhenAvailable>true</StartWhenAvailable><ExecutionTimeLimit>PT24H</ExecutionTimeLimit><Enabled>true</Enabled></Settings><Actions Context="Author"><Exec><Command>{}</Command><Arguments>schedule run {}</Arguments></Exec></Actions></Task>"#, xml_escape(exe), job.id))
}

#[cfg(windows)]
fn scheduler(args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("schtasks.exe");
    cmd.args(args);
    crate::sys::proc::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(format!("Windows 작업 스케줄러 오류: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}
#[cfg(not(windows))]
fn scheduler(_args: &[&str]) -> Result<(), String> { Err("예약 자동화는 Windows에서만 지원합니다".into()) }

pub fn create(home: &Path, user: &Path, exe: &Path, request: Request) -> Result<Job, String> {
    validate(home, user, &request)?;
    if !exe.is_file() { return Err("예약 실행용 O CLI를 찾지 못했습니다".into()); }
    fs::create_dir_all(dir(home)).map_err(|e| e.to_string())?;
    let id = store::new_run_id(Utc::now());
    let job = Job { id, enabled: true, request, last: None };
    let path = job_path(home, &job.id)?;
    let xml_path = dir(home).join(format!("{}.xml", job.id));
    let xml = task_xml(exe, &job)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
    let data = serde_json::to_vec_pretty(&job).map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(&data) { drop(file); let _ = fs::remove_file(&path); return Err(e.to_string()); }
    drop(file);
    let encoded: Vec<u8> = std::iter::once(0xfeff).chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect();
    if let Err(e) = fs::write(&xml_path, encoded) { let _ = fs::remove_file(&path); return Err(e.to_string()); }
    let name = format!("ProjectO-{}", job.id);
    if let Err(e) = scheduler(&["/Create", "/TN", &name, "/XML", &xml_path.to_string_lossy()]) {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&xml_path);
        return Err(e);
    }
    Ok(job)
}

pub fn delete(home: &Path, id: &str) -> Result<(), String> {
    let path = job_path(home, id)?;
    get(home, id)?;
    let name = format!("ProjectO-{id}");
    scheduler(&["/Delete", "/TN", &name, "/F"])?;
    fs::remove_file(path).map_err(|e| e.to_string())?;
    let xml = dir(home).join(format!("{id}.xml"));
    if xml.exists() { fs::remove_file(xml).map_err(|e| e.to_string())?; }
    let last = dir(home).join(format!("{id}.last"));
    if last.exists() { fs::remove_file(last).map_err(|e| e.to_string())?; }
    Ok(())
}

pub fn set_enabled(home: &Path, id: &str, enabled: bool) -> Result<Job, String> {
    let mut job = get(home, id)?;
    let name = format!("ProjectO-{id}");
    scheduler(&["/Change", "/TN", &name, if enabled { "/ENABLE" } else { "/DISABLE" }])?;
    job.enabled = enabled;
    let temp = dir(home).join(format!("{}-{}.tmp", id, store::new_run_id(Utc::now())));
    fs::write(&temp, serde_json::to_vec(&job).map_err(|e| e.to_string())?).map_err(|e| format!("스케줄러는 변경됐지만 앱 기록을 저장하지 못했습니다: {e}"))?;
    fs::rename(temp, job_path(home, id)?).map_err(|e| format!("스케줄러는 변경됐지만 앱 기록을 저장하지 못했습니다: {e}"))?;
    Ok(job)
}

pub fn record(home: &Path, id: &str, result: &Result<(), String>) -> Result<(), String> {
    get(home, id)?;
    let last = LastRun { at: Utc::now().to_rfc3339(), ok: result.is_ok(), error: result.as_ref().err().cloned() };
    fs::write(dir(home).join(format!("{id}.last")), serde_json::to_vec(&last).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_has_only_fixed_arguments_and_escapes_executable_path() {
        let job = Job { id: "abc-123".into(), enabled: true, request: Request { name: "name".into(), project: "ignored".into(), provider: "codex".into(), text: "untrusted & private prompt".into(), start: "2099-12-31T23:30".into(), repeat: "weekly".into(), clean: true, auto_dispatch: false }, last: None };
        let xml = task_xml(Path::new("C:/program & tools/o.exe"), &job).unwrap();
        assert!(xml.contains("program &amp; tools/o.exe"));
        assert!(xml.contains("<Arguments>schedule run abc-123</Arguments>"));
        assert!(!xml.contains("untrusted"));
        assert!(xml.contains("<Thursday/>"));
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
    }
}
