//! `o`: argument parsing and printing. Everything that decides or touches
//! state lives in `o_core`; the guard commands live in `o_guard`. Each command
//! is a file in `commands/`, registered in `commands::COMMANDS`.

use o_core::run::store::default_home;

mod commands;

fn main() {
    // Children (`o _run`, agent CLIs, cards running `o msg`/`o context`) find this `o`, also
    // when a scheduled task starts it by its full path with its folder not on PATH.
    o_core::sys::resolve::own_folder_first_on_path();
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    if std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) || std::env::var_os("O_CARD_ID").is_some_and(|v| !v.is_empty()) {
        let blocked = matches!(args.first().map(String::as_str), Some("dispatch" | "followup" | "_run" | "_lead-watch" | "_followup" | "review" | "_review" | "image" | "schedule" | "changes"))
            || matches!((args.first().map(String::as_str), args.get(1).map(String::as_str)), (Some("lead"), Some("send" | "dispatch" | "note" | "wake" | "retry")) | (Some("projects"), Some("open")) | (Some("issues"), Some("record" | "classify" | "resolve" | "reopen")));
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
