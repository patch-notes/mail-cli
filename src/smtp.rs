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
    let mut builder = Message::builder().from(from).subject(&msg.subject);
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
