//! mail-cli — a stateless, env-configured IMAP/SMTP command-line email client.
//!
//! `main` is thin: it defines the clap surface, parses env into `Config` at the
//! boundary, resolves message selectors live (no cache), and dispatches.

mod config;
mod imap;
mod output;
mod parse;
mod smtp;

use mail_cli::search;

use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};

use config::{ImapConfig, SmtpConfig};
use parse::BodyKind;

/// Stateless IMAP/SMTP email client. All connection settings come from
/// environment variables (see MAILCLI_* in the README).
#[derive(Parser)]
#[command(name = "mail-cli", version, about, long_about = None)]
struct Cli {
    /// Emit JSON instead of human-friendly output (read commands only).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List mailboxes (folders) for the account.
    Folders,

    /// List messages in a folder, newest first.
    List {
        /// Folder to list.
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Maximum number of messages to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Raw IMAP SEARCH query (e.g. "FROM alice UNSEEN").
        #[arg(long)]
        search: Option<String>,
    },

    /// Search a folder with typed flags, compiled to a server-side IMAP SEARCH.
    ///
    /// Flags are AND-combined. Values are substring, server-side matches.
    /// Output is a normal listing (same columns as `list`); UID is the stable
    /// handle — scripts should use --json and address messages by `uid:`.
    Search {
        /// Folder to search.
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Maximum number of messages to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,

        /// Substring match on the From header.
        #[arg(long, group = "structured")]
        from: Option<String>,
        /// Substring match on the To header.
        #[arg(long, group = "structured")]
        to: Option<String>,
        /// Substring match on the Cc header.
        #[arg(long, group = "structured")]
        cc: Option<String>,
        /// Substring match on the Subject.
        #[arg(long, group = "structured")]
        subject: Option<String>,
        /// Substring match on the body text only.
        #[arg(long, group = "structured")]
        body: Option<String>,
        /// Substring match on headers + body.
        #[arg(long, group = "structured")]
        text: Option<String>,

        /// On/after this date (YYYY-MM-DD).
        #[arg(long, group = "structured")]
        since: Option<String>,
        /// Before this date (YYYY-MM-DD).
        #[arg(long, group = "structured")]
        before: Option<String>,
        /// On exactly this date (YYYY-MM-DD).
        #[arg(long, group = "structured")]
        on: Option<String>,

        /// Only messages marked \Seen.
        #[arg(long, group = "structured", conflicts_with = "unseen")]
        seen: bool,
        /// Only messages NOT marked \Seen.
        #[arg(long, group = "structured")]
        unseen: bool,
        /// Only messages marked \Flagged.
        #[arg(long, group = "structured", conflicts_with = "unflagged")]
        flagged: bool,
        /// Only messages NOT marked \Flagged.
        #[arg(long, group = "structured")]
        unflagged: bool,
        /// Only messages marked \Answered.
        #[arg(long, group = "structured")]
        answered: bool,

        /// Only messages larger than N bytes.
        #[arg(long, group = "structured")]
        larger: Option<u64>,
        /// Only messages smaller than N bytes.
        #[arg(long, group = "structured")]
        smaller: Option<u64>,

        /// Raw IMAP SEARCH escape hatch; conflicts with all structured flags.
        #[arg(long, conflicts_with = "structured")]
        raw: Option<String>,
    },

    /// Read one message. SELECTOR is `N` (live index) or `uid:NNN`.
    Read {
        folder: String,
        selector: Selector,
        /// Print the raw HTML part instead of down-converted text.
        #[arg(long)]
        html: bool,
        /// Print the untouched RFC822 source.
        #[arg(long)]
        raw: bool,
    },

    /// List a message's attachments. SELECTOR is `N` or `uid:NNN`.
    Attachments { folder: String, selector: Selector },

    /// Download attachments. SELECTOR is `N` or `uid:NNN`.
    Download {
        folder: String,
        selector: Selector,
        /// Download only attachment K (1-based).
        #[arg(long, conflicts_with = "all")]
        attachment: Option<usize>,
        /// Download all attachments.
        #[arg(long)]
        all: bool,
        /// Output directory (default: current directory).
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },

