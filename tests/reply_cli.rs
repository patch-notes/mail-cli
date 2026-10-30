//! CLI-validation tests for the `reply` subcommand (REPLY-PLAN §6.1, §12.2.3).
//!
//! No server is needed: every case below must fail during argument or env
//! validation, BEFORE any network call. IMAP and SMTP are pointed at
//! `127.0.0.1:9` (the discard port, nothing listens) so any accidental
//! connect attempt fails instantly — and we assert the error is NOT a
//! connection error, which is how a wrong ordering (network first) would show
//! up. These mirror the `send` binary tests in `tests/send_e2e.rs`.

use std::process::Command;

/// `mail-cli reply …` with a clean, fully-configured env whose servers are
/// dead (127.0.0.1:9). `env_clear` keeps the test deterministic: only the
/// vars we set are visible, exactly as a clean shell would run it.
fn reply(args: &[&str]) -> std::process::Command {
    let bin = env!("CARGO_BIN_EXE_mail-cli");
    let mut cmd = Command::new(bin);
    cmd.args(args);
    cmd.env_clear()
        .env("MAILCLI_IMAP_HOST", "127.0.0.1")
        .env("MAILCLI_IMAP_PORT", "9") // discard port: connect refused
        .env("MAILCLI_SMTP_HOST", "127.0.0.1")
        .env("MAILCLI_SMTP_PORT", "9")
        .env("MAILCLI_EMAIL", "me@example.com")
        .env("MAILCLI_PASSWORD", "pw");
    cmd
}

/// The known connection-failure messages from mail-cli's IMAP/SMTP layers. A
/// validation error must surface instead of any of these. Also rejects clap's
/// `unrecognized subcommand`: before `reply` exists every invocation "fails",
/// and a test must not go green (or pass an `!success()` check) for that
/// wrong reason.
fn assert_no_network_error(label: &str, stderr: &str) {
    assert!(
        !stderr.contains("unrecognized subcommand"),
        "{label}: the `reply` subcommand must exist; got: {stderr}"
    );
    for marker in [
        "failed to connect to IMAP",
        "failed to send message via",
        "IMAP login failed",
    ] {
        assert!(
            !stderr.contains(marker),
            "{label}: a validation error must surface, not a connection error ({marker}); got: {stderr}"
        );
    }
}

/// `reply --help` must advertise the reply flags (advertise-what-you-deliver).
#[test]
fn reply_help_advertises_the_reply_flags() {
    let out = reply(&["reply", "--help"])
        .output()
        .expect("run mail-cli reply --help");
    assert!(
        out.status.success(),
        "reply --help should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let help = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--all",
        "--to",
        "--cc",
        "--bcc",
        "--no-quote",
        "--html",
        "--sent-folder",
        "--no-save-sent",
    ] {
        assert!(
            help.contains(flag),
            "reply --help must advertise {flag}; got:\n{help}"
        );
    }
    // FOLDER is positional (matches read/attachments/download), not --folder.
    assert!(
        !help.contains("--folder"),
        "FOLDER is a positional argument, not a flag; got:\n{help}"
    );
}

/// `--html` requires `--no-quote` (clap `requires`): a loud parse error
/// naming the missing flag, before any env read or network call.
#[test]
fn reply_html_without_no_quote_is_rejected() {
    let out = reply(&["reply", "INBOX", "uid:1", "--html", "--body", "hi"])
        .output()
        .expect("run mail-cli reply --html");
    assert!(
        !out.status.success(),
        "reply --html without --no-quote must be rejected; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--no-quote"),
        "the clap error must name --no-quote as required; got: {stderr}"
    );
    assert_no_network_error("reply --html without --no-quote", &stderr);
}

/// A missing body is rejected during argument resolution (step 1 of
/// REPLY-PLAN §5.2), before IMAP/SMTP config and before any connect.
#[test]
fn reply_missing_body_is_rejected_before_network() {
    let out = reply(&["reply", "INBOX", "uid:1"])
        .output()
        .expect("run mail-cli reply without body");
    assert!(
        !out.status.success(),
        "reply without --body/--body-file must fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--body"),
        "the error must point at --body/--body-file; got: {stderr}"
    );
    assert_no_network_error("reply with no body", &stderr);
}

/// `--body` and `--body-file` are mutually exclusive (clap conflicts). Must be
/// the clap conflict error — not any other failure (e.g. an unknown subcommand).
#[test]
fn reply_body_and_body_file_conflict() {
    let out = reply(&[
        "reply",
        "INBOX",
        "uid:1",
        "--body",
        "hi",
        "--body-file",
        "x.txt",
    ])
    .output()
    .expect("run mail-cli reply with both bodies");
    assert!(
        !out.status.success(),
        "reply --body + --body-file must be rejected; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "the clap conflict error must be reported; got: {stderr}"
    );
    assert_no_network_error("reply --body + --body-file", &stderr);
}

/// Bad selectors are rejected by the `Selector` parse, i.e. at the clap layer,
/// before any env read or network call. The error must be the selector's own.
#[test]
fn reply_bad_selector_is_rejected_before_network() {
    for (selector, expect) in [("0", "1-based"), ("abc", "selector must be N or uid:NNN")] {
        let out = reply(&["reply", "INBOX", selector, "--body", "hi"])
            .output()
            .expect("run mail-cli reply with bad selector");
        assert!(
            !out.status.success(),
            "selector {selector:?} must be rejected; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expect),
            "selector {selector:?}: the error must contain {expect:?}; got: {stderr}"
        );
        assert_no_network_error(&format!("reply with selector {selector:?}"), &stderr);
    }
}

/// `reply` needs SMTP: a missing `MAILCLI_SMTP_HOST` fails loud naming the
/// variable, before any connect (REPLY-PLAN §5.2 step 3).
#[test]
fn reply_missing_smtp_env_is_named_before_connect() {
    let bin = env!("CARGO_BIN_EXE_mail-cli");
    let out = Command::new(bin)
        .args(["reply", "INBOX", "uid:1", "--body", "hi"])
        .env_clear()
        .env("MAILCLI_IMAP_HOST", "127.0.0.1")
        .env("MAILCLI_IMAP_PORT", "9")
        .env("MAILCLI_EMAIL", "me@example.com")
        .env("MAILCLI_PASSWORD", "pw")
        // no MAILCLI_SMTP_HOST on purpose
        .output()
        .expect("run mail-cli reply without SMTP host");
    assert!(
        !out.status.success(),
        "reply without MAILCLI_SMTP_HOST must fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("MAILCLI_SMTP_HOST"),
        "the error must name MAILCLI_SMTP_HOST; got: {stderr}"
    );
    assert_no_network_error("reply without MAILCLI_SMTP_HOST", &stderr);
}

/// Once validation passes, the first network step is the IMAP connect:
/// `reply INBOX uid:1 --body x` against the dead IMAP server (127.0.0.1:9)
/// must exit non-zero with the IMAP connection error — and never reach SMTP,
/// so nothing is sent and a non-zero exit invites no double-send.
#[test]
fn reply_dead_imap_fails_with_connect_error_not_send() {
    let out = reply(&["reply", "INBOX", "uid:1", "--body", "x"])
        .output()
        .expect("run mail-cli reply");
    assert!(
        !out.status.success(),
        "reply against a dead IMAP server must fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("failed to connect to IMAP"),
        "the failure must be the IMAP connect; got: {stderr}"
    );
    assert!(
        !stderr.contains("failed to send message via"),
        "SMTP must never be reached; got: {stderr}"
    );
}
