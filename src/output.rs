//! Output rendering: human-friendly tables by default, `--json` for scripts.

use anyhow::Result;
use serde::Serialize;

use crate::imap::Folder;
use crate::parse::{Attachment, Rendered, Summary};

/// Print any serializable value as pretty JSON.
pub fn json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Truncate a string to `width` display chars, adding an ellipsis if cut.
fn clip(s: &str, width: usize) -> String {
    let s = s.replace(['\n', '\r', '\t'], " ");
    if s.chars().count() <= width {
        return s;
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Render folders as a plain list.
pub fn folders_human(folders: &[Folder]) {
    for f in folders {
        println!("{}", f.name);
    }
}

/// Render a message listing as an aligned table.
pub fn list_human(summaries: &[Summary]) {
    if summaries.is_empty() {
        println!("(no messages)");
        return;
    }
    println!(
        "{:>3}  {:>7}  {:<24}  {:<40}  {:<25}  📎",
        "#", "UID", "FROM", "SUBJECT", "DATE"
    );
    for (i, s) in summaries.iter().enumerate() {
        let paperclip = if s.has_attachments { "📎" } else { " " };
        println!(
            "{:>3}  {:>7}  {:<24}  {:<40}  {:<25}  {}",
            i + 1,
            s.uid,
            clip(&s.from, 24),
            clip(&s.subject, 40),
            clip(&s.date, 25),
            paperclip,
        );
    }
}

/// Render a single message (`read`).
pub fn read_human(r: &Rendered, index: Option<usize>) {
    match index {
        Some(n) => println!("message uid:{} (#{n})", r.uid),
        None => println!("message uid:{}", r.uid),
    }
    println!("From:    {}", r.from);
    if !r.to.is_empty() {
        println!("To:      {}", r.to);
    }
    if !r.cc.is_empty() {
        println!("Cc:      {}", r.cc);
    }
    println!("Subject: {}", r.subject);
    if !r.date.is_empty() {
        println!("Date:    {}", r.date);
    }
    if !r.attachments.is_empty() {
        let names: Vec<&str> = r.attachments.iter().map(|a| a.filename.as_str()).collect();
        println!("Attach:  {} ({})", r.attachments.len(), names.join(", "));
    }
    println!();
    println!("{}", r.body);
}

/// Render an attachment listing (`attachments`).
pub fn attachments_human(uid: u32, index: Option<usize>, atts: &[Attachment]) {
    match index {
        Some(n) => println!("message uid:{uid} (#{n})"),
        None => println!("message uid:{uid}"),
    }
    if atts.is_empty() {
        println!("(no attachments)");
        return;
    }
    println!(
        "{:>2}  {:<30}  {:<28}  {:>10}",
        "K", "FILENAME", "MIME", "SIZE"
    );
    for a in atts {
        println!(
            "{:>2}  {:<30}  {:<28}  {:>10}",
            a.index,
            clip(&a.filename, 30),
            clip(&a.mime, 28),
            human_size(a.size),
        );
    }
}

/// Human-readable byte size.
fn human_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_short_unchanged() {
        assert_eq!(clip("hi", 10), "hi");
    }

    #[test]
    fn clip_long_gets_ellipsis() {
        assert_eq!(clip("abcdef", 4), "abc…");
    }

    #[test]
    fn size_formatting() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
    }
}
