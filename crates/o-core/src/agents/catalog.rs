//! Adapted from stablyai/orca (MIT); see THIRD_PARTY_NOTICES.md.
//! Every agent CLI O knows of (ADR-0015, research `o-research/agent-clis.md` §2): one
//! table the app (Settings → 에이전트 → 다른 에이전트 CLI) and `o agents list --all` both read.
//!
//! An entry is connected when a `ProviderId` with its id exists; otherwise it is planned (with
//! its place in the connection order) or not supported, with the reason. Detection looks for
//! the binary on PATH and asks it `--version` only (a short deadline, no window, no shell):
//! nothing else is ever run for an entry that is not connected.
//!
//! The clean profile layer reads [`profile_support`]: a CLI whose config cannot be moved into
//! a profile home is "프로필 미지원", and O never runs it with "project_O 프로필 사용".

use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::sync::{LazyLock, Mutex};

use serde::Serialize;

use crate::run::plan::ProviderId;
use crate::sys::proc;
use crate::sys::resolve::{refresh_paths, resolve_cli, which};

/// What O plans for a CLI that is not connected yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Connect it, in this order (1 first; research §3.1: detected first, then by how well it fits).
    Planned(u8),
    /// Not connected, and why.
    Unsupported(&'static str),
}

/// Whether the CLI's config can move into a clean profile home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// How it moves (the variable or folder O points at the profile).
    Relocatable(&'static str),
    /// Why it cannot (a clean launch would read the person's own config).
    Unsupported(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct AgentSpec {
    /// The id it would have as a provider (`ProviderId::as_str`).
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    /// Binary names to look for on PATH, first match wins.
    pub bins: &'static [&'static str],
    pub connection: Option<&'static Connection>,
    /// The vendor's official install page (https), when there is one.
    pub install_url: Option<&'static str>,
    pub plan: Plan,
    pub profile: Profile,
    /// One line on terms of service or quirks people should know before connecting it.
    pub note: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Headless {
    pub fixed: &'static [&'static str],
    pub model_flag: &'static str,
    pub prompt: PromptDelivery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptDelivery { Stdin, Argument, AfterSeparator }

pub type Entry = AgentSpec;

/// Only connected providers have launch recipes; policy and parsers remain code.
#[derive(Debug)]
pub struct Connection {
    pub headless: Headless,
    pub models: &'static [&'static str],
}

const CLAUDE: Connection = Connection { headless: Headless { fixed: &["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--allowedTools"], model_flag: "--model", prompt: PromptDelivery::Stdin }, models: &[] };
const CODEX: Connection = Connection { headless: Headless { fixed: &["exec", "--json", "-s", "workspace-write"], model_flag: "-m", prompt: PromptDelivery::Stdin }, models: &["debug", "models"] };
const AGY: Connection = Connection { headless: Headless { fixed: &["--output-format", "stream-json", "--mode", "accept-edits"], model_flag: "--model", prompt: PromptDelivery::Argument }, models: &["models"] };
const OPENCODE: Connection = Connection { headless: Headless { fixed: &["run", "--format", "json", "--standalone"], model_flag: "-m", prompt: PromptDelivery::Stdin }, models: &["models", "--standalone"] };
const DEVIN: Connection = Connection { headless: Headless { fixed: &["-p"], model_flag: "--model", prompt: PromptDelivery::AfterSeparator }, models: &[] };

pub fn connection(id: &str) -> &'static Connection {
    entry(id).and_then(|agent| agent.connection).expect("connected provider has a recipe")
}

macro_rules! extra {
    ($id:literal, $name:literal, [$($bin:literal),+], $url:literal) => {
        AgentSpec { id: $id, name: $name, vendor: "", bins: &[$($bin),+], connection: None, install_url: Some($url),
            plan: Plan::Unsupported("연결 방식 미검증"), profile: Profile::Unsupported("독립 프로필 경로 미검증"), note: "" }
    };
}

