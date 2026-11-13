//! A detached process must not hold its starter's output pipe: a caller reading `o dispatch`'s
//! output waited until the whole run ended (release gate run 20261109-154052-ksn0).
#![cfg(windows)]

use std::process::Command;
use std::time::{Duration, Instant};

const ROLE: &str = "O_TEST_DETACHED_ROLE";
const NAME: &str = "a_detached_child_does_not_hold_the_starters_output_pipe";

fn this_test() -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", NAME, "--nocapture", "--test-threads", "1"]);
    command
}

#[test]
fn a_detached_child_does_not_hold_the_starters_output_pipe() {
    match std::env::var(ROLE).as_deref() {
        // The detached runner: outlives its starter.
        Ok("runner") => std::thread::sleep(Duration::from_secs(6)),
        // `o dispatch`: starts the runner and returns at once.
        Ok("starter") => {
            let started = o_core::sys::proc::spawn_detached(this_test().env(ROLE, "runner"));
            assert!(started.is_ok(), "{started:?}");
            println!("started");
        }
        // The caller reads the starter's output through a pipe.
        _ => {
            let start = Instant::now();
            let output = this_test().env(ROLE, "starter").output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("started"));
            assert!(start.elapsed() < Duration::from_secs(4), "the output pipe stayed open for {:?}", start.elapsed());
        }
    }
}
