//! project_O.exe: the React screens (app/dist) in one window, reading runs, usage and
//! agent CLIs through o-core directly. The screens cannot start processes
//! themselves; every command here is fixed and runs off the UI thread.

// A release build is a windowed app: no console behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

/// Puts this exe's folder first on PATH, so every child (agent CLIs, `o dispatch`, cards
/// running `o msg`/`o context`) finds the `o.exe` installed beside project_O.exe, even when
/// `o` was never `cargo install`ed. Runs first in `main`, before any thread exists.
fn own_folder_first_on_path() {
    let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.to_path_buf())) else { return };
    let rest: Vec<_> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|p| *p != dir).collect()).unwrap_or_default();
    if let Ok(path) = std::env::join_paths(std::iter::once(dir).chain(rest)) {
        std::env::set_var("PATH", path);
    }
}

fn main() {
    own_folder_first_on_path();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::runs,
            commands::usage,
            commands::usage_refresh,
            commands::budget,
            commands::budget_set_percent,
            commands::agents_status,
            commands::agent_install,
            commands::agent_login,
            commands::open_external,
            commands::about,
            commands::review,
            commands::tokens,
        ])
        .run(tauri::generate_context!())
        .expect("Project O 창을 열지 못했습니다");
}
