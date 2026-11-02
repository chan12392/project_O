//! NSIS data removal. Never accepts arbitrary data paths, and never opens profile files.
mod installer_paths;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use installer_paths::{fresh_dir, no_links};
use o_core::sys::proc;

const LOCAL_NAMES: &[&str] = &["project_O", "project_O-agents", "project_O-lead", "local.project-o.desktop"];
const TASKS: &[&str] = &["project_O mirror sync (test)", "project_O mirror sync (test_1)", "project_O mirror sync (test_2)", "project_O mirror sync (main)"];

fn root(name: &str) -> Result<PathBuf, String> {
    let value = std::env::var_os(name).filter(|v| !v.is_empty()).ok_or_else(|| format!("{name} is empty"))?;
    let path = PathBuf::from(value);
    no_links(&path)?;
    if path.parent().is_none() || !path.is_dir() { return Err(format!("invalid {name}")); }
    Ok(path)
}

fn targets(local: &Path, roaming: &Path) -> Result<Vec<PathBuf>, String> {
    no_links(local)?;
    no_links(roaming)?;
    let paths: Vec<_> = LOCAL_NAMES.iter().map(|name| local.join(name)).chain([roaming.join("local.project-o.desktop")]).collect();
    for path in &paths { no_links(path)?; }
    Ok(paths)
}

fn purge(paths: &[PathBuf]) -> Result<(), String> {
    // std::fs::remove_dir_all removes child links themselves, never their targets.
    for path in paths {
        no_links(path)?;
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Ok(())
}

fn installed_tasks() -> Result<Vec<&'static str>, String> {
    let out = proc::hide_window(Command::new("schtasks.exe").args(["/Query", "/FO", "CSV", "/NH"]))
        .output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err("예약 작업 목록을 확인하지 못했습니다".into()); }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(TASKS.iter().copied().filter(|name| text.lines().any(|line| line.split(',').next() == Some(format!("\"\\{name}\"").as_str()))).collect())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let dry_run = args.first().is_some_and(|s| s == "--dry-run");
    let args = if dry_run { &args[1..] } else { &args[..] };
    let local = root("LOCALAPPDATA")?;
    let roaming = root("APPDATA")?;
    let (local, roaming, fixture) = match args {
        [] => (local, roaming, false),
        [flag, id] if flag == "--test-id" && id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => {
            // Test mode is confined to this named sandbox; real tasks and data stay intact.
            let base = local.join("project_O-uninstall-tests").join(id);
            no_links(&base)?;
            (base.join("Local"), base.join("Roaming"), true)
        }
        _ => return Err("expected [--dry-run] [--test-id <32 hex digits>]".into()),
    };
    let paths = targets(&local, &roaming)?;
    let tasks = if fixture { Vec::new() } else { installed_tasks()? };
    if dry_run {
        let report = fresh_dir(&std::env::temp_dir(), "project_O-purge-plan")?.join("plan.json");
        fs::write(&report, serde_json::to_vec_pretty(&serde_json::json!({ "dryRun": true, "folders": paths, "tasks": tasks })).unwrap()).map_err(|e| e.to_string())?;
        println!("{}", report.display());
        return Ok(());
    }
    // Remove the writer before its log/state folder; never change its mirror repository.
    for name in tasks {
        let status = proc::hide_window(Command::new("schtasks.exe").args(["/Delete", "/TN", name, "/F"]))
            .status().map_err(|e| e.to_string())?;
        if !status.success() { return Err(format!("예약 작업을 제거하지 못했습니다: {name}")); }
    }
    purge(&paths)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_removes_only_known_names_and_their_attachments() {
        let base = fresh_dir(&std::env::temp_dir(), "project_O-uninstall-test").unwrap();
        let local = base.join("Local");
        let roaming = base.join("Roaming");
        let paths = targets(&local, &roaming).unwrap();
        for path in &paths {
            fs::create_dir_all(path.join("attachments")).unwrap();
            fs::write(path.join("attachments/fake.txt"), b"fake").unwrap();
        }
        for name in [".claude", ".codex", ".gemini", ".agents", "repositories", "projects"] {
            fs::create_dir_all(local.join(name)).unwrap();
            fs::write(local.join(name).join("keep.txt"), b"keep").unwrap();
        }
        purge(&paths).unwrap();
        assert!(paths.iter().all(|p| !p.exists()));
        for name in [".claude", ".codex", ".gemini", ".agents", "repositories", "projects"] {
            assert_eq!(fs::read(local.join(name).join("keep.txt")).unwrap(), b"keep");
        }
        assert!(targets(Path::new(""), &roaming).is_err());
        assert!(targets(Path::new("../outside"), &roaming).is_err());
    }
}
