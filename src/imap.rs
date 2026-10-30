//! IMAP module — the ONLY place that talks IMAP. Isolated so a future swap to
//! `async-imap` (if v2 ever needs IDLE/streaming) is a contained change.
//!
//! Functions are generic over the connection stream so we never have to name
//! the concrete TLS type the `ClientBuilder` produces.

use anyhow::{Context, Result, anyhow, bail};
use imap::types::Flag;
use imap::{Connection, ConnectionMode, Session};

use crate::config::{AuthMethod, ImapConfig, TlsMode};
use crate::parse::{self, Summary};

/// A listed mailbox.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Folder {
    pub name: String,
    pub delimiter: Option<String>,
}

/// SASL XOAUTH2 authenticator. `process` returns the raw challenge response;
/// the `imap` crate base64-encodes it before sending.
struct XOAuth2 {
    user: String,
    token: String,
}

impl imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

/// A live authenticated session over the `imap` crate's type-erased connection.
pub type Conn = Session<Connection>;

/// Connect, negotiate TLS, and authenticate. Returns a ready session.
pub fn connect(cfg: &ImapConfig) -> Result<Conn> {
    let mode = match cfg.tls {
        TlsMode::Implicit => ConnectionMode::Tls,
        TlsMode::StartTls => ConnectionMode::StartTls,
    };
    let client = imap::ClientBuilder::new(&cfg.host, cfg.port)
        .mode(mode)
        .connect()
        .with_context(|| format!("failed to connect to IMAP {}:{}", cfg.host, cfg.port))?;

    let session = match cfg.auth {
        AuthMethod::Login => client
            .login(&cfg.user, &cfg.password)
            .map_err(|(e, _)| anyhow!(e))
            .context("IMAP login failed (check credentials)")?,
        AuthMethod::Xoauth2 => {
            let auth = XOAuth2 {
                user: cfg.user.clone(),
                token: cfg.password.clone(),
            };
            client
                .authenticate("XOAUTH2", &auth)
                .map_err(|(e, _)| anyhow!(e))
                .context("IMAP XOAUTH2 authentication failed")?
        }
    };
    Ok(session)
}

/// List all mailboxes for the account.
pub fn list_folders(session: &mut Conn) -> Result<Vec<Folder>> {
    let names = session.list(None, Some("*")).context("LIST failed")?;
    let mut folders: Vec<Folder> = names
        .iter()
        .map(|n| Folder {
            name: n.name().to_string(),
            delimiter: n.delimiter().map(|d| d.to_string()),
        })
        .collect();
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

/// Return UIDs of a folder, newest-first (highest UID first), optionally
/// filtered by a raw IMAP SEARCH query, capped at `limit`.
pub fn newest_uids(
    session: &mut Conn,
    folder: &str,
    limit: usize,
    search: Option<&str>,
) -> Result<Vec<u32>> {
    session
        .select(folder)
        .with_context(|| format!("cannot select folder {folder:?}"))?;
    let query = search.unwrap_or("ALL");
    let mut uids: Vec<u32> = session
        .uid_search(query)
        .with_context(|| format!("SEARCH {query:?} failed"))?
        .into_iter()
        .collect();
    uids.sort_unstable_by(|a, b| b.cmp(a)); // descending = newest first
    uids.truncate(limit);
    Ok(uids)
}

/// Fetch summaries for the given UIDs (in the order provided).
pub fn fetch_summaries(session: &mut Conn, folder: &str, uids: &[u32]) -> Result<Vec<Summary>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    session
        .select(folder)
        .with_context(|| format!("cannot select folder {folder:?}"))?;
    let set = uids
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(&set, "(UID FLAGS BODY.PEEK[])")
        .context("UID FETCH failed")?;

    // Index fetched messages by UID so we can preserve caller order.
    let mut by_uid = std::collections::HashMap::new();
    for f in fetches.iter() {
        if let Some(uid) = f.uid {
            let body = f.body().unwrap_or_default().to_vec();
            let flags = format_flags(f.flags());
            by_uid.insert(uid, (flags, body));
        }
    }
    let mut out = Vec::with_capacity(uids.len());
    for &uid in uids {
        if let Some((flags, body)) = by_uid.remove(&uid) {
            out.push(parse::summarize(uid, flags, &body)?);
        }
    }
    Ok(out)
}

/// Fetch the raw RFC822 bytes of a single message by UID (without marking read).
pub fn fetch_raw(session: &mut Conn, folder: &str, uid: u32) -> Result<Vec<u8>> {
    session
        .select(folder)
        .with_context(|| format!("cannot select folder {folder:?}"))?;
    let fetches = session
        .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
        .context("UID FETCH failed")?;
    let msg = fetches
        .iter()
        .next()
        .ok_or_else(|| anyhow!("no message with uid:{uid} in {folder}"))?;
    match msg.body() {
        Some(b) => Ok(b.to_vec()),
        None => bail!("message uid:{uid} had no body"),
    }
}

/// APPEND a copy of a just-sent message into the Sent folder, flagged `\Seen`
/// (we file our own outgoing mail as already read). The raw bytes are the exact
/// RFC822 payload that went over SMTP. GreenMail and most servers auto-create a
/// missing mailbox on APPEND; when they don't, this fails and the caller warns.
pub fn append_to_sent(session: &mut Conn, folder: &str, raw: &[u8]) -> Result<()> {
    let mut cmd = session.append(folder, raw);
    cmd.flag(Flag::Seen);
    cmd.finish()
        .with_context(|| format!("APPEND to {folder:?} failed"))?;
    Ok(())
}

/// Discover the Sent folder via the RFC 6154 SPECIAL-USE `\Sent` attribute.
/// Returns the mailbox name flagged `\Sent`, or `None` if the server advertises
/// no such mailbox (or doesn't support special-use at all). Best-effort: a
/// failed LIST just yields `None` so the caller falls back to its default.
pub fn discover_sent(session: &mut Conn) -> Option<String> {
    let names = session.list(None, Some("*")).ok()?;
    names
        .iter()
        .find(|n| {
            n.attributes()
                .iter()
                .any(|a| matches!(a, imap_proto::NameAttribute::Sent))
        })
        .map(|n| n.name().to_string())
}

/// Add `\Answered` to one message (SELECT + `UID STORE uid +FLAGS.SILENT
/// (\Answered)`). The only new IMAP code `reply` needs (REPLY-PLAN §6.2);
/// idempotent by design (adding an already-set flag is a no-op).
// used by cmd_reply (T7)
#[allow(dead_code)]
pub fn mark_answered(session: &mut Conn, folder: &str, uid: u32) -> Result<()> {
    session
        .select(folder)
        .with_context(|| format!("cannot select folder {folder:?}"))?;
    session
        .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Answered)")
        .with_context(|| format!("UID STORE failed for uid:{uid} in {folder}"))?;
    Ok(())
}

/// Render IMAP flags to their canonical `\Seen`-style strings.
fn format_flags(flags: &[Flag<'_>]) -> Vec<String> {
    flags
        .iter()
        .map(|f| match f {
            Flag::Seen => "\\Seen".into(),
            Flag::Answered => "\\Answered".into(),
            Flag::Flagged => "\\Flagged".into(),
            Flag::Deleted => "\\Deleted".into(),
            Flag::Draft => "\\Draft".into(),
            Flag::Recent => "\\Recent".into(),
            Flag::MayCreate => "\\*".into(),
            Flag::Custom(c) => c.to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}
