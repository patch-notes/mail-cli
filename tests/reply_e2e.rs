//! End-to-end test for the `reply` feature (REPLY-PLAN §12.2).
//!
//! Same honesty rule as `send_e2e.rs`: mail-cli's **real** code everywhere
//! (`imap::fetch_raw`, `parse::original`, the `reply::*` rules, `smtp::build`/
//! `formatted`, `imap::append_to_sent`, `imap::mark_answered`) — only the
//! encrypted transport hop is substituted (lettre's webpki-only SMTP stack
//! cannot validate GreenMail's self-signed cert, and the tool has no
//! accept-invalid-certs escape hatch by design). The byte-identical
//! `smtp::formatted()` payload goes over GreenMail's plaintext SMTP via
//! lettre's explicit dangerous transport (test-only).
//!
//! Gated on `MAILCLI_IT=1`. Run with GreenMail up:
//!
//! ```sh
//! podman run -d --name greenmail-mailcli -p 3025:3025 -p 3143:3143 \
//!   -e GREENMAIL_OPTS='-Dgreenmail.setup.test.smtp -Dgreenmail.setup.test.imap \
//!      -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.auth.disabled' \
//!   docker.io/greenmail/standalone:2.1.0
//! MAILCLI_IT=1 cargo test --test reply_e2e -- --nocapture --test-threads=1
//! ```

use std::sync::Mutex;

use imap::types::Flag;
use imap::{ConnectionMode, Session};
use lettre::{SmtpTransport, Transport};
use mail_cli::config::{AuthMethod, SmtpConfig, TlsMode};
use mail_cli::parse::original;
use mail_cli::reply;
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

/// A distinct user per test-run so mailboxes never collide (nanos + process id).
fn unique_user(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{tag}.{nanos}.{pid}@example.com", pid = std::process::id())
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

/// APPEND one raw RFC822 message to INBOX.
fn seed_raw(session: &mut Session<imap::Connection>, raw: &str) {
    session
        .append("INBOX", raw.as_bytes())
        .finish()
        .expect("APPEND to INBOX");
}

/// The UID of the single message in `folder` whose Subject matches `subject`
/// (GreenMail SUBJECT is substring; our subjects are unique per run).
fn uid_of_subject(session: &mut Session<imap::Connection>, folder: &str, subject: &str) -> u32 {
    session.select(folder).expect("SELECT");
    let hits = session
        .uid_search(format!("SUBJECT \"{subject}\""))
        .expect("UID SEARCH by subject");
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one message with subject {subject:?} in {folder}, got {hits:?}"
    );
    *hits.iter().next().unwrap()
}

/// The raw RFC822 bytes of the single message in `folder` matching `subject`,
/// via mail-cli's real `fetch_raw` (BODY.PEEK — no flag side effects).
fn raw_of_subject(session: &mut Session<imap::Connection>, folder: &str, subject: &str) -> Vec<u8> {
    let uid = uid_of_subject(session, folder, subject);
    mc_imap::fetch_raw(session, folder, uid).expect("fetch_raw")
}

/// Whether `uid` in `folder` carries `flag`.
fn has_flag(
    session: &mut Session<imap::Connection>,
    folder: &str,
    uid: u32,
    flag: Flag<'static>,
) -> bool {
    session.select(folder).expect("SELECT");
    let fetches = session
        .uid_fetch(uid.to_string(), "(FLAGS)")
        .expect("UID FETCH FLAGS");
    fetches
        .iter()
        .next()
        .expect("one fetch result")
        .flags()
        .contains(&flag)
}

/// The value of the first Message-ID header in raw bytes, if any.
fn message_id_of(raw: &[u8]) -> Option<String> {
    String::from_utf8_lossy(raw)
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID:"))
        .map(|value| value.trim().to_string())
}

/// The unfolded value of the first `name:` header in the raw bytes
/// (continuation lines joined with a single space, per RFC 5322 §2.2.3),
/// if present. Header folding is an RFC-equivalent transport artifact, so
/// assertions compare the unfolded value, not the physical lines.
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

