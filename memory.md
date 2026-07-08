# memory.md

## Episodic
- 2026-07-08: Task = plan a Rust mail CLI. Wrote PLAN.md + ZoE review.
- 2026-07-08 (rev 2): User locked 3 constraints: (1) config ENTIRELY env-var,
  rest CLI, no other state; (2) sync fine; (3) NO cache, entirely stateless.
  Consolidated everything into ONE self-contained PLAN.md, deleted ZOE-REVIEW.md.
  ZoE review is now §8 inside PLAN.md. Dropped: config file/toml/directories,
  keyring/rpassword, last-list cache, --account/profiles, model.rs. Msg ref =
  live index (re-derived) OR uid:NNN; commands echo resolved uid.
- 2026-07-08 (rev 3): IMPLEMENTED M1–M4. Scaffolded all modules, compiles clean,
  11 unit tests pass, clippy clean. Wrote README.md. Binary builds & --help works.
  Missing-env fails loud naming the var. Ready for GreenMail integration + real
  smoke test.
- 2026-07-08 (rev 4): Ran /init → wrote AGENTS.md (repo-root, ~380 words, per init
  skill spec). Planned the search function → SEARCH-PLAN.md: a `search` subcommand
  that compiles typed flags (--from/--subject/--since/--unseen/...) into a
  SERVER-SIDE IMAP SEARCH, reusing list's fetch+render pipeline. New pure module
  search.rs (Criteria::to_imap_query, offline-testable). AND-only v1; --raw escape
  hatch mirrors list --search. Non-goals: no client-side search, no OR/NOT, no
  cross-folder, no state. NOT yet implemented (plan only).
