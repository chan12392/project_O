//! The desktop windows (desktop only, no dev `/api` or `o` twin): the main window from
//! tauri.conf.json, the right panel's pop-out, and where a link asking for a new window goes.
//! WebView2 refuses `window.open` and `target="_blank"` unless the app answers, so every
//! window here answers the same way: an https page opens in the person's browser through
//! `open_external`, and nothing else opens; the page is told why in a short notice.

use tauri::webview::{NewWindowFeatures, NewWindowResponse};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, LogicalSize, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub const MAIN: &str = "main";
/// The one pop-out panel window.
pub const PANEL: &str = "panel";
const PANEL_SIZE: (f64, f64) = (760.0, 880.0);
/// Window events the screens listen for (app/src/shell/panelLayout.ts, app/src/shell/LinkNotice.tsx).
const LINK_BLOCKED: &str = "project-o:link-blocked";
const POPOUT_CLOSED: &str = "project-o:popout-closed";
const CLOSE_REQUESTED: &str = "project-o:close-requested";
const TRAY_ACTION: &str = "project-o:tray-action";

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(panel) = app.get_webview_window(PANEL) { let _ = panel.show(); }
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "열기", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "설정", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "정보", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &about, &quit])?;
    let icon = app.default_window_icon().cloned().expect("app icon");
    TrayIconBuilder::new().icon(icon).tooltip("Project O").menu(&menu).show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            show_main(app);
            if event.id().as_ref() != "open" {
                if let Some(window) = app.get_webview_window(MAIN) {
                    let _ = window.eval(window_event(TRAY_ACTION, Some(serde_json::json!(event.id().as_ref()))));
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
pub async fn hide_main(app: AppHandle) -> Result<(), String> {
    if let Some(panel) = app.get_webview_window(PANEL) { panel.hide().map_err(|e| e.to_string())?; }
    app.get_webview_window(MAIN).ok_or("메인 창이 없습니다")?.hide().map_err(|e| e.to_string())
}

/// A question waits on the person: the taskbar button flashes while the main window is
/// minimized or behind others (a window hidden in the tray has no button; its toast tells).
#[tauri::command]
pub async fn attention(app: AppHandle) -> Result<(), String> {
    let window = app.get_webview_window(MAIN).ok_or("메인 창이 없습니다")?;
    if window.is_focused().unwrap_or(false) { return Ok(()); }
    window.request_user_attention(Some(tauri::UserAttentionType::Critical)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn quit_activity() -> Result<serde_json::Value, String> {
    super::blocking(|| quit_activity_in(&o_core::run::store::default_home())).await?
}

fn quit_activity_in(home: &std::path::Path) -> Result<serde_json::Value, String> {
    for name in ["runs", "active"] {
        if let Err(error) = std::fs::read_dir(home.join(name)) {
            if error.kind() != std::io::ErrorKind::NotFound { return Err(error.to_string()); }
        }
    }
    let cards = o_core::run::store::active_markers(home).len();
    let leads = o_core::lead::active_count(home).map_err(|e| e.to_string())?;
    let runs = o_core::run::store::list_runs(home).iter().filter(|id| {
        o_core::run::snapshot::snapshot(&o_core::run::store::read_events(home, id))
            .is_some_and(|run| run.status == o_core::run::snapshot::RunStatus::Running)
    }).count();
    Ok(serde_json::json!({ "cards": cards, "leads": leads, "runs": runs }))
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) { app.exit(0); }

/// What happens to a link that asks for a new window.
#[derive(Debug, PartialEq)]
enum LinkTarget {
    /// An https page: the person's browser.
    Browser,
    /// Not opened; the reason is shown on the page.
    Refused(&'static str),
}

fn link_target(url: &str) -> LinkTarget {
    if o_core::agents::is_https_url(url) {
        LinkTarget::Browser
    } else if url.starts_with("http://") {
        LinkTarget::Refused("보안을 위해 https 주소만 브라우저로 엽니다. 이 http 링크는 열지 않았습니다.")
    } else {
        LinkTarget::Refused("이 링크는 앱에서 열 수 없습니다.")
    }
}

/// Never opens a webview window for a link: https goes to the browser, the rest is refused.
fn on_new_window<R: Runtime>(app: AppHandle<R>, label: &'static str) -> impl Fn(Url, NewWindowFeatures) -> NewWindowResponse<R> + Send + 'static {
    move |url, _| {
        let url = url.to_string();
        match link_target(&url) {
            LinkTarget::Browser => {
                // Off the UI thread: the shell call may take a moment.
                let app = app.clone();
                std::thread::spawn(move || {
                    if let Err(error) = o_core::agents::open_external(&url) {
                        tell(&app, label, &url, &error);
                    }
                });
            }
            LinkTarget::Refused(why) => tell(&app, label, &url, why),
        }
        NewWindowResponse::Deny
    }
}

/// Raises `project-o:link-blocked` with `{ url, message }` in that window.
fn tell<R: Runtime>(app: &AppHandle<R>, label: &str, url: &str, message: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.eval(window_event(LINK_BLOCKED, Some(serde_json::json!({ "url": url, "message": message }))));
    }
}

/// A fixed script raising one window event; the detail is JSON, so it stays data.
fn window_event(name: &str, detail: Option<serde_json::Value>) -> String {
    let name = serde_json::Value::from(name);
    match detail {
        Some(detail) => format!("window.dispatchEvent(new CustomEvent({name}, {{ detail: {detail} }}))"),
        None => format!("window.dispatchEvent(new Event({name}))"),
    }
}

/// The main window as tauri.conf.json describes it (`create: false` there), with the link handler.
pub fn open_main(app: &AppHandle) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned().expect("tauri.conf.json의 main 창");
    let window = WebviewWindowBuilder::from_config(app, &config)?.visible(false).on_new_window(on_new_window(app.clone(), MAIN)).build()?;
    fit_to_work_area(&window, (config.width, config.height), config.min_width.zip(config.min_height));
    window.show()?;
    // Closing the main window closes the pop-out too, so the app ends.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(main) = handle.get_webview_window(MAIN) {
                let _ = main.eval(window_event(CLOSE_REQUESTED, None));
            }
        }
        if let WindowEvent::Destroyed = event {
            if let Some(panel) = handle.get_webview_window(PANEL) {
                let _ = panel.destroy();
            }
        }
    });
    Ok(())
}

