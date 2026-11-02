//! Finds an official CLI on PATH. npm installs CLIs as `.cmd` shims, which would
//! pass every argument through cmd.exe; we read which script the shim runs and
//! start it with node instead, so no argument is ever parsed by a shell. A shim
//! we cannot read is refused rather than run through cmd.exe.

use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

/// How to start an official CLI without a shell: the program, and arguments that go before the CLI's own.
#[derive(Clone, Debug, PartialEq)]
pub struct CliCommand {
    pub command: PathBuf,
    pub prefix: Vec<String>,
}

static SHIM_SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)"%dp0%\\([^"%]+\.(?:js|mjs|cjs))""#).unwrap());

pub fn resolve_cli(bin: &str) -> Option<CliCommand> {
    let found = which(bin)?;
    if !is_script_shim(&found) {
        return Some(CliCommand { command: found, prefix: Vec::new() });
    }
    let text = String::from_utf8_lossy(&fs::read(&found).ok()?).into_owned();
    let script = SHIM_SCRIPT.captures(&text)?.get(1)?.as_str().to_string();
    let path = found.parent()?.join(script);
    if !path.exists() {
        return None;
    }
    // The shim's own interpreter, found the same way; never cmd.exe.
    let node = which("node").filter(|n| !is_script_shim(n))?;
    Some(CliCommand { command: node, prefix: vec![path.to_string_lossy().into_owned()] })
}

fn is_script_shim(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

/// The first `bin` (.exe, .cmd or .bat on Windows) in a PATH folder.
pub fn which(bin: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for ext in exts {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
