//! GreenMail integration tests for the `search` feature (SEARCH-PLAN.md §7 / S3).
//!
//! These assert that the strings produced by `mail_cli::search::Criteria` are
//! (a) accepted by a real IMAP `SEARCH` implementation and (b) select exactly
//! the intended messages. They are **gated on `MAILCLI_IT=1`** so an ordinary
//! `cargo test` never needs a server; run the whole thing with:
//!
//! ```sh
//! podman run -d --name greenmail-mailcli -p 3025:3025 -p 3143:3143 \
//!   -e GREENMAIL_OPTS='-Dgreenmail.setup.test.imap -Dgreenmail.users.login=email' \
//!   docker.io/greenmail/standalone:2.1.0
//! MAILCLI_IT=1 cargo test --test search_integration -- --nocapture
//! ```
//!
//! Design note: mail-cli's own `connect()` is TLS-only by design (the plan's
//! "TLS everywhere"), and GreenMail's plaintext port has no usable cert. Rather
//! than bolt a plaintext mode onto the stateless binary purely for tests, these
//! tests open a plaintext `imap` session directly and drive the SAME query
//! builder + `uid_search` path the binary uses. The builder is the code under
//! test; the transport is incidental.

use std::collections::HashSet;
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset};
use imap::types::Flag;
use imap::{ConnectionMode, Session};
use mail_cli::search::Criteria;

const HOST: &str = "127.0.0.1";
const IMAP_PORT: u16 = 3143;

/// GreenMail (esp. with `auth.disabled`) shares global state, and concurrent
/// logins/APPENDs across threads interfere. Serialize these tests so each runs
/// against a quiescent server, regardless of `--test-threads`.
static SERIAL: Mutex<()> = Mutex::new(());

/// Skip the body of a test (returning early) unless MAILCLI_IT=1 is set.
macro_rules! require_greenmail {
    () => {
        if std::env::var("MAILCLI_IT").ok().as_deref() != Some("1") {
            eprintln!("skipping (set MAILCLI_IT=1 with GreenMail running)");
            return;
        }
    };
}

/// Open a plaintext IMAP session for a fresh, unique user. GreenMail
/// auto-provisions users on first login (`greenmail.users.login=email`), so each
/// test gets an isolated mailbox by using a unique address.
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

/// Append one message to INBOX with the given headers/body, INTERNALDATE, and
/// flags. UID assignment is read back separately (see `uid_of`), so we don't
/// depend on the server returning APPENDUID.
fn seed(
    session: &mut Session<imap::Connection>,
    from: &str,
    subject: &str,
    body: &str,
    internal_date: DateTime<FixedOffset>,
    flags: &[Flag<'static>],
) {
    let raw =
        format!("From: {from}\r\nTo: rcpt@example.com\r\nSubject: {subject}\r\n\r\n{body}\r\n");
    let mut cmd = session.append("INBOX", raw.as_bytes());
    cmd.internal_date(internal_date);
    for f in flags {
        cmd.flag(f.clone());
    }
    cmd.finish().expect("APPEND");
}

/// Resolve the UID of the (assumed unique) message whose Subject equals
/// `subject`, by SEARCHing the server. Keeps tests independent of APPENDUID.
fn uid_of(session: &mut Session<imap::Connection>, subject: &str) -> u32 {
    session.select("INBOX").expect("SELECT INBOX");
    let hits = session
        .uid_search(format!("SUBJECT \"{subject}\""))
        .expect("UID SEARCH by subject");
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one message with subject {subject:?}, got {hits:?}"
    );
    hits.into_iter().next().unwrap()
}

/// Run `criteria` through the builder + `uid_search` and return the UID set.
fn search_uids(session: &mut Session<imap::Connection>, criteria: &Criteria) -> HashSet<u32> {
    let query = criteria.to_imap_query().expect("valid query");
    session.select("INBOX").expect("SELECT INBOX");
    session.uid_search(&query).expect("UID SEARCH")
}

fn dt(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).expect("rfc3339 date")
}

/// A distinct user per test-run so mailboxes never collide.
fn unique_user(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{tag}.{nanos}@example.com")
}

