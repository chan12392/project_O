/**
 * Provider-policy patterns shared by the static scanner, the agent hooks and
 * the dispatch policy. This file is the only place allowed to spell them out.
 *
 * The line we hold (see docs/DESIGN.md §8): every model call goes through an
 * unmodified official CLI that signs itself in. Our code never touches a
 * provider's credentials, never calls a provider backend directly and never
 * turns off a CLI's own permission checks.
 */
export interface Rule {
  id: string;
  pattern: RegExp;
  reason: string;
}

/** Credential stores and tokens that belong to the official CLIs. */
export const CREDENTIAL_RULES: Rule[] = [
  { id: "claude-credentials", pattern: /\.credentials\.json/i, reason: "Claude Code 로그인 정보 파일에 접근할 수 없습니다" },
  { id: "claude-oauth-token", pattern: /CLAUDE_CODE_OAUTH_TOKEN|sk-ant-oat\d*-|\bsetup-token\b/, reason: "Claude 구독 OAuth 토큰을 다룰 수 없습니다" },
  { id: "codex-auth", pattern: /\bauth\.json\b/i, reason: "Codex 로그인 정보 파일(auth.json)에 접근할 수 없습니다" },
  { id: "gemini-oauth", pattern: /oauth_creds\.json|google_accounts\.json/i, reason: "Gemini/Antigravity 로그인 정보 파일에 접근할 수 없습니다" },
];

/** Provider backends. Talking to them ourselves is what the terms forbid. */
export const BACKEND_RULES: Rule[] = [
  { id: "anthropic-api", pattern: /api\.anthropic\.com|anthropic-beta[^\n]{0,40}oauth/i, reason: "Anthropic 서버를 직접 호출할 수 없습니다 — claude CLI를 거치세요" },
  { id: "openai-backend", pattern: /chatgpt\.com\/backend-api|api\.openai\.com/i, reason: "OpenAI 서버를 직접 호출할 수 없습니다 — codex CLI를 거치세요" },
  { id: "google-backend", pattern: /cloudcode-pa\.googleapis\.com|generativelanguage\.googleapis\.com|daily-cloudcode-pa/i, reason: "Google 모델 서버를 직접 호출할 수 없습니다 — agy CLI를 거치세요" },
];

/** Switches that disable a CLI's own permission checks. */
export const BYPASS_RULES: Rule[] = [
  {
    id: "permission-bypass",
    pattern: /dangerously-skip-permissions|dangerously-bypass-approvals-and-sandbox|bypassPermissions|--yolo\b/,
    reason: "CLI의 권한 확인을 끄는 옵션은 쓸 수 없습니다",
  },
];

export const CODE_RULES: Rule[] = [...CREDENTIAL_RULES, ...BACKEND_RULES, ...BYPASS_RULES];

/**
 * Paths that must mention the patterns to do their job. Changing this list is
 * a shared-boundary change (ADR + both approvals).
 */
export const ALLOWED_PATHS: RegExp[] = [
  /^src\/guard\/rules\.ts$/,
  /^tests\/guard\.test\.ts$/,
  /^tests\/policy\.test\.ts$/,
  /^docs\//,
];

export interface Hit {
  rule: string;
  reason: string;
  line?: number;
}

export function findHits(text: string, rules: Rule[]): Hit[] {
  const hits: Hit[] = [];
  for (const rule of rules) {
    const match = rule.pattern.exec(text);
    if (match) {
      const line = text.slice(0, match.index).split("\n").length;
      hits.push({ rule: rule.id, reason: rule.reason, line });
    }
  }
  return hits;
}

export function isAllowedPath(repoRelative: string): boolean {
  const normalized = repoRelative.replace(/\\/g, "/").replace(/^\.\//, "");
  return ALLOWED_PATHS.some((p) => p.test(normalized));
}