/// CLI identities, aliases, launch recipes and model discovery.
pub const CATALOG: &[Entry] = &[
    Entry { id: "claude", name: "Claude Code", vendor: "Anthropic", bins: &["claude"], connection: Some(&CLAUDE), install_url: Some("https://code.claude.com/docs/en/setup"), plan: Plan::Planned(0), profile: Profile::Relocatable("CLAUDE_CONFIG_DIR"), note: "" },
    Entry { id: "codex", name: "Codex", vendor: "OpenAI", bins: &["codex"], connection: Some(&CODEX), install_url: Some("https://github.com/openai/codex"), plan: Plan::Planned(0), profile: Profile::Relocatable("CODEX_HOME"), note: "" },
    Entry { id: "agy", name: "Antigravity", vendor: "Google", bins: &["agy"], connection: Some(&AGY), install_url: Some("https://antigravity.google/docs/cli-overview"), plan: Plan::Planned(0), profile: Profile::Relocatable("USERPROFILE"), note: "" },
    extra!("claude-agent-teams", "Claude Agent Teams", ["orca", "orca-dev", "orca-ide"], "https://code.claude.com/docs/en/agent-teams"),
    extra!("openclaude", "OpenClaude", ["openclaude"], "https://openclaude.gitlawb.com"),
    extra!("opencode2", "OpenCode 2", ["opencode2"], "https://opencode.ai/v2/docs/"),
    extra!("mimo-code", "MiMo Code", ["mimo"], "https://mimo.xiaomi.com/coder"),
    extra!("ante", "Ante", ["ante"], "https://github.com/AntigmaLabs/ante-preview"),
    extra!("trae", "Trae", ["traecli"], "https://docs.trae.cn/cli_get-started-with-trae-cli"),
    extra!("muse", "Muse", ["muse"], "https://dev.meta.ai/docs/muse-code"),
    extra!("dsh", "DeepSeek Harness", ["dsh-tui", "dst"], "https://deepseek-harness.github.io/deepseek-harness"),
    extra!("qoder", "Qoder CLI", ["qodercli", "qoder"], "https://docs.qoder.com/cli/overview"),
    extra!("qoder-cn", "Qoder CLI China", ["qoderclicn", "qodercn"], "https://docs.qoder.cn/cli/overview"),
    extra!("zcode", "ZCode", ["zcode"], "https://zcode.z.ai/en/docs"),
    extra!("prime-agent", "Prime Agent", ["prime-agent"], "https://github.com/PrimeIntellect-ai/prime-agent"),
    extra!("autohand", "Autohand Code", ["autohand"], "https://github.com/autohandai/code-cli"),
    extra!("cline", "Cline", ["cline"], "https://docs.cline.bot/cline-cli/overview"),
    extra!("codebuff", "Codebuff", ["codebuff"], "https://codebuff.com/docs/help/quick-start"),
    extra!("freebuff", "Freebuff", ["freebuff"], "https://freebuff.com/cli"),
    extra!("rovo", "Rovo Dev", ["acli"], "https://support.atlassian.com/rovo/"),
    extra!("codebuddy", "CodeBuddy", ["codebuddy", "cbc"], "https://www.codebuddy.ai/cli"),
    extra!("jcode", "Jcode", ["jcode"], "https://github.com/1jehuang/jcode"),
    Entry {
        id: "omp",
        connection: None,
        name: "Oh My Pi",
        vendor: "Stencil Labs",
        bins: &["omp"],
        install_url: Some("https://github.com/can1357/oh-my-pi"),
        plan: Plan::Unsupported("연결 방식 미검증"),
        profile: Profile::Unsupported("독립 프로필 경로 미검증"),
        note: "설정 흔적과 실행 파일은 별개입니다.",
    },
    Entry {
        id: "commandcode",
        connection: None,
        name: "Command Code",
        vendor: "Command Code AI",
        bins: &["command-code", "commandcode"],
        install_url: Some("https://github.com/CommandCodeAI/command-code"),
        plan: Plan::Unsupported("연결 방식 미검증"),
        profile: Profile::Unsupported("독립 프로필 경로 미검증"),
        note: "설정 흔적과 실행 파일은 별개입니다.",
    },
    Entry {
        id: "opencode",
        connection: Some(&OPENCODE),
        name: "OpenCode",
        vendor: "Anomaly (구 SST), 오픈소스",
        bins: &["opencode"],
        install_url: Some("https://opencode.ai/v2/docs/"),
        plan: Plan::Planned(1),
        profile: Profile::Relocatable("USERPROFILE·HOME 이동 (설정·데이터·상태·캐시 폴더가 모두 따라감), XDG_CONFIG_HOME"),
        note: "OpenCode에 설정한 제공자(OpenCode Console/Go, OpenRouter, 본인 API 키)로만 씁니다. Claude 구독 로그인은 약관상 연결하면 안 됩니다. API 인증 환경변수는 전달됩니다. 사용량 조회가 없어 하루 예산이 적용되지 않습니다.",
    },
    Entry {
        id: "devin",
        connection: Some(&DEVIN),
        name: "Devin CLI",
        vendor: "Cognition",
        bins: &["devin"],
        install_url: Some("https://docs.devin.ai/cli"),
        plan: Plan::Planned(2),
        profile: Profile::Unsupported("설정 폴더(%APPDATA%\\devin)를 환경변수로 옮길 수 없습니다. USERPROFILE을 바꾸면 실행이 실패합니다"),
        note: "카드만 지원합니다. 도구 단계는 실행이 끝난 뒤 기록에서 읽으며 하루 예산은 적용되지 않습니다.",
    },
    Entry {
        id: "cursor",
        connection: None,
        name: "Cursor CLI",
        vendor: "Cursor",
        bins: &["cursor-agent"],
        install_url: Some("https://cursor.com/docs/cli/installation"),
        plan: Plan::Planned(3),
        profile: Profile::Relocatable("CURSOR_CONFIG_DIR"),
        note: "실행 파일 이름은 agent이고 cursor-agent는 호환 별칭입니다. 감지는 cursor-agent로만 합니다.",
    },
    Entry {
        id: "gemini",
        connection: None,
        name: "Gemini CLI",
        vendor: "Google",
        bins: &["gemini"],
        install_url: Some("https://geminicli.com/docs/get-started/installation/"),
        plan: Plan::Planned(3),
        profile: Profile::Relocatable("GEMINI_CLI_HOME"),
        note: "개인 Google 로그인은 2026-06-18에 Antigravity CLI로 옮겨져 유료 API 키 전용입니다. 서드파티 도구로 백엔드에 접근하면 계정 정지 사유입니다.",
    },
    Entry {
        id: "qwen",
        connection: None,
        name: "Qwen Code",
        vendor: "Alibaba (QwenLM)",
        bins: &["qwen"],
        install_url: Some("https://qwenlm.github.io/qwen-code-docs/en/users/quickstart/"),
        plan: Plan::Planned(3),
        profile: Profile::Relocatable("QWEN_HOME, QWEN_RUNTIME_DIR"),
        note: "Qwen OAuth 무료 티어는 2026-04-15에 끝났습니다.",
    },
    Entry {
        id: "droid",
        connection: None,
        name: "Factory Droid",
        vendor: "Factory",
        bins: &["droid"],
        install_url: Some("https://docs.factory.com/cli/getting-started/quickstart"),
        plan: Plan::Planned(3),
        profile: Profile::Relocatable("USERPROFILE 이동 (전용 변수는 미확인)"),
        note: "droid exec는 기본이 읽기 전용입니다. 종료 코드 0이 작업 성공을 뜻하지 않습니다.",
    },
    Entry {
        id: "copilot",
        connection: None,
        name: "GitHub Copilot CLI",
        vendor: "GitHub",
        bins: &["copilot"],
        install_url: Some("https://docs.github.com/en/copilot/how-tos/set-up/install-copilot-cli"),
        plan: Plan::Planned(3),
        profile: Profile::Relocatable("COPILOT_HOME"),
        note: "AI credits로 과금됩니다. GH_TOKEN을 카드 환경에서만 빼야 gh를 쓰는 O 기능이 그대로입니다.",
    },
    Entry {
        id: "pi",
        connection: None,
        name: "pi",
        vendor: "Earendil (badlogic)",
        bins: &["pi"],
        install_url: Some("https://github.com/badlogic/pi-mono"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("PI_CODING_AGENT_DIR"),
        note: "도구마다 승인을 묻지 않는 것이 기본입니다(권한 모드 없음). Claude 구독 로그인은 약관상 쓰면 안 됩니다.",
    },
    Entry {
        id: "hermes",
        connection: None,
        name: "Hermes Agent",
        vendor: "Nous Research",
        bins: &["hermes"],
        install_url: Some("https://hermes-agent.nousresearch.com/docs/getting-started/installation"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("HERMES_HOME"),
        note: "hermes usage --json이 세션 없이 한도 창을 보여 줍니다.",
    },
    Entry {
        id: "openclaw",
        connection: None,
        name: "OpenClaw",
        vendor: "오픈소스",
        bins: &["openclaw"],
        install_url: Some("https://docs.openclaw.ai/install"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("OPENCLAW_HOME, OPENCLAW_STATE_DIR"),
        note: "Antigravity 운영자와 Kiro 약관이 OpenClaw 같은 하네스를 문제 삼습니다. API 키로만 씁니다.",
    },
    Entry {
        id: "grok",
        connection: None,
        name: "Grok Build",
        vendor: "xAI",
        bins: &["grok"],
        install_url: Some("https://docs.x.ai/build/overview"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("GROK_HOME"),
        note: "커뮤니티 grok-cli도 실행 파일 이름이 grok입니다. 버전 줄로 구별하세요.",
    },
    Entry {
        id: "goose",
        connection: None,
        name: "goose",
        vendor: "AAIF (Block)",
        bins: &["goose"],
        install_url: Some("https://goose-docs.ai/docs/guides/goose-cli-commands"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("GOOSE_PATH_ROOT"),
        note: "기본 모드(GOOSE_MODE=auto)가 이미 완전 자율입니다. 연결하려면 승인 모드를 따로 정해야 합니다.",
    },
    Entry {
        id: "kilo",
        connection: None,
        name: "Kilo Code CLI",
        vendor: "Kilo-Org",
        bins: &["kilo", "kilocode"],
        install_url: Some("https://kilo.ai/docs/code-with-ai/platforms/cli-reference"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("KILO_CONFIG"),
        note: "OpenCode 계열 구조입니다.",
    },
    Entry {
        id: "vibe",
        connection: None,
        name: "Mistral Vibe",
        vendor: "Mistral AI",
        bins: &["vibe", "mistral-vibe"],
        install_url: Some("https://docs.mistral.ai/vibe/code/cli/work-with-cli"),
        plan: Plan::Planned(4),
        profile: Profile::Relocatable("VIBE_HOME"),
        note: "공식 지원은 UNIX이고 Windows는 동작한다고만 적혀 있습니다.",
    },
    Entry {
        id: "kimi",
        connection: None,
        name: "Kimi Code CLI",
        vendor: "Moonshot AI",
        bins: &["kimi", "kimi-code"],
        install_url: Some("https://moonshotai.github.io/kimi-code/en/guides/getting-started.html"),
        plan: Plan::Unsupported("약관: Kimi Code 구독은 대화형 사용만 허용합니다(비대화형 자동화 금지)"),
        profile: Profile::Relocatable("KIMI_CODE_HOME"),
        note: "구독 로그인으로 작업카드를 돌리면 안 됩니다.",
    },
    Entry {
        id: "amp",
        connection: None,
        name: "Amp",
        vendor: "Amp (구 Sourcegraph)",
        bins: &["amp"],
        install_url: Some("https://ampcode.com/docs/cli"),
        plan: Plan::Unsupported("기본이 권한 확인 없음이고 약관이 자동·대량 사용을 금지합니다. Windows는 WSL 기준으로만 문서화되어 있습니다"),
        profile: Profile::Relocatable("--settings-file (폴더 변수 미확인)"),
        note: "",
    },
    Entry {
        id: "aider",
        connection: None,
        name: "Aider",
        vendor: "Aider-AI, 오픈소스",
        bins: &["aider"],
        install_url: Some("https://aider.chat/docs/install.html"),
        plan: Plan::Unsupported("JSON 출력과 세션 id가 없어 작업카드 기록을 만들 수 없습니다"),
        profile: Profile::Relocatable("AIDER_* 환경변수, -c 설정 파일"),
        note: "자동 커밋이 기본으로 켜져 있어 카드의 커밋 흐름과 부딪힙니다.",
    },
    Entry {
        id: "crush",
        connection: None,
        name: "Crush",
        vendor: "Charm",
        bins: &["crush"],
        install_url: Some("https://github.com/charmbracelet/crush"),
        plan: Plan::Unsupported("헤드리스 출력(crush run)이 텍스트뿐입니다"),
        profile: Profile::Relocatable("CRUSH_GLOBAL_CONFIG, CRUSH_GLOBAL_DATA"),
        note: "",
    },
    Entry {
        id: "auggie",
        connection: None,
        name: "Auggie",
        vendor: "Augment Code",
        bins: &["auggie"],
        install_url: Some("https://docs.augmentcode.com/cli/reference.md"),
        plan: Plan::Unsupported("Windows는 WSL만 문서화되어 있습니다"),
        profile: Profile::Unsupported("설정 폴더를 옮기는 변수가 없습니다(--augment-cache-dir 플래그만)"),
        note: "",
    },
    Entry {
        id: "kiro",
        connection: None,
        name: "Kiro CLI",
        vendor: "AWS",
        bins: &["kiro-cli"],
        install_url: Some("https://kiro.dev/faq/"),
        plan: Plan::Unsupported("약관: 서드파티 하네스로 쓰는 것을 금지합니다"),
        profile: Profile::Relocatable("KIRO_HOME"),
        note: "",
    },
    Entry {
        id: "warp",
        connection: None,
        name: "Warp (oz)",
        vendor: "Warp",
        bins: &["warp", "oz"],
        install_url: Some("https://docs.warp.dev/agents/cli/oz-cli.md"),
        plan: Plan::Unsupported("헤드리스 출력 형식이 문서에 없고, oz는 Windows 단독 패키지가 없습니다"),
        profile: Profile::Unsupported("설정 폴더 변수가 문서에 없습니다"),
        note: "",
    },
    Entry {
        id: "continue",
        connection: None,
        name: "Continue CLI",
        vendor: "Continue",
        bins: &["cn"],
        install_url: Some("https://github.com/continuedev/continue"),
        plan: Plan::Unsupported("저장소가 읽기 전용이고 유지보수가 끝났습니다"),
        profile: Profile::Relocatable("CONTINUE_GLOBAL_DIR (소스에서만 확인)"),
        note: "",
    },
    Entry {
        id: "iflow",
        connection: None,
        name: "iFlow CLI",
        vendor: "iFlow",
        bins: &["iflow"],
        install_url: None,
        plan: Plan::Unsupported("서비스가 종료되었습니다"),
        profile: Profile::Unsupported("서비스 종료"),
        note: "",
    },
    Entry {
        id: "qodo",
        connection: None,
        name: "Qodo Command",
        vendor: "Qodo",
        bins: &["qodo"],
        install_url: None,
        plan: Plan::Unsupported("제품이 폐기되었습니다"),
        profile: Profile::Unsupported("제품 폐기"),
        note: "",
    },
];

/// The catalog entry of a provider, if it is one of the "other" CLIs.
pub fn entry(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Whether a provider may run with a clean profile: `Err` with the reason ("프로필 미지원")
/// when its config cannot be moved there. Claude, Codex and Antigravity always can.
pub fn profile_support(provider: ProviderId) -> Result<(), &'static str> {
    match entry(provider.as_str()).map(|e| e.profile) {
        Some(Profile::Unsupported(reason)) => Err(reason),
        _ => Ok(()),
    }
}

/// One entry as the app and `o agents list --all --json` show it (`OtherAgent` in app/src/platform/backend/types.ts).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub binary: &'static str,
    pub detected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// "connected", "planned" or "unsupported".
    pub status: &'static str,
    /// Why it is not supported; empty otherwise.
    pub reason: &'static str,
    /// Its place in the connection order, while planned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<&'static str>,
    pub profile_supported: bool,
    /// How the profile moves, or why it cannot.
    pub profile: &'static str,
    pub note: &'static str,
}

/// What a binary found on PATH said to `--version`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

/// An entry with what detection found (pure: no file or process).
pub fn listed(entry: &'static Entry, found: Found) -> Listed {
    let connected = ProviderId::parse(entry.id).is_some();
    let (status, reason, order) = match (connected, entry.plan) {
        (true, _) => ("connected", "", None),
        (false, Plan::Planned(order)) => ("planned", "", Some(order)),
        (false, Plan::Unsupported(reason)) => ("unsupported", reason, None),
    };
    let (profile_supported, profile) = match entry.profile {
        Profile::Relocatable(how) => (true, how),
        Profile::Unsupported(why) => (false, why),
    };
    Listed {
        id: entry.id,
        name: entry.name,
        vendor: entry.vendor,
        binary: entry.bins[0],
        detected: found.path.is_some(),
        path: found.path.map(|p| p.to_string_lossy().into_owned()),
        version: found.version,
        status,
        reason,
        order,
        install_url: entry.install_url,
        profile_supported,
        profile,
        note: entry.note,
    }
}

/// How long `--version` may take before the entry shows no version.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// Finds the entry's binary on PATH and asks it `--version` (nothing else). A shim O cannot
/// start without a shell still counts as detected, without a version.
pub fn detect(entry: &Entry) -> Found {
    if (entry.id == "claude-agent-teams" && (cfg!(windows) || which("claude").is_none())) || (entry.id == "dsh" && which("dsh").is_none()) {
        return Found::default();
    }
    let Some((bin, path)) = entry.bins.iter().find_map(|bin| which(bin).map(|p| (*bin, p))) else { return Found::default() };
    let version = resolve_cli(bin).and_then(|cli| {
        let mut command = Command::new(&cli.command);
        command.args(&cli.prefix).arg("--version");
        let (status, out, err) = proc::capture(&mut command, VERSION_TIMEOUT).ok()?;
        let text = if out.trim().is_empty() { err } else { out };
        let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
        status.success().then(|| line.chars().take(80).collect())
    });
    Found { path: Some(path), version }
}

/// The whole catalog with detection, every entry checked in parallel, in table order.
pub fn list() -> Vec<Listed> {
    refresh_paths();
    thread::scope(|s| {
        let checks: Vec<_> = CATALOG.iter().filter(|e| !matches!(e.id, "claude" | "codex" | "agy")).map(|e| (e, s.spawn(move || detect(e)))).collect();
        checks.into_iter().map(|(e, check)| listed(e, check.join().unwrap_or_default())).collect()
    })
}

static CACHE: LazyLock<Mutex<Option<Vec<Listed>>>> = LazyLock::new(|| Mutex::new(None));

/// A forced refresh replaces the old snapshot only after detection completes.
pub fn cached_list(refresh: bool) -> Vec<Listed> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if refresh || cache.is_none() { *cache = Some(list()); }
    cache.clone().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn identities_are_unique_and_connected_recipes_keep_approvals() {
        let ids: HashSet<_> = CATALOG.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), CATALOG.len());
        assert!(ids.len() >= 45);
        for provider in ProviderId::ALL {
            let agent = entry(provider.as_str()).unwrap();
            assert_eq!(agent.bins[0], provider.as_str());
            let recipe = connection(provider.as_str());
            assert!(o_guard::rules::find_hits(&recipe.headless.fixed.join(" "), o_guard::rules::code_rules().filter(|r| r.id == "permission-bypass")).is_empty());
        }
        assert!(entry("qoder").unwrap().bins.contains(&"qoder"));
        assert!(entry("commandcode").unwrap().bins.contains(&"command-code"));
    }
}
