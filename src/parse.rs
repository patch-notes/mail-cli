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

/// Parse raw bytes into a `mail_parser::Message`, with context on failure.
fn parse(raw: &[u8]) -> Result<Message<'_>> {
    MessageParser::default()
        .parse(raw)
        .context("failed to parse message (malformed MIME)")
}

/// Join address-list headers into a display string like `Name <a@b>, c@d`.
fn addrs(list: Option<&mail_parser::Address<'_>>) -> String {
    let Some(addr) = list else { return String::new() };
    let mut out = Vec::new();
    for a in addr.iter() {
        let email = a.address().unwrap_or_default();
        if email.is_empty() {
            continue;
        }
        match a.name() {
            Some(name) if !name.is_empty() => out.push(format!("{name} <{email}>")),
            _ => out.push(email.to_string()),
        }
    }
    out.join(", ")
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
                filename: part
                    .attachment_name()
                    .unwrap_or("unnamed")
                    .to_string(),
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

/// Render a full message for `read`.
pub fn render(uid: u32, raw: &[u8], kind: BodyKind) -> Result<Rendered> {
    let msg = parse(raw)?;
    let body = match kind {
        BodyKind::Text => msg
            .body_text(0)
            .map(|c| c.into_owned())
            .or_else(|| msg.body_html(0).map(|c| c.into_owned()))
            .unwrap_or_default(),
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
}
