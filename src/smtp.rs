//! SMTP module — build a `lettre` message (with attachments) and send it.
//! `lettre` builds the outgoing MIME; we never touch `mail-builder`.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{Message, SmtpTransport, Transport};

use crate::config::{AuthMethod, SmtpConfig, TlsMode};

/// Everything needed to compose one outgoing message.
pub struct Outgoing {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub html: bool,
    pub attachments: Vec<PathBuf>,
    /// `"<id>"` of the parent, or `None` (send, or parent had no Message-ID).
    pub in_reply_to: Option<String>,
    /// `"<id>"` list for References; empty = header omitted.
    pub references: Vec<String>,
}

/// Parse a comma-or-single address into a lettre `Mailbox`.
fn mailbox(addr: &str) -> Result<Mailbox> {
    addr.trim()
        .parse::<Mailbox>()
        .with_context(|| format!("invalid email address: {addr:?}"))
}

fn add_recipients(
    mut builder: lettre::message::MessageBuilder,
    to: &[String],
    cc: &[String],
    bcc: &[String],
) -> Result<lettre::message::MessageBuilder> {
    for a in to {
        builder = builder.to(mailbox(a)?);
    }
    for a in cc {
        builder = builder.cc(mailbox(a)?);
    }
    for a in bcc {
        builder = builder.bcc(mailbox(a)?);
    }
    Ok(builder)
}

/// Read a file into a lettre `SinglePart` attachment, guessing the MIME type.
fn build_attachment(path: &PathBuf) -> Result<SinglePart> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read attachment {path:?}"))?;
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("attachment path has no filename: {path:?}"))?;
    let mime = mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream");
    let content_type = ContentType::parse(mime)
        .unwrap_or_else(|_| ContentType::parse("application/octet-stream").unwrap());
    Ok(Attachment::new(filename).body(bytes, content_type))
}

/// Build the `lettre::Message` from config + outgoing spec.
pub fn build(cfg: &SmtpConfig, msg: &Outgoing) -> Result<Message> {
    let from = mailbox(&cfg.email)?;
    // Always emit a client-side Message-ID (lettre generates <uuid@hostname>).
    // Without one, the Sent copy and the relay-stamped delivered copy can't be
    // threaded together (REPLY-PLAN §8 R2).
    let mut builder = Message::builder()
        .from(from)
        .subject(&msg.subject)
        .message_id(None);
    if let Some(id) = &msg.in_reply_to {
        builder = builder.in_reply_to(id.clone());
    }
    if !msg.references.is_empty() {
        builder = builder.references(msg.references.join(" "));
    }
    builder = add_recipients(builder, &msg.to, &msg.cc, &msg.bcc)?;

    let body_type = if msg.html {
        ContentType::TEXT_HTML
    } else {
        ContentType::TEXT_PLAIN
    };
    let body_part = SinglePart::builder()
        .header(body_type)
        .body(msg.body.clone());

    let email = if msg.attachments.is_empty() {
        builder
            .singlepart(body_part)
            .context("failed to build message")?
    } else {
        let mut multipart = MultiPart::mixed().singlepart(body_part);
        for path in &msg.attachments {
            multipart = multipart.singlepart(build_attachment(path)?);
        }
        builder
            .multipart(multipart)
            .context("failed to build message")?
    };
    Ok(email)
}

/// Build the transport, honoring TLS mode, port, and auth method.
fn transport(cfg: &SmtpConfig) -> Result<SmtpTransport> {
    let builder = match cfg.tls {
        TlsMode::Implicit => SmtpTransport::relay(&cfg.host),
        TlsMode::StartTls => SmtpTransport::starttls_relay(&cfg.host),
    }
    .with_context(|| format!("failed to set up SMTP transport for {}", cfg.host))?;

    let creds = Credentials::new(cfg.user.clone(), cfg.password.clone());
    let mechanisms = match cfg.auth {
        AuthMethod::Login => vec![Mechanism::Plain, Mechanism::Login],
        AuthMethod::Xoauth2 => vec![Mechanism::Xoauth2],
    };
    Ok(builder
        .port(cfg.port)
        .credentials(creds)
        .authentication(mechanisms)
        .build())
}

/// Send a composed message.
pub fn send(cfg: &SmtpConfig, email: &Message) -> Result<()> {
    let mailer = transport(cfg)?;
    mailer
        .send(email)
        .map(|_| ())
        .with_context(|| format!("failed to send message via {}:{}", cfg.host, cfg.port))
}

/// The exact RFC822 bytes of a built message — the same payload `send` puts on
/// the wire. Used to file an identical copy into the Sent folder via IMAP.
pub fn formatted(email: &Message) -> Vec<u8> {
    email.formatted()
}

/// The bare address of the account (`MAILCLI_EMAIL`), for "me" matching in
/// `reply`. A display-name value (`"Name <a@b>"`) yields `a@b`.
pub fn account_address(cfg: &SmtpConfig) -> Result<String> {
    Ok(mailbox(&cfg.email)?.email.to_string())
}

