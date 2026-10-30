//! Pure reply-derivation rules (REPLY-PLAN §4.2, §5.1, §6.2).
//!
//! Everything here is pure: no IO, no lettre. Given a parsed
//! [`crate::parse::Original`] (the parent message) and the account address,
//! these functions work out the reply's recipients, `Re:` subject,
//! `In-Reply-To`/`References` threading headers, and the quoted body. The
//! caller (`cmd_reply`) turns the result into an `smtp::Outgoing` and reuses
//! the shared send path.
//!

use crate::parse::{Addr, Original};
use serde::Serialize;

/// Threading headers, angle-bracketed and ready for lettre.
/// Every combination is RFC-legal: if the parent has no Message-ID, there's no
/// `In-Reply-To`, but `References` may still carry the parent's own
/// `References`.
// used by cmd_reply (T7)
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Threading {
    /// `"<id>"` of the parent, or `None` (parent had no Message-ID).
    pub in_reply_to: Option<String>,
    /// `["<a>", "<b>", "<id>"]`: no duplicates, order kept; the last element
    /// is `in_reply_to` when that is `Some`.
    pub references: Vec<String>,
}

/// What `reply` did. Built after all side effects; the single thing rendered
/// (`output::reply_human` / `--json`).
// used by cmd_reply (T7)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct ReplyReport {
    /// Folder the parent lived in.
    pub folder: String,
    /// The parent's UID.
    pub uid: u32,
    /// Our new Message-ID (`"<…>"`).
    pub message_id: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Sent folder, or `None` if the copy was skipped/failed.
    pub saved_to: Option<String>,
    /// Whether `\Answered` was successfully set on the parent.
    pub answered: bool,
}

