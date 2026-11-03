//! Adapted from stablyai/orca (MIT); see THIRD_PARTY_NOTICES.md.
//! Finds an official CLI on PATH. npm installs CLIs as `.cmd` shims, which would
//! pass every argument through cmd.exe; we read which script the shim runs and
//! start it with node instead, so no argument is ever parsed by a shell. A shim
//! we cannot read is refused rather than run through cmd.exe.

use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock, RwLock};

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
static EXTRA_PATHS: LazyLock<RwLock<Vec<PathBuf>>> = LazyLock::new(|| RwLock::new(Vec::new()));
static PATHS_READY: OnceLock<()> = OnceLock::new();

pub fn ensure_paths() {
    if PATHS_READY.get().is_none() { refresh_paths(); }
}

/// Refresh registry PATH and common installation locations without changing process PATH.
pub fn refresh_paths() {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        use std::process::Command;
        use std::time::Duration;
        let reg = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("System32").join("reg.exe");
        let variables = Regex::new(r"%([^%]+)%").unwrap();
        for key in [r"HKCU\Environment", r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment"] {
            let mut command = Command::new(&reg);
            command.args(["query", key, "/v", "Path"]);
            if let Ok((status, out, _)) = crate::sys::proc::capture(&mut command, Duration::from_secs(5)) {
                if status.success() {
                    for line in out.lines() {
                        if let Some((_, value)) = line.split_once("REG_EXPAND_SZ").or_else(|| line.split_once("REG_SZ")) {
                            let expanded = variables.replace_all(value.trim(), |caps: &regex::Captures<'_>| std::env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string()));
                            paths.extend(std::env::split_paths(std::ffi::OsStr::new(expanded.as_ref())));
                        }
                    }
                }
            }
        }
        if let Some(appdata) = std::env::var_os("APPDATA") { paths.push(PathBuf::from(appdata).join("npm")); }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            paths.extend([local.join("pnpm"), local.join("Yarn/bin"), local.join("Microsoft/WinGet/Links"), local.join("devin/cli/bin")]);
        }
    }
    for name in ["PNPM_HOME", "NVM_SYMLINK"] {
        if let Some(path) = std::env::var_os(name) { paths.push(PathBuf::from(path)); }
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let home = PathBuf::from(home);
        paths.extend([home.join(".local/bin"), home.join(".volta/bin"), home.join(".bun/bin")]);
        if !cfg!(windows) {
            if let Ok(versions) = fs::read_dir(home.join(".nvm/versions/node")) {
                paths.extend(versions.flatten().map(|entry| entry.path().join("bin")));
            }
        }
    }
    if let Some(nvm) = std::env::var_os("NVM_HOME") {
        if let Ok(versions) = fs::read_dir(nvm) {
            paths.extend(versions.flatten().map(|entry| if cfg!(windows) { entry.path() } else { entry.path().join("bin") }));
        }
    }
    paths.retain(|p| !p.as_os_str().is_empty());
    *EXTRA_PATHS.write().unwrap_or_else(|e| e.into_inner()) = paths;
    let _ = PATHS_READY.set(());
}

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
    let path = std::env::var_os("PATH").unwrap_or_default();
    let extra = EXTRA_PATHS.read().unwrap_or_else(|e| e.into_inner());
    find_in(bin, std::env::split_paths(&path).chain(extra.iter().cloned()))
}

fn find_in(bin: &str, dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    for dir in dirs {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_precedes_fallback_and_fallback_finds_new_install() {
        let root = std::env::temp_dir().join(format!("o-resolve-{}", std::process::id()));
        let first = root.join("path");
        let second = root.join("fallback");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let name = if cfg!(windows) { "fake-agent.exe" } else { "fake-agent" };
        fs::write(second.join(name), "").unwrap();
        assert_eq!(find_in("fake-agent", [first.clone(), second.clone()]), Some(second.join(name)));
        fs::write(first.join(name), "").unwrap();
        assert_eq!(find_in("fake-agent", [first.clone(), second]), Some(first.join(name)));
    }
}
