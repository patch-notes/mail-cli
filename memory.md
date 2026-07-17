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

- 2026-07-08 (rev 9): README refresh. Removed dead `PLAN.md` link (file no longer
  exists) -> pointed design/rationale link at AGENTS.md instead. Verified README
  usage still matches code: Command enum (Folders/List/Search/Read/Attachments/
  Download/Send/InstallSkill) + all MAILCLI_* env vars in config.rs -> already
  current, no drift. Added a "Quick install (for agents)" section with a
  copy-paste one-line prompt (clone repo -> `cargo install --path .` ->
  `mail-cli install-skill`). Repo origin = github.com/patch-notes/mail-cli.
  Expanded Install block (cargo install vs build) and fixed License section
  ("See repository" -> "MIT — see LICENSE"; LICENSE is MIT, (c) 2026 Exe Zulliger).
  grep confirms no stale refs to plan.md/zoe-review/search-plan/model.rs remain.

- 2026-07-08 (rev 10): Reworked README install to DOWNLOAD PREBUILT BINARIES from
  the latest GitHub release instead of building from source. Latest release =
  v0.1.0 (github-actions bot); 4 assets: mail-cli-0.1.0-{linux-amd64,macos-amd64,
  macos-arm64,windows-amd64.exe}. Asset names are version-pinned, so the
  version-agnostic trick = hit /releases/latest API and
  `grep -o 'https://[^"]*linux-amd64'` -> resolves the browser_download_url
  (verified: yields exactly the v0.1.0 linux-amd64 URL, one line). Agent one-liner
  now tells the agent to fetch /releases/latest, pick the OS/arch asset, save as
  `mail-cli` on PATH, chmod +x, run install-skill. cargo build/install demoted to
  an optional "Build from source" subsection. GH releases created by CI
  (PR #1 by @thepeak99: "Add GitHub Actions CI + release pipelines").

- 2026-07-08 (rev 11): Added `install.sh` (repo root, POSIX sh, chmod +x,
  `sh -n` clean) and swapped the README's ugly multi-line curl|grep|mv snippet
  for a single `curl -fsSL .../master/install.sh | sh`. Script: detects OS
  (Linux->linux, Darwin->macos) + arch (x86_64->amd64, arm64/aarch64->arm64),
  guards linux=amd64-only, resolves the asset via /releases/latest API +
  `grep -o "https://[^\"]*${suffix}\(\.exe\)\?" | head -n1` (verified on this box:
  suffix=linux-amd64 -> correct v0.1.0 URL), downloads to mktemp w/ EXIT trap,
  installs to BINDIR (default /usr/local/bin, sudo if needed, else falls back to
  ~/.local/bin), warns if bindir not on PATH, then runs `install-skill` unless
  INSTALL_SKILL=0. Raw URL uses master branch (release target_commitish=master).
  Agent one-liner is now just the curl|sh command. Left AGENTS.md untouched (it
  documents src/ internals, not top-level scripts).

- 2026-07-09 (rev 12): PLANNED multi-account support (env-var suffixes _1/_2/…).
  Wrote ACCOUNTS-PLAN.md (plan only, not implemented). Design: an account = the
  set of MAILCLI_* vars sharing a numeric suffix. Selector resolution =
  `--account <TOK>` global flag > MAILCLI_ACCOUNT env > none(=bare vars=today's
  DEFAULT account, zero breakage). Token is numeric by convention but any
  non-empty string works (one code path; `--account work`→*_work free superset);
  empty token rejected loud. FALLBACK = STRICT/no inheritance (v1): acct N reads
  ONLY *_N; missing→error naming the real var e.g. MAILCLI_PASSWORD_2. OK because
  optional vars already default (993/465/implicit/login, USER=EMAIL) so an acct
  only MUST set IMAP_HOST/EMAIL/PASSWORD (+SMTP_HOST for send). Layered
  inheritance deferred behind opt-in MAILCLI_ACCOUNT_INHERIT (v2, non-goal).
  IMPL confined to config.rs+main.rs: introduce `Env{suffix:Option<String>}` with
  var(base)/opt/required/port/tls_mode/auth_method/login_user methods; from_env
  takes &Env. main.rs adds global `--account` flag, resolves once, threads &Env
  into cmd_* (install-skill untouched—reads no env). config.rs STAYS the ONLY env
  boundary (confirmed via grep: skill.rs only reads HOME/USERPROFILE). Tests:
  pure Env::var/new cases; optional injected lookup fn for from_env DI (avoids
  env-race mutex). Optional `accounts` subcommand (milestone B) lists configured
  suffixes WITHOUT printing PASSWORD. Milestones A(core)/B(discovery)/C(docs+
  SKILL sync+eval — skill/ must stay byte-identical, per rev 8). TENSION noted:
  rev 2 dropped --account/profiles; this env-only suffix form stays inside the
  env-only+stateless constraints (no file/cache), so it supersedes that line ONLY
  for this form — user must confirm.

- 2026-07-09 (rev 13): IMPLEMENTED multi-account (ACCOUNTS-PLAN.md, all 3
  milestones A/B/C). User confirmed: ship `accounts` cmd; no --account = default
  (bare vars). config.rs: replaced free opt/required/port/tls_mode/auth_method/
  login_user fns with methods on new pub `Env{suffix:Option<String>, lookup:
  fn(&str)->Option<String>}`; var(base) appends `_<suffix>`; Env::new(None)=
  default, Some(empty/ws)=loud error, Some(tok)=suffix. ImapConfig/SmtpConfig::
  from_env now take &Env. Added pub `scan_accounts()` + pure `scan_accounts_from(
  Vec<(String,String)>)` + `AccountInfo{name,email,imap_host,smtp_host,
  has_password}` (serde::Serialize, NO password value ever) + account_sort_key
  (default<numeric<lexical). main.rs: global `--account:Option<String>` flag
  (next to --json); run() resolves account = flag > MAILCLI_ACCOUNT env > None,
  builds Env once, threads &env into cmd_folders/list/read/attachments/download/
  send; install-skill untouched. New `Accounts` subcommand → cmd_accounts(json)
  → output::accounts_human (STATUS ok/incomplete, never prints pw) or --json.
  output.rs imports crate::config::AccountInfo. 11 new config tests (var
  resolution, empty-token reject, suffixed-error naming via injected `|_|None`
  lookup, scanner: default+numeric, incomplete-no-leak, sort order, empty-value
  skip). 45 tests total pass (16 lib+26 bin+3 integ), clippy clean (used
  let-chains to collapse if, sort_by_key, inlined literal), fmt clean. Smoke
  verified real binary: accounts table+json (no pw leak), empty --account
  rejected, MAILCLI_PASSWORD_2 / MAILCLI_IMAP_HOST_3 named on miss, MAILCLI_
  ACCOUNT env selects, --help shows global --account. Docs: README (config table
  +MAILCLI_ACCOUNT row, new "Multiple accounts" section, accounts subcommand,
  usage synopsis `[--account N]`), AGENTS.md (config.rs desc + Security rule
  "accounts env-suffixed, selection is a CLI flag"). SKILL synced: SKILL.md
  (+MAILCLI_ACCOUNT, Multiple accounts section, accounts cmd), references/
  reference.md (AccountInfo JSON schema, Accounts section, error cases, fixed
  stale "no --account/profiles" invariant), evals.json (+eval 6 --account
  select, +eval 7 accounts discovery/no-leak). VERIFIED `install-skill --force`
  output is byte-identical to skill/ via diff -rq. Design decisions: STRICT
  no-fallback (acct N only reads *_N); numeric convention but any non-empty token
  = one code path; default account = bare vars = zero breakage. TENSION w/ rev-2
  "dropped --account/profiles" resolved: this env-only suffix form stays inside
  env-only+stateless, so it supersedes that line for THIS form only (user
  confirmed).
- 2026-07-09 (rev 14): RELEASED v0.2.0. Changes since v0.1.0: install.sh
  one-liner (PR #3, already on master) + multi-account (PR #4, already
  merged). Wrote CHANGELOG.md (Keep a Changelog format; v0.1.0 baseline
  so the file stands alone) + bumped Cargo.toml 0.1.0 → 0.2.0 on master.
  Release commit `c484886`. Tagged v0.2.0 annotated; pushed; release
  workflow queued as run 29002861696. Default-account = bare MAILCLI_* =
  zero breakage, so this is a MINOR not MAJOR bump. Release workflow
  builds linux-amd64, macos-amd64, macos-arm64, windows-amd64 binaries
  and publishes as a GitHub release.
- 2026-07-09 (rev 15): RELEASE COMPLETE. All 4 build jobs green, publish
  job 12s, run 29002861696 finished in ~3 min total. Verified via REST
  /releases/tags/v0.2.0 (gh CLI hung twice; curl + gh auth token +
  python parse = clean summary). 4 assets, all state=uploaded, sizes
  5.5/4.9/4.5/5.5 MiB for linux-amd64/macos-amd64/macos-arm64/windows-
  amd64.exe. Published 2026-07-09T07:55:29Z; draft=false, prerelease=
  false. Telegram ping sent via skill script (success). NOTES for next
  release: gh release view hung at 30s AND 120s — fall back to
  /releases/tags/<tag> JSON endpoint with gh auth token if it hangs
  again; gh run watch REQUIRES a run-id arg when started non-interactive
  (got "run ID required when not running interactively" — fixed on
  second attempt).

- 2026-07-17 (rev 16): PLANNED save-to-Sent (plan only, not implemented).
  Wrote plans/SAVE-TO-SENT-PLAN.md. ROOT CAUSE: `send` only calls smtp::send
  (SMTP relay); it never IMAP-APPENDs a copy into Sent. SMTP delivery != Sent
  mailbox; Gmail auto-appends but most providers don't. FIX: after SMTP ok,
  APPEND lettre `Message::formatted()` bytes into Sent, flagged \Seen,
  INTERNALDATE=now, via new imap::append_to_sent (crate AppendCmd, same path as
  the search integration test). DESIGN: folder = --sent-folder > MAILCLI_SENT_
  FOLDER > SPECIAL-USE \Sent autodiscover > "Sent"; default ON but best-effort:
  skip silently if IMAP not configured & not explicitly requested, else loud;
  APPEND failure = WARN + exit 0 (mail already went out — never fail a delivered
  send or a retry double-sends); --json gains saved_to_sent/sent_folder/
  save_error. config.rs stays sole env boundary (adds sent_folder()/save_sent()
  methods, no new REQUIRED vars = zero breakage); IMAP calls stay in imap.rs.
  Non-goals v1: no auto-CREATE, no Fcc/bcc-self, no offline retry queue. 3 open
  Qs for user (default on?, autodiscover in v1?, warn-vs-fail on APPEND error).

- 2026-07-17 (rev 17): IMPLEMENTED save-to-Sent (SAVE-TO-SENT-PLAN). User
  answered: (1) default ON, (2) DO the SPECIAL-USE autodiscovery in v1, (3) WARN
  (not fail). Ran ZoE review first → 2 cuts adopted: (a) DROPPED --json-on-send
  entirely (send had no JSON today; unrequested feature = YAGNI), (b) DROPPED the
  --save-sent force flag → single bool: default on, --no-save-sent /
  MAILCLI_SAVE_SENT=0 to opt out; if IMAP unconfigured print a one-line note:
  (not silent, not fatal) since a silent skip looks like the very bug. TDD:
  wrote failing tests FIRST (config unit tests for sent_folder()/save_sent();
  tests/sent_integration.rs), confirmed red (missing methods + private
  mail_cli::imap), then went green. IMPL: config.rs adds Env::sent_folder()->
  Option<String> (raw env read; "Sent" DEFAULT lives at point-of-use in main.rs,
  per ZoE) + save_sent()->Result<bool> (1/0,true/false,yes/no,on/off; garbage
  rejected naming resolved var). imap.rs adds append_to_sent (session.append().
  flag(Seen).finish()) + discover_sent (LIST, match imap_proto::NameAttribute::
  Sent). Added imap-proto="=0.16.7" as a DIRECT dep (pin to imap's resolved
  version) to name that enum — imap doesn't re-export NameAttribute. smtp.rs
  adds formatted(&Message)->Vec<u8> (single source = the built Message; no
  double-format cleverness, no send_raw). main.rs: send gains --sent-folder /
  --no-save-sent; cmd_send does primary send (fatal on fail) then best-effort
  save_to_sent() helper that NEVER returns Err (all paths warn/note + return);
  folder precedence --sent-folder > env.sent_folder() > discover_sent >
  "Sent". lib.rs now exposes pub mod config/imap/parse (was search-only) for the
  integration test — FIXED its now-false doc comment (misleading-comment rule).
  VERIFY: 38 lib + 33 bin unit pass; clippy clean; fmt clean. Integration ran
  under GreenMail (podman, MUST bind 0.0.0.0 + auth.disabled per rev 6) — both
  sent tests PASS (copy lands in Sent flagged \Seen; discovery selects a valid
  mailbox). install-skill --force output BYTE-IDENTICAL to skill/ (diff -rq).
  Docs synced: README (config table +2 rows, Saving-to-Sent section, testing
  cmd), AGENTS.md (imap.rs desc, testing guidelines de-"planned"), SKILL.md +
  reference.md + evals.json (+eval 8). CHANGELOG [Unreleased] added (MINOR:
  additive, degrades to skip when IMAP absent). Opened PR (see below).

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