/// Compute the reply's (To, Cc) from the parent (REPLY-PLAN §5.1, locked):
/// `Reply-To` over `From`; if every address of the primary is `me` (replying
/// to our own message), the pools become the parent's `To`/`Cc`. `all`
/// (reply-all) fills Cc with the parent's `To`+`Cc` minus `me` and minus To.
/// `me` is the bare `MAILCLI_EMAIL` address; comparisons are case-insensitive;
/// dedupe keeps the first occurrence, including its display name.
pub fn recipients(o: &Original, me: &str, all: bool) -> (Vec<Addr>, Vec<Addr>) {
    let primary = if o.reply_to.is_empty() {
        &o.from
    } else {
        &o.reply_to
    };
    let own = !primary.is_empty() && primary.iter().all(|a| is_me(&a.email, me));
    let (to_pool, cc_pool) = if own {
        (&o.to, &o.cc)
    } else {
        (primary, &concat_lists(&o.to, &o.cc))
    };
    let to = dedupe(to_pool.iter().filter(|a| !is_me(&a.email, me)).cloned());
    let to_emails = to
        .iter()
        .map(|a| a.email.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let cc = if all {
        dedupe(
            cc_pool
                .iter()
                .filter(|a| {
                    !is_me(&a.email, me) && !to_emails.contains(&a.email.to_ascii_lowercase())
                })
                .cloned(),
        )
    } else {
        Vec::new()
    };
    (to, cc)
}

/// `Re:`-prefix a subject without stacking: `t = s.trim()`; if `t` starts
/// with `re:` (ASCII case-insensitive) it is returned unchanged, else
/// `"Re: " + t` (`""` → `"Re:"`). No other prefix (`AW:`, `Fwd:`, …) is
/// recognized (REPLY-PLAN §5.1, §9 Q3).
pub fn subject(original: &str) -> String {
    let t = original.trim();
    if t.to_ascii_lowercase().starts_with("re:") {
        t.to_string()
    } else if t.is_empty() {
        "Re:".to_string()
    } else {
        format!("Re: {t}")
    }
}

/// Build the reply's threading headers from the parent (RFC 5322 §3.6.4,
/// REPLY-PLAN §5.1, verbatim): `in_reply_to = bracket(o.message_id?)`;
/// `base = o.references`, else `o.in_reply_to` if it has exactly one id, else
/// `[]`; `references = dedupe_keep_order(base ++ [o.message_id?])` bracketed.
pub fn threading(o: &Original) -> Threading {
    let in_reply_to = o.message_id.as_deref().map(bracket);
    let base = if !o.references.is_empty() {
        o.references.clone()
    } else if o.in_reply_to.len() == 1 {
        o.in_reply_to.clone()
    } else {
        Vec::new()
    };
    let mut references = dedupe_ids(base);
    if let Some(id) = &o.message_id
        && !references.iter().any(|r| r == id)
    {
        references.push(id.clone());
    }
    Threading {
        in_reply_to,
        references: references.into_iter().map(|id| bracket(&id)).collect(),
    }
}

/// Top-post `body` over the quoted parent text (REPLY-PLAN §5.1): the body
/// (trailing newlines trimmed), a blank line, `On {date}, {from} wrote:`
/// (date omitted when absent; `unknown sender` when From is empty), then the
/// parent's text with CRLF→LF, trailing blanks trimmed, each line prefixed
/// `>` (an empty line becomes a bare `>`; a line already starting with `>`
/// gets another). An empty parent text returns `body` unchanged.
pub fn quoted_body(body: &str, o: &Original) -> String {
    let text = o.text.replace("\r\n", "\n");
    let text = text.trim_end();
    if text.is_empty() {
        return body.to_string();
    }
    let sender = if o.from.is_empty() {
        "unknown sender".to_string()
    } else {
        o.from
            .iter()
            .map(display_addr)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let attribution = match &o.date {
        Some(date) => format!("On {date}, {sender} wrote:"),
        None => format!("{sender} wrote:"),
    };
    let quoted = text
        .lines()
        .map(|line| {
            if line.starts_with('>') {
                // A line already quoted gets one more '>' ("> a" -> ">> a").
                format!(">{line}")
            } else if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}\n\n{}\n{quoted}", body.trim_end(), attribution)
}

/// `Name <a@b>` for a named address, the bare address otherwise — the same
/// format `read` shows.
fn display_addr(a: &Addr) -> String {
    match &a.name {
        Some(name) => format!("{name} <{}>", a.email),
        None => a.email.clone(),
    }
}

/// ASCII case-insensitive address equality against the account address.
fn is_me(email: &str, me: &str) -> bool {
    email.eq_ignore_ascii_case(me)
}

/// `a ++ b` as one owned list (both kept in their original order).
fn concat_lists(a: &[Addr], b: &[Addr]) -> Vec<Addr> {
    a.iter().cloned().chain(b.iter().cloned()).collect()
}

/// Drop case-insensitive duplicates by email, keeping the first occurrence
/// (and its display name).
fn dedupe(addrs: impl Iterator<Item = Addr>) -> Vec<Addr> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for a in addrs {
        if !seen.contains(&a.email.to_ascii_lowercase()) {
            seen.push(a.email.to_ascii_lowercase());
            out.push(a);
        }
    }
    out
}

/// Drop exact duplicates of bare message ids, order kept.
fn dedupe_ids(ids: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Wrap a bare message id in angle brackets (ids are bracketed here, and only
/// here, per REPLY-PLAN §4.4).
fn bracket(id: &str) -> String {
    format!("<{id}>")
}

#[cfg(test)]
mod tests {
    //! Offline contract tests for the reply rules (REPLY-PLAN §12.1).
    //!
    //! Fixtures are inline `.eml` bytes run through `parse::original` — the
    //! same parse boundary `cmd_reply` uses — so a test failure names the
    //! behaviour that is missing, end to end.

    use super::*;

    /// Parse an inline fixture the same way `cmd_reply` does.
    fn orig(raw: &str) -> Original {
        crate::parse::original(raw.as_bytes()).expect("parse::original on test fixture")
    }

    /// One expected `Addr` (display name + address).
    fn a(name: &str, email: &str) -> Addr {
        Addr {
            name: Some(name.to_string()),
            email: email.to_string(),
        }
    }

    // ---------- recipients (REPLY-PLAN §5.1 "Recipients") ----------

    const ME: &str = "me@example.com";

    #[test]
    fn recipients_no_reply_to_uses_from() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             To: Me <me@example.com>\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, false);
        assert_eq!(to, vec![a("Alice", "alice@example.com")]);
        assert!(cc.is_empty(), "plain reply must not Cc anyone; got {cc:?}");
    }

    #[test]
    fn recipients_reply_to_wins_over_from() {
        let o = orig(
            "Reply-To: List <list@example.com>\r\n\
             From: Alice <alice@example.com>\r\n\
             To: Me <me@example.com>\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, false);
        assert_eq!(
            to,
            vec![a("List", "list@example.com")],
            "a list Reply-To must win (standard behaviour); From is ignored"
        );
        assert!(cc.is_empty());
    }

    #[test]
    fn recipients_all_adds_to_and_cc_minus_me() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             To: Me <me@example.com>, Carol <carol@example.com>\r\n\
             Cc: Dave <dave@example.com>\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, true);
        assert_eq!(to, vec![a("Alice", "alice@example.com")]);
        assert_eq!(
            cc,
            vec![
                a("Carol", "carol@example.com"),
                a("Dave", "dave@example.com")
            ],
            "reply-all Cc = parent To + Cc, minus me"
        );
    }

    #[test]
    fn recipients_self_removal_is_case_insensitive() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             To: ME@Example.COM, carol@example.com\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, true);
        assert_eq!(to, vec![a("Alice", "alice@example.com")]);
        assert_eq!(
            cc,
            vec![Addr {
                name: None,
                email: "carol@example.com".to_string()
            }],
            "ME@Example.COM must count as me (ASCII case-insensitive); got {cc:?}"
        );
    }

    #[test]
    fn recipients_dedupe_keeps_first_display_name() {
        let o = orig(
            "From: Bob <bob@example.com>\r\n\
             To: Alice One <alice@example.com>, Alice Two <ALICE@example.com>\r\n\
             Cc: alice@EXAMPLE.com, Bob <bob@example.com>\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, true);
        assert_eq!(to, vec![a("Bob", "bob@example.com")]);
        assert_eq!(
            cc,
            vec![a("Alice One", "alice@example.com")],
            "dedupe (across parent To and Cc) compares case-insensitively and keeps the FIRST display name; Bob (already in To) is dropped from Cc; got {cc:?}"
        );
    }

    #[test]
    fn recipients_not_all_leaves_cc_empty() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             Cc: Dave <dave@example.com>\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, false);
        assert_eq!(to, vec![a("Alice", "alice@example.com")]);
        assert!(
            cc.is_empty(),
            "without --all the parent's Cc must NOT be copied; got {cc:?}"
        );
    }

    #[test]
    fn recipients_own_message_targets_parent_to() {
        let o = orig(
            "From: Me <me@example.com>\r\n\
             To: Alice <alice@example.com>, Carol <carol@example.com>\r\n\
             Cc: Dave <dave@example.com>\r\n\
             Subject: My own message\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, false);
        assert_eq!(
            to,
            vec![
                a("Alice", "alice@example.com"),
                a("Carol", "carol@example.com")
            ],
            "replying to our own message must target the original To (not mail ourselves)"
        );
        assert!(cc.is_empty());

        let (to, cc) = recipients(&o, ME, true);
        assert_eq!(
            to,
            vec![
                a("Alice", "alice@example.com"),
                a("Carol", "carol@example.com")
            ]
        );
        assert_eq!(
            cc,
            vec![a("Dave", "dave@example.com")],
            "own-message reply-all: Cc = parent Cc minus me/to; got {cc:?}"
        );
    }

    #[test]
    fn recipients_note_to_self_yields_empty_to() {
        let o = orig(
            "From: Me <me@example.com>\r\n\
             To: Me <me@example.com>\r\n\
             Subject: Note to self\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, false);
        assert!(
            to.is_empty() && cc.is_empty(),
            "a note to self has no one to reply to (cmd_reply bails asking for --to); got {to:?}, {cc:?}"
        );
    }

    #[test]
    fn recipients_reply_to_is_me_but_from_is_other() {
        let o = orig(
            "Reply-To: Me <me@example.com>\r\n\
             From: Alice <alice@example.com>\r\n\
             To: Bob <bob@example.com>, Carol <carol@example.com>\r\n\
             Subject: Hi\r\n\
             \r\n\
             body\r\n",
        );
        let (to, cc) = recipients(&o, ME, false);
        assert_eq!(
            to,
            vec![a("Bob", "bob@example.com"), a("Carol", "carol@example.com")],
            "primary (Reply-To) is all me → own-message rule, target the parent's To"
        );
        assert!(cc.is_empty());
    }

    // ---------- subject (REPLY-PLAN §5.1 "Subject") ----------

    #[test]
    fn subject_plain_gets_re_prefix() {
        assert_eq!(subject("Hello"), "Re: Hello");
    }

    #[test]
    fn subject_re_is_not_stacked() {
        assert_eq!(subject("Re: Hello"), "Re: Hello");
    }

    #[test]
    fn subject_re_prefix_is_case_insensitive() {
        assert_eq!(subject("RE: Hello"), "RE: Hello");
        assert_eq!(subject("re:Hello"), "re:Hello");
    }

    #[test]
    fn subject_trims_surrounding_whitespace() {
        assert_eq!(subject("  Re: x  "), "Re: x");
        assert_eq!(subject("  Hello  "), "Re: Hello");
    }

    #[test]
    fn subject_empty_becomes_bare_re() {
        assert_eq!(subject(""), "Re:");
    }

    #[test]
    fn subject_localized_prefixes_are_not_recognized() {
        // AW:/SV:/Antw: are deliberately NOT treated as reply prefixes
        // (REPLY-PLAN §9 Q3) — they get one plain `Re: ` in front.
        assert_eq!(subject("AW: x"), "Re: AW: x");
        assert_eq!(subject("SV: x"), "Re: SV: x");
    }

    #[test]
    fn subject_fwd_prefix_is_not_recognized() {
        assert_eq!(subject("Fwd: x"), "Re: Fwd: x");
    }

    // ---------- threading (REPLY-PLAN §5.1 "Threading") ----------

    fn brackets(id: &str) -> String {
        format!("<{id}>")
    }

    #[test]
    fn threading_message_id_with_references() {
        let o = orig(
            "Message-ID: <orig-1@example.com>\r\n\
             References: <root-1@example.com> <parent-1@example.com>\r\n\
             In-Reply-To: <parent-1@example.com>\r\n\
             From: a@b.com\r\n\
             \r\n\
             x\r\n",
        );
        assert_eq!(
            threading(&o),
            Threading {
                in_reply_to: Some(brackets("orig-1@example.com")),
                references: vec![
                    brackets("root-1@example.com"),
                    brackets("parent-1@example.com"),
                    brackets("orig-1@example.com")
                ],
            },
            "References = parent's References ++ parent's Message-ID"
        );
    }

    #[test]
    fn threading_message_id_no_refs_single_in_reply_to() {
        let o = orig(
            "Message-ID: <orig-2@example.com>\r\n\
             In-Reply-To: <irt-2@example.com>\r\n\
             From: a@b.com\r\n\
             \r\n\
             x\r\n",
        );
        assert_eq!(
            threading(&o),
            Threading {
                in_reply_to: Some(brackets("orig-2@example.com")),
                references: vec![
                    brackets("irt-2@example.com"),
                    brackets("orig-2@example.com")
                ],
            },
            "with no References, a single In-Reply-To seeds the base"
        );
    }

    #[test]
    fn threading_message_id_no_refs_multiple_in_reply_to() {
        let o = orig(
            "Message-ID: <orig-3@example.com>\r\n\
             In-Reply-To: <i3a@example.com> <i3b@example.com>\r\n\
             From: a@b.com\r\n\
             \r\n\
             x\r\n",
        );
        assert_eq!(
            threading(&o),
            Threading {
                in_reply_to: Some(brackets("orig-3@example.com")),
                references: vec![brackets("orig-3@example.com")],
            },
            "two In-Reply-To ids are not a valid base; References = just the parent id"
        );
    }

    #[test]
    fn threading_no_message_id_keeps_parent_references() {
        let o = orig(
            "References: <root-4@example.com>\r\n\
             From: a@b.com\r\n\
             \r\n\
             x\r\n",
        );
        assert_eq!(
            threading(&o),
            Threading {
                in_reply_to: None,
                references: vec![brackets("root-4@example.com")],
            },
            "no parent Message-ID → no In-Reply-To, References still carry the parent's"
        );
    }

    #[test]
    fn threading_nothing_at_all_is_empty() {
        let o = orig("From: a@b.com\r\nSubject: bare\r\n\r\nx\r\n");
        assert_eq!(
            threading(&o),
            Threading {
                in_reply_to: None,
                references: vec![],
            },
            "a parent with no thread headers yields no threading headers"
        );
    }

    #[test]
    fn threading_parent_refs_already_containing_id_do_not_duplicate() {
        let o = orig(
            "Message-ID: <orig-5@example.com>\r\n\
             References: <root-5@example.com> <orig-5@example.com>\r\n\
             From: a@b.com\r\n\
             \r\n\
             x\r\n",
        );
        assert_eq!(
            threading(&o),
            Threading {
                in_reply_to: Some(brackets("orig-5@example.com")),
                references: vec![
                    brackets("root-5@example.com"),
                    brackets("orig-5@example.com")
                ],
            },
            "dedupe: the parent id must appear exactly once, in last position"
        );
    }

    #[test]
    fn threading_brackets_appear_exactly_once() {
        let o = orig(
            "Message-ID: <orig-6@example.com>\r\n\
             References: <root-6@example.com>\r\n\
             From: a@b.com\r\n\
             \r\n\
             x\r\n",
        );
        let t = threading(&o);
        let all = t
            .in_reply_to
            .iter()
            .chain(t.references.iter())
            .collect::<Vec<_>>();
        assert!(
            !all.is_empty(),
            "fixture has ids, so headers must be produced"
        );
        for id in all {
            let inner = id
                .strip_prefix('<')
                .and_then(|v| v.strip_suffix('>'))
                .unwrap_or("");
            assert_ne!(
                inner, "",
                "{id:?} must be bracketed exactly once (bare ids get one pair)"
            );
            assert!(
                !inner.contains('<') && !inner.contains('>'),
                "{id:?} must contain no nested brackets"
            );
        }
    }

    // ---------- quoted_body (REPLY-PLAN §5.1 "Body") ----------

    const QUOTE_ME: &str = "From: Alice <alice@example.com>\r\n\
                           Date: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
                           Subject: Quote me\r\n\
                           \r\n\
                           Line one.\r\n\
                           \r\n\
                           Line three.\r\n";

    #[test]
    fn quoted_body_top_posts_with_date_and_from_attribution() {
        let o = orig(QUOTE_ME);
        assert_eq!(
            quoted_body("My reply.", &o),
            "My reply.\n\n\
             On Sat, 20 Nov 2021 14:22:01 -0800, Alice <alice@example.com> wrote:\n\
             > Line one.\n\
             >\n\
             > Line three.",
            "user text first, blank line, attribution, then `> `-quoted parent"
        );
    }

    #[test]
    fn quoted_body_empty_lines_become_bare_greater() {
        let o = orig(QUOTE_ME);
        let out = quoted_body("My reply.", &o);
        assert!(
            out.contains("\n>\n"),
            "an empty parent line must be quoted as a bare '>', got:\n{out}"
        );
        assert!(
            !out.lines().any(|l| l == "> "),
            "a bare '>' must have NO trailing space; got:\n{out}"
        );
    }

    #[test]
    fn quoted_body_without_date_drops_the_date_part() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             Subject: nd\r\n\
             \r\n\
             Only line.\r\n",
        );
        assert_eq!(
            quoted_body("My reply.", &o),
            "My reply.\n\nAlice <alice@example.com> wrote:\n> Only line.",
            "missing Date: attribution is just '<from> wrote:'"
        );
    }

    #[test]
    fn quoted_body_unknown_sender_when_from_missing() {
        let o = orig("Subject: nofrom\r\n\r\nOnly line.\r\n");
        assert_eq!(
            quoted_body("My reply.", &o),
            "My reply.\n\nunknown sender wrote:\n> Only line.",
            "empty From → 'unknown sender wrote:'"
        );
    }

    #[test]
    fn quoted_body_nests_existing_greaters() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             Subject: q\r\n\
             \r\n\
             > already quoted\r\n",
        );
        let out = quoted_body("My reply.", &o);
        assert!(
            out.contains(">> already quoted"),
            "a line starting with '>' gets one more '>'; got:\n{out}"
        );
    }

    #[test]
    fn quoted_body_normalizes_crlf_to_lf() {
        let o = orig(QUOTE_ME);
        let out = quoted_body("My reply.", &o);
        assert!(
            !out.contains('\r'),
            "CRLF must be turned into LF first; got:\n{out:?}"
        );
    }

    #[test]
    fn quoted_body_trims_parent_trailing_blank_lines() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             Subject: t\r\n\
             \r\n\
             Last line.\r\n\
             \r\n\
             \r\n",
        );
        assert_eq!(
            quoted_body("My reply.", &o),
            "My reply.\n\nAlice <alice@example.com> wrote:\n> Last line.",
            "parent trailing blank lines must not become trailing '>' lines"
        );
    }

    #[test]
    fn quoted_body_empty_parent_text_returns_body_unchanged() {
        let o = orig("From: Alice <alice@example.com>\r\nSubject: empty\r\n\r\n");
        assert_eq!(
            quoted_body("hi", &o),
            "hi",
            "an empty parent text gives back the body unchanged, with no attribution"
        );
    }

    #[test]
    fn quoted_body_trims_user_body_trailing_newlines() {
        let o = orig(
            "From: Alice <alice@example.com>\r\n\
             Subject: t2\r\n\
             \r\n\
             Only line.\r\n",
        );
        assert_eq!(
            quoted_body("hi\n\n\n", &o),
            "hi\n\nAlice <alice@example.com> wrote:\n> Only line.",
            "the user body's trailing newlines are trimmed before the blank separator"
        );
    }

    // ---------- ReplyReport shape (REPLY-PLAN §5.5 --json contract) ----------

    #[test]
    fn reply_report_serializes_the_json_shape() {
        let r = ReplyReport {
            folder: "INBOX".to_string(),
            uid: 4821,
            message_id: Some("<9f0c@example.com>".to_string()),
            to: vec!["Alice <alice@example.com>".to_string()],
            cc: vec![],
            bcc: vec![],
            subject: "Re: Quarterly numbers".to_string(),
            in_reply_to: Some("<orig-123@example.com>".to_string()),
            references: vec![
                "<root-1@example.com>".to_string(),
                "<orig-123@example.com>".to_string(),
            ],
            saved_to: Some("Sent".to_string()),
            answered: true,
        };
        let v = serde_json::to_value(&r).expect("ReplyReport must serialize");
        assert_eq!(v["folder"], "INBOX");
        assert_eq!(v["uid"], 4821);
        assert_eq!(v["message_id"], "<9f0c@example.com>");
        assert_eq!(v["to"][0], "Alice <alice@example.com>");
        assert!(v["cc"].as_array().unwrap().is_empty());
        assert_eq!(v["subject"], "Re: Quarterly numbers");
        assert_eq!(v["in_reply_to"], "<orig-123@example.com>");
        assert_eq!(v["references"][1], "<orig-123@example.com>");
        assert_eq!(v["saved_to"], "Sent");
        assert_eq!(v["answered"], true);
    }

    #[test]
    fn reply_report_nulls_are_serde_none() {
        let r = ReplyReport {
            folder: "INBOX".to_string(),
            uid: 1,
            message_id: None,
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "Re:".to_string(),
            in_reply_to: None,
            references: vec![],
            saved_to: None,
            answered: false,
        };
        let v = serde_json::to_value(&r).expect("ReplyReport must serialize");
        assert!(v["message_id"].is_null());
        assert!(v["in_reply_to"].is_null());
        assert!(v["saved_to"].is_null());
        assert_eq!(v["answered"], false);
    }
}
