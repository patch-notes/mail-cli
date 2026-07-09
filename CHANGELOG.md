# Changelog

All notable changes to `mail-cli` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-07-09

### Added

- **Multiple accounts via suffixed env vars.** Configure any number of
  accounts at once by giving each its own `MAILCLI_*_<TOKEN>` set
  (numeric by convention: `_1`, `_2`, …; any non-empty token works), then
  pick one per invocation with the global `--account <TOKEN>` flag or the
  `MAILCLI_ACCOUNT` env var. Omitting both selects the **default account**
  = the bare, un-suffixed vars, so every existing setup keeps working
  unchanged. Resolution order: `--account` flag → `MAILCLI_ACCOUNT` env →
  default. Strict no-fallback: account `N` reads **only** `*_N`, and errors
  name the actual missing variable (e.g. `MAILCLI_PASSWORD_2`). Optional
  vars keep their built-in defaults (port 993/465, TLS implicit, auth
  login, `USER = EMAIL`), so a fully-specified account only requires
  `MAILCLI_IMAP_HOST_N`, `MAILCLI_EMAIL_N`, `MAILCLI_PASSWORD_N` (plus
  `MAILCLI_SMTP_HOST_N` for `send`). See [`README.md`](README.md) →
  *Multiple accounts* for the full example.
- **`accounts` subcommand** for discovery. Lists every account detected in
  the current environment with its status (`configured` / `incomplete`)
  and connection targets (host + email). **Never prints the password
  value.** Supports `--json` for scripted use.
- **`accounts` JSON schema** (`AccountInfo`): `name`, `email`, `imap_host`,
  `smtp_host`, `has_password` (bool). Documented in
  `skill/references/reference.md` and shipped via `install-skill`.

### Changed

- **`install.sh`** at the repo root: a POSIX-`sh` one-line installer that
  resolves the latest GitHub release, downloads the matching prebuilt
  binary, installs to `BINDIR` (default `/usr/local/bin`, falling back to
  `~/.local/bin`), and runs `mail-cli install-skill`. Detects OS/arch and
  refuses combinations that have no published asset (e.g. linux-arm64).
- **README install flow** replaced the multi-line `curl | grep | mv`
  snippet with a single `curl -fsSL .../master/install.sh | sh` and adds
  a copy-paste one-liner for agents. Dropped the dead `PLAN.md` link
  (pointed at `AGENTS.md` instead); fixed the License section to MIT.
- **`config.rs` refactor:** the free `opt / required / port / tls_mode /
  auth_method / login_user` helpers are now methods on a small `Env`
  resolver that carries the active account suffix — same call sites,
  suffix-aware. All env reading still happens in `config.rs` (the only
  boundary).

### Docs & skill

- `README.md`: config table gains `MAILCLI_ACCOUNT`, new *Multiple
  accounts* section, new `accounts` subcommand entry, and `[--account N]`
  in the usage synopsis.
- `AGENTS.md`: `config.rs` description notes the `Env` suffix resolver;
  Security rule adds *"accounts env-suffixed; selection is a CLI flag"*.
- Bundled skill (`skill/SKILL.md` + `references/reference.md`): teaches
  `--account` / `MAILCLI_ACCOUNT`, the strict-no-fallback rule, and the
  `accounts` discovery command. Two new evals cover account selection
  and the no-password-leak invariant.
- `plans/ACCOUNTS-PLAN.md` is now a tracked design doc (was in `.gitignore`
  during development).

## [0.1.0] - 2026-07-08

### Added

- First public release. Stateless IMAP/SMTP email client: connect, list,
  search, read, attachments, download, send — all driven by `MAILCLI_*`
  env vars and CLI flags. No config file, no cache, no stored state.
- **`search` subcommand** (server-side IMAP SEARCH) with structured flags
  (`--from`, `--subject`, `--since`, `--before`, `--unseen`, `--flagged`,
  `--larger`, `--smaller`) and a `--raw` escape hatch for arbitrary
  queries. AND-only v1; conflicts with the structured flag set at the
  clap level.
- **`install-skill` subcommand** that materializes the bundled agent
  skill (`SKILL.md` + `references/` + `evals/`) into
  `~/.agents/skills/mail-cli/` so the binary teaches agents how to use
  itself. Refuses to clobber without `--force`.
- **`--json` global flag** for scriptable output on every list-shaped
  command (`list`, `accounts`, …).
- **GitHub Actions CI + release pipeline.** `ci.yml` runs
  `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` on
  every PR to `master`. `release.yml` builds 4 target binaries
  (linux-amd64, macos-amd64, macos-arm64, windows-amd64) on every
  `v*` tag, uploads them as a GitHub release with auto-generated notes,
  and verifies the tag matches `Cargo.toml`.
- GreenMail integration test suite (gated on `MAILCLI_IT=1`) covering
  from/subject + flag AND-combine, SINCE/BEFORE date bounds, and LARGER
  size bounds.
- 16 lib + 26 bin unit + 3 integration tests; `cargo clippy --all-targets
  -D warnings` and `cargo fmt` clean.

[0.2.0]: https://github.com/patch-notes/mail-cli/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/patch-notes/mail-cli/releases/tag/v0.1.0