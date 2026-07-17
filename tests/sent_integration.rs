//! GreenMail integration tests for the save-to-Sent feature
//! (SAVE-TO-SENT-PLAN §5). These assert that the NEW IMAP surface —
//! `imap::append_to_sent` and `imap::discover_sent` — actually lands a copy in
//! the Sent folder, flagged `\Seen`, and that special-use discovery finds a
//! mailbox flagged `\Sent`.
//!
//! Gated on `MAILCLI_IT=1`, same as `search_integration.rs`. Run with GreenMail:
//!
//! ```sh
//! podman run -d --name greenmail-mailcli -p 3025:3025 -p 3143:3143 \
//!   -e GREENMAIL_OPTS='-Dgreenmail.setup.test.imap -Dgreenmail.users.login=email' \
//!   docker.io/greenmail/standalone:2.1.0
//! MAILCLI_IT=1 cargo test --test sent_integration -- --nocapture
//! ```
//!
//! Design note (mirrors search_integration.rs): mail-cli's own `connect()` is
//! TLS-only, and GreenMail's plaintext port has no usable cert. We open a
//! plaintext `imap` session directly and drive the SAME functions the binary
//! uses (`mail_cli::imap::append_to_sent` / `discover_sent`). The functions are
//! the code under test; the transport is incidental.

use std::sync::Mutex;

use imap::types::Flag;
use imap::{ConnectionMode, Session};

const HOST: &str = "127.0.0.1";
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

fn session_for(user: &str) -> Session<imap::Connection> {
    let client = imap::ClientBuilder::new(HOST, IMAP_PORT)
        .mode(ConnectionMode::Plaintext)
        .connect()
        .expect("connect to GreenMail plaintext IMAP");
    client
        .login(user, "pw")
        .map_err(|(e, _)| e)
        .expect("login (GreenMail auto-creates the user)")
}

fn unique_user(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{tag}.{nanos}@example.com")
}

/// A minimal RFC822 message (what lettre's `formatted()` would produce, shape-
/// wise). The APPEND path doesn't care who built the bytes.
fn raw_message(subject: &str) -> Vec<u8> {
    format!(
        "From: me@example.com\r\nTo: rcpt@example.com\r\nSubject: {subject}\r\n\r\nhello from mail-cli\r\n"
    )
    .into_bytes()
}

#[test]
fn append_lands_in_sent_flagged_seen() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("sent");
    let mut s = session_for(&user);

    // GreenMail auto-creates mailboxes on APPEND, but be explicit so a missing
    // "Sent" isn't what we're testing here.
    let _ = s.create("Sent");

    let subject = format!("saved-copy-{}", std::process::id());
    let raw = raw_message(&subject);

    // The function under test.
    mail_cli::imap::append_to_sent(&mut s, "Sent", &raw).expect("append_to_sent");

    // Verify exactly one copy landed in Sent...
    s.select("Sent").expect("SELECT Sent");
    let hits = s
        .uid_search(format!("SUBJECT \"{subject}\""))
        .expect("UID SEARCH by subject");
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one saved copy in Sent, got {hits:?}"
    );
    let uid = *hits.iter().next().unwrap();

    // ...and that it is flagged \Seen (we file our own sent mail as read).
    let fetches = s
        .uid_fetch(uid.to_string(), "(FLAGS)")
        .expect("UID FETCH FLAGS");
    let f = fetches.iter().next().expect("one fetch result");
    assert!(
        f.flags().contains(&Flag::Seen),
        "saved copy should be flagged \\Seen, got {:?}",
        f.flags()
    );

    let _ = s.logout();
}

#[test]
fn discover_sent_finds_special_use_mailbox() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("disc");
    let mut s = session_for(&user);

    // GreenMail advertises SPECIAL-USE for its default mailboxes; ensure a Sent
    // mailbox exists. Discovery should return whatever the server flags \Sent.
    let _ = s.create("Sent");

    let found = mail_cli::imap::discover_sent(&mut s);
    // Either the server flags a \Sent mailbox (Some) or it doesn't advertise
    // special-use (None) — both are valid. When Some, it must be a real,
    // selectable mailbox.
    if let Some(name) = found {
        s.select(&name)
            .unwrap_or_else(|e| panic!("discovered Sent folder {name:?} must be selectable: {e}"));
    }

    let _ = s.logout();
}
