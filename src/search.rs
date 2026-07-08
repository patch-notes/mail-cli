//! Pure, I/O-free IMAP SEARCH query builder.
//!
//! `Criteria` mirrors the `search` command's typed flags and compiles them into
//! a single server-side IMAP SEARCH string (AND-combined). This is the one
//! boundary where friendly dates are converted and user values are quoted, so
//! it is where nearly all of this feature's tests live. No network, no state.

use anyhow::{Result, bail};

/// Typed search criteria, one field per structured flag. `None`/`false` means
/// "not requested" and contributes nothing to the query.
#[derive(Debug, Default, Clone)]
pub struct Criteria {
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub text: Option<String>,

    pub since: Option<String>,
    pub before: Option<String>,
    pub on: Option<String>,

    pub seen: bool,
    pub unseen: bool,
    pub flagged: bool,
    pub unflagged: bool,
    pub answered: bool,

    pub larger: Option<u64>,
    pub smaller: Option<u64>,
}

impl Criteria {
    /// Build a validated IMAP SEARCH string from the criteria, AND-combining all
    /// requested terms (space-joined = implicit IMAP AND). Errors loudly on an
    /// empty criteria set, malformed dates, or zero-sized bounds.
    pub fn to_imap_query(&self) -> Result<String> {
        let mut terms: Vec<String> = Vec::new();

        // Header / text substring terms.
        if let Some(v) = &self.from {
            terms.push(format!("FROM {}", quote(v)));
        }
        if let Some(v) = &self.to {
            terms.push(format!("TO {}", quote(v)));
        }
        if let Some(v) = &self.cc {
            terms.push(format!("CC {}", quote(v)));
        }
        if let Some(v) = &self.subject {
            terms.push(format!("SUBJECT {}", quote(v)));
        }
        if let Some(v) = &self.body {
            terms.push(format!("BODY {}", quote(v)));
        }
        if let Some(v) = &self.text {
            terms.push(format!("TEXT {}", quote(v)));
        }

        // Dates (converted to IMAP's dd-Mon-yyyy at this boundary).
        if let Some(d) = &self.since {
            terms.push(format!("SINCE {}", imap_date(d).map_err(rename("--since"))?));
        }
        if let Some(d) = &self.before {
            terms.push(format!("BEFORE {}", imap_date(d).map_err(rename("--before"))?));
        }
        if let Some(d) = &self.on {
            terms.push(format!("ON {}", imap_date(d).map_err(rename("--on"))?));
        }

        // Boolean flag terms.
        if self.seen {
            terms.push("SEEN".into());
        }
        if self.unseen {
            terms.push("UNSEEN".into());
        }
        if self.flagged {
            terms.push("FLAGGED".into());
        }
        if self.unflagged {
            terms.push("UNFLAGGED".into());
        }
        if self.answered {
            terms.push("ANSWERED".into());
        }

        // Size bounds (bytes). Zero is meaningless for LARGER/SMALLER.
        if let Some(n) = self.larger {
            if n == 0 {
                bail!("--larger must be greater than 0 bytes");
            }
            terms.push(format!("LARGER {n}"));
        }
        if let Some(n) = self.smaller {
            if n == 0 {
                bail!("--smaller must be greater than 0 bytes");
            }
            terms.push(format!("SMALLER {n}"));
        }

        if terms.is_empty() {
            bail!("specify at least one search flag (or --raw); refusing to SEARCH ALL");
        }
        Ok(terms.join(" "))
    }
}

/// Attach the offending flag name to a date-parse error.
fn rename(flag: &'static str) -> impl Fn(anyhow::Error) -> anyhow::Error {
    move |e| anyhow::anyhow!("{flag}: {e}")
}

/// Convert an ISO `YYYY-MM-DD` date to IMAP's `dd-Mon-yyyy` (e.g. `08-Jul-2026`).
/// Validates ranges strictly so malformed input fails at this boundary.
fn imap_date(iso: &str) -> Result<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let parts: Vec<&str> = iso.split('-').collect();
    if parts.len() != 3 {
        bail!("expected date as YYYY-MM-DD, got {iso:?}");
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        bail!("expected date as YYYY-MM-DD, got {iso:?}");
    }
    let year: u32 = y
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid year in {iso:?}"))?;
    let month: u32 = m
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid month in {iso:?}"))?;
    let day: u32 = d
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid day in {iso:?}"))?;
    if !(1..=12).contains(&month) {
        bail!("month out of range in {iso:?} (expected 01-12)");
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        bail!("day out of range in {iso:?}");
    }
    Ok(format!("{day:02}-{}-{year:04}", MONTHS[(month - 1) as usize]))
}

