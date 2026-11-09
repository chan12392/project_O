//! Resume delivery of stored card messages while the app is running.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::run::{events::OEvent, plan::Plan, runner::Spawner, store, team};

/// Runs that still have an unprocessed message and have not ended or been stopped.
pub fn pending(home: &Path) -> Vec<String> {
    store::list_runs(home).into_iter().filter(|run_id| {
        if !store::is_run_id(run_id) || store::stop_requested(home, run_id)
            || store::read_events(home, run_id).iter().any(|e| matches!(e, OEvent::RunEnd { .. })) {
            return false;
        }
        let Ok(plan) = store::read_plan(home, run_id).and_then(|v| serde_json::from_value::<Plan>(v).map_err(std::io::Error::other)) else { return false };
        let speaker = team::Speaker { run_id: run_id.clone(), me: "lead".into(), cards: plan.cards.into_iter().map(|c| c.id).collect() };
        let Ok(messages) = team::read(home, &speaker, false) else { return false };
        let Some(first) = messages.first() else { return false };
        if first.kind == "approval" {
            return team::read(home, &speaker, true).is_ok_and(|all| all.iter().any(|m| m.from == "lead" && m.to == first.from && m.reply_to.as_deref() == Some(&first.id)));
        }
        first.state == "stored"
    }).collect()
}

/// Keep one recovery attempt per run; the lead's existing locks prevent duplicates
/// across this monitor, the runner, and other application processes.
pub fn watch(home: PathBuf, spawner: Arc<Spawner>) -> ! {
    let (_keep, stop) = mpsc::channel();
    watch_until(home, spawner, stop, Duration::from_secs(2));
    unreachable!()
}

/// The same monitor with a stop signal, used by hosts that control its lifetime.
pub fn watch_until(home: PathBuf, spawner: Arc<Spawner>, stop: mpsc::Receiver<()>, interval: Duration) {
    let mut jobs: HashMap<String, JoinHandle<()>> = HashMap::new();
    loop {
        jobs.retain(|_, job| !job.is_finished());
        for run_id in pending(&home) {
            if jobs.contains_key(&run_id) { continue; }
            let (home, spawner, id) = (home.clone(), Arc::clone(&spawner), run_id.clone());
            jobs.insert(run_id, thread::spawn(move || {
                while super::wake_run(&home, &id, spawner.as_ref()) == Ok(true) {}
            }));
        }
        if stop.recv_timeout(interval) != Err(mpsc::RecvTimeoutError::Timeout) { break; }
    }
    for (_, job) in jobs { let _ = job.join(); }
}
