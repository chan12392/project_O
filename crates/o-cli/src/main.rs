//! `o`: argument parsing and printing. Everything that decides or touches
//! state lives in `o_core`; the guard commands live in `o_guard`. Each command
//! is a file in `commands/`, registered in `commands::COMMANDS`.

use o_core::run::store::default_home;

mod commands;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
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
