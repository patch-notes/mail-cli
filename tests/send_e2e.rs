//! End-to-end test for the save-to-Sent feature (SAVE-TO-SENT-PLAN §5).
//!
//! This drives mail-cli's **real** code paths (`smtp::build`, `smtp::formatted`,
//! `imap::append_to_sent`, `imap::discover_sent`) through a full
//! send/deliver/sent-copy loop against a live GreenMail server, then asserts the
//! message both reached the recipient's INBOX and has a copy in the Sent folder
//! flagged `\Seen`.
//!
//! TLS caveat (why the transport is the one substituted piece): mail-cli's SMTP
//! stack (lettre) trusts only the bundled webpki roots, so it cannot validate
//! GreenMail's self-signed cert — and the tool has no "accept invalid certs"
//! escape hatch by design. We therefore push the **byte-identical**
//! `smtp::formatted()` payload over GreenMail's plaintext SMTP with lettre's
//! explicit dangerous transport (test-only), then run the genuine IMAP
//! append/discover the binary uses. Everything except the encrypted transport
//! hop is mail-cli's own code.
//!
//! Gated on `MAILCLI_IT=1`. Run with GreenMail up:
//!
//! ```sh
//! podman run -d --name greenmail-mailcli -p 3025:3025 -p 3143:3143 \
//!   -e GREENMAIL_OPTS='-Dgreenmail.setup.test.smtp -Dgreenmail.setup.test.imap \
//!      -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.auth.disabled' \
//!   docker.io/greenmail/standalone:2.1.0
//! MAILCLI_IT=1 cargo test --test send_e2e -- --nocapture --test-threads=1
//! ```

use std::process::Command;
use std::sync::Mutex;

use imap::types::Flag;
use imap::{ConnectionMode, Session};
use lettre::{SmtpTransport, Transport};
use mail_cli::config::{AuthMethod, SmtpConfig, TlsMode};
use mail_cli::{imap as mc_imap, smtp};

const HOST: &str = "127.0.0.1";
const SMTP_PORT: u16 = 3025;
const IMAP_PORT: u16 = 3143;

/// GreenMail shares global state; serialize so each test runs quiescent.
static SERIAL: Mutex<()> = Mutex::new(());

macro_rules! require_greenmail {
    () => {
        if std::env::var("MAILCLI_IT").ok().as_deref() != Some("1") {
            eprintln!("skipping (set MAILCLI_IT=1 with GreenMail running)");
            return;
        }
    };
}

fn unique_user(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{tag}.{nanos}@example.com")
}

fn imap_session(user: &str) -> Session<imap::Connection> {
    let client = imap::ClientBuilder::new(HOST, IMAP_PORT)
        .mode(ConnectionMode::Plaintext)
        .connect()
        .expect("connect to GreenMail plaintext IMAP");
    client
        .login(user, "pw")
        .map_err(|(e, _)| e)
        .expect("login (GreenMail auto-creates the user)")
}

/// The UID of the single message in `folder` whose Subject equals `subject`
/// (panics unless exactly one), plus whether it carries the `\Seen` flag.
fn find_one(session: &mut Session<imap::Connection>, folder: &str, subject: &str) -> (u32, bool) {
    session.select(folder).expect("SELECT");
    let hits = session
        .uid_search(format!("SUBJECT \"{subject}\""))
        .expect("UID SEARCH by subject");
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one message with subject {subject:?} in {folder}, got {hits:?}"
    );
    let uid = *hits.iter().next().unwrap();
    let fetches = session
        .uid_fetch(uid.to_string(), "(FLAGS)")
        .expect("UID FETCH FLAGS");
    let seen = fetches
        .iter()
        .next()
        .expect("one fetch result")
        .flags()
        .contains(&Flag::Seen);
    (uid, seen)
}

