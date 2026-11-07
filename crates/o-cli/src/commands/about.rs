//! `o about [--json]`: this build's version and where O keeps its state.

use std::path::Path;

use o_core::agents::clean;
use serde_json::json;

use super::Command;

pub const ABOUT: Command = Command {
    name: "about",
    usage: "  o about [--json]           버전과 상태·프로필 폴더 위치",
    run: |home, args| about(home, args.iter().any(|a| a == "--json")),
};

fn about(home: &Path, as_json: bool) -> i32 {
    let version = env!("CARGO_PKG_VERSION");
    let agents_home = clean::default_root();
    if as_json {
        println!("{}", json!({ "version": version, "home": home.to_string_lossy(), "agentsHome": agents_home.to_string_lossy(), "capabilities": ["work-graph-v1", "lead-worktrees-v1", "lead-copy-workspaces-v1", "session-followup-v1", "overall-lead-v1"] }));
    } else {
        println!("o {version}\n상태 폴더   {}\n에이전트 프로필 {}", home.to_string_lossy(), agents_home.to_string_lossy());
    }
    0
}
