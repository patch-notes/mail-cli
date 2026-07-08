# mail-cli

A single-binary, **stateless** command-line email client in Rust. Read, list,
fetch, and search mail over IMAP; send mail (with attachments) over SMTP.

Configuration comes **entirely from environment variables**; everything else is
a CLI flag. **No config file, no cache, no stored state of any kind.**
Human-friendly by default, script-friendly with `--json`.

See [`PLAN.md`](PLAN.md) for the full design and rationale.

## Install

```sh
cargo build --release
# binary at target/release/mail-cli
```

## Configuration (environment only)

| Variable | Required | Default | Meaning |
|---|---|---|---|
| `MAILCLI_IMAP_HOST` | for read cmds | — | IMAP server host. |
| `MAILCLI_IMAP_PORT` | no | `993` | IMAP port. |
| `MAILCLI_SMTP_HOST` | for `send` | — | SMTP server host. |
| `MAILCLI_SMTP_PORT` | no | `465` | SMTP port. |
| `MAILCLI_EMAIL` | yes | — | Account address (From + default login user). |
| `MAILCLI_USER` | no | `=EMAIL` | Login username if different from the address. |
| `MAILCLI_PASSWORD` | yes | — | Password / app-password / (with `MAILCLI_AUTH=xoauth2`) an OAuth access token. |
| `MAILCLI_TLS` | no | `implicit` | `implicit` (TLS on connect) or `starttls`. |
| `MAILCLI_AUTH` | no | `login` | `login` (PLAIN/LOGIN) or `xoauth2` (token passthrough). |

Only the variables a given command needs are required. Switch accounts by
switching environment (shell, `direnv`, a wrapper script) — there are no
profiles.

## Usage

```
mail-cli [--json] <SUBCOMMAND>
```

### folders
List mailboxes for the account.
```sh
mail-cli folders
```

### list
List messages newest-first.
```sh
mail-cli list --folder INBOX --limit 20
mail-cli list --search "FROM alice UNSEEN"
```
Columns: `#  UID  FROM  SUBJECT  DATE  📎`. `#` is a cosmetic row number for
*this* listing only. `UID` is the stable machine id — use it across commands.

### search
Search a folder with typed flags. They compile to a single **server-side** IMAP
`SEARCH` and are **AND-combined**; the output is a normal listing (same columns
and `--json` as `list`).
```sh
mail-cli search --from alice --unseen
mail-cli search --subject "invoice" --since 2026-01-01 --before 2026-07-01
mail-cli search --to bob --larger 1000000        # >1 MB to bob
mail-cli search --raw "OR FROM alice FROM bob"   # raw IMAP escape hatch
```
Flags: `--from --to --cc --subject --body --text`, dates `--since/--before/--on`
(`YYYY-MM-DD`), state `--seen/--unseen --flagged/--unflagged --answered`, size
`--larger/--smaller BYTES`, plus `--folder` and `--limit`.

Notes:
- **Substring, server-side match** — `--from alice` matches any From header
  *containing* `alice`, not an exact address.
- `--raw` conflicts with all structured flags; conflicting state pairs
  (`--seen`/`--unseen`, `--flagged`/`--unflagged`) are rejected.
- With no criteria it errors loudly (it will not silently `SEARCH ALL` — use
  `list` for that).
- Results reshuffle `#` more than `list` does; **use `--json` and `uid:` in
  scripts.**

### read
Render one message. `SELECTOR` is either a live index `N` or a stable `uid:NNN`.
```sh
mail-cli read INBOX 3          # 3rd newest right now
mail-cli read INBOX uid:4821   # stable — what scripts should use
mail-cli read INBOX 3 --html   # raw HTML part
mail-cli read INBOX 3 --raw    # untouched RFC822 source
```
The output header echoes the resolved UID, e.g. `message uid:4821 (#3)`.

### attachments
```sh
mail-cli attachments INBOX uid:4821
```

### download
```sh
mail-cli download INBOX uid:4821 --attachment 1 --out ./downloads
mail-cli download INBOX uid:4821 --all
```

### send
```sh
mail-cli send --to a@x.com,b@y.com --cc c@z.com \
  --subject "Hello" --body "Hi there" \
  --attach ./report.pdf

# body from a file, or from stdin with '-'
mail-cli send --to a@x.com --subject S --body-file note.txt
echo "hi" | mail-cli send --to a@x.com --subject S --body-file -

# HTML body
mail-cli send --to a@x.com --subject S --html --body '<p>Hi</p>'
```

### install-skill
Install the agent usage skill that ships **inside the binary** — no repo clone
needed. It writes `SKILL.md` plus its `references/` and `evals/` to disk.
Purely local: no `MAILCLI_*` env vars are read.
```sh
mail-cli install-skill                       # -> ~/.agents/skills/mail-cli
mail-cli install-skill --dir ./my-skills/mc  # custom location
mail-cli install-skill --force               # overwrite an existing install
```
Without `--force` it refuses to clobber existing files, naming the offender.

## JSON output

Add `--json` to any read command (`folders`, `list`, `read`, `attachments`) for
machine-readable output:
```sh
mail-cli --json list --limit 5 | jq '.[].uid'
```

## Message references (stateless)

Two ways to name a message, both without any cache:

- **`uid:NNN`** — canonical, stable id. Scripts should use this.
- **`N`** — a live index, re-derived each call (`read INBOX 3` = select INBOX,
  list newest-first, take the 3rd). Valid only for the mailbox's current state.
  Commands echo the resolved `uid:` so a shift between glance and command is
  visible rather than silent.

## Authentication

- `MAILCLI_AUTH=login` (default): `LOGIN`/`PLAIN` with a password or
  app-specific password.
- `MAILCLI_AUTH=xoauth2`: `MAILCLI_PASSWORD` is treated as an OAuth access token
  (SASL XOAUTH2). Obtain/refresh the token externally — no token store here.

## Testing

Unit tests are pure and offline:
```sh
cargo test
```
The `search` query builder and parsing/selector logic are fully covered here.

Integration tests exercise `search` against a real IMAP server
([GreenMail](https://greenmail-mail-test.github.io/greenmail/)). They are
**gated on `MAILCLI_IT=1`** so a plain `cargo test` never needs a server:
```sh
podman run -d --name greenmail-mailcli -p 3025:3025 -p 3143:3143 \
  -e GREENMAIL_OPTS='-Dgreenmail.setup.test.imap -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.auth.disabled' \
  docker.io/greenmail/standalone:2.1.0

MAILCLI_IT=1 cargo test --test search_integration
```
(`docker` works too if you prefer it to `podman`.)

## License

See repository.
