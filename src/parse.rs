//! Parsing wrappers around `mail-parser`.
//!
//! The wire boundary parses raw RFC822 bytes exactly once into small, owned,
//! `Serialize`-able structs. Nothing downstream re-parses; we trust the structs.

use anyhow::{Context, Result};
use mail_parser::{Message, MessageParser, MimeHeaders};
use serde::Serialize;

/// A one-line summary of a message, for `list`.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Server-assigned stable UID.
    pub uid: u32,
    pub from: String,
    pub subject: String,
    /// RFC3339 date, or empty if the message had none.
    pub date: String,
    pub flags: Vec<String>,
    pub has_attachments: bool,
}

/// A rendered, single message for `read`.
#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: String,
    pub body: String,
    pub attachments: Vec<Attachment>,
}

/// One attachment's metadata (and, when downloading, its bytes).
#[derive(Debug, Clone, Serialize)]
pub struct Attachment {
    /// 1-based index within the message (the `K` in commands).
    pub index: usize,
    pub filename: String,
    pub mime: String,
    pub size: usize,
    /// Decoded bytes. Skipped in JSON/listing output.
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

/// One parsed address. Invariant: `email` is non-empty (empties are dropped at
/// parse time, same rule `addrs()` already applies). `name` is None or non-empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addr {
    /// Display name, if the header had one.
    pub name: Option<String>,
    /// The bare email address (non-empty).
    pub email: String,
}

/// What a reply needs from its parent, extracted once from the raw bytes.
/// Message ids are BARE (no angle brackets; mail-parser strips them).
#[derive(Debug, Clone)]
pub struct Original {
    /// Message-ID; `None` if absent/empty.
    pub message_id: Option<String>,
    /// The parent's In-Reply-To ids (may be empty).
    pub in_reply_to: Vec<String>,
    /// The parent's References ids (may be empty).
    pub references: Vec<String>,
    pub reply_to: Vec<Addr>,
    pub from: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    /// Raw Subject, `""` if absent (NOT `"(no subject)"`).
    // used by cmd_reply (T7)
    #[allow(dead_code)]
    pub subject: String,
    /// `DateTime::to_rfc822()`, e.g. `"Sat, 20 Nov 2021 14:22:01 -0800"`.
    pub date: Option<String>,
    /// `text_body()`: the same text `read` would show, `""` if none.
    pub text: String,
}

/// Parse raw bytes into a `mail_parser::Message`, with context on failure.
fn parse(raw: &[u8]) -> Result<Message<'_>> {
    MessageParser::default()
        .parse(raw)
        .context("failed to parse message (malformed MIME)")
}

/// Parse an address-list header into owned `Addr`s. Entries with an empty
/// address are dropped here, so every `Addr.email` is non-empty downstream.
fn addr_list(list: Option<&mail_parser::Address<'_>>) -> Vec<Addr> {
    list.into_iter()
        .flat_map(|l| l.iter())
        .filter_map(|a| {
            let email = a.address().unwrap_or_default();
            (!email.is_empty()).then(|| Addr {
                name: a.name().filter(|n| !n.is_empty()).map(str::to_string),
                email: email.to_string(),
            })
        })
        .collect()
}

/// One address as `Name <a@b>` (or the bare address when it has no name).
pub fn display(a: &Addr) -> String {
    match &a.name {
        Some(name) => format!("{name} <{}>", a.email),
        None => a.email.clone(),
    }
}

/// Join address-list headers into a display string like `Name <a@b>, c@d`.
fn addrs(list: Option<&mail_parser::Address<'_>>) -> String {
    addr_list(list)
        .iter()
        .map(display)
        .collect::<Vec<_>>()
        .join(", ")
}

fn first_from(msg: &Message<'_>) -> String {
    addrs(msg.from())
}

fn subject_of(msg: &Message<'_>) -> String {
    msg.subject().unwrap_or("(no subject)").to_string()
}

fn date_of(msg: &Message<'_>) -> String {
    msg.date().map(|d| d.to_rfc3339()).unwrap_or_default()
}

/// Build a listing `Summary` from raw bytes plus server-side UID and flags.
pub fn summarize(uid: u32, flags: Vec<String>, raw: &[u8]) -> Result<Summary> {
    let msg = parse(raw)?;
    Ok(Summary {
        uid,
        from: first_from(&msg),
        subject: subject_of(&msg),
        date: date_of(&msg),
        flags,
        has_attachments: msg.attachment_count() > 0,
    })
}

