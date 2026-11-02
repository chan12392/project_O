//! The lead chat (ADR-0008): `o lead show|send|new|attach|dispatch|stop`, dev `/api/lead/*`.
//! A turn runs the lead CLI read-only through o-core on a blocking thread; a plan is
//! dispatched by `o _run` in the background, as `o dispatch` does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use o_core::agents::clean;
use o_core::lead::{self, files, DispatchError, SendRequest};
use o_core::providers::{self, ModelChoice};
use o_core::run::plan::ProviderId;
use o_core::run::runner::official_command;
use o_core::run::store;
use o_core::sys::proc;
use serde_json::{json, Value};

use super::{blocking, o_binary};

/// An absolute folder that exists; o-core checks again whether it may be a project.
fn project_arg(project: &str) -> Result<(), String> {
    if !Path::new(project).is_absolute() || !Path::new(project).is_dir() {
        return Err(format!("프로젝트 폴더가 없습니다: {project}"));
    }
    Ok(())
}

/// The project's current lead thread and every project with one (`o lead show --json`).
#[tauri::command]
pub async fn lead_show(project: Option<String>) -> Result<Value, String> {
    blocking(move || lead::show(&store::default_home(), project.as_deref())).await
}

/// Starts a new thread for the project ("새 대화", `o lead new`).
#[tauri::command]
pub async fn lead_new(project: String) -> Result<Value, String> {
    project_arg(&project)?;
    blocking(move || {
        let thread = lead::start(&store::default_home(), &project)?;
        Ok(json!({ "threadId": thread, "project": project }))
    })
    .await?
}

/// Percent-decoding for the header values the screen encodes (`encodeURIComponent`).
fn decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3).ok_or("잘못된 머리글 값")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "잘못된 머리글 값")?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "잘못된 머리글 값".to_string())
}

/// Saves one attachment: the bytes are the raw request body, the project and file name
/// come percent-encoded in the `x-project` and `x-name` headers (`o lead attach`).
#[tauri::command]
pub async fn lead_attach(request: tauri::ipc::Request<'_>) -> Result<Value, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("첨부는 파일 내용 그대로 보내야 합니다".into());
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(decode).transpose();
    let project = header("x-project")?.ok_or("x-project가 필요합니다")?;
    let name = header("x-name")?.ok_or("x-name이 필요합니다")?;
    project_arg(&project)?;
    if bytes.len() as u64 > files::MAX_BYTES {
        return Err("첨부 파일은 하나에 10MB까지입니다".into());
    }
    let bytes = bytes.clone();
    blocking(move || {
        let home = store::default_home();
        lead::check_folder(&home, &project)?;
        let thread = lead::thread_for(&home, &project, false).map_err(|e| e.to_string())?;
        files::save(&home, &thread, &name, &bytes).map(|a| json!(a))
    })
    .await?
}

/// One message to the lead (`o lead send`); returns when the lead has answered.
#[tauri::command]
pub async fn lead_send(
    project: String,
    provider: String,
    model: Option<String>,
    effort: Option<String>,
    text: String,
    attachments: Vec<String>,
    clean: bool,
) -> Result<Value, String> {
    project_arg(&project)?;
    let provider = ProviderId::parse(&provider).ok_or_else(|| format!("알 수 없는 에이전트: {provider}"))?;
    blocking(move || {
        let home = store::default_home();
        let attach: Vec<PathBuf> = attachments.iter().map(PathBuf::from).collect();
        let root = clean.then(clean::default_root);
        let req = SendRequest {
            project: &project,
            provider,
            model: model.as_deref(),
            effort: effort.as_deref(),
            text: &text,
            attach: &attach,
            fresh: false,
            clean: root.as_deref(),
        };
        let events = lead::send(&home, &req, &official_command)?;
        Ok(json!({ "threadId": lead::current_thread(&home, &project), "events": events }))
    })
    .await?
}

/// Runs a proposed plan once after the policy check (`o lead dispatch`); the run starts
/// as `o _run <runId>` in the background, like `o dispatch`.
#[tauri::command]
pub async fn lead_dispatch(plan_id: String, clean: bool) -> Result<Value, String> {
    blocking(move || {
        let o = o_binary().ok_or("o CLI를 찾지 못했습니다 (cargo install --path crates/o-cli)")?;
        let start = |run_id: &str| proc::spawn_detached(Command::new(&o).args(["_run", run_id])).map(|_| ());
        match lead::dispatch(&store::default_home(), &plan_id, clean, &start) {
            Ok(done) => Ok(json!(done)),
            Err(DispatchError::Blocked(violations)) => {
                let lines: Vec<String> = violations.iter().map(|v| format!("[{}] {}", v.rule, v.message)).collect();
                Err(format!("차단됨 — 정책 위반으로 실행하지 않았습니다:\n{}", lines.join("\n")))
            }
            Err(DispatchError::Other(message)) => Err(message),
        }
    })
    .await?
}

/// How long a lead model list is reused before the CLI is asked again.
const MODELS_TTL: Duration = Duration::from_secs(10 * 60);

static MODELS: Mutex<BTreeMap<ProviderId, (Instant, Vec<ModelChoice>)>> = Mutex::new(BTreeMap::new());

/// The models a lead of this agent may name (`o lead models`), from the CLI's own list,
/// kept for a few minutes. A failed listing is not kept, so the next open asks again.
#[tauri::command]
pub async fn lead_models(provider: String) -> Result<Value, String> {
    let provider = ProviderId::parse(&provider).ok_or_else(|| format!("알 수 없는 에이전트: {provider}"))?;
    if let Some((at, models)) = MODELS.lock().map_err(|e| e.to_string())?.get(&provider) {
        if at.elapsed() < MODELS_TTL {
            return Ok(json!({ "provider": provider, "models": models }));
        }
    }
    let models = blocking(move || providers::lead_models(provider)).await??;
    MODELS.lock().map_err(|e| e.to_string())?.insert(provider, (Instant::now(), models.clone()));
    Ok(json!({ "provider": provider, "models": models }))
}

/// Stops the project's lead while it answers (`o lead stop`).
#[tauri::command]
pub async fn lead_stop(project: Option<String>) -> Result<Value, String> {
    blocking(move || json!({ "stopped": lead::stop(&store::default_home(), project.as_deref()) })).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::temp;

    #[test]
    fn header_values_decode_and_refuse_broken_escapes() {
        assert_eq!(decode("%ED%99%94%EB%A9%B4%201.png").unwrap(), "화면 1.png");
        assert_eq!(decode("C%3A%2Fwork%2Fapp").unwrap(), "C:/work/app");
        for bad in ["%", "%G1", "%ff"] {
            assert!(decode(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn lead_commands_take_only_existing_absolute_folders() {
        let dir = temp("lead");
        assert!(project_arg(&dir.to_string_lossy()).is_ok());
        assert!(project_arg("relative/dir").is_err());
        assert!(project_arg(&dir.join("missing").to_string_lossy()).is_err());
    }
}
