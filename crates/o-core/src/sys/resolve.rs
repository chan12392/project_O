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
/// A shim that runs a program its package ships, not a script (OpenCode v2). Only read when the
/// shim names no script: a script shim also names `%dp0%\node.exe`, the interpreter it prefers.
static SHIM_PROGRAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)"%dp0%\\(node_modules\\[^"%]+\.exe)""#).unwrap());

pub fn resolve_cli(bin: &str) -> Option<CliCommand> {
    let found = which(bin)?;
    if !is_script_shim(&found) {
        return Some(CliCommand { command: found, prefix: Vec::new() });
    }
    shim_target(&found)
}

/// What an npm `.cmd` shim runs: a script started with node, or a native program the
/// package ships (OpenCode's shim calls `node_modules\@opencode\cli\bin\opencode.exe`),
/// started directly. Either way no argument passes through cmd.exe.
pub fn shim_target(shim: &std::path::Path) -> Option<CliCommand> {
    let text = String::from_utf8_lossy(&fs::read(shim).ok()?).into_owned();
    let Some(script) = SHIM_SCRIPT.captures(&text).and_then(|c| c.get(1)) else {
        let program = shim.parent()?.join(SHIM_PROGRAM.captures(&text)?.get(1)?.as_str());
        return program.is_file().then(|| CliCommand { command: program, prefix: Vec::new() });
    };
    let path = shim.parent()?.join(script.as_str());
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