/// Render one recipient as the display string `Outgoing` takes:
/// `Name <a@b>` (the name quoted as needed) via lettre's own `Mailbox`
/// `Display`, so `smtp::mailbox` parses it back losslessly.
pub fn mailbox_string(name: Option<&str>, email: &str) -> Result<String> {
    let address = email
        .parse()
        .with_context(|| format!("invalid email address: {email:?}"))?;
    Ok(Mailbox::new(name.map(str::to_owned), address).to_string())
}

/// The client-side Message-ID (`"<id>"`) of a built message, if any.
pub fn message_id(email: &Message) -> Option<String> {
    email
        .headers()
        .get_raw("Message-ID")
        .map(|v| v.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed offline config; host/port are never dialed by `build`.
    fn test_cfg() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.example.com".to_string(),
            port: 465,
            user: "me@example.com".to_string(),
            password: "pw".to_string(),
            email: "me@example.com".to_string(),
            tls: TlsMode::Implicit,
            auth: AuthMethod::Login,
        }
    }

    fn test_outgoing() -> Outgoing {
        Outgoing {
            to: vec!["alice@example.com".to_string()],
            cc: vec![],
            bcc: vec![],
            subject: "Subject line".to_string(),
            body: "hello".to_string(),
            html: false,
            attachments: vec![],
            in_reply_to: None,
            references: vec![],
        }
    }

    /// The value of the first Message-ID header in formatted bytes, if any.
    fn message_id_of(raw: &[u8]) -> Option<String> {
        String::from_utf8_lossy(raw)
            .lines()
            .find_map(|line| line.strip_prefix("Message-ID:"))
            .map(|value| value.trim().to_string())
    }

    fn count_message_id_headers(raw: &[u8]) -> usize {
        String::from_utf8_lossy(raw)
            .lines()
            .filter(|line| line.starts_with("Message-ID:"))
            .count()
    }

    /// R2 (REPLY-PLAN §8): `smtp::build` must always emit exactly one
    /// angle-bracketed Message-ID header. Without one, the Sent copy and the
    /// relay-stamped delivered copy can't be threaded together.
    #[test]
    fn build_emits_exactly_one_angle_bracketed_message_id() {
        let raw = formatted(&build(&test_cfg(), &test_outgoing()).expect("smtp::build"));
        let text = String::from_utf8_lossy(&raw);
        assert_eq!(
            count_message_id_headers(&raw),
            1,
            "built message must carry exactly one Message-ID header; got:\n{text}"
        );
        let id = message_id_of(&raw).expect("Message-ID header present");
        let inner = id
            .strip_prefix('<')
            .and_then(|v| v.strip_suffix('>'))
            .unwrap_or("");
        assert!(
            !inner.is_empty() && inner.contains('@'),
            "Message-ID must be in <random@domain> form, got {id:?}"
        );
    }

    /// R2 (REPLY-PLAN §8): each build must generate a fresh Message-ID, so two
    /// sends are never conflated in a thread.
    #[test]
    fn two_builds_produce_distinct_message_ids() {
        let first = formatted(&build(&test_cfg(), &test_outgoing()).expect("smtp::build"));
        let second = formatted(&build(&test_cfg(), &test_outgoing()).expect("smtp::build"));
        let id_first = message_id_of(&first).expect("first build has a Message-ID");
        let id_second = message_id_of(&second).expect("second build has a Message-ID");
        assert_ne!(
            id_first, id_second,
            "two builds must produce distinct Message-IDs, both were {id_first:?}"
        );
    }

    // ---- threading headers in build (REPLY-PLAN T1, §4.3, R4) ----

    /// Unfolded header value (continuation lines joined) for `name`, if present.
    fn header_value(raw: &[u8], name: &str) -> Option<String> {
        let text = String::from_utf8_lossy(raw);
        let prefix = format!("{name}:");
        let mut lines = text.split("\r\n").take_while(|l| !l.is_empty()).peekable();
        while let Some(line) = lines.next() {
            if let Some(rest) = line.strip_prefix(&prefix) {
                let mut value = rest.trim().to_string();
                while let Some(next) = lines.peek() {
                    if next.starts_with(' ') || next.starts_with('\t') {
                        value.push(' ');
                        value.push_str(next.trim());
                        lines.next();
                    } else {
                        break;
                    }
                }
                return Some(value);
            }
        }
        None
    }

    #[test]
    fn build_emits_in_reply_to_and_references_when_set() {
        let outgoing = Outgoing {
            in_reply_to: Some("<orig@example.com>".to_string()),
            references: vec![
                "<root@example.com>".to_string(),
                "<orig@example.com>".to_string(),
            ],
            ..test_outgoing()
        };
        let raw = formatted(&build(&test_cfg(), &outgoing).expect("smtp::build"));
        assert_eq!(
            header_value(&raw, "In-Reply-To").as_deref(),
            Some("<orig@example.com>"),
            "In-Reply-To must carry the bracketed parent id"
        );
        assert_eq!(
            header_value(&raw, "References").as_deref(),
            Some("<root@example.com> <orig@example.com>"),
            "References must be the space-joined bracketed ids, in order"
        );
    }

    #[test]
    fn build_omits_threading_headers_for_a_plain_send() {
        let raw = formatted(&build(&test_cfg(), &test_outgoing()).expect("smtp::build"));
        assert_eq!(
            header_value(&raw, "In-Reply-To"),
            None,
            "send has no parent"
        );
        assert_eq!(header_value(&raw, "References"), None, "send has no parent");
    }

    #[test]
    fn build_emits_references_without_in_reply_to_when_parent_had_no_id() {
        let outgoing = Outgoing {
            in_reply_to: None,
            references: vec!["<root@example.com>".to_string()],
            ..test_outgoing()
        };
        let raw = formatted(&build(&test_cfg(), &outgoing).expect("smtp::build"));
        assert_eq!(header_value(&raw, "In-Reply-To"), None);
        assert_eq!(
            header_value(&raw, "References").as_deref(),
            Some("<root@example.com>")
        );
    }

    /// R4 (REPLY-PLAN §8): 50 References ids must be folded; no physical line
    /// may exceed the RFC 5322 hard limit of 998 bytes.
    #[test]
    fn build_folds_a_long_references_header_under_998_bytes() {
        let refs: Vec<String> = (0..50)
            .map(|i| format!("<ref-{i:02}-0123456789abcdef0123456789abcdef@example.com>"))
            .collect();
        let outgoing = Outgoing {
            in_reply_to: refs.last().cloned(),
            references: refs.clone(),
            ..test_outgoing()
        };
        let raw = formatted(&build(&test_cfg(), &outgoing).expect("smtp::build"));
        let text = String::from_utf8_lossy(&raw);
        let longest = text.split("\r\n").map(str::len).max().unwrap_or(0);
        assert!(
            longest <= 998,
            "no line may exceed 998 bytes; longest was {longest}"
        );
        assert_eq!(
            header_value(&raw, "References").map(|v| v.split(' ').count()),
            Some(50),
            "all 50 ids must survive folding"
        );
    }

    // ---- reply helpers (REPLY-PLAN T2, §6.2) ----

    /// A display-name `MAILCLI_EMAIL` yields the bare address for "me" matching.
    #[test]
    fn account_address_strips_display_name() {
        let cfg = SmtpConfig {
            email: "Me <me@example.com>".to_string(),
            ..test_cfg()
        };
        assert_eq!(
            account_address(&cfg).expect("account_address"),
            "me@example.com",
            "account_address is the bare address of MAILCLI_EMAIL"
        );
    }

    /// `mailbox_string` output must parse back into a `Mailbox` losslessly and
    /// survive a real `build` (the round trip REPLY-PLAN §6.2 locks).
    #[test]
    fn mailbox_string_round_trips_through_mailbox_and_build() {
        let s = mailbox_string(Some("Doe, John"), "john@example.com").expect("mailbox_string");
        let mb: Mailbox = s
            .parse()
            .expect("mailbox_string output must parse back as a Mailbox");
        assert_eq!(mb.email.to_string(), "john@example.com");
        assert_eq!(mb.name.as_deref(), Some("Doe, John"));

        let outgoing = Outgoing {
            to: vec![s],
            cc: vec![],
            bcc: vec![],
            subject: "S".to_string(),
            body: "b".to_string(),
            html: false,
            attachments: vec![],
            in_reply_to: None,
            references: vec![],
        };
        let bytes = formatted(&build(&test_cfg(), &outgoing).expect("build"));
        let raw = String::from_utf8_lossy(&bytes);
        assert!(
            raw.contains("john@example.com"),
            "a built message must carry the recipient; got:\n{raw}"
        );
    }

    /// Non-atom display names (commas, quotes, non-ASCII) must round-trip
    /// through lettre's own quoting (REPLY-PLAN T2).
    #[test]
    fn mailbox_string_quotes_non_atom_names() {
        for name in [Some("Doe, John"), Some("Zoë \"Z\""), None] {
            let s = mailbox_string(name, "zoe@example.com").expect("mailbox_string");
            let mb: Mailbox = s
                .parse()
                .expect("mailbox_string output must parse back as a Mailbox: {s:?}");
            assert_eq!(mb.email.to_string(), "zoe@example.com", "for name {name:?}");
            assert_eq!(mb.name.as_deref(), name, "for name {name:?}");
        }
    }

    /// The Message-ID helper must return the bracketed id the builder emitted.
    #[test]
    fn message_id_helper_matches_formatted_bytes() {
        let email = build(&test_cfg(), &test_outgoing()).expect("smtp::build");
        let id = message_id(&email).expect("message_id");
        assert!(
            id.starts_with('<') && id.ends_with('>'),
            "Message-ID must be angle-bracketed, got {id:?}"
        );
        let raw = formatted(&email);
        assert!(
            raw.windows(id.len()).any(|w| w == id.as_bytes()),
            "the formatted bytes must carry the exact Message-ID the helper returns"
        );
    }
}