#[test]
fn from_and_subject_and_flags() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("hdr");
    let mut s = session_for(&user);

    let d = dt("2026-03-10T12:00:00+00:00");
    seed(
        &mut s,
        "alice@example.com",
        "Invoice #7",
        "body one",
        d,
        &[Flag::Seen],
    );
    seed(&mut s, "bob@example.com", "Lunch plans", "body two", d, &[]);
    seed(
        &mut s,
        "alice@example.com",
        "Weekly report",
        "body three",
        d,
        &[],
    );
    let alice_seen = uid_of(&mut s, "Invoice #7");
    let bob_unseen = uid_of(&mut s, "Lunch plans");
    let alice_unseen = uid_of(&mut s, "Weekly report");

    // NOTE: mail-cli documents FROM as a *substring* match, which real IMAP
    // servers implement. GreenMail 2.1.0's FROM is an exact address-token match,
    // so we search the full address here — this still exercises the FROM key and
    // AND-combination; substring semantics are the server's responsibility.
    let from_alice = Criteria {
        from: Some("alice@example.com".into()),
        ..Default::default()
    };
    assert_eq!(
        search_uids(&mut s, &from_alice),
        HashSet::from([alice_seen, alice_unseen]),
        "--from should match exactly the two alice messages"
    );

    // --subject invoice → just the invoice.
    let subj = Criteria {
        subject: Some("Invoice".into()),
        ..Default::default()
    };
    assert_eq!(search_uids(&mut s, &subj), HashSet::from([alice_seen]));

    // --unseen → the two unseen ones.
    let unseen = Criteria {
        unseen: true,
        ..Default::default()
    };
    assert_eq!(
        search_uids(&mut s, &unseen),
        HashSet::from([bob_unseen, alice_unseen])
    );

    // AND-combining: --from alice --unseen → just the unseen alice message.
    let from_alice_unseen = Criteria {
        from: Some("alice@example.com".into()),
        unseen: true,
        ..Default::default()
    };
    assert_eq!(
        search_uids(&mut s, &from_alice_unseen),
        HashSet::from([alice_unseen]),
        "AND of --from and --unseen should intersect"
    );

    let _ = s.logout();
}

#[test]
fn date_bounds_match_internaldate() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("date");
    let mut s = session_for(&user);

    seed(
        &mut s,
        "a@example.com",
        "January",
        "b",
        dt("2026-01-15T09:00:00+00:00"),
        &[],
    );
    seed(
        &mut s,
        "a@example.com",
        "June",
        "b",
        dt("2026-06-15T09:00:00+00:00"),
        &[],
    );
    let jan = uid_of(&mut s, "January");
    let jun = uid_of(&mut s, "June");
    // SINCE 2026-05-01 → only the June message.
    let since = Criteria {
        since: Some("2026-05-01".into()),
        ..Default::default()
    };
    assert_eq!(search_uids(&mut s, &since), HashSet::from([jun]));

    // BEFORE 2026-05-01 → only the January message.
    let before = Criteria {
        before: Some("2026-05-01".into()),
        ..Default::default()
    };
    assert_eq!(search_uids(&mut s, &before), HashSet::from([jan]));

    // NOTE: the `ON` key is intentionally not asserted here — GreenMail 2.1.0
    // does not implement `SEARCH ON` (it matches nothing regardless of
    // INTERNALDATE). The builder's `ON` output is verified by unit tests and
    // honored by real IMAP servers.

    let _ = s.logout();
}

#[test]
fn size_bounds() {
    require_greenmail!();
    let _guard = SERIAL.lock().unwrap();
    let user = unique_user("size");
    let mut s = session_for(&user);
    let d = dt("2026-03-10T12:00:00+00:00");

    seed(&mut s, "a@example.com", "tiny", "x", d, &[]);
    seed(&mut s, "a@example.com", "huge", &"y".repeat(5000), d, &[]);
    let small = uid_of(&mut s, "tiny");
    let big = uid_of(&mut s, "huge");

    // LARGER 2000 → only the big message.
    let larger = Criteria {
        larger: Some(2000),
        ..Default::default()
    };
    let got = search_uids(&mut s, &larger);
    assert!(got.contains(&big), "big message should match LARGER 2000");
    assert!(
        !got.contains(&small),
        "small message should NOT match LARGER 2000"
    );

    let _ = s.logout();
}