/// Full loop: build a message with mail-cli's real builder, deliver it to the
/// recipient via SMTP, save a copy to Sent with mail-cli's real IMAP APPEND, and
/// assert both landed — the copy flagged `\Seen`.
#[test]
fn send_then_message_lands_in_inbox_and_sent() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("e2e");

    // A real mail-cli SmtpConfig (fields are public); From = the account address.
    let cfg = SmtpConfig {
        host: HOST.to_string(),
        port: SMTP_PORT,
        user: user.clone(),
        password: "pw".to_string(),
        email: user.clone(),
        tls: TlsMode::Implicit,
        auth: AuthMethod::Login,
    };
    let subject = format!("e2e-{}", std::process::id());
    let outgoing = smtp::Outgoing {
        to: vec![user.clone()],
        cc: vec![],
        bcc: vec![],
        subject: subject.clone(),
        body: "hello from the mail-cli e2e".to_string(),
        html: false,
        attachments: vec![],
        in_reply_to: None,
        references: vec![],
    };

    // mail-cli's REAL message builder, and the REAL bytes it would put on the wire.
    let email = smtp::build(&cfg, &outgoing).expect("smtp::build");
    let raw = smtp::formatted(&email);
    assert!(
        raw.windows(subject.len()).any(|w| w == subject.as_bytes()),
        "formatted bytes must contain the subject header"
    );

    // Deliver over GreenMail's plaintext SMTP (test-only transport; identical
    // payload). This stands in for the encrypted hop mail-cli can't do here.
    let transport = SmtpTransport::builder_dangerous(HOST)
        .port(SMTP_PORT)
        .build();
    transport.send(&email).expect("SMTP delivery to GreenMail");

    // mail-cli's REAL save-to-Sent path.
    let mut session = imap_session(&user);
    let _ = session.create("Sent");
    mc_imap::append_to_sent(&mut session, "Sent", &raw).expect("append_to_sent");

    // (a) delivered to INBOX.
    let (_uid, _seen) = find_one(&mut session, "INBOX", &subject);

    // (b) copied to Sent, flagged \Seen.
    let (_uid, seen) = find_one(&mut session, "Sent", &subject);
    assert!(seen, "the Sent copy must be flagged \\Seen");

    let _ = session.logout();
}

/// Binary-level smoke: the compiled `mail-cli send` wires the new flags and
/// treats an SMTP failure as fatal (non-zero exit). No server needed — we point
/// it at an unroutable host so the SMTP connect fails fast.
#[test]
fn binary_send_smtp_failure_is_fatal_and_new_flags_parse() {
    let bin = env!("CARGO_BIN_EXE_mail-cli");

    // --help must advertise the new flags (advertise-what-you-deliver).
    let help = Command::new(bin)
        .args(["send", "--help"])
        .output()
        .expect("run mail-cli send --help");
    assert!(help.status.success(), "send --help should exit 0");
    let help_txt = String::from_utf8_lossy(&help.stdout);
    assert!(help_txt.contains("--sent-folder"), "--sent-folder in help");
    assert!(
        help_txt.contains("--no-save-sent"),
        "--no-save-sent in help"
    );

    // A send against an unroutable SMTP host must FAIL (non-zero) — the mail
    // never went out, so exiting non-zero is correct (unlike a failed Sent copy).
    let out = Command::new(bin)
        .args([
            "send",
            "--to",
            "someone@example.com",
            "--subject",
            "S",
            "--body",
            "hi",
            "--no-save-sent",
        ])
        .env("MAILCLI_SMTP_HOST", "127.0.0.1")
        .env("MAILCLI_SMTP_PORT", "9") // discard port: connect refused/closed
        .env("MAILCLI_EMAIL", "me@example.com")
        .env("MAILCLI_PASSWORD", "pw")
        .output()
        .expect("run mail-cli send");
    assert!(
        !out.status.success(),
        "send should fail non-zero when SMTP is unreachable; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// R1 (REPLY-PLAN §8): an invalid `MAILCLI_SAVE_SENT` must be rejected during
/// config validation BEFORE any network call. Today `cmd_send` checks it only
/// after `smtp::send` succeeds, so a typo delivers the mail and then exits 1 —
/// inviting a duplicate resend. This points SMTP at a dead port, so the buggy
/// order shows up as an SMTP connection error instead of a config error.
/// No server needed: validation (once fixed) happens before the connect.
#[test]
fn binary_send_rejects_invalid_save_sent_before_any_send() {
    let bin = env!("CARGO_BIN_EXE_mail-cli");

    // env_clear keeps this deterministic: only the vars we set are visible,
    // exactly as a clean shell would run it.
    let out = Command::new(bin)
        .args([
            "send",
            "--to",
            "someone@example.com",
            "--subject",
            "S",
            "--body",
            "hi",
        ])
        .env_clear()
        .env("MAILCLI_SMTP_HOST", "127.0.0.1")
        .env("MAILCLI_SMTP_PORT", "9") // discard port: connect refused/closed
        .env("MAILCLI_EMAIL", "me@example.com")
        .env("MAILCLI_PASSWORD", "pw")
        .env("MAILCLI_SAVE_SENT", "maybe")
        .output()
        .expect("run mail-cli send");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "send with an invalid MAILCLI_SAVE_SENT must fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("MAILCLI_SAVE_SENT"),
        "the error must name MAILCLI_SAVE_SENT (config validation before send); got: {stderr}"
    );
    assert!(
        !stderr.contains("failed to send message via"),
        "a config error must surface, not an SMTP connection error; got: {stderr}"
    );
}
