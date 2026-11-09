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

/// The shim's own folder is `%dp0%\` in npm's shims (after `SET dp0=%~dp0`) and `%~dp0` itself,
/// with or without a backslash after it, in the older cmd-shim output.
static SHIM_SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)"(?:%dp0%\\|%~dp0\\?)([^"%]+\.(?:js|mjs|cjs))""#).unwrap());
/// A shim that runs a program its package ships, not a script (OpenCode v2). Only read when the
/// shim names no script: a script shim also names `%dp0%\node.exe`, the interpreter it prefers.
static SHIM_PROGRAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)"(?:%dp0%\\|%~dp0\\?)(node_modules\\[^"%]+\.exe)""#).unwrap());
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
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    paths.push(PathBuf::from("/opt/homebrew/bin"));
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from("/usr/local/bin"));
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
        if cfg!(windows) {
            return Some(CliCommand { command: found, prefix: Vec::new() });
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        let on_path = find_in(bin, std::env::split_paths(&path)).as_ref() == Some(&found);
        let node_on_path = find_in("node", std::env::split_paths(&path)).is_some();
        return node_script(found, on_path, node_on_path);
    }
    shim_target(&found)
}

/// A `#!/usr/bin/env node` script found only in a fallback folder (nvm's `bin`) would look for
/// node on the unchanged PATH: it starts with the node beside it instead, so the version it was
/// installed with runs and PATH stays as it is. One found on PATH, or not a node script, starts as
/// it is; one with no node beside it or on PATH cannot start and is `None`.
fn node_script(found: PathBuf, on_path: bool, node_on_path: bool) -> Option<CliCommand> {
    if on_path || !wants_node(&found) {
        return Some(CliCommand { command: found, prefix: Vec::new() });
    }
    let node = found.parent()?.join(if cfg!(windows) { "node.exe" } else { "node" });
    if node.is_file() {
        return Some(CliCommand { command: node, prefix: vec![found.to_string_lossy().into_owned()] });
    }
    node_on_path.then(|| CliCommand { command: found, prefix: Vec::new() })
}

/// The script's first line names node as its interpreter (`#!/usr/bin/env node`, `#!/path/node`).
fn wants_node(script: &std::path::Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 256];
    let Ok(read) = fs::File::open(script).and_then(|mut f| f.read(&mut head)) else { return false };
    let text = String::from_utf8_lossy(&head[..read]);
    let Some(line) = text.lines().next().and_then(|l| l.strip_prefix("#!")) else { return false };
    let mut words = line.split_whitespace();
    let mut interpreter = words.next().unwrap_or_default();
    if interpreter.rsplit('/').next() == Some("env") {
        interpreter = words.find(|w| !w.starts_with('-')).unwrap_or_default();
    }
    interpreter.rsplit('/').next() == Some("node")
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

/// Puts the running exe's folder first on this process's PATH (never the saved PATH), so every
/// child (agent CLIs, `o _run`, cards running `o msg`/`o context`) finds the `o` beside it, even
/// when `o` was never `cargo install`ed nor its folder added to PATH. Other entries stay in order
/// behind it. Called first in `main` (the app and `o`), before any thread exists.
pub fn own_folder_first_on_path() {
    let Some(dir) = std::env::current_exe().ok().filter(|exe| exe.is_file()).and_then(|exe| exe.parent().map(|d| d.to_path_buf())) else { return };
    if let Some(path) = first_on_path(&dir, std::env::var_os("PATH").as_deref()) {
        std::env::set_var("PATH", path);
    }
}

