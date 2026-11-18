//! project_O.exe: the React screens (app/dist) in one window, reading runs, usage and
//! agent CLIs through o-core directly. The screens cannot start processes
//! themselves; every command here is fixed and runs off the UI thread.

// A release build is a windowed app: no console behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod installer_paths;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if args.first().is_some_and(|arg| arg == "--o-handle-owner") {
        std::process::exit(i32::from(o_core::sys::win_launch::run_handle_owner(&args[1..].iter().map(Into::into).collect::<Vec<_>>()).is_err()));
    }
    #[cfg(target_os = "macos")]
    if args.first().is_some_and(|arg| arg == "--o-mac-worker") {
        std::process::exit(i32::from(o_core::sys::macos_worker::run_helper(&args[1..].iter().map(Into::into).collect::<Vec<_>>()).is_err()));
    }
    #[cfg(windows)]
    if args.first().is_some_and(|arg| arg == "_job-watch") {
        if o_core::agents::agy::inside_agent() { std::process::exit(2); }
        std::process::exit(i32::from(o_core::run::job_watch::run(&args[1..]).is_err()));
    }
    if args.first().is_some_and(|arg| matches!(arg.as_str(), "_run-approved" | "_followup-approved")) {
        if o_core::agents::agy::inside_agent() { std::process::exit(2); }
        o_core::sys::resolve::own_folder_first_on_path();
        let result = args.get(1).ok_or_else(|| "실행 id가 없습니다".to_string())
            .and_then(|id| o_core::run::input::receive(&o_core::run::store::default_home(), id, args[0] == "_followup-approved"))
            .and_then(|input| o_core::run::runner::run_detached(&input));
        std::process::exit(i32::from(result.is_err()));
    }
    // Children find the o.exe installed beside project_O.exe (the same init as `o`).
    o_core::sys::resolve::own_folder_first_on_path();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| commands::window::show_main(app)))
        .plugin(tauri_plugin_notification::init())
        // Only Rust opens the folder picker (commands::projects::pick_folder); no capability grants the screens dialog calls.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            commands::window::setup_tray(app.handle())?;
            commands::window::open_main(app.handle())?;
            commands::update::start_checks();
            std::thread::spawn(|| o_core::lead::recovery::watch(o_core::run::store::default_home(), std::sync::Arc::new(o_core::run::runner::official_command)));
            Ok(())
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("Project O 창을 열지 못했습니다");
}

#[cfg(test)]
mod tests {
    /// The only plugin permissions the screens get: showing a notification and asking for leave
    /// to (the main window only). The app's own commands are not plugin permissions.
    #[test]
    fn capabilities_grant_notifications_only() {
        let capability: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        let permissions: Vec<&str> = capability["permissions"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        assert_eq!(permissions, ["notification:allow-is-permission-granted", "notification:allow-request-permission", "notification:allow-notify"]);
        assert_eq!(std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities")).unwrap().count(), 1, "다른 권한 파일이 없어야 합니다");
    }
}
