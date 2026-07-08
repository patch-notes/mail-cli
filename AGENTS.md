# Repository Guidelines

`mail-cli` is a single-binary, **stateless** IMAP/SMTP email client in Rust.
Config comes **only** from `MAILCLI_*` environment variables; everything else is
a CLI flag. No config file, no cache, no stored state. Read [`PLAN.md`](PLAN.md)
before non-trivial work — it is the locked source of truth.

## Project Structure & Module Organization

```
src/
  main.rs    // clap surface, Selector enum, live selector resolution, dispatch
  config.rs  // the ONLY boundary that reads env; per-command validation
  imap.rs    // the ONLY place that talks IMAP (connect/auth/list/fetch)
  parse.rs   // mail-parser wrappers; Summary/Rendered/Attachment structs
  smtp.rs    // lettre message build (+attachments) and send
  output.rs  // human tables + --json serialization
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
- Integration (planned): GreenMail in Docker for connect→list→fetch and send.

## Commit & Pull Request Guidelines

- No history yet; use concise, imperative commits (e.g. `add search command`).
- One logical change per commit; keep diffs small and focused.
- PRs: describe intent, link plan section, note test coverage; ensure
  `cargo test` + `cargo clippy` are clean.

## Security & Configuration

- Secrets come only from `MAILCLI_PASSWORD` (env). Never persist, prompt, or log
  credentials. Guard the stateless line — persistence must be explicit and
  opt-in, never an implementation detail.