/// Extract every attachment's metadata and decoded bytes.
pub fn attachments(raw: &[u8]) -> Result<Vec<Attachment>> {
    let msg = parse(raw)?;
    Ok(collect_attachments(&msg))
}

fn collect_attachments(msg: &Message<'_>) -> Vec<Attachment> {
    msg.attachments()
        .enumerate()
        .map(|(i, part)| {
            let bytes = part.contents().to_vec();
            Attachment {
                index: i + 1,
                filename: part.attachment_name().unwrap_or("unnamed").to_string(),
                mime: part
                    .content_type()
                    .map(|ct| match ct.subtype() {
                        Some(sub) => format!("{}/{}", ct.ctype(), sub),
                        None => ct.ctype().to_string(),
                    })
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                size: bytes.len(),
                bytes,
            }
        })
        .collect()
}

/// Which body flavour `read` should render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// Prefer text; down-convert HTML when no text part exists.
    Text,
    /// The raw HTML part (as-is).
    Html,
}

/// The text `read` shows by default: the text body, else the HTML part
/// down-converted. Shared with `original` so a reply quotes exactly this.
fn text_body(msg: &Message<'_>) -> String {
    msg.body_text(0)
        .map(|c| c.into_owned())
        .or_else(|| msg.body_html(0).map(|c| c.into_owned()))
        .unwrap_or_default()
}

