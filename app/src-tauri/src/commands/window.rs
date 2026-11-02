//! The desktop windows (desktop only, no dev `/api` or `o` twin): the main window from
//! tauri.conf.json, the right panel's pop-out, and where a link asking for a new window goes.
//! WebView2 refuses `window.open` and `target="_blank"` unless the app answers, so every
//! window here answers the same way: an https page opens in the person's browser through
//! `open_external`, and nothing else opens; the page is told why in a short notice.

use tauri::webview::{NewWindowFeatures, NewWindowResponse};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const MAIN: &str = "main";
/// The one pop-out panel window.
pub const PANEL: &str = "panel";
const PANEL_SIZE: (f64, f64) = (760.0, 880.0);
/// Window events the screens listen for (app/src/shell/panelLayout.ts, app/src/shell/LinkNotice.tsx).
const LINK_BLOCKED: &str = "project-o:link-blocked";
const POPOUT_CLOSED: &str = "project-o:popout-closed";

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
    let window = WebviewWindowBuilder::from_config(app, &config)?.on_new_window(on_new_window(app.clone(), MAIN)).build()?;
    // Closing the main window closes the pop-out too, so the app ends.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            if let Some(panel) = handle.get_webview_window(PANEL) {
                let _ = panel.destroy();
            }
        }
    });
    Ok(())
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
        .on_new_window(on_new_window(app.clone(), PANEL))
        .build()
        .map_err(|e| format!("패널 창을 열지 못했습니다: {e}"))?;
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
    fn popout_opens_the_live_panel_with_an_encoded_selection() {
        let path = popout_path("board", Some("C:\\work\\app & co"), Some("run-1"), None).unwrap();
        assert_eq!(path, "index.html?popout=panel&source=live&view=board&project=C%3A%5Cwork%5Capp+%26+co&run=run-1");
        assert!(popout_path("", None, None, None).is_err());
        assert!(popout_path("board&source=demo", None, None, None).is_err());
        assert!(popout_path("board", Some("a\nb"), None, None).is_err());
        assert!(popout_path("board", None, None, Some(&"x".repeat(1025))).is_err());
    }
}
