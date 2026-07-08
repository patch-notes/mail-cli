---
name: mail-cli
description: Drive the `mail-cli` tool — a stateless, env-configured IMAP/SMTP command-line email client written in Rust — to read, list, search, and send email from the terminal. Use this skill whenever the user wants to check/list their inbox, read a specific message, search mail by sender/subject/date/flags, download attachments, or compose and send an email through mail-cli. Trigger on requests like "check my email", "list my inbox", "find the invoice from alice", "read message 3", "download the attachment", "send an email to bob with this file attached", "search my mail for unread messages since Monday", or any task that runs the `mail-cli` binary. Also trigger when the user mentions MAILCLI_* environment variables, IMAP/SMTP from the CLI, or `uid:` message selectors.
---

# mail-cli

`mail-cli` is a single-binary, **stateless** email client. It reads and sends
mail over IMAP/SMTP with **no config file, no cache, and no stored state**.
Everything about *where* and *who* comes from `MAILCLI_*` environment variables;
everything about *what to do* is a CLI flag.

Because it's stateless, the two rules that matter most are:

1. **Configure via environment, not files.** Never invent a config file or a
   credentials store — there isn't one. Set/inspect `MAILCLI_*` env vars.
2. **Address messages by `uid:NNN`, not by row number, in anything scripted.**
   A plain `N` is a *live index* recomputed on every call; it's convenient for a
   human glancing at a list, but it shifts as mail arrives. `uid:NNN` is stable.

If the binary isn't already on `PATH`, build it with `cargo build --release`
(binary lands at `target/release/mail-cli`) and invoke it by that path.

## Configuration (environment only)

Set these before running read commands (IMAP) or `send` (SMTP). Only the vars a
given command needs are required; a missing one fails loud, naming the variable.

| Variable | Needed for | Default | Meaning |
|---|---|---|---|
| `MAILCLI_IMAP_HOST` | read cmds | — | IMAP server host |
| `MAILCLI_IMAP_PORT` | no | `993` | IMAP port |
| `MAILCLI_SMTP_HOST` | `send` | — | SMTP server host |
| `MAILCLI_SMTP_PORT` | no | `465` | SMTP port |
| `MAILCLI_EMAIL` | yes | — | Account address (From + default login user) |
| `MAILCLI_USER` | no | `=EMAIL` | Login username, if different from the address |
| `MAILCLI_PASSWORD` | yes | — | Password / app-password / (with `xoauth2`) an OAuth access token |
| `MAILCLI_TLS` | no | `implicit` | `implicit` (TLS on connect) or `starttls` |
| `MAILCLI_AUTH` | no | `login` | `login` (PLAIN/LOGIN) or `xoauth2` (token passthrough) |

Switch accounts by switching environment (shell export, `direnv`, a wrapper
script). There are no profiles. **Never persist or log the password.**

For provider-specific setup (Gmail app passwords, Outlook, XOAUTH2 tokens),
read `references/reference.md`.

## `--json`: a global flag — canonical placement is before the subcommand

```sh
mail-cli --json list --limit 5        # canonical, always correct
mail-cli list --limit 5 --json        # also works (it's a global flag)
```

`--json` is declared global, so clap accepts it on either side of the
subcommand. Prefer putting it **before** the subcommand — that's the form shown
throughout the docs and it reads unambiguously. It applies to the read commands
(`folders`, `list`, `search`, `read`, `attachments`) and prints
machine-readable output. Always use it when you're going to parse the result
(e.g. piping to `jq`) rather than scraping the human table.

## Commands

### folders — list mailboxes
```sh
mail-cli folders
```

### list — messages newest-first
```sh
mail-cli list --folder INBOX --limit 20
mail-cli list --search "FROM alice UNSEEN"   # raw IMAP SEARCH string
```
Table columns: `#  UID  FROM  SUBJECT  DATE  📎`. `#` is a throwaway row number
for *this* listing; `UID` is the durable handle.

### search — typed flags compiled to a server-side IMAP SEARCH
Prefer this over `list --search` when the user describes criteria in words. All
flags are **AND-combined** and matched **server-side as substrings**.
```sh
mail-cli search --from alice --unseen
mail-cli search --subject invoice --since 2026-01-01 --before 2026-07-01
mail-cli search --to bob --larger 1000000        # >1 MB, addressed to bob
mail-cli search --raw "OR FROM alice FROM bob"   # escape hatch for OR/NOT
```
Flags: `--from --to --cc --subject --body --text`; dates `--since/--before/--on`
(`YYYY-MM-DD`); state `--seen/--unseen --flagged/--unflagged --answered`; size
`--larger/--smaller BYTES`; plus `--folder` and `--limit`. Things to remember:
- `--from alice` matches any From header *containing* "alice" — not an exact
  address. Real servers vary; some (e.g. GreenMail) do exact address tokens.
- At least one criterion is required; empty criteria errors on purpose (it will
  **not** silently `SEARCH ALL` — use `list` for that).
- `--raw` conflicts with every structured flag; so do the pairs
  `--seen`/`--unseen` and `--flagged`/`--unflagged`.
- Output columns and `--json` are identical to `list`.

### read — render one message
`SELECTOR` is a live index `N` **or** a stable `uid:NNN`.
```sh
mail-cli read INBOX uid:4821       # stable — what scripts should use
mail-cli read INBOX 3              # 3rd newest *right now*
mail-cli read INBOX uid:4821 --html   # the raw HTML part
mail-cli read INBOX uid:4821 --raw    # untouched RFC822 source (bytes)
```
The header echoes the resolved UID (`message uid:4821 (#3)`) so a shift between
glance and command is visible.

### attachments — list a message's attachments
```sh
mail-cli attachments INBOX uid:4821
```
Columns: `K  FILENAME  MIME  SIZE`, where `K` is the 1-based attachment index.

### download — save attachments to disk
Requires either `--attachment K` or `--all`.
```sh
mail-cli download INBOX uid:4821 --attachment 1 --out ./downloads
mail-cli download INBOX uid:4821 --all --out ./downloads
```

### send — compose over SMTP
```sh
mail-cli send --to a@x.com,b@y.com --cc c@z.com \
  --subject "Hello" --body "Hi there" --attach ./report.pdf
```
- `--to/--cc/--bcc` take comma-separated lists. At least one `--to` is required.
- Body is exactly one of `--body TEXT` or `--body-file PATH` (`-` reads stdin).
- `--html` sends the body as HTML. `--attach` is repeatable (one file each).
```sh
mail-cli send --to a@x.com --subject S --body-file note.txt
echo "hi" | mail-cli send --to a@x.com --subject S --body-file -
mail-cli send --to a@x.com --subject S --html --body '<p>Hi</p>'
```

## Recommended workflow for an agent

1. **Confirm config.** Check the needed `MAILCLI_*` vars are set. If a read
   command fails with "required environment variable … is not set", surface
   exactly which var and how to set it — don't guess a config file.
2. **Find the message.** Use `search` (typed) or `list` to locate it, ideally
   with `--json` so you can read the `uid` field programmatically.
3. **Act on it by UID.** Feed `uid:NNN` to `read`/`attachments`/`download`.
   This survives new mail arriving between steps.
4. **For sending,** assemble the `send` invocation; use `--body-file -` when the
   body is long or comes from another step rather than cramming it into `--body`.

When you need exact JSON field names, the full env/provider notes, or edge-case
behavior, read `references/reference.md`.
