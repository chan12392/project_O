//! project_O.exe: the React screens (app/dist) in one window, reading runs, usage and
//! agent CLIs through o-core directly. The screens cannot start processes
//! themselves; every command here is fixed and runs off the UI thread.

// A release build is a windowed app: no console behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
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
        ])
        .run(tauri::generate_context!())
        .expect("Project O 창을 열지 못했습니다");
}
