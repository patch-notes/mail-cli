# mail-cli reference

Deep reference for the `mail-cli` skill. Read this when you need exact JSON
field names, provider/auth setup, or edge-case behavior. The everyday workflow
lives in `SKILL.md`.

## Table of contents
- [JSON output schemas](#json-output-schemas)
- [Selectors: `N` vs `uid:NNN`](#selectors-n-vs-uidnnn)
- [Authentication](#authentication)
- [TLS modes](#tls-modes)
- [Provider setup notes](#provider-setup-notes)
- [Error behavior](#error-behavior)
- [Scripting recipes](#scripting-recipes)
- [Design invariants (don't fight these)](#design-invariants-dont-fight-these)

## JSON output schemas

`--json` is a global flag, so it works on either side of the subcommand; the
canonical form used throughout is `mail-cli --json <cmd>`. Output is
pretty-printed JSON.

### `folders`
Array of objects; each folder currently exposes a `name`:
```json
[ { "name": "INBOX" }, { "name": "Sent" } ]
```

### `list` and `search` — array of Summary
```json
[
  {
    "uid": 4821,
    "from": "Alice <alice@example.com>",
    "subject": "Invoice #12",
    "date": "2026-06-30T14:22:01-08:00",
    "flags": ["\\Seen"],
    "has_attachments": true
  }
]
```
- `date` is RFC3339, or `""` if the message had no Date header.
- `subject` is `"(no subject)"` when absent.
- `flags` are raw IMAP flags (e.g. `\Seen`, `\Flagged`, `\Answered`).

### `read` — a single Rendered object
```json
{
  "uid": 4821,
  "from": "Alice <alice@example.com>",
  "to": "Bob <bob@example.com>",
  "cc": "",
  "subject": "Invoice #12",
  "date": "2026-06-30T14:22:01-08:00",
  "body": "…plain text (HTML down-converted if no text part)…",
  "attachments": [ /* Attachment objects, see below */ ]
}
```
With `--html`, `body` is the raw HTML part instead. With `--raw`, the command
writes untouched RFC822 **bytes to stdout** and ignores `--json`.

### `attachments` — array of Attachment
```json
[
  { "index": 1, "filename": "note.txt", "mime": "text/plain", "size": 2 }
]
```
- `index` is the 1-based `K` used by `download --attachment K`.
- Raw bytes are never included in JSON (only written to disk by `download`).

## Selectors: `N` vs `uid:NNN`

Both name a message in a folder, without any cache:

- **`uid:NNN`** — canonical, stable server id. Resolves directly, no listing
  needed. **Use this in scripts and multi-step flows.**
- **`N`** — a 1-based live index. The tool re-selects the folder, lists
  newest-first, and takes the Nth. Valid only for the mailbox's state *at that
  instant*; if mail arrives between your `list` and your `read 3`, `3` points at
  a different message. Index `0` is rejected (1-based). Every command echoes the
  resolved `uid:` so any drift is visible rather than silent.

Pattern to go from a human description to a stable handle:
```sh
uid=$(mail-cli --json search --from alice --unseen | jq '.[0].uid')
mail-cli read INBOX "uid:$uid"
```

## Authentication

Set with `MAILCLI_AUTH`:

- `login` (default): `LOGIN`/`PLAIN` using `MAILCLI_PASSWORD` as the password or
  app-specific password.
- `xoauth2`: SASL XOAUTH2. `MAILCLI_PASSWORD` is treated as an **OAuth access
  token** (not a password). Obtain and refresh the token externally — there is
  no token store. The login username is `MAILCLI_USER` if set, else
  `MAILCLI_EMAIL`.

## TLS modes

Set with `MAILCLI_TLS`:

- `implicit` (default): TLS negotiated on connect. Typical ports: IMAP 993,
  SMTP 465.
- `starttls`: plain connect upgraded via STARTTLS. Typical ports: IMAP 143,
  SMTP 587. Remember to set `MAILCLI_IMAP_PORT` / `MAILCLI_SMTP_PORT`
  accordingly.

## Provider setup notes

### Gmail (app password)
Modern Gmail blocks basic-auth passwords. Enable 2FA, create an **App Password**,
and use it as `MAILCLI_PASSWORD` with `MAILCLI_AUTH=login`.
```sh
export MAILCLI_IMAP_HOST=imap.gmail.com
export MAILCLI_SMTP_HOST=smtp.gmail.com
export MAILCLI_EMAIL=you@gmail.com
export MAILCLI_PASSWORD='xxxx xxxx xxxx xxxx'   # the 16-char app password
# implicit TLS + 993/465 are the defaults
```

### Gmail / Google Workspace (OAuth)
Mint an access token out-of-band, then:
```sh
export MAILCLI_AUTH=xoauth2
export MAILCLI_PASSWORD="$OAUTH_ACCESS_TOKEN"
```

### Outlook / Microsoft 365
Basic auth is deprecated; use an app password (where still allowed) or XOAUTH2.
Hosts are typically `outlook.office365.com` (IMAP) and `smtp.office365.com`
(STARTTLS on 587).

### Self-hosted / STARTTLS servers
```sh
export MAILCLI_TLS=starttls
export MAILCLI_IMAP_PORT=143
export MAILCLI_SMTP_PORT=587
```

## Error behavior

The binary fails loud and exits non-zero, printing `error: …`:

- **Missing env var** → names the exact variable, e.g.
  `required environment variable MAILCLI_PASSWORD is not set`. Fix the env; do
  not look for a config file.
- **Bad `MAILCLI_TLS` / `MAILCLI_AUTH`** → states the allowed values.
- **Empty `search` criteria** → refuses rather than matching everything.
- **`download` without `--attachment`/`--all`** → `specify --attachment K or --all`.
- **Selector out of range** → `no message #N in <folder> (only M present)`.
- **`send` without `--to`** → `at least one --to recipient is required`.

When you hit one of these, relay the exact message and the concrete fix; don't
paper over it.

## Scripting recipes

```sh
# Count unread in INBOX
mail-cli --json search --unseen | jq 'length'

# Newest UID from a sender
mail-cli --json search --from billing@vendor.com | jq '.[0].uid'

# Save the first attachment of the newest matching invoice
uid=$(mail-cli --json search --subject invoice | jq '.[0].uid')
mail-cli download INBOX "uid:$uid" --attachment 1 --out ./invoices

# Forward-ish: pull a body and re-send it
mail-cli --json read INBOX "uid:$uid" | jq -r '.body' \
  | mail-cli send --to archive@me.com --subject "copy" --body-file -
```

## Design invariants (don't fight these)

These are deliberate; work with them rather than around them:

- **Stateless.** No cache, no last-list memory, no `--account`/profiles. Account
  switching = environment switching.
- **Env is the only config boundary.** If something feels like it should be a
  flag but isn't, check whether it's a `MAILCLI_*` var before assuming it's
  missing.
- **Read commands are IMAP; `send` is SMTP.** They validate independently, so
  you can run reads with only IMAP vars set.
- **`--raw` (RFC822) output is bytes on stdout.** Redirect to a file if you want
  to keep it; don't expect JSON.