type Size = (f64, f64);

/// The window's size and minimum size, each no larger than the room the monitor's work area
/// leaves for the page (logical px). 1360×880 with a 1100×700 minimum does not fit a
/// 1920×1080 screen at 150% (about 1280×690 above the taskbar).
fn fit(size: Size, min: Option<Size>, room: Size) -> (Size, Option<Size>) {
    let clamp = |(w, h): Size| (w.min(room.0), h.min(room.1));
    (clamp(size), min.map(clamp))
}

/// Shrinks a window that does not fit its monitor's work area and centres it again.
fn fit_to_work_area<R: Runtime>(window: &WebviewWindow<R>, size: Size, min: Option<Size>) {
    let (Ok(Some(monitor)), Ok(outer), Ok(inner)) = (window.current_monitor(), window.outer_size(), window.inner_size()) else { return };
    let scale = monitor.scale_factor();
    let area = monitor.work_area().size.to_logical::<f64>(scale);
    // The title bar and borders take room too.
    let frame = (outer.width.saturating_sub(inner.width) as f64 / scale, outer.height.saturating_sub(inner.height) as f64 / scale);
    let (fitted, fitted_min) = fit(size, min, (area.width - frame.0, area.height - frame.1));
    if fitted == size && fitted_min == min {
        return;
    }
    let _ = window.set_min_size(fitted_min.map(|(w, h)| LogicalSize::new(w, h)));
    let _ = window.set_size(LogicalSize::new(fitted.0, fitted.1));
    let _ = window.center();
}

/// The pop-out's fixed app address: the panel only, always this PC's live runs (the desktop
/// app never shows the recorded demo), and the selection to open on.
fn popout_path(view: &str, project: Option<&str>, run: Option<&str>, card: Option<&str>) -> Result<String, String> {
    if view.is_empty() || view.len() > 32 || !view.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        return Err(format!("알 수 없는 패널 보기입니다: {view}"));
    }
    let pairs = [("project", project), ("run", run), ("card", card)];
    if pairs.iter().filter_map(|(_, v)| *v).any(|v| v.len() > 1024 || v.chars().any(char::is_control)) {
        return Err("패널 선택 값이 올바르지 않습니다".into());
    }
    let mut url = Url::parse("http://app.invalid/").expect("고정 주소");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("popout", "panel").append_pair("source", "live").append_pair("view", view);
        for (key, value) in pairs {
            if let Some(value) = value {
                query.append_pair(key, value);
            }
        }
    }
    Ok(format!("index.html?{}", url.query().unwrap_or_default()))
}

/// "별도 창으로 띄우기": the panel in a window of its own, or the open one brought forward.
#[tauri::command]
pub async fn popout_open(app: AppHandle, view: String, project: Option<String>, run: Option<String>, card: Option<String>) -> Result<(), String> {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.unminimize();
        return panel.set_focus().map_err(|e| e.to_string());
    }
    let path = popout_path(&view, project.as_deref(), run.as_deref(), card.as_deref())?;
    let window = WebviewWindowBuilder::new(&app, PANEL, WebviewUrl::App(path.into()))
        .title("Project O 패널")
        .inner_size(PANEL_SIZE.0, PANEL_SIZE.1)
        .visible(false)
        .on_new_window(on_new_window(app.clone(), PANEL))
        .build()
        .map_err(|e| format!("패널 창을 열지 못했습니다: {e}"))?;
    fit_to_work_area(&window, PANEL_SIZE, None);
    window.show().map_err(|e| e.to_string())?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            if let Some(main) = handle.get_webview_window(MAIN) {
                let _ = main.eval(window_event(POPOUT_CLOSED, None));
            }
        }
    });
    Ok(())
}

