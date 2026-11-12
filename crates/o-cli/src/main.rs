//! `o`: argument parsing and printing. Everything that decides or touches
//! state lives in `o_core`; the guard commands live in `o_guard`. Each command
//! is a file in `commands/`, registered in `commands::COMMANDS`.

use o_core::run::store::default_home;

mod commands;

fn main() {
    #[cfg(target_os = "macos")]
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|arg| arg == "--o-mac-worker") {
            std::process::exit(i32::from(o_core::sys::macos_worker::run_helper(&args[1..]).is_err()));
        }
    }
    #[cfg(windows)]
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|arg| arg == "--o-handle-owner") {
            std::process::exit(i32::from(o_core::sys::win_launch::run_handle_owner(&args[1..]).is_err()));
        }
    }
    // Children (`o _run`, agent CLIs, cards running `o msg`/`o context`) find this `o`, also
    // when a scheduled task starts it by its full path with its folder not on PATH.
    o_core::sys::resolve::own_folder_first_on_path();
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    if commands::inside_agent() {
        let blocked = matches!(args.first().map(String::as_str), Some("dispatch" | "followup" | "_run" | "_run-approved" | "_lead-watch" | "_job-watch" | "_followup" | "_followup-approved" | "review" | "_review" | "_review-approved" | "image" | "schedule" | "changes" | "stop" | "sync-mirror" | "events" | "runs"))
            || matches!((args.first().map(String::as_str), args.get(1).map(String::as_str)), (Some("lead"), Some("send" | "dispatch" | "note" | "wake" | "retry" | "decide" | "stop" | "stop-all" | "forward" | "attach" | "new")) | (Some("projects"), Some("open")) | (Some("agents"), Some("setup")) | (Some("issues"), Some("record" | "classify" | "resolve" | "reopen")))
            || (args.first().is_some_and(|a| a == "_lead-approve") && o_core::agents::agy::inside_card())
            || (args.first().is_some_and(|a| a == "resources") && args.get(1).is_some_and(|a| a == "recover") && args.iter().any(|a| a == "--confirm-exited"));
        if blocked {
            eprintln!("에이전트 안에서는 작업을 시작할 수 없습니다. 사용자가 실행해 주세요.");
            return 2;
        }
    }
    let home = default_home();
    let (name, rest) = match args.split_first() {
        Some((name, rest)) => (Some(name.as_str()), rest),
        None => (None, &[][..]),
    };
    match name.and_then(commands::find) {
        Some(command) => (command.run)(&home, rest),
        None => {
            println!("{}", commands::help());
            i32::from(name.is_some_and(|c| c != "help" && c != "--help"))
        }
    }
}
