//! Test fixture with the same handle-owner entry point as the installed `o` binary.
#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--o-handle-owner") {
        std::process::exit(i32::from(o_core::sys::win_launch::run_handle_owner(&args[1..]).is_err()));
    }
    std::process::exit(2);
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() { std::process::exit(2); }

#[cfg(target_os = "macos")]
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--o-mac-worker") {
        std::process::exit(i32::from(o_core::sys::macos_worker::run_helper(&args[1..]).is_err()));
    }
    std::process::exit(2);
}