/// Parse a parent message once into what `reply` derives from (REPLY-PLAN §4.1).
// used by cmd_reply (T7)
#[allow(dead_code)]
pub fn original(raw: &[u8]) -> Result<Original> {
    let msg = parse(raw)?;
    let ids = |v: &mail_parser::HeaderValue<'_>| -> Vec<String> {
        v.as_text_list()
            .map(|list| {
                list.iter()
                    .filter(|id| !id.is_empty())
                    .map(|id| id.to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(Original {
        message_id: msg
            .message_id()
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        in_reply_to: ids(msg.in_reply_to()),
        references: ids(msg.references()),
        reply_to: addr_list(msg.reply_to()),
        from: addr_list(msg.from()),
        to: addr_list(msg.to()),
        cc: addr_list(msg.cc()),
        subject: msg.subject().unwrap_or_default().to_string(),
        date: msg.date().map(|d| d.to_rfc822()),
        text: text_body(&msg),
    })
}

/// Render a full message for `read`.
pub fn render(uid: u32, raw: &[u8], kind: BodyKind) -> Result<Rendered> {
    let msg = parse(raw)?;
    let body = match kind {
        BodyKind::Text => text_body(&msg),
        BodyKind::Html => msg
            .body_html(0)
            .map(|c| c.into_owned())
            .or_else(|| msg.body_text(0).map(|c| c.into_owned()))
            .unwrap_or_default(),
    };
    Ok(Rendered {
        uid,
        from: first_from(&msg),
        to: addrs(msg.to()),
        cc: addrs(msg.cc()),
        subject: subject_of(&msg),
        date: date_of(&msg),
        body,
        attachments: collect_attachments(&msg),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT_ONLY: &[u8] = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: Hello\r\n\
Date: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
\r\n\
Just a plain body.\r\n";

    const WITH_ATTACHMENT: &[u8] = b"From: Alice <alice@example.com>\r\n\
Subject: Report\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
See attached.\r\n\
--b\r\n\
Content-Type: text/plain; name=\"note.txt\"\r\n\
Content-Disposition: attachment; filename=\"note.txt\"\r\n\
\r\n\
hi\r\n\
--b--\r\n";

    #[test]
    fn summarize_text_only() {
        let s = summarize(42, vec!["\\Seen".into()], TEXT_ONLY).unwrap();
        assert_eq!(s.uid, 42);
        assert_eq!(s.subject, "Hello");
        assert!(s.from.contains("alice@example.com"));
        assert!(!s.has_attachments);
        assert!(s.date.starts_with("2021-11-20"));
    }

    #[test]
    fn render_text_body() {
        let r = render(1, TEXT_ONLY, BodyKind::Text).unwrap();
        assert!(r.body.contains("plain body"));
        assert!(r.to.contains("bob@example.com"));
        assert!(r.attachments.is_empty());
    }

    #[test]
    fn attachments_are_extracted() {
        let s = summarize(1, vec![], WITH_ATTACHMENT).unwrap();
        assert!(s.has_attachments);
        let att = attachments(WITH_ATTACHMENT).unwrap();
        assert_eq!(att.len(), 1);
        assert_eq!(att[0].index, 1);
        assert_eq!(att[0].filename, "note.txt");
        assert_eq!(att[0].bytes, b"hi");
    }

    #[test]
    fn missing_subject_has_placeholder() {
        let raw = b"From: a@b.com\r\n\r\nbody\r\n";
        let s = summarize(1, vec![], raw).unwrap();
        assert_eq!(s.subject, "(no subject)");
    }

    // ---- original(): the reply parse boundary (REPLY-PLAN §4.1, T3) ----

    const FULL_HEADERS: &[u8] = b"Message-ID: <orig-1@example.com>\r\n\
References: <root-1@example.com>\r\n\
\x20<parent-1@example.com>\r\n\
In-Reply-To: <parent-1@example.com>\r\n\
Reply-To: List <list@example.com>\r\n\
From: Alice <alice@example.com>\r\n\
To: Me <me@example.com>, Carol <carol@example.com>\r\n\
Cc: Dave <dave@example.com>\r\n\
Subject: Weekly sync\r\n\
Date: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
\r\n\
Line one.\r\n\
\r\n\
Line three.\r\n";

    #[test]
    fn original_extracts_full_headers() {
        let o = original(FULL_HEADERS).expect("original");
        assert_eq!(
            o.message_id.as_deref(),
            Some("orig-1@example.com"),
            "Message-ID must be BARE (mail-parser strips the brackets)"
        );
        assert_eq!(
            o.references,
            vec!["root-1@example.com", "parent-1@example.com"],
            "a folded (multi-line) References must be split into bare ids"
        );
        assert_eq!(o.in_reply_to, vec!["parent-1@example.com"]);
        assert_eq!(
            o.reply_to,
            vec![Addr {
                name: Some("List".to_string()),
                email: "list@example.com".to_string()
            }]
        );
        assert_eq!(
            o.from,
            vec![Addr {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string()
            }]
        );
        assert_eq!(
            o.to,
            vec![
                Addr {
                    name: Some("Me".to_string()),
                    email: "me@example.com".to_string()
                },
                Addr {
                    name: Some("Carol".to_string()),
                    email: "carol@example.com".to_string()
                }
            ]
        );
        assert_eq!(
            o.cc,
            vec![Addr {
                name: Some("Dave".to_string()),
                email: "dave@example.com".to_string()
            }]
        );
        assert_eq!(o.subject, "Weekly sync");
        assert_eq!(
            o.date.as_deref(),
            Some("Sat, 20 Nov 2021 14:22:01 -0800"),
            "date is DateTime::to_rfc822()"
        );
        assert!(
            o.text.contains("Line one.") && o.text.contains("Line three."),
            "text is what read would show; got {o:?}"
        );
    }

    #[test]
    fn original_without_thread_headers_is_empty() {
        let o = original(b"From: a@b.com\r\nSubject: x\r\n\r\nbody\r\n").expect("original");
        assert_eq!(o.message_id, None);
        assert!(o.in_reply_to.is_empty());
        assert!(o.references.is_empty());
        assert!(o.reply_to.is_empty());
        assert_eq!(
            o.from,
            vec![Addr {
                name: None,
                email: "a@b.com".to_string()
            }]
        );
    }

    #[test]
    fn original_missing_subject_is_empty_not_placeholder() {
        let o = original(b"From: a@b.com\r\n\r\nbody\r\n").expect("original");
        assert_eq!(
            o.subject, "",
            "Original.subject is the RAW subject (\"\" if absent), unlike Summary's placeholder"
        );
    }

    #[test]
    fn original_html_only_body_downconverts_to_text() {
        let raw = b"From: a@b.com\r\nSubject: h\r\nContent-Type: text/html\r\n\r\n<p>Hello &amp; welcome</p>\r\n";
        let o = original(raw).expect("original");
        assert!(
            o.text.contains("Hello"),
            "an HTML-only parent must yield its text form for quoting; got {o:?}"
        );
    }
}
