//! The Codex sandbox check (ADR-0040 개정 1) against a fake Codex. The fake is this test binary
//! itself: started with O_FAKE_CODEX it answers `--version`, `app-server` and `exec` as the
//! scenario says, so no test calls the real Codex or a model.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use o_core::providers::codex_sandbox::{self, Deadlines, CHANGED, HANG_ISSUE, PROBE_COMMAND, RECORD};
use serde_json::{json, Value};

const FAKE: &str = "O_FAKE_CODEX";
const FAKE_ARGS: &str = "O_FAKE_CODEX_ARGS";
/// Where the fake leaves what it saw: its arguments, the prompt, the app-server methods, pids.
const FAKE_DIR: &str = "O_FAKE_CODEX_DIR";
/// The profile's config.toml, which the fake's sandbox setup writes as Codex's does.
const FAKE_CONFIG: &str = "O_FAKE_CODEX_CONFIG";

fn main() {
    if let Ok(scenario) = std::env::var(FAKE) {
        fake_codex(&scenario);
        return;
    }
    let tests: [(&str, fn()); 4] = [
        ("an unset sandbox is set up, the probe runs its command, and the record makes Codex ready", setup_then_probe_passes),
        ("a probe that hangs is stopped with its whole tree by the deadline and recorded as the known bug", hanging_probe_is_stopped),
        ("a probe whose command is refused records what Codex said", refused_probe_is_recorded),
        ("a Codex that cannot say its version is not asked anything else", no_version_no_check),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("\ncodex_sandbox: {} passed; {failed} failed", tests.len() - failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

fn seen(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(FAKE_DIR).unwrap()).join(name)
}

/// Stops this process here for good: what a Codex stuck before its first command looks like.
fn park() -> ! {
    loop {
        std::thread::park();
    }
}

fn fake_codex(scenario: &str) {
    if scenario == "park" {
        std::fs::write(seen("child.pid"), std::process::id().to_string()).unwrap();
        park();
    }
    let args: Vec<String> = serde_json::from_str(&std::env::var(FAKE_ARGS).unwrap()).unwrap();
    match args.first().map(String::as_str) {
        Some("--version") if scenario == "no-version" => std::process::exit(1),
        Some("--version") => println!("codex-cli 9.9.9"),
        Some("app-server") => app_server(scenario),
        Some("exec") => exec(scenario, &args),
        other => panic!("unexpected fake codex call {other:?}"),
    }
}

fn app_server(scenario: &str) {
    let mut log = Vec::new();
    let send = |value: Value| {
        println!("{value}");
        std::io::stdout().flush().unwrap();
    };
    for line in std::io::stdin().lock().lines() {
        let message: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = message["method"].as_str().unwrap_or_default().to_string();
        log.push(method.clone());
        std::fs::write(seen("app-server.json"), json!(log).to_string()).unwrap();
        match method.as_str() {
            "initialize" => send(json!({"id":1,"result":{"userAgent":"fake"}})),
            "windowsSandbox/readiness" => send(json!({"id":2,"result":{"status": if scenario == "unset" { "notConfigured" } else { "ready" }}})),
            "windowsSandbox/setupStart" => {
                std::fs::write(std::env::var(FAKE_CONFIG).unwrap(), "[windows]\nsandbox = \"unelevated\"\n").unwrap();
                send(json!({"id":3,"result":{"started":true}}));
                send(json!({"method":"windowsSandbox/setupCompleted","params":{"mode":"unelevated","success":true,"error":null}}));
            }
            _ => {}
        }
    }
}

fn exec(scenario: &str, args: &[String]) {
    std::fs::write(seen("exec.json"), json!(args).to_string()).unwrap();
    let mut prompt = String::new();
    std::io::stdin().read_to_string(&mut prompt).unwrap();
    std::fs::write(seen("prompt.txt"), &prompt).unwrap();
    let folder = Path::new(&args[args.iter().position(|a| a == "-C").unwrap() + 1]).to_path_buf();
    println!("{}", json!({"type":"thread.started","thread_id":"fake"}));
    match scenario {
        "hang" => {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child.env(FAKE, "park").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            // Never waited for on purpose: both stay until the check ends the whole tree.
            #[allow(clippy::zombie_processes)]
            let _child = o_core::sys::proc::hide_window(&mut child).spawn().unwrap();
            std::fs::write(seen("exec.pid"), std::process::id().to_string()).unwrap();
            park();
        }
        "refused" => {
            println!("{}", json!({"type":"item.completed","item":{"type":"command_execution","command":PROBE_COMMAND,"status":"declined","aggregated_output":"rejected: blocked by policy"}}));
            println!("{}", json!({"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}));
        }
        _ => {
            std::fs::write(folder.join("probe.txt"), "o-sandbox-ok\r\n").unwrap();
            println!("{}", json!({"type":"item.completed","item":{"type":"command_execution","command":PROBE_COMMAND,"status":"completed","exit_code":0}}));
            println!("{}", json!({"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}));
        }
    }
}

/// A throwaway agents root with the Codex profile home made, and the fake's notes folder.
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    notes: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("o-codex-sandbox-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
        let home = o_core::agents::clean::profile_home(&root, o_core::run::plan::ProviderId::Codex);
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let notes = root.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        Fixture { root, home, notes }
    }

    fn config(&self) -> PathBuf {
        self.home.join(".codex").join("config.toml")
    }

    fn check(&self, scenario: &'static str, deadlines: &Deadlines) -> codex_sandbox::Record {
        let (notes, config) = (self.notes.clone(), self.config());
        let make = move |args: &[&str]| -> std::io::Result<Command> {
            let mut command = Command::new(std::env::current_exe()?);
            command.env(FAKE, scenario).env(FAKE_ARGS, json!(args).to_string()).env(FAKE_DIR, &notes).env(FAKE_CONFIG, &config);
            Ok(command)
        };
        codex_sandbox::check(&self.home, &make, deadlines).unwrap()
    }

    fn note(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.notes.join(name)).ok()
    }

    /// Probe folders the check made beside the profile home and did not take away.
    fn probe_folders(&self) -> Vec<String> {
        std::fs::read_dir(self.home.parent().unwrap()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("sandbox-probe-")).collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const QUICK: Deadlines = Deadlines { reply: Duration::from_secs(20), setup: Duration::from_secs(20), probe: Duration::from_secs(60) };

fn setup_then_probe_passes() {
    let fixture = Fixture::new("ok");
    let record = fixture.check("unset", &QUICK);
    assert!(record.ok, "{record:?}");
    assert_eq!(record.version, "codex-cli 9.9.9");
    // The hash is of the config Codex's setup wrote, read after the setup.
    assert!(std::fs::read_to_string(fixture.config()).unwrap().contains("sandbox = \"unelevated\""));
    assert_eq!(record.config_hash, codex_sandbox::config_hash(&fixture.home));
    assert_eq!(codex_sandbox::read_record(&fixture.home).as_ref(), Some(&record));
    assert!(fixture.root.join("codex").join(RECORD).is_file());
    assert_eq!(serde_json::from_str::<Value>(&fixture.note("app-server.json").unwrap()).unwrap(), json!(["initialize", "initialized", "windowsSandbox/readiness", "windowsSandbox/setupStart"]));
    // The probe is a card's own launch: the fixed card arguments, a fresh folder, the prompt on stdin.
    let args: Vec<String> = serde_json::from_str(&fixture.note("exec.json").unwrap()).unwrap();
    assert_eq!(args[..4], ["exec", "--json", "-s", "workspace-write"]);
    assert_eq!(args[4], "-C");
    assert!(Path::new(&args[5]).starts_with(fixture.root.join("codex")) && !Path::new(&args[5]).starts_with(&fixture.home), "{}", args[5]);
    assert_eq!(args[6..], ["--skip-git-repo-check", "-"]);
    assert!(fixture.note("prompt.txt").unwrap().contains(PROBE_COMMAND));
    assert_eq!(fixture.probe_folders(), Vec::<String>::new());

    // Ready while Codex and the config are as checked; a different version or config asks again.
    assert_eq!(codex_sandbox::not_ready(&fixture.home, || Some("codex-cli 9.9.9".into())), None);
    assert_eq!(codex_sandbox::not_ready(&fixture.home, || Some("codex-cli 9.9.10".into())).as_deref(), Some(CHANGED));
    std::fs::write(fixture.config(), "[windows]\nsandbox = \"unelevated\"\n[notice]\n").unwrap();
    assert_eq!(codex_sandbox::not_ready(&fixture.home, || Some("codex-cli 9.9.9".into())).as_deref(), Some(CHANGED));

    // Already ready: no setup this time.
    let fixture = Fixture::new("ready");
    assert!(fixture.check("ready", &QUICK).ok);
    assert_eq!(serde_json::from_str::<Value>(&fixture.note("app-server.json").unwrap()).unwrap(), json!(["initialize", "initialized", "windowsSandbox/readiness"]));
}

fn wait_gone(pid: u32) {
    let until = Instant::now() + Duration::from_secs(20);
    while o_core::sys::proc::is_alive(pid) {
        assert!(Instant::now() < until, "process {pid} still runs");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn hanging_probe_is_stopped() {
    let fixture = Fixture::new("hang");
    std::fs::write(fixture.config(), "[windows]\nsandbox = \"unelevated\"\n").unwrap();
    let started = Instant::now();
    let record = fixture.check("hang", &Deadlines { probe: Duration::from_secs(5), ..QUICK });
    assert!(started.elapsed() < Duration::from_secs(40), "{:?}", started.elapsed());
    assert!(!record.ok);
    assert!(record.detail.contains(HANG_ISSUE) && record.detail.contains("5초"), "{}", record.detail);
    let reason = codex_sandbox::not_ready(&fixture.home, || panic!("a failed check needs no version")).unwrap();
    assert!(reason.starts_with("샌드박스 점검 실패(codex)") && reason.contains(HANG_ISSUE), "{reason}");
    let exec: u32 = fixture.note("exec.pid").expect("the fake exec started").parse().unwrap();
    wait_gone(exec);
    if cfg!(windows) {
        let child: u32 = fixture.note("child.pid").expect("the fake exec's child started").parse().unwrap();
        wait_gone(child);
    }
}

fn refused_probe_is_recorded() {
    let fixture = Fixture::new("refused");
    let record = fixture.check("refused", &QUICK);
    assert!(!record.ok);
    assert!(record.detail.contains("probe.txt") && record.detail.contains("rejected: blocked by policy"), "{}", record.detail);
    assert_eq!(fixture.probe_folders(), Vec::<String>::new());
}

fn no_version_no_check() {
    let fixture = Fixture::new("no-version");
    let record = fixture.check("no-version", &QUICK);
    assert!(!record.ok && record.version.is_empty());
    assert!(record.detail.contains("버전"), "{}", record.detail);
    assert_eq!(fixture.note("app-server.json"), None);
    assert_eq!(fixture.note("exec.json"), None);
}
