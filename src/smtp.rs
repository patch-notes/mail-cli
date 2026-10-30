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
}