    /// Compose and send a message via SMTP.
    Send {
        /// Recipient(s), comma-separated.
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        /// Cc recipient(s), comma-separated.
        #[arg(long, value_delimiter = ',')]
        cc: Vec<String>,
        /// Bcc recipient(s), comma-separated.
        #[arg(long, value_delimiter = ',')]
        bcc: Vec<String>,
        #[arg(long)]
        subject: String,
        /// Body text (mutually exclusive with --body-file).
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read body from a file; `-` means stdin.
        #[arg(long)]
        body_file: Option<String>,
        /// Send the body as HTML.
        #[arg(long)]
        html: bool,
        /// Attach a file (repeatable).
        #[arg(long)]
        attach: Vec<PathBuf>,
    },
}

/// A stateless message reference: a live index or a stable UID.
#[derive(Debug, Clone, Copy)]
enum Selector {
    Index(u32),
    Uid(u32),
}

impl FromStr for Selector {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        if let Some(rest) = s.strip_prefix("uid:") {
            let uid = rest
                .parse::<u32>()
                .with_context(|| format!("invalid uid in selector {s:?}"))?;
            Ok(Selector::Uid(uid))
        } else {
            let idx = s
                .parse::<u32>()
                .with_context(|| format!("selector must be N or uid:NNN, got {s:?}"))?;
            if idx == 0 {
                bail!("index selector is 1-based; 0 is not valid");
            }
            Ok(Selector::Index(idx))
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Folders => cmd_folders(cli.json),
        Command::List {
            folder,
            limit,
            search,
        } => cmd_list(cli.json, &folder, limit, search.as_deref()),
        Command::Search {
            folder,
            limit,
            from,
            to,
            cc,
            subject,
            body,
            text,
            since,
            before,
            on,
            seen,
            unseen,
            flagged,
            unflagged,
            answered,
            larger,
            smaller,
            raw,
        } => {
            let query = if let Some(raw) = raw {
                raw
            } else {
                let criteria = search::Criteria {
                    from,
                    to,
                    cc,
                    subject,
                    body,
                    text,
                    since,
                    before,
                    on,
                    seen,
                    unseen,
                    flagged,
                    unflagged,
                    answered,
                    larger,
                    smaller,
                };
                criteria.to_imap_query()?
            };
            cmd_list(cli.json, &folder, limit, Some(&query))
        }
        Command::Read {
            folder,
            selector,
            html,
            raw,
        } => cmd_read(cli.json, &folder, selector, html, raw),
        Command::Attachments { folder, selector } => {
            cmd_attachments(cli.json, &folder, selector)
        }
        Command::Download {
            folder,
            selector,
            attachment,
            all,
            out,
        } => cmd_download(&folder, selector, attachment, all, &out),
        Command::Send {
            to,
            cc,
            bcc,
            subject,
            body,
            body_file,
            html,
            attach,
        } => cmd_send(to, cc, bcc, subject, body, body_file, html, attach),
    }
}

// ---------- selector resolution (live, no cache) ----------

/// Resolve a selector to a concrete UID (plus the 1-based index if that's how
/// it was addressed), by re-deriving the newest-first listing at the point of use.
fn resolve(
    session: &mut imap::Conn,
    folder: &str,
    selector: Selector,
) -> Result<(u32, Option<usize>)> {
    match selector {
        Selector::Uid(uid) => Ok((uid, None)),
        Selector::Index(idx) => {
            let n = idx as usize;
            let uids = imap::newest_uids(session, folder, n, None)?;
            let uid = uids.get(n - 1).copied().ok_or_else(|| {
                anyhow!("no message #{idx} in {folder} (only {} present)", uids.len())
            })?;
            Ok((uid, Some(n)))
        }
    }
}

// ---------- commands ----------

fn cmd_folders(json: bool) -> Result<()> {
    let cfg = ImapConfig::from_env()?;
    let mut session = imap::connect(&cfg)?;
    let folders = imap::list_folders(&mut session)?;
    let _ = session.logout();
    if json {
        output::json(&folders)
    } else {
        output::folders_human(&folders);
        Ok(())
    }
}

fn cmd_list(json: bool, folder: &str, limit: usize, search: Option<&str>) -> Result<()> {
    let cfg = ImapConfig::from_env()?;
    let mut session = imap::connect(&cfg)?;
    let uids = imap::newest_uids(&mut session, folder, limit, search)?;
    let summaries = imap::fetch_summaries(&mut session, folder, &uids)?;
    let _ = session.logout();
    if json {
        output::json(&summaries)
    } else {
        output::list_human(&summaries);
        Ok(())
    }
}

