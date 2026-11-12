//! The Tauri commands the screens call (app/src/platform/backend, app/src/data/useLiveRuns.ts),
//! one file per domain and registered once in [`handler`]. Each runs its work on a
//! blocking thread and calls o-core directly: no shell, and any CLI it asks runs
//! without a window (sign-in alone opens a console the person drives). Every
//! domain command has a dev-server twin under `/api` (app/dev/liveRuns.ts) and an `o`
//! command that prints the same JSON. Desktop updates only expose an unavailable notice in dev;
//! the window commands (pop-out) exist only in the desktop app.

mod about;
mod agents;
mod agy;
mod graph;
mod lead;
mod images;
mod issues;
mod projects;
mod review;
mod runs;
mod schedule;
mod term;
mod tokens;
mod usage;
mod dream;
pub(crate) mod update;
pub(crate) mod window;

/// Every command the screens may call.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        runs::runs,
        term::term_open,
        term::term_list,
        term::term_read,
        term::term_write,
        term::term_resize,
        term::term_paste,
        term::term_close,
        runs::run_changes,
        runs::run_followup,
        runs::run_reply,
        usage::usage,
        usage::resource_leases,
        usage::resource_recover,
        usage::usage_refresh,
        usage::budget,
        usage::budget_set_percent,
        usage::budget_set_enabled,
        usage::ram_get,
        usage::ram_set,
        usage::billing_get,
        usage::billing_set,
        usage::fallback_get,
        usage::fallback_set,
        agy::agy_get,
        agy::agy_set,
        usage::yolo_get,
        usage::yolo_set,
        agents::agents_status,
        agents::agents_catalog,
        agents::agent_install,
        agents::agent_login,
        agents::agent_setup,
        agents::agent_terminal,
        agents::open_external,
        about::about,
        review::review,
        issues::issues_search,
        issues::issues_list,
        issues::issues_record,
        issues::issues_classify,
        issues::issues_resolve,
        tokens::tokens,
        lead::lead_show,
        lead::lead_note,
        lead::lead_new,
        lead::lead_close,
        lead::lead_writer,
        lead::lead_attach,
        lead::lead_send,
        lead::lead_dispatch,
        lead::lead_forward,
        lead::lead_stop,
        lead::lead_decide,
        lead::lead_wake,
        lead::lead_retry,
        lead::lead_answer_approval,
        lead::lead_stop_all,
        lead::lead_models,
        graph::graph_view,
        graph::graph_overall_part,
        graph::graph_build,
        graph::graph_deep,
        graph::graph_settings,
        dream::dream_list,
        dream::dream_read,
        dream::dream_graph,
        dream::dream_add,
        dream::dream_remove,
        dream::dream_changes,
        dream::dream_revert,
        images::image_read,
        images::image_open,
        schedule::schedule_list,
        schedule::schedule_create,
        schedule::schedule_delete,
        schedule::schedule_set_enabled,
        projects::projects_list,
        projects::project_open,
        projects::projects_add,
        projects::projects_remove,
        projects::projects_new,
        projects::projects_root,
        projects::projects_check,
        projects::pick_folder,
        update::update_check,
        update::update_install,
        update::update_pending,
        update::update_cancel,
        update::update_status,
        window::popout_open,
        window::popout_close,
        window::hide_main,
        window::attention,
        window::quit_activity,
        window::quit_app,
    ]
}

/// The installed sidecar or the CLI built beside this app by prepare-local-cli.mjs.
fn o_binary() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "o.exe" } else { "o" };
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))).filter(|p| p.is_file());
    beside
}

/// An absolute folder that exists; o-core checks again whether it may be a project.
fn project_arg(project: &str) -> Result<(), String> {
    let path = std::path::Path::new(project);
    if !path.is_absolute() || !path.is_dir() {
        return Err(format!("프로젝트 폴더가 없습니다: {project}"));
    }
    Ok(())
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())
}

#[cfg(test)]
fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("project-o-shell-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