/// "패널 다시 붙이기": closes the pop-out (a page cannot close a window it did not open).
#[tauri::command]
pub async fn popout_close(app: AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_warning_counts_running_work_and_old_lead_threads() {
        use o_core::run::events::OEvent;
        use o_core::run::plan::ProviderId;
        use o_core::run::store::{append_event, mark_active, ActiveMarker};
        let home = crate::commands::temp("quit-activity");
        let project = crate::commands::temp("quit-project");
        let project = project.to_string_lossy().into_owned();
        let thread = o_core::lead::start(&home, &project).unwrap();
        let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(o_core::lead::thread_dir(&home, &thread).join("turn.lock")).unwrap();
        lock.try_lock().unwrap();
        o_core::lead::start(&home, &project).unwrap();
        std::fs::create_dir_all(home.join("runs").join("r1")).unwrap();
        append_event(&home, "r1", &OEvent::RunStart { at: "2026-11-02T00:00:00Z".into(), run_id: "r1".into(), title: "t".into(), project, cards: vec![] }).unwrap();
        mark_active(&home, &ActiveMarker { run_id: "r1".into(), card_id: "a".into(), provider: ProviderId::Claude, pid: std::process::id() }).unwrap();
        assert_eq!(quit_activity_in(&home).unwrap(), serde_json::json!({ "cards": 1, "leads": 1, "runs": 1 }));
        drop(lock);
        assert_eq!(quit_activity_in(&home).unwrap()["leads"], 0);
    }

    #[test]
    fn only_https_links_reach_the_browser() {
        assert_eq!(link_target("https://github.com/acme/app/pull/7"), LinkTarget::Browser);
        // WebView2 hands over a parsed URL, so the scheme is already lowercase.
        assert!(matches!(link_target("http://example.com/"), LinkTarget::Refused(why) if why.contains("https")));
        for url in ["file:///C:/Windows/System32/calc.exe", "javascript:alert(1)", "http://tauri.localhost/index.html?popout=panel", "about:blank", "https://user@evil.example/"] {
            assert_ne!(link_target(url), LinkTarget::Browser, "{url}");
        }
    }

    /// On Windows the native file-drop handler swallows the drag, so the composer's HTML5
    /// `drop` (attachments) never fires unless the main window turns it off. Nothing listens
    /// for Tauri's own drag-drop events.
    #[test]
    fn main_window_leaves_file_drops_to_the_page() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let main = config["app"]["windows"].as_array().unwrap().iter().find(|w| w["label"] == MAIN).unwrap();
        assert_eq!(main["dragDropEnabled"], false);
        assert_eq!(main["create"], false, "open_main이 새 창 처리기와 함께 만든다");
    }

    #[test]
    fn window_events_carry_their_detail_as_json() {
        assert_eq!(window_event(POPOUT_CLOSED, None), r#"window.dispatchEvent(new Event("project-o:popout-closed"))"#);
        let script = window_event(LINK_BLOCKED, Some(serde_json::json!({ "url": "http://x/\"); alert(1); (\"", "message": "m" })));
        assert_eq!(script, r#"window.dispatchEvent(new CustomEvent("project-o:link-blocked", { detail: {"url":"http://x/\"); alert(1); (\"","message":"m"} }))"#);
    }

    #[test]
    fn windows_never_start_larger_than_the_work_area() {
        // 1920×1080 at 150% leaves about 1280×688 above the taskbar; the title bar takes ~31 more.
        let room = (1280.0, 657.0);
        assert_eq!(fit((1360.0, 880.0), Some((1100.0, 700.0)), room), ((1280.0, 657.0), Some((1100.0, 657.0))));
        // A roomy screen keeps the configured sizes.
        assert_eq!(fit((1360.0, 880.0), Some((1100.0, 700.0)), (2560.0, 1400.0)), ((1360.0, 880.0), Some((1100.0, 700.0))));
        assert_eq!(fit((760.0, 880.0), None, room), ((760.0, 657.0), None));
    }

    #[test]
    fn popout_opens_the_live_panel_with_an_encoded_selection() {
        let path = popout_path("board", Some("C:\\work\\app & co"), Some("run-1"), None).unwrap();
        assert_eq!(path, "index.html?popout=panel&source=live&view=board&project=C%3A%5Cwork%5Capp+%26+co&run=run-1");
        assert!(popout_path("", None, None, None).is_err());
        assert!(popout_path("board&source=demo", None, None, None).is_err());
        assert!(popout_path("board", Some("a\nb"), None, None).is_err());
        assert!(popout_path("board", None, None, Some(&"x".repeat(1025))).is_err());
    }
}