fn cmd_read(json: bool, folder: &str, selector: Selector, html: bool, raw: bool) -> Result<()> {
    let cfg = ImapConfig::from_env()?;
    let mut session = imap::connect(&cfg)?;
    let (uid, index) = resolve(&mut session, folder, selector)?;
    let bytes = imap::fetch_raw(&mut session, folder, uid)?;
    let _ = session.logout();

    if raw {
        // Raw RFC822 source, untouched.
        use std::io::Write;
        std::io::stdout().write_all(&bytes)?;
        return Ok(());
    }

    let kind = if html { BodyKind::Html } else { BodyKind::Text };
    let rendered = parse::render(uid, &bytes, kind)?;
    if json {
        output::json(&rendered)
    } else {
        output::read_human(&rendered, index);
        Ok(())
    }
}

fn cmd_attachments(json: bool, folder: &str, selector: Selector) -> Result<()> {
    let cfg = ImapConfig::from_env()?;
    let mut session = imap::connect(&cfg)?;
    let (uid, index) = resolve(&mut session, folder, selector)?;
    let bytes = imap::fetch_raw(&mut session, folder, uid)?;
    let _ = session.logout();

    let atts = parse::attachments(&bytes)?;
    if json {
        output::json(&atts)
    } else {
        output::attachments_human(uid, index, &atts);
        Ok(())
    }
}

fn cmd_download(
    folder: &str,
    selector: Selector,
    attachment: Option<usize>,
    all: bool,
    out: &std::path::Path,
) -> Result<()> {
    if attachment.is_none() && !all {
        bail!("specify --attachment K or --all");
    }
    let cfg = ImapConfig::from_env()?;
    let mut session = imap::connect(&cfg)?;
    let (uid, index) = resolve(&mut session, folder, selector)?;
    let bytes = imap::fetch_raw(&mut session, folder, uid)?;
    let _ = session.logout();

    let atts = parse::attachments(&bytes)?;
    if atts.is_empty() {
        bail!("message uid:{uid} has no attachments");
    }
    std::fs::create_dir_all(out).with_context(|| format!("cannot create output dir {out:?}"))?;

    let selected: Vec<&parse::Attachment> = if all {
        atts.iter().collect()
    } else {
        let k = attachment.unwrap();
        let a = atts
            .iter()
            .find(|a| a.index == k)
            .ok_or_else(|| anyhow!("no attachment #{k} (message has {})", atts.len()))?;
        vec![a]
    };

    match index {
        Some(n) => println!("message uid:{uid} (#{n})"),
        None => println!("message uid:{uid}"),
    }
    for a in selected {
        let dest = out.join(&a.filename);
        std::fs::write(&dest, &a.bytes).with_context(|| format!("cannot write {dest:?}"))?;
        println!(
            "saved {} ({} bytes) -> {}",
            a.filename,
            a.bytes.len(),
            dest.display()
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_send(
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body: Option<String>,
    body_file: Option<String>,
    html: bool,
    attach: Vec<PathBuf>,
) -> Result<()> {
    if to.is_empty() {
        bail!("at least one --to recipient is required");
    }
    let body = resolve_body(body, body_file)?;
    let cfg = SmtpConfig::from_env()?;
    let outgoing = smtp::Outgoing {
        to,
        cc,
        bcc,
        subject,
        body,
        html,
        attachments: attach,
    };
    let email = smtp::build(&cfg, &outgoing)?;
    smtp::send(&cfg, &email)?;
    println!("message sent");
    Ok(())
}

/// Resolve the single body input path: `--body`, or `--body-file PATH` (`-` = stdin).
fn resolve_body(body: Option<String>, body_file: Option<String>) -> Result<String> {
    match (body, body_file) {
        (Some(b), _) => Ok(b),
        (None, Some(path)) if path == "-" => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read body from stdin")?;
            Ok(buf)
        }
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read body file {path:?}")),
        (None, None) => bail!("provide --body TEXT or --body-file PATH (use '-' for stdin)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_parses_index() {
        assert!(matches!("3".parse::<Selector>().unwrap(), Selector::Index(3)));
    }

    #[test]
    fn selector_parses_uid() {
        assert!(matches!(
            "uid:4821".parse::<Selector>().unwrap(),
            Selector::Uid(4821)
        ));
    }

    #[test]
    fn selector_rejects_zero_index() {
        assert!("0".parse::<Selector>().is_err());
    }

    #[test]
    fn selector_rejects_garbage() {
        assert!("abc".parse::<Selector>().is_err());
        assert!("uid:x".parse::<Selector>().is_err());
    }
}
