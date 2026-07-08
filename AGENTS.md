# Repository Guidelines

`mail-cli` is a single-binary, **stateless** IMAP/SMTP email client in Rust.
Config comes **only** from `MAILCLI_*` environment variables; everything else is
a CLI flag. No config file, no cache, no stored state.

This file and this repo are designed for **both humans and AI coding agents**.
Agents are first-class: the `skill/` directory ships an agent-facing usage
skill (`SKILL.md` + references + evals) that is **embedded into the binary**
and materialized by `mail-cli install-skill` — see
[Install for agents](#install-for-agents) below.

## Project Structure & Module Organization

```
src/
  main.rs    // clap surface, Selector enum, live selector resolution, dispatch
  lib.rs     // library facade; re-exports only `search` (pure) for integration tests
  config.rs  // the ONLY boundary that reads env; per-command validation
  imap.rs    // the ONLY place that talks IMAP (connect/auth/list/fetch)
  parse.rs   // mail-parser wrappers; Summary/Rendered/Attachment structs
  search.rs  // Criteria -> server-side IMAP SEARCH query builder (pure, offline-testable)
  smtp.rs    // lettre message build (+attachments) and send
  skill.rs   // embeds skill/ assets; install-skill writes them to disk
  output.rs  // human tables + --json serialization

skill/       // the bundled agent skill (SKILL.md + references/ + evals/),
             // embedded into the binary via include_str! and materialized by
             // `mail-cli install-skill`. Keep in sync with the shipped skill.

tests/       // integration tests, gated on MAILCLI_IT=1 (GreenMail)
```

Structs live with the module that owns them (no `model.rs`). Keep IMAP calls
inside `imap.rs` so a future async swap stays contained. `search.rs` is pure
on purpose so the query builder stays unit-testable without a server.

## Install for agents

The `install-skill` subcommand reads **no** `MAILCLI_*` env vars — it is purely
local — so it's safe to run before any account config exists.

Prebuilt release assets live at
[`github.com/patch-notes/mail-cli/releases/latest`](https://github.com/patch-notes/mail-cli/releases/latest).
Naming: `mail-cli-<version>-<platform>[-amd64|-arm64][.exe]`
(e.g. `mail-cli-0.1.0-linux-amd64`, `mail-cli-0.1.0-macos-arm64.exe` — note
`.exe` only on Windows).

Linux:

```sh
# 1. Download the latest prebuilt binary for Linux amd64
curl -L -o mail-cli \
  https://github.com/patch-notes/mail-cli/releases/latest/download/mail-cli-linux-amd64
chmod +x mail-cli
```

macOS (Apple Silicon shown; swap `arm64` -> `amd64` for Intel):

```sh
curl -L -o mail-cli \
  https://github.com/patch-notes/mail-cli/releases/latest/download/mail-cli-macos-arm64
chmod +x mail-cli
xattr -d com.apple.quarantine mail-cli   # gatekeeper, first run only
```

Windows (PowerShell):

```powershell
Invoke-WebRequest -OutFile mail-cli.exe `
  https://github.com/patch-notes/mail-cli/releases/latest/download/mail-cli-windows-amd64.exe
```

Available asset names: `mail-cli-linux-amd64`, `mail-cli-macos-amd64`,
`mail-cli-macos-arm64`, `mail-cli-windows-amd64.exe`. Pin a version by replacing
`latest` with `v0.1.0` etc.

Then, regardless of platform, materialize the skill:

```sh
./mail-cli install-skill   # or .\mail-cli.exe install-skill on Windows
# -> ~/.agents/skills/mail-cli/   (or %USERPROFILE%\.agents\skills\mail-cli)
```

`gh` users:
`gh release download -R patch-notes/mail-cli -p 'mail-cli-*linux-amd64*'`.
Source builders: `cargo build --release`, then
`./target/release/mail-cli install-skill`.

Finally, optionally move the binary somewhere on `PATH`:

```sh
sudo mv mail-cli /usr/local/bin/   # Linux/macOS — or anywhere on $PATH
```

After this, the `mail-cli` skill is on disk and the agent can run any
`mail-cli` command — but real mail access still requires the
`MAILCLI_*` env vars from [Security & Configuration](#security--configuration).

## Build, Test, and Development Commands

- `cargo build` — debug build.
- `cargo build --release` — optimized binary at `target/release/mail-cli`.
- `cargo test` — run unit tests (in-module `#[cfg(test)]`).
- `cargo clippy --all-targets` — lint; keep it **warning-free**.
- `cargo run -- <subcommand>` — e.g. `cargo run -- folders`.
- `MAILCLI_IT=1 cargo test --test search_integration` — GreenMail integration
  (requires a running GreenMail server; see README).

## Coding Style & Naming Conventions

- Rust 2024 edition; format with `cargo fmt` (4-space indent).
- Errors: `anyhow::Result` with `.context(...)`; no custom error enums.
- Parse/validate at the boundary (env in `config.rs`, selectors in `main.rs`)
  so illegal states are unrepresentable downstream.
- Fail loud, naming the offending variable/flag.

## Testing Guidelines

- Unit tests sit beside code in `mod tests`. Cover parsing (inline `.eml`
  fixtures), `Selector` parsing, and `Config` env cases.
- **Every bug fix starts with a failing test** that reproduces it, then the fix.
- Integration: `tests/search_integration.rs` (GreenMail) — gated on
  `MAILCLI_IT=1` so plain `cargo test` stays offline.

## Commit & Pull Request Guidelines

- Use concise, imperative commits (e.g. `add search command`).
- One logical change per commit; keep diffs small and focused.
- PRs: describe intent, note test coverage; ensure
  `cargo test` + `cargo clippy` are clean.

## Security & Configuration

- Secrets come only from `MAILCLI_PASSWORD` (env). Never persist, prompt, or log
  credentials. Guard the stateless line — persistence must be explicit and
  opt-in, never an implementation detail.
