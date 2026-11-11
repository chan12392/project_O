//! The team channel of a sandboxed card (ADR-0043). Its sandbox may write one folder, the card's
//! mailbox, and nothing else of the run: `o msg` inside the card leaves a request there and the
//! runner, which knows which card it started, runs it as that card and leaves the answer. The run's
//! events, plan and the other cards' files stay out of the card's reach.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::team_command::Output;

/// Set for a card whose `o msg` goes through its mailbox: the mailbox folder.
pub const DIR_ENV: &str = "O_MSG_DIR";

/// The verbs a card may send through its mailbox: `o msg` less what only a person or a card
/// with fixed commands uses (`answer-approval`, `post`).
const VERBS: [&str; 8] = ["send", "read", "wait", "ack", "delivered", "unknown", "retry", "cancel"];
const MAX_REQUEST: u64 = 256 * 1024;
const MAX_ARGS: usize = 64;
/// `o msg wait` holds its answer for up to 30 seconds.
const ANSWER_WITHIN: Duration = Duration::from_secs(75);
/// Requests answered at once; one more waits for the next pass.
const AT_ONCE: usize = 4;

/// `<run folder>/mailbox/<card>`.
pub fn mailbox(run_dir: &str, card_id: &str) -> PathBuf {
    Path::new(run_dir).join("mailbox").join(card_id)
}

/// The mailbox this process was given, if it is a card that has one.
pub fn from_env() -> Option<PathBuf> {
    std::env::var_os(DIR_ENV).filter(|dir| !dir.is_empty()).map(PathBuf::from)
}