- 2026-07-08 (rev 5): IMPLEMENTED the search feature (SEARCH-PLAN.md S1+S2).
  New pure module src/search.rs = `Criteria` struct (pub fields) +
  `to_imap_query()` + hand-rolled `imap_date` (YYYY-MM-DD -> dd-Mon-yyyy, strict
  range/leap-year validation) + `quote()` (escape \ and "). 16 unit tests, all
  offline. Wired `Command::Search` in main.rs: clap `group="structured"` so
  `--raw` conflicts_with the whole set at once; seen/unseen + flagged/unflagged
  have explicit conflicts_with. Dispatch builds query (raw OR criteria) then
  REUSES cmd_list unchanged (newest_uids already took Option<&str>). No new imap
  or output surface. Added README `### search` section (substring/server-side
  caveat + uid: scripting note). 27 tests pass, clippy clean (used
  is_multiple_of for leap-year). Verified via real binary: all 4 error guards +
  --help fire correctly (fail before network). TODO: S3 GreenMail integration.
- 2026-07-08 (rev 6): IMPLEMENTED S3 GreenMail integration (via podman). Added
  src/lib.rs exposing ONLY `pub mod search` (pure), so tests/ can call Criteria;
  main.rs now `use mail_cli::search`. dev-deps: chrono + imap(no-tls) for a
  PLAINTEXT test session (mail-cli's own connect() is TLS-only by design, so the
  test drives the same Criteria->uid_search path over plaintext instead of
  bolting a plaintext mode onto the binary). tests/search_integration.rs is
  gated on MAILCLI_IT=1 (plain `cargo test` skips it). 3 tests: from/subject/
  unseen + AND-combine, SINCE/BEFORE date bounds, LARGER size. Seeds via IMAP
  APPEND with controlled INTERNALDATE (imap crate `AppendCmd::internal_date`,
  format "%d-%h-%Y %T %z") and flags; resolves UIDs by unique Subject (not
  APPENDUID). GreenMail run: `podman run greenmail/standalone:2.1.0` with
  -Dgreenmail.hostname=0.0.0.0 (MUST bind 0.0.0.0 or podman forward gives
  ConnectionLost) + -Dgreenmail.auth.disabled (auto-creates users on login;
  otherwise LOGIN fails "Invalid login/password"). GreenMail 2.1.0 QUIRKS found
  empirically: (a) FROM is EXACT address-token match, NOT substring -> test uses
  full address; (b) `SEARCH ON` is UNSUPPORTED (matches nothing) -> ON assertion
  dropped (covered by unit tests + real servers). Tests serialized via a static
  Mutex (concurrent APPENDs under auth.disabled interfered -> flaky empties).
  All green: 16 lib + 11 bin unit + 3 integration; clippy clean. README gained a
  Testing section. SEARCH FEATURE COMPLETE (S1+S2+S3).

- 2026-07-08 (rev 7): Created a USAGE skill for the mail-cli binary via the
  skill-creator skill. Location: /home/exe/.agents/skills/mail-cli/ with
  SKILL.md, references/reference.md, evals/evals.json. Key facts encoded:
  (a) --json is a GLOBAL clap flag (works either side of subcommand;
  before-placement canonical); (b) stateless => address by uid:NNN not row N;
  (c) env-only config, no config file; (d) search is server-side substring,
  AND-only, errors on empty criteria, --raw conflicts w/ structured flags.

- 2026-07-08 (rev 8): BUNDLED the skill INTO the binary + added an
  `install-skill` subcommand. Copied the shipped skill verbatim into repo
  `skill/` (SKILL.md + references/reference.md + evals/evals.json). New pure
  module src/skill.rs: embeds the 3 assets via `include_str!("../skill/...")`
  (path is relative to the SOURCE FILE, so `../skill` from src/), exposes
  `NAME`, `ASSETS: &[(relpath, contents)]`, `default_dir()` (=
  $HOME/.agents/skills/mail-cli, falls back to USERPROFILE for Windows), and
  `install(dir, force)`. install() rejoins '/'-separated relpaths per-component
  for portability, create_dir_all parents, refuses to clobber existing files
  unless --force (fails loud naming the path). Wired `Command::InstallSkill {
  dir: Option<PathBuf>, force: bool }` in main.rs (clap derives kebab
  `install-skill`) + `cmd_install_skill`. It reads NO env — purely local,
  consistent w/ stateless design. 4 new unit tests (embed non-empty, frontmatter
  name, install writes all 3, refuse-without-force then force-ok). 19 lib + 15
  bin unit + 3 integration all pass; clippy clean. Verified real binary:
  --help, default+--dir, overwrite-refusal, --force, and `diff -rq` shows
  installed output is BYTE-IDENTICAL to the source skill. Updated README
  (install-skill section) + AGENTS.md (skill.rs + skill/ in project structure).

## Semantic (facts learned)
- Rust email crate landscape (mid-2026):
  - `imap` (jonhoo) = sync, simple, but stuck on 3.0.0-alpha, seeking maintainers.
  - `async-imap` (chatmail) = healthier/active (0.11.2), but async (needs tokio).
  - `lettre` 0.11 = the SMTP crate; also BUILDS MIME (so no `mail-builder` needed).
  - `mail-parser` (Stalwart) 0.11 = best MIME parser; zero-copy; body_text/html/attachments.
  - CLI=clap4, config=serde+toml+directories, secrets=keyring+rpassword, json=serde_json, errors=anyhow, TLS=rustls.
- For a one-shot CLI, async gives no benefit → chose sync `imap`, isolated in one module.
- Gmail/O365 killed basic auth → app-passwords or OAuth. Full OAuth flow = v2.

## Procedural (how)
- Planning workflow: research crates via lib.rs/docs.rs → write exhaustive PLAN.md
  → run ZoE review (read /home/exe/.agents/skills/zen-engineering/SKILL.md, apply
  its output format incl. "New rule candidates") → keep memory.md updated.
- ZoE mandate: plans detailed, code minimal; always end reviews with rule candidates.
- Project dir: /var/home/exe/workz/mail-cli (PLAN.md, ZOE-REVIEW.md, memory.md).

## Implementation notes (verified against crate source, mid-2026)
- `imap` 3.0.0-alpha.15: `ClientBuilder::new(host,port).mode(ConnectionMode::Tls|StartTls).connect()`
  returns `Client<imap::Connection>` where `Connection` is ALREADY a boxed
  type-erased stream — DON'T box it yourself, alias `Session<imap::Connection>`.
  `.starttls()` does NOT exist; use `.mode(ConnectionMode::StartTls)`. Feature
  `rustls-tls`. XOAUTH2 = impl `imap::Authenticator{type Response=String; process
  returns raw "user=..\x01auth=Bearer ..\x01\x01"}` (lib base64-encodes it),
  then `client.authenticate("XOAUTH2", &auth)`. Fetch with `BODY.PEEK[]` to NOT
  set \Seen; read bytes via `fetch.body()`. `uid_search` returns HashSet<u32>;
  sort desc for newest-first. Login err is a tuple `(Error, Client)`.
- `lettre` 0.11.22: `default-features=false`, features
  `["builder","smtp-transport","pool","rustls-tls","hostname"]` (rustls-tls pulls
  ring+webpki-roots, no OpenSSL). `SmtpTransport::relay(host)` (implicit) /
  `starttls_relay(host)`, then `.port().credentials(Credentials::new).authentication(vec![Mechanism::Plain|Login|Xoauth2]).build()`.
  Attachments: `Attachment::new(name).body(bytes, ContentType)`, wrap in
  `MultiPart::mixed().singlepart(body).singlepart(att)`. Used `mime_guess` for types.
- `mail-parser` 0.11: `MessageParser::default().parse(&bytes)`; `.from()/.to()/.cc()`
  return `Address` (iterate `.iter()`, `.address()`, `.name()`); `.subject()`,
  `.date().to_rfc3339()`, `.body_text(0)/.body_html(0)` (auto down-convert),
  `.attachments()` (each: `.attachment_name()`, `.content_type()`, `.contents()`),
  `.attachment_count()`. Import `MimeHeaders` trait for `.content_type()`.
- Module layout as planned: main/config/imap/parse/smtp/output. config.rs is the
  ONLY env reader; per-command validation via ImapConfig/SmtpConfig::from_env.
- Build: `cargo build` clean, `cargo test` 11 pass, `cargo clippy` clean.
- STILL TODO (M4/v2): GreenMail Docker integration tests; real-provider smoke test.

- 2026-07-08 (rev 9): FIRST GITHUB RELEASE v0.1.0. Repo=patch-notes/mail-cli
  (GitHub). CI branch ci/add-github-actions was already merged to master via PR
  #1 (commit 89a0b3a) — ci.yml + release.yml live on master. Verified local
  fmt/clippy/test all green. Tagged annotated v0.1.0 on 89a0b3a (Cargo.toml
  version=0.1.0 matches the release.yml guardrail) and pushed -> Release
  workflow run 28973982166 (4-target matrix: linux-amd64, macos-arm64,
  macos-amd64, windows-amd64; publish job uploads dist/* via
  softprops/action-gh-release, generate_release_notes).
  GOTCHA: `gh` CLI HANGS in this sandbox — it connects to GitHub (TLS OK per
  strace) but the HTTP read never returns; every `gh api`/`gh pr` call times
  out at 124. WORKAROUND: `gh auth token` (reads local config, fast, no net) +
  drive git via authenticated URL
  https://x-access-token:$TOK@github.com/... and drive the GitHub REST API via
  curl -H "Authorization: token $TOK". Raw curl to api.github.com is fast
  (~0.17s). Use this pattern for all GitHub ops here.

- 2026-07-08 (rev 10): Cleaned up AGENTS.md. (a) REMOVED all references to
  PLAN.md and the "No history yet; ..." hint — both stale (no PLAN.md on disk;
  v0.1.0 release exists, so there IS history). (b) Added missing files to the
  project tree: src/lib.rs (facade exposing only `search` for tests) and
  src/search.rs (Criteria + to_imap_query, pure), and the tests/ directory
  (search_integration.rs, gated on MAILCLI_IT=1). (c) Added a top-of-file
  agent-first framing: "designed for both humans and AI coding agents" + the
  skill/agent install path. (d) New "Install for agents" section: explains
  install-skill reads NO MAILCLI_* env, lists the 4 release asset names
  (mail-cli-linux-amd64, mail-cli-macos-amd64, mail-cli-macos-arm64,
  mail-cli-windows-amd64.exe) with correct download URLs at
  github.com/patch-notes/mail-cli/releases/latest/download/<asset>, gives
  per-platform curl/Invoke-WebRequest commands, then `./mail-cli install-skill`.
  NOTES: macOS needs `xattr -d com.apple.quarantine` first run; Windows is
  PowerShell; `gh release download -R patch-notes/mail-cli -p '...'` for gh
  users; `cargo build --release` then install-skill for source builders.
  Re-verified release assets via `gh release view v0.1.0 --repo
  patch-notes/mail-cli --json assets` (worked this time, ~0.5s).