/// Full reply-all loop: seed a threaded parent in `me`'s INBOX, derive the
/// reply with mail-cli's real pure rules, build it with the real builder,
/// deliver it, file the Sent copy, mark the parent \Answered — then assert
/// every recipient (and non-recipient) sees exactly what the plan promises.
#[test]
fn reply_all_lands_threaded_in_sent_parent_answered() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let me = unique_user("rply.me");
    let alice = unique_user("rply.alice");
    let carol = unique_user("rply.carol");

    let tag = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let subject = format!("thread-{tag}");
    let reply_subject = format!("Re: {subject}");
    let orig_id = format!("orig-{tag}@example.com");
    let root_id = format!("root-{tag}@example.com");

    // Seed `me`'s INBOX with a threaded parent from alice, cc'ing carol.
    let seed = format!(
        "From: Alice <{alice}>\r\n\
         To: Me <{me}>\r\n\
         Cc: Carol <{carol}>\r\n\
         Message-ID: <{orig_id}>\r\n\
         References: <{root_id}>\r\n\
         Subject: {subject}\r\n\
         Date: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
         \r\n\
         Original line one.\r\n\
         Original line two.\r\n"
    );
    let mut me_s = imap_session(&me);
    seed_raw(&mut me_s, &seed);
    let parent_uid = uid_of_subject(&mut me_s, "INBOX", &subject);

    // mail-cli's REAL reply derivation (REPLY-PLAN §5.2 steps 8-12).
    let raw = mc_imap::fetch_raw(&mut me_s, "INBOX", parent_uid).expect("fetch_raw (BODY.PEEK)");
    let orig = original(&raw).expect("parse::original on the seeded parent");

    let (to, cc) = reply::recipients(&orig, &me, true);
    assert!(
        !to.is_empty(),
        "reply-all to alice's message must produce a To list"
    );
    let reply_subject_derived = reply::subject(&orig.subject);
    assert_eq!(
        reply_subject_derived, reply_subject,
        "the subject must be Re:-prefixed exactly once"
    );
    let thread = reply::threading(&orig);
    assert_eq!(
        thread.in_reply_to.as_deref(),
        Some(format!("<{orig_id}>").as_str()),
        "In-Reply-To must be the parent's bracketed Message-ID"
    );
    let body = reply::quoted_body("My reply body.", &orig);
    assert!(
        body.starts_with("My reply body."),
        "top-post: the user text goes first; got: {body}"
    );

    // Addresses to display strings via the real helper, then the real builder.
    let to_strs = to
        .iter()
        .map(|a| smtp::mailbox_string(a.name.as_deref(), &a.email).expect("mailbox_string"))
        .collect::<Vec<_>>();
    let cc_strs = cc
        .iter()
        .map(|a| smtp::mailbox_string(a.name.as_deref(), &a.email).expect("mailbox_string"))
        .collect::<Vec<_>>();

    let cfg = SmtpConfig {
        host: HOST.to_string(),
        port: SMTP_PORT,
        user: me.clone(),
        password: "pw".to_string(),
        email: me.clone(),
        tls: TlsMode::Implicit,
        auth: AuthMethod::Login,
    };
    let outgoing = smtp::Outgoing {
        to: to_strs,
        cc: cc_strs,
        bcc: vec![],
        subject: reply_subject_derived,
        body,
        html: false,
        attachments: vec![],
        in_reply_to: thread.in_reply_to.clone(),
        references: thread.references.clone(),
    };
    let email = smtp::build(&cfg, &outgoing).expect("smtp::build");
    let raw_out = smtp::formatted(&email);

    // Deliver over GreenMail's plaintext SMTP (the one substituted hop).
    let transport = SmtpTransport::builder_dangerous(HOST)
        .port(SMTP_PORT)
        .build();
    transport.send(&email).expect("SMTP delivery to GreenMail");

    // mail-cli's REAL side effects: Sent copy first, then \Answered.
    let _ = me_s.create("Sent");
    mc_imap::append_to_sent(&mut me_s, "Sent", &raw_out).expect("append_to_sent");
    mc_imap::mark_answered(&mut me_s, "INBOX", parent_uid).expect("mark_answered");

    // alice: exactly one reply, threaded and quoted.
    let mut alice_s = imap_session(&alice);
    let alice_raw = raw_of_subject(&mut alice_s, "INBOX", &reply_subject);
    let alice_text = String::from_utf8_lossy(&alice_raw);
    assert!(
        alice_text.contains(&format!("In-Reply-To: <{orig_id}>")),
        "the delivered reply must carry In-Reply-To: {alice_text}"
    );
    assert_eq!(
        header_value(&alice_raw, "References").as_deref(),
        Some(format!("<{root_id}> <{orig_id}>").as_str()),
        "the delivered reply must carry References (unfolded): root, then parent: {alice_text}"
    );
    assert!(
        alice_text.contains("> Original line one."),
        "the quoted parent line must be in the body: {alice_text}"
    );
    assert!(
        header_value(&alice_raw, "To")
            .map(|v| v.contains(&format!("Alice <{alice}>")))
            .unwrap_or(false),
        "alice must be in To (unfolded): {alice_text}"
    );

    // carol: got it via Cc.
    let mut carol_s = imap_session(&carol);
    let carol_raw = raw_of_subject(&mut carol_s, "INBOX", &reply_subject);
    let carol_text = String::from_utf8_lossy(&carol_raw);
    assert!(
        carol_text.contains(&format!("Cc: Carol <{carol}>")),
        "carol must be in Cc (reply-all): {carol_text}"
    );

    // me: did NOT get it (self excluded from the computed recipients).
    me_s.select("INBOX").expect("SELECT INBOX");
    let self_hits = me_s
        .uid_search(format!("SUBJECT \"{reply_subject}\""))
        .expect("UID SEARCH in me's INBOX");
    assert!(
        self_hits.is_empty(),
        "the replier must not receive their own reply (self excluded); hits: {self_hits:?}"
    );

    // Sent: copy flagged \Seen, carrying the same Message-ID as the delivered one.
    let sent_uid = uid_of_subject(&mut me_s, "Sent", &reply_subject);
    assert!(
        has_flag(&mut me_s, "Sent", sent_uid, Flag::Seen),
        "the Sent copy must be flagged \\Seen"
    );
    let sent_raw = mc_imap::fetch_raw(&mut me_s, "Sent", sent_uid).expect("fetch Sent copy");
    let built_id = message_id_of(&raw_out).expect("built payload has a Message-ID");
    let sent_id = message_id_of(&sent_raw).expect("Sent copy has a Message-ID");
    let delivered_id = message_id_of(&alice_raw).expect("delivered copy has a Message-ID");
    assert_eq!(
        sent_id, built_id,
        "Sent copy and built payload must share the Message-ID"
    );
    assert_eq!(
        delivered_id, built_id,
        "delivered copy and built payload must share the Message-ID"
    );

    // The parent in me's INBOX is now \Answered.
    assert!(
        has_flag(&mut me_s, "INBOX", parent_uid, Flag::Answered),
        "the parent must be flagged \\Answered"
    );

    let _ = me_s.logout();
}

/// `mark_answered` is idempotent: calling it twice on the same message must
/// not error, and the message ends up with exactly the \Answered flag.
#[test]
fn mark_answered_is_idempotent() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("answ");
    let mut s = imap_session(&user);
    seed_raw(
        &mut s,
        "From: a@b.com\r\nSubject: answ-subject\r\n\r\nbody\r\n",
    );
    let uid = uid_of_subject(&mut s, "INBOX", "answ-subject");
    mc_imap::mark_answered(&mut s, "INBOX", uid).expect("first mark_answered");
    mc_imap::mark_answered(&mut s, "INBOX", uid)
        .expect("second mark_answered must not error (idempotent)");
    assert!(
        has_flag(&mut s, "INBOX", uid, Flag::Answered),
        "the message must carry \\Answered after two stores"
    );
    let _ = s.logout();
}
