한국어 문서: [CONTRIBUTING.ko.md](CONTRIBUTING.ko.md)

# Contributing

project_O is a personal project a small team works on. Bug reports and pull requests are welcome, but merges follow the team's branch flow ([AGENTS.md](AGENTS.md)). This guide covers what you need to build it, run the checks and send a change. The Korean version is [CONTRIBUTING.ko.md](CONTRIBUTING.ko.md).

## Pull requests from outside (forks)

- A PR opened from a fork is not merged as it is. `.github/workflows/branch-flow.yml` fails any PR whose head branch lives in a fork ("포크 저장소의 브랜치는 허용하지 않습니다").
- When a change is accepted, a team member brings it into their own work branch (`<personal>-<topic>`) and opens a fresh PR to `test_2`. Taking the commits over as they are keeps the original author's information. The original PR gets a link to the new one and is closed.
- For a large change, an issue first is appreciated before the PR.
- Anything you send is taken under the project licence ([Apache-2.0](LICENSE)).

## Development environment

Windows, Rust (stable), Node.js 22.18 or later. macOS (Apple Silicon) is experimental and built from source; scheduled runs and the installer are Windows-only ([README](README.md#supported-operating-systems)).

### Install `o` first

The git hooks and the agent hooks call the `o` on your PATH, so install it before anything else in a fresh clone.

```powershell
cargo install --path crates/o-cli   # %USERPROFILE%\.cargo\bin\o — `o` from anywhere
o --help
```

Alternatively, add the installed desktop app's folder (`%LOCALAPPDATA%\Programs\project_O`) to your user PATH and use the `o.exe` that ships with the app ([docs/INSTALL.md](docs/INSTALL.md)). After changing `o`, reinstall it with `cargo install --path crates/o-cli`.

### Agent hooks

Using Claude Code or Codex inside this repository runs a project hook that calls `o hook` on every tool call.

| File | Command | If `o` is missing or the hook ends in an error |
|---|---|---|
| `.claude/settings.json` | `o hook`, `"onFailure": "block"` | Every tool call is blocked — Bash, PowerShell, Write, Edit, Read, Grep, Glob and the rest (Claude Code 2.1.295 or later) |
| `.codex/hooks.json` | `o hook \|\| exit 2` | Bash, apply_patch, Write and Edit calls are blocked |

If a session is blocked from the start, check that `o` is on your PATH (`o --help`). Codex may ask for trust approval the first time it uses the project hook and whenever the hook command changes.

What the hooks block — credential file and token access, direct calls to provider servers, options that disable a CLI's permission checks, commits and pushes on protected branches, and more — is listed in [AGENTS.md](AGENTS.md) under "프로바이더 정책 자동 차단". Do not work around a block. If you think a rule is wrong, say so in an issue or a PR.

### Git hooks

```sh
npm install   # repository root; sets git config core.hooksPath .githooks
```

`pre-commit` runs `o scan --staged` and `pre-push` runs `o scan --pre-push`. They apply the same policy scan and block commits and pushes on the protected branches (`test_1`, `test_1_hotfix`, `test_2`, `main_hotfix`, `main`).

## Checks

Run these locally before a PR. Use the development profile — never `--release`, which makes every rebuild far slower.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p o-cli -- scan --all   # policy scan over every file
npm test                           # screen logic tests (root)
cd app && npm install && npm run check   # screens: TypeScript
```

If you changed the screens, look at them with `cd app && npm run dev` (a browser; `?source=demo` replays recorded runs) or `npm run preview:app` (a desktop preview window).

CI (`.github/workflows/guard.yml`) runs on pull requests only — a branch pushed without a PR is not checked by CI, so run the checks yourself.

## Commits and PRs

- Commit messages: `<type>: <summary>` (`feat`, `fix`, `docs`, `refactor`, `test`, `chore`).
- Fill in `.github/pull_request_template.md`: what you changed and how you verified it.
- Never commit subscription account credentials, tokens or `.env` files, and keep real account names and email addresses out of test and demo data.
- A change to a shared boundary (tech stack, shared data model, how a provider is connected, the routing unit, the top level of the screens) needs an ADR in `docs/decisions/`. Module map and the checklist for adding a provider, screen or command: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- A change that loosens a policy rule (`crates/o-guard/src/rules.rs`) or a limit (`LIMITS` in `crates/o-core/src/policy.rs`) is sent together with an ADR.