fn is_nonce(text: &str) -> bool {
    !text.is_empty() && text.len() <= 64 && text.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

/// The card's side: leaves `args` in the mailbox and waits for the runner's answer.
pub fn ask(dir: &Path, args: &[String]) -> io::Result<Output> {
    ask_within(dir, args, ANSWER_WITHIN)
}

fn ask_within(dir: &Path, args: &[String], within: Duration) -> io::Result<Output> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let nonce = format!("{nanos:x}-{:x}-{:x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let request = dir.join(format!("q-{nonce}.json"));
    let answer = dir.join(format!("r-{nonce}.json"));
    let draft = dir.join(format!("q-{nonce}.tmp"));
    fs::write(&draft, serde_json::to_vec(&json!({ "args": args })).map_err(io::Error::other)?)?;
    fs::rename(&draft, &request)?;
    let deadline = Instant::now() + within;
    loop {
        if let Ok(bytes) = fs::read(&answer) {
            let _ = fs::remove_file(&answer);
            return serde_json::from_slice(&bytes).map_err(|_| io::Error::other("팀 대화 응답을 읽지 못했습니다"));
        }
        if Instant::now() >= deadline {
            let _ = fs::remove_file(&request);
            return Err(io::Error::other("팀 대화 중계가 응답하지 않습니다. 카드를 실행한 O가 끝났을 수 있습니다"));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// The runner's side, for as long as the card runs.
pub struct Relay {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Relay {
    /// Makes the mailbox and answers what the card leaves in it with `handler`, which runs the
    /// request as the card the runner started: nothing in a request says who is asking.
    pub fn start(dir: PathBuf, handler: impl Fn(&[String]) -> Output + Send + Sync + 'static) -> io::Result<Relay> {
        fs::create_dir_all(&dir)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handler = Arc::new(handler);
        let busy = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker = thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                for (nonce, args) in requests(&dir, AT_ONCE.saturating_sub(busy.load(Ordering::Acquire))) {
                    let (dir, handler, busy) = (dir.clone(), Arc::clone(&handler), Arc::clone(&busy));
                    busy.fetch_add(1, Ordering::AcqRel);
                    thread::spawn(move || {
                        let output = match args {
                            Ok(args) => handler(&args),
                            Err(why) => Output { code: 1, out: String::new(), err: format!("{why}\n") },
                        };
                        let _ = answer(&dir, &nonce, &output);
                        busy.fetch_sub(1, Ordering::AcqRel);
                    });
                }
                thread::sleep(Duration::from_millis(40));
            }
        });
        Ok(Relay { stop, worker: Some(worker) })
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

/// A mailbox the card replaced with a link is not read or written: the runner would follow it.
fn plain_folder(dir: &Path) -> bool {
    fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir())
}

/// Takes up to `room` requests out of the mailbox. Each is removed as it is taken, so it is
/// answered once; what is not a plain, small request file is removed unanswered.
fn requests(dir: &Path, room: usize) -> Vec<(String, Result<Vec<String>, &'static str>)> {
    if room == 0 || !plain_folder(dir) { return Vec::new(); }
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if found.len() >= room { break; }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(nonce) = name.strip_prefix("q-").and_then(|rest| rest.strip_suffix(".json")).filter(|nonce| is_nonce(nonce)) else { continue };
        let path = entry.path();
        let plain = fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_REQUEST);
        let bytes = if plain { fs::read(&path).ok() } else { None };
        if fs::remove_file(&path).or_else(|_| fs::remove_dir(&path)).is_err() { continue; }
        found.push((nonce.to_string(), bytes.ok_or("팀 대화 요청이 너무 크거나 파일이 아닙니다").and_then(|bytes| parse(&bytes))));
    }
    found
}

fn parse(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
    const BAD: &str = "팀 대화 요청 형식이 올바르지 않습니다";
    let value: Value = serde_json::from_slice(bytes).map_err(|_| BAD)?;
    let args = value.get("args").and_then(Value::as_array).filter(|args| args.len() <= MAX_ARGS).ok_or(BAD)?;
    let args: Vec<String> = args.iter().map(|arg| arg.as_str().map(str::to_string)).collect::<Option<_>>().ok_or(BAD)?;
    if !args.first().is_some_and(|verb| VERBS.contains(&verb.as_str())) {
        return Err("이 카드에서는 o msg send·read·wait·ack·delivered·unknown·retry·cancel만 쓸 수 있습니다");
    }
    Ok(args)
}

/// Written under a name the card cannot know first, then renamed: a half-written answer is never read.
fn answer(dir: &Path, nonce: &str, output: &Output) -> io::Result<()> {
    if !plain_folder(dir) { return Err(io::Error::other("팀 대화 폴더가 바뀌었습니다")); }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let draft = dir.join(format!("r-{nonce}.{nanos:x}-{:x}.tmp", NEXT.fetch_add(1, Ordering::Relaxed)));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&draft)?;
    file.write_all(&serde_json::to_vec(output).map_err(io::Error::other)?)?;
    drop(file);
    fs::rename(&draft, dir.join(format!("r-{nonce}.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("o-msg-relay-{name}-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_request_is_answered_once_by_the_runner_side() {
        let dir = folder("answer").join("mailbox").join("a");
        let calls = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&calls);
        let _relay = Relay::start(dir.clone(), move |args| {
            seen.fetch_add(1, Ordering::Relaxed);
            Output { code: 0, out: args.join("|"), err: String::new() }
        }).unwrap();
        let output = ask_within(&dir, &["send".into(), "--to".into(), "lead".into(), "안녕".into()], Duration::from_secs(10)).unwrap();
        assert_eq!(output, Output { code: 0, out: "send|--to|lead|안녕".into(), err: String::new() });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "the request and its answer are gone");
    }

    #[test]
    fn what_is_not_a_team_request_never_reaches_the_handler() {
        let dir = folder("refuse");
        let _relay = Relay::start(dir.clone(), |_| panic!("the handler must not run")).unwrap();
        for args in [vec!["answer-approval", "--id", "m1", "-"], vec!["post"], vec![]] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            let output = ask_within(&dir, &args, Duration::from_secs(10)).unwrap();
            assert_eq!(output.code, 1, "{args:?}");
            assert!(output.err.contains("o msg send"), "{output:?}");
        }
        fs::write(dir.join("q-bad.json"), b"not json").unwrap();
        fs::write(dir.join("q-big.json"), vec![b' '; MAX_REQUEST as usize + 1]).unwrap();
        fs::create_dir(dir.join("q-folder.json")).unwrap();
        fs::write(dir.join("q-..json"), b"{}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !(dir.join("r-bad.json").exists() && dir.join("r-big.json").exists() && dir.join("r-folder.json").exists()) {
            assert!(Instant::now() < deadline, "stray requests were not refused");
            thread::sleep(Duration::from_millis(20));
        }
        for name in ["bad", "big", "folder"] {
            let output: Output = serde_json::from_slice(&fs::read(dir.join(format!("r-{name}.json"))).unwrap()).unwrap();
            assert_eq!(output.code, 1);
            assert!(!dir.join(format!("q-{name}.json")).exists());
        }
        assert!(dir.join("q-..json").exists(), "a name that is not a request is left alone");
    }

    #[test]
    fn nobody_answers_once_the_runner_is_gone() {
        let dir = folder("gone");
        drop(Relay::start(dir.clone(), |_| Output::default()).unwrap());
        let error = ask_within(&dir, &["read".into()], Duration::from_millis(200)).unwrap_err();
        assert!(error.to_string().contains("응답하지 않습니다"), "{error}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }
}
