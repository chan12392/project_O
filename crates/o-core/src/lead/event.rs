//! Events persisted in lead conversation histories.

use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all_fields = "camelCase")]
pub enum LeadEvent {
    /// A new thread for a project ("새 대화" starts another).
    #[serde(rename = "lead.start")]
    Start { at: String, thread_id: String, project: String },
    #[serde(rename = "lead.user")]
    User {
        at: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<Attachment>,
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        /// The profile this message ran on (project_O 프로필 사용, ADR-0013); absent in threads from before the switch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The turn ran in YOLO 모드 (ADR-0014): the lead could edit files and run commands.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        yolo: bool,
        /// Who wrote it when not the person: "overall" for a request the overall lead passed on
        /// (ADR-0032). Absent: the person.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
    /// The overall lead passed a request on to a project's lead (ADR-0032), which
    /// `o _lead-forward` then sends with the same agent, model and profile.
    #[serde(rename = "lead.forward")]
    Forward {
        at: String,
        project: String,
        text: String,
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        profile: Profile,
        /// The `lead.forward-plan` the person sent on (ADR-0036); absent in threads from before,
        /// whose requests went on by themselves and whose plans are not reviewed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proposal: Option<String>,
        /// The plan this request asks the project lead to revise (an overall review's `revise`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
    },
    /// Requests the overall lead proposes to pass on (ADR-0036); they go on when the person
    /// presses 요청 전달 (`overall::send_on`), as `lead.forward`s.
    #[serde(rename = "lead.forward-plan")]
    ForwardPlan {
        at: String,
        proposal_id: String,
        items: Vec<prompt::Forward>,
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        profile: Profile,
    },
    /// The overall lead's review of a project lead's plan (ADR-0036), in both threads.
    #[serde(rename = "lead.review")]
    Review { at: String, plan_id: String, project: String, verdict: prompt::Verdict, #[serde(default, skip_serializing_if = "String::is_empty")] notes: String },
    /// Untrusted data sent by a card, never a user instruction or approval.
    #[serde(rename = "lead.worker")]
    Worker { at: String, run_id: String, card_id: String, message_id: String, text: String },
    /// The CLI's own session id, resumed on the thread's next turn with that CLI. `clean` is
    /// the profile it lives in: a turn on the other profile cannot resume it and starts a new one.
    /// Absent in threads from before the switch, where any turn resumes it.
    #[serde(rename = "lead.session")]
    Session {
        at: String,
        provider: ProviderId,
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clean: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_tools: Option<bool>,
    },
    /// A completed card-question session, isolated from ordinary and YOLO sessions.
    #[serde(rename = "lead.worker-session")]
    WorkerSession {
        at: String, provider: ProviderId, session_id: String, model: Option<String>,
        effort: Option<String>, profile: Profile, result_tools: bool, through: usize, history_hash: String,
    },
    /// The lead's answer as Markdown, without its plan block.
    #[serde(rename = "lead.reply")]
    Reply { at: String, provider: ProviderId, text: String, #[serde(default, skip_serializing_if = "Vec::is_empty")] reported_files: Vec<String> },
    #[serde(rename = "lead.design")]
    Design { at: String, design: Value },
    /// Questions for the person (an `o-ask` block) in place of an answer: the app asks them
    /// as choices, and the next `lead.user` carries the picks. `intro` is the answer's prose.
    #[serde(rename = "lead.ask")]
    Ask { at: String, ask_id: String, provider: ProviderId, #[serde(default, skip_serializing_if = "String::is_empty")] intro: String, questions: Vec<prompt::AskQuestion> },
    /// A proposed plan with the project forced to the thread's and the policy check's result.
    #[serde(rename = "lead.plan")]
    Plan { at: String, plan_id: String, plan: Value, ok: bool, violations: Vec<Violation> },
    #[serde(rename = "lead.dispatch")]
    Dispatch {
        at: String,
        plan_id: String,
        run_id: String,
        clean: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The run's cards launch in YOLO 모드 (ADR-0014).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        yolo: bool,
    },
    #[serde(rename = "lead.followup")]
    Followup { at: String, source_run_id: String, card_id: String, run_id: String },
    #[serde(rename = "lead.autorun")]
    AutoRun { at: String, plan_id: String, hash: Option<String>, reason: Option<String> },
    /// Tokens one lead turn spent (`o tokens` counts them as planning overhead).
    #[serde(rename = "lead.usage")]
    Usage { at: String, provider: ProviderId, input: f64, cached: f64, output: f64, #[serde(default, skip_serializing_if = "Option::is_none")] complete: Option<bool> },
    #[serde(rename = "lead.warning")]
    Warning { at: String, text: String },
    #[serde(rename = "lead.error")]
    Error { at: String, text: String },
    /// A CLI session got the turn whose `lead.user` is event `through` of the thread (as
    /// [`read_thread`] counts): it has seen the thread up to that turn, its own answer included.
    #[serde(rename = "lead.seen")]
    Seen { at: String, provider: ProviderId, session_id: String, through: usize },
    /// A normal-mode lead asks to run one action (ADR-0008 개정 3); `lead.decision` with the
    /// same id says what became of it. `detail` is the request as the CLI sent it (data).
    #[serde(rename = "lead.approval")]
    Approval {
        at: String,
        id: String,
        provider: ProviderId,
        kind: approvals::Kind,
        tool: String,
        summary: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        detail: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<approvals::FileDelta>,
        expires: String,
    },
    /// `decision`: allow · deny · timeout · stopped; `by`: user · guard (O's rules) · timeout · stop · o.
    #[serde(rename = "lead.decision")]
    Decision {
        at: String,
        id: String,
        decision: String,
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// One finished step of a turn (`steps::Step`): what the lead read, ran, searched or changed.
    #[serde(rename = "lead.step")]
    Step {
        at: String,
        id: String,
        kind: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        added: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        removed: Option<u64>,
        status: String,
    },
}

impl LeadEvent {
    pub(super) fn step(step: steps::Step) -> Self {
        LeadEvent::Step { at: now_iso(), id: step.id, kind: step.kind, label: step.label, file: step.file, added: step.added, removed: step.removed, status: step.status }
    }
}