fn first_on_path(dir: &std::path::Path, path: Option<&std::ffi::OsStr>) -> Option<std::ffi::OsString> {
    let rest: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).filter(|p| p != dir).collect()).unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(rest)).ok()
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

    /// #36: an nvm-style `bin` outside PATH, node not on PATH. The script starts with the node
    /// beside it (each version its own), one found on PATH still goes through PATH, and one with
    /// no node anywhere is refused rather than reported as runnable.
    #[test]
    fn fallback_node_script_starts_with_the_node_beside_it() {
        let root = std::env::temp_dir().join(format!("o-resolve-nvm-{}", std::process::id()));
        let node = if cfg!(windows) { "node.exe" } else { "node" };
        let script = |version: &str, shebang: &str| {
            let bin = root.join(".nvm/versions/node").join(version).join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("fake-agent"), format!("{shebang}\nrequire('x')\n")).unwrap();
            bin
        };
        let v20 = script("v20.0.0", "#!/usr/bin/env node");
        let v22 = script("v22.0.0", "#!/usr/bin/env -S node --no-warnings");
        fs::write(v20.join(node), "").unwrap();
        fs::write(v22.join(node), "").unwrap();
        for bin in [&v20, &v22] {
            let found = bin.join("fake-agent");
            assert_eq!(node_script(found.clone(), false, false), Some(CliCommand { command: bin.join(node), prefix: vec![found.to_string_lossy().into_owned()] }));
            assert_eq!(node_script(found.clone(), true, false), Some(CliCommand { command: found, prefix: Vec::new() }));
        }
        let bare = script("v18.0.0", "#!/usr/bin/env node").join("fake-agent");
        assert_eq!(node_script(bare.clone(), false, false), None);
        assert_eq!(node_script(bare.clone(), false, true), Some(CliCommand { command: bare, prefix: Vec::new() }));
        let shell = script("v16.0.0", "#!/bin/sh").join("fake-agent");
        assert_eq!(node_script(shell.clone(), false, false), Some(CliCommand { command: shell, prefix: Vec::new() }));
        let _ = fs::remove_dir_all(&root);
    }

    /// #46: the exe's folder goes first, once, with every other PATH entry kept in order.
    #[test]
    fn own_folder_goes_first_and_keeps_path() {
        let root = std::env::temp_dir();
        let (dir, a, b) = (root.join("o-install"), root.join("a"), root.join("b"));
        let path = std::env::join_paths([a.clone(), dir.clone(), b.clone()]).unwrap();
        let joined = first_on_path(&dir, Some(&path)).unwrap();
        assert_eq!(std::env::split_paths(&joined).collect::<Vec<_>>(), [dir.clone(), a, b]);
        let alone = first_on_path(&dir, None).unwrap();
        assert_eq!(std::env::split_paths(&alone).collect::<Vec<_>>(), [dir]);
    }

    /// A shim names its own folder `%dp0%\` (npm) or `%~dp0` (older cmd-shim output, which other
    /// package managers' global shims may follow); both start the script with node.
    #[test]
    fn shim_script_is_read_with_either_spelling_of_its_folder() {
        let script = |text: &str| SHIM_SCRIPT.captures(text).map(|c| c[1].to_string());
        assert_eq!(script(r#"endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\node_modules\asar\bin\asar.js" %*"#).as_deref(), Some(r"node_modules\asar\bin\asar.js"));
        assert_eq!(script(r#"  node  "%~dp0\global\5\node_modules\fake\bin\fake.js" %*"#).as_deref(), Some(r"global\5\node_modules\fake\bin\fake.js"));
        assert_eq!(script(r#""%~DP0node_modules\fake\bin\fake.mjs" %*"#).as_deref(), Some(r"node_modules\fake\bin\fake.mjs"));
        // The interpreter a shim prefers is not the script, and nor is a file named from another folder.
        assert_eq!(script(r#"@IF EXIST "%~dp0\node.exe" ("%~dp0\node.exe" %*)"#), None);
        assert_eq!(script(r#""%dp0%\node.exe"  "%OTHER%\x.js" %*"#), None);
    }

    #[test]
    fn shim_program_is_read_with_either_spelling_of_its_folder() {
        let program = |text: &str| SHIM_PROGRAM.captures(text).map(|c| c[1].to_string());
        assert_eq!(program(r#""%dp0%\node_modules\@opencode\cli\bin\opencode.exe" %*"#).as_deref(), Some(r"node_modules\@opencode\cli\bin\opencode.exe"));
        assert_eq!(program(r#""%~dp0node_modules\fake\bin\fake-win32-x64.exe" %*"#).as_deref(), Some(r"node_modules\fake\bin\fake-win32-x64.exe"));
        assert_eq!(program(r#""%~dp0\node_modules\fake\bin\fake.exe" %*"#).as_deref(), Some(r"node_modules\fake\bin\fake.exe"));
        assert_eq!(program(r#""%~dp0fake.exe" %*"#), None);
    }

    /// A shim that names no script or program of its own package is refused, never run through cmd.exe.
    #[test]
    fn a_shim_that_cannot_be_read_is_refused() {
        let dir = std::env::temp_dir().join(format!("o-resolve-shim-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("fake-agent.cmd");
        for text in ["@echo off\r\ncall \"%~dp0other.cmd\" %*\r\n", "@echo off\r\ncall \"%dp0%\\other.cmd\" %*\r\n", "@echo off\r\nfake-agent-real %*\r\n"] {
            fs::write(&shim, text).unwrap();
            assert_eq!(shim_target(&shim), None, "{text}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_program_shim_in_the_tilde_spelling_starts_the_program_itself() {
        let dir = std::env::temp_dir().join(format!("o-resolve-program-{}", std::process::id()));
        let program = dir.join(r"node_modules\fake\bin\fake-win32-x64.exe");
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::write(&program, "").unwrap();
        let shim = dir.join("fake.cmd");
        fs::write(&shim, "@ECHO off\r\n\"%~dp0node_modules\\fake\\bin\\fake-win32-x64.exe\" %*\r\n").unwrap();
        assert_eq!(shim_target(&shim), Some(CliCommand { command: program, prefix: Vec::new() }));
        let _ = fs::remove_dir_all(&dir);
    }
}