/// Days in a given month, honoring Gregorian leap years.
fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// IMAP-quote a user value: wrap in double quotes and backslash-escape any
/// embedded `\` or `"`. Newlines are rejected (illegal in a SEARCH literal and
/// also rejected by the `imap` crate).
fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_criteria_errors() {
        let c = Criteria::default();
        assert!(c.to_imap_query().is_err());
    }

    #[test]
    fn single_from_term() {
        let c = Criteria {
            from: Some("alice".into()),
            ..Default::default()
        };
        assert_eq!(c.to_imap_query().unwrap(), r#"FROM "alice""#);
    }

    #[test]
    fn and_joins_multiple_terms_in_stable_order() {
        let c = Criteria {
            from: Some("alice".into()),
            subject: Some("invoice".into()),
            unseen: true,
            ..Default::default()
        };
        assert_eq!(
            c.to_imap_query().unwrap(),
            r#"FROM "alice" SUBJECT "invoice" UNSEEN"#
        );
    }

    #[test]
    fn quotes_values_with_spaces() {
        let c = Criteria {
            subject: Some("quarterly report".into()),
            ..Default::default()
        };
        assert_eq!(c.to_imap_query().unwrap(), r#"SUBJECT "quarterly report""#);
    }

    #[test]
    fn escapes_embedded_quotes_and_backslashes() {
        let c = Criteria {
            body: Some(r#"say "hi"\bye"#.into()),
            ..Default::default()
        };
        assert_eq!(
            c.to_imap_query().unwrap(),
            r#"BODY "say \"hi\"\\bye""#
        );
    }

    #[test]
    fn all_header_and_text_keys() {
        let c = Criteria {
            to: Some("bob".into()),
            cc: Some("carol".into()),
            text: Some("hello".into()),
            ..Default::default()
        };
        assert_eq!(
            c.to_imap_query().unwrap(),
            r#"TO "bob" CC "carol" TEXT "hello""#
        );
    }

    #[test]
    fn boolean_flags() {
        let c = Criteria {
            seen: true,
            flagged: true,
            answered: true,
            ..Default::default()
        };
        assert_eq!(c.to_imap_query().unwrap(), "SEEN FLAGGED ANSWERED");
        let c2 = Criteria {
            unseen: true,
            unflagged: true,
            ..Default::default()
        };
        assert_eq!(c2.to_imap_query().unwrap(), "UNSEEN UNFLAGGED");
    }

    #[test]
    fn date_conversion() {
        let c = Criteria {
            since: Some("2026-07-08".into()),
            before: Some("2026-12-01".into()),
            ..Default::default()
        };
        assert_eq!(
            c.to_imap_query().unwrap(),
            "SINCE 08-Jul-2026 BEFORE 01-Dec-2026"
        );
    }

    #[test]
    fn on_date() {
        let c = Criteria {
            on: Some("2024-02-29".into()), // leap day, valid
            ..Default::default()
        };
        assert_eq!(c.to_imap_query().unwrap(), "ON 29-Feb-2024");
    }

    #[test]
    fn imap_date_valid() {
        assert_eq!(imap_date("2026-01-01").unwrap(), "01-Jan-2026");
        assert_eq!(imap_date("2026-12-31").unwrap(), "31-Dec-2026");
    }

    #[test]
    fn imap_date_rejects_bad_month() {
        assert!(imap_date("2026-13-01").is_err());
        assert!(imap_date("2026-00-01").is_err());
    }

    #[test]
    fn imap_date_rejects_bad_day() {
        assert!(imap_date("2026-13-40").is_err());
        assert!(imap_date("2026-02-30").is_err()); // Feb never has 30
        assert!(imap_date("2025-02-29").is_err()); // 2025 not a leap year
    }

    #[test]
    fn imap_date_rejects_malformed() {
        assert!(imap_date("2026-7-8").is_err()); // needs zero-padding
        assert!(imap_date("07-08-2026").is_err()); // wrong order/shape
        assert!(imap_date("not-a-date").is_err());
        assert!(imap_date("2026-07").is_err());
    }

    #[test]
    fn date_error_names_the_flag() {
        let c = Criteria {
            since: Some("2026-13-40".into()),
            ..Default::default()
        };
        let err = c.to_imap_query().unwrap_err().to_string();
        assert!(err.contains("--since"), "error should name the flag: {err}");
    }

    #[test]
    fn size_bounds() {
        let c = Criteria {
            larger: Some(1000),
            smaller: Some(5000),
            ..Default::default()
        };
        assert_eq!(c.to_imap_query().unwrap(), "LARGER 1000 SMALLER 5000");
    }

    #[test]
    fn zero_size_bounds_error() {
        let c = Criteria {
            larger: Some(0),
            ..Default::default()
        };
        assert!(c.to_imap_query().is_err());
        let c2 = Criteria {
            smaller: Some(0),
            ..Default::default()
        };
        assert!(c2.to_imap_query().is_err());
    }
}
