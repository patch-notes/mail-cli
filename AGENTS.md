# Repository Guidelines

`mail-cli` is a single-binary, **stateless** IMAP/SMTP email client in Rust.
Config comes **only** from `MAILCLI_*` environment variables; everything else is
a CLI flag. No config file, no cache, no stored state. Read [`PLAN.md`](PLAN.md)
before non-trivial work — it is the locked source of truth.

## Project Structure & Module Organization

```
src/
  main.rs    // clap surface, Selector enum, live selector resolution, dispatch
  config.rs  // the ONLY boundary that reads env; per-command validation;
             //   Env account-suffix resolver + accounts scan
  imap.rs    // the ONLY place that talks IMAP (connect/auth/list/fetch;
             //   append-to-Sent + \Sent special-use discovery)
  parse.rs   // mail-parser wrappers; Summary/Rendered/Attachment structs
  smtp.rs    // lettre message build (+attachments) and send
  skill.rs   // embeds skill/ assets; install-skill writes them to disk
  output.rs  // human tables + --json serialization

skill/       // the bundled agent skill (SKILL.md + references/ + evals/),
             // embedded into the binary via include_str! and materialized by
             // `mail-cli install-skill`. Keep in sync with the shipped skill.

plans/       // local, git-ignored working design docs (e.g. ACCOUNTS-PLAN.md).
             // The directory stays in the repo via its tracked .gitignore; its
             // contents are NOT committed. Write plans here, not at root.
```

Structs live with the module that owns them (no `model.rs`). Keep IMAP calls
inside `imap.rs` so a future async swap stays contained.

## Build, Test, and Development Commands

- `cargo build` — debug build.
- `cargo build --release` — optimized binary at `target/release/mail-cli`.
- `cargo test` — run unit tests (in-module `#[cfg(test)]`).
- `cargo clippy --all-targets` — lint; keep it **warning-free**.
- `cargo run -- <subcommand>` — e.g. `cargo run -- folders`.

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
- Integration: GreenMail in Docker/podman, gated on `MAILCLI_IT=1`, and run on
  every PR by the `integration (GreenMail)` CI job. Covers the `search` query
  builder (`tests/search_integration.rs`), the save-to-Sent `APPEND`/`\Sent`
  discovery paths (`tests/sent_integration.rs`), and a full send→INBOX+Sent e2e
  loop (`tests/send_e2e.rs`) that drives mail-cli's real build/append code — only
  the encrypted transport hop is substituted (SMTP TLS trusts bundled webpki
  roots, so it can't validate GreenMail's self-signed cert; IMAP TLS uses OS
  roots). GreenMail must bind `0.0.0.0` (`-Dgreenmail.hostname=0.0.0.0`) or
  podman/CI port-forwarding drops the connection. Run serialized
  (`--test-threads=1`). The lib crate exposes `imap`/`smtp`/`config`/`parse` for
  these tests.

## Commit & Pull Request Guidelines

- No history yet; use concise, imperative commits (e.g. `add search command`).
- One logical change per commit; keep diffs small and focused.
- PRs: describe intent, link plan section, note test coverage; ensure
  `cargo test` + `cargo clippy` are clean.

## Security & Configuration

- Secrets come only from `MAILCLI_PASSWORD` (env). Never persist, prompt, or log
  credentials. Guard the stateless line — persistence must be explicit and
  opt-in, never an implementation detail.
- **Accounts are env-suffixed, selection is a CLI flag.** Config *values* stay
  env-only; the runtime *choice* of which account to use is `--account <TOKEN>`
  (global) with `MAILCLI_ACCOUNT` as the default. No `--account` → the bare
  `MAILCLI_*` vars (the `default` account). An account is read strictly from its
  own `*_<TOKEN>` names — no fallback to the bare vars. All suffix resolution
  lives in `config.rs::Env`; `main.rs` only parses the flag and passes `&Env`
  in. `mail-cli accounts` scans the env and must never print password values.
