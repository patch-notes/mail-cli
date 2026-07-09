//! Configuration — the ONLY boundary that reads the environment.
//!
//! Everything is read once, at startup. Validation is per-command: a command
//! asks for exactly the pieces it needs (IMAP, SMTP, common) and gets a loud,
//! variable-naming error if something required is missing or malformed.

use anyhow::{Context, Result, bail};
use std::env;

/// How TLS is established with the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// TLS negotiated on connect (implicit; 993/465).
    Implicit,
    /// Plain connect, upgraded via STARTTLS.
    StartTls,
}

/// Authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// PLAIN/LOGIN with a password or app-password.
    Login,
    /// SASL XOAUTH2 — the password field is treated as an access token.
    Xoauth2,
}

/// IMAP connection settings (read commands).
#[derive(Debug, Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub tls: TlsMode,
    pub auth: AuthMethod,
}

/// SMTP connection settings (`send`).
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub email: String,
    pub tls: TlsMode,
    pub auth: AuthMethod,
}

/// The active account selector: an optional env-var suffix.
///
/// `None` reads the bare `MAILCLI_*` names (the default account, exactly as the
/// tool worked before multi-account existed). `Some("1")` reads `MAILCLI_*_1`,
/// and so on. The suffix is numeric by convention but may be any non-empty
/// token, so `MAILCLI_*_work` works too. Reading is strict: an account is read
/// only from its own suffixed names, with no fallback to the bare vars.
///
/// This is the ONLY place that touches the process environment, so all
/// account/name resolution stays contained at the boundary.
#[derive(Debug, Clone)]
pub struct Env {
    /// `None` => bare names; `Some(s)` => `<BASE>_<s>`.
    suffix: Option<String>,
    /// Injectable lookup, so `from_env` is testable without touching the
    /// process-global environment. Defaults to `std::env::var`.
    lookup: fn(&str) -> Option<String>,
}

fn env_lookup(name: &str) -> Option<String> {
    env::var(name).ok()
}

impl Env {
    /// Resolve the active account from the selector (already reduced from
    /// `--account` > `MAILCLI_ACCOUNT` > none by the caller). An empty or
    /// whitespace-only token is rejected loud rather than silently collapsing
    /// to the default account.
    pub fn new(account: Option<String>) -> Result<Self> {
        let suffix = match account {
            None => None,
            Some(tok) if tok.trim().is_empty() => {
                bail!(
                    "account selector is empty; drop --account/MAILCLI_ACCOUNT for the default account, or give a non-empty token like 1"
                )
            }
            Some(tok) => Some(tok),
        };
        Ok(Self {
            suffix,
            lookup: env_lookup,
        })
    }

    /// The concrete env-var name for a base: `MAILCLI_IMAP_HOST` for the default
    /// account, `MAILCLI_IMAP_HOST_1` when the suffix is `1`.
    fn var(&self, base: &str) -> String {
        match &self.suffix {
            Some(s) => format!("{base}_{s}"),
            None => base.to_string(),
        }
    }

    /// Read an optional var, treating empty/whitespace as absent.
    fn opt(&self, base: &str) -> Option<String> {
        match (self.lookup)(&self.var(base)) {
            Some(v) if !v.trim().is_empty() => Some(v),
            _ => None,
        }
    }

    /// Read a required var, failing loud with the *resolved* variable name.
    fn required(&self, base: &str) -> Result<String> {
        let name = self.var(base);
        self.opt(base)
            .with_context(|| format!("required environment variable {name} is not set"))
    }

    /// Parse a port, defaulting when unset, failing loud when malformed.
    fn port(&self, base: &str, default: u16) -> Result<u16> {
        match self.opt(base) {
            None => Ok(default),
            Some(v) => {
                let name = self.var(base);
                v.parse::<u16>().with_context(|| {
                    format!("environment variable {name} is not a valid port: {v:?}")
                })
            }
        }
    }

    fn tls_mode(&self) -> Result<TlsMode> {
        let name = self.var("MAILCLI_TLS");
        match self.opt("MAILCLI_TLS").as_deref() {
            None | Some("implicit") => Ok(TlsMode::Implicit),
            Some("starttls") => Ok(TlsMode::StartTls),
            Some(other) => bail!("{name} must be 'implicit' or 'starttls', got {other:?}"),
        }
    }

    fn auth_method(&self) -> Result<AuthMethod> {
        let name = self.var("MAILCLI_AUTH");
        match self.opt("MAILCLI_AUTH").as_deref() {
            None | Some("login") => Ok(AuthMethod::Login),
            Some("xoauth2") => Ok(AuthMethod::Xoauth2),
            Some(other) => bail!("{name} must be 'login' or 'xoauth2', got {other:?}"),
        }
    }

    /// Login username: `MAILCLI_USER[_N]` if set, else the account address.
    fn login_user(&self) -> Result<String> {
        match self.opt("MAILCLI_USER") {
            Some(u) => Ok(u),
            None => self.required("MAILCLI_EMAIL"),
        }
    }
}

impl ImapConfig {
    /// Build IMAP settings from the environment, validating only what read
    /// commands require.
    pub fn from_env(env: &Env) -> Result<Self> {
        Ok(Self {
            host: env.required("MAILCLI_IMAP_HOST")?,
            port: env.port("MAILCLI_IMAP_PORT", 993)?,
            user: env.login_user()?,
            password: env.required("MAILCLI_PASSWORD")?,
            tls: env.tls_mode()?,
            auth: env.auth_method()?,
        })
    }
}

impl SmtpConfig {
    /// Build SMTP settings from the environment, validating only what `send`
    /// requires.
    pub fn from_env(env: &Env) -> Result<Self> {
        Ok(Self {
            host: env.required("MAILCLI_SMTP_HOST")?,
            port: env.port("MAILCLI_SMTP_PORT", 465)?,
            user: env.login_user()?,
            password: env.required("MAILCLI_PASSWORD")?,
            email: env.required("MAILCLI_EMAIL")?,
            tls: env.tls_mode()?,
            auth: env.auth_method()?,
        })
    }
}

/// A configured account discovered by scanning the environment. Secret-free:
/// carries only enough to tell the human which accounts exist and whether they
/// look usable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccountInfo {
    /// The selector: `"default"` for the bare vars, else the suffix (`"1"`).
    pub name: String,
    /// The account address, if `MAILCLI_EMAIL[_N]` is set.
    pub email: Option<String>,
    /// The IMAP host, if `MAILCLI_IMAP_HOST[_N]` is set.
    pub imap_host: Option<String>,
    /// The SMTP host, if `MAILCLI_SMTP_HOST[_N]` is set.
    pub smtp_host: Option<String>,
    /// Whether `MAILCLI_PASSWORD[_N]` is present (never the value itself).
    pub has_password: bool,
}

/// Bases that define an account. A key belongs to an account iff it is one of
/// these, optionally followed by `_<suffix>`.
const ACCOUNT_BASES: &[&str] = &[
    "MAILCLI_IMAP_HOST",
    "MAILCLI_IMAP_PORT",
    "MAILCLI_SMTP_HOST",
    "MAILCLI_SMTP_PORT",
    "MAILCLI_EMAIL",
    "MAILCLI_USER",
    "MAILCLI_PASSWORD",
    "MAILCLI_TLS",
    "MAILCLI_AUTH",
];

/// Discover configured accounts from the process environment. Never reads or
/// returns secret values — only presence. `default` (bare vars) is included iff
/// any bare account var is set. Suffixes sort numerically-then-lexically.
pub fn scan_accounts() -> Vec<AccountInfo> {
    scan_accounts_from(env::vars().collect::<Vec<_>>())
}

/// Pure core of [`scan_accounts`], testable with an injected var list.
fn scan_accounts_from(vars: Vec<(String, String)>) -> Vec<AccountInfo> {
    use std::collections::BTreeSet;

    let nonempty = |v: &str| !v.trim().is_empty();

    // Collect the set of suffixes in use (None = default/bare).
    let mut suffixes: BTreeSet<Option<String>> = BTreeSet::new();
    for (k, v) in &vars {
        if !nonempty(v) {
            continue;
        }
        for base in ACCOUNT_BASES {
            if k == base {
                suffixes.insert(None);
            } else if let Some(sfx) = k.strip_prefix(&format!("{base}_"))
                && !sfx.is_empty()
            {
                suffixes.insert(Some(sfx.to_string()));
            }
        }
    }

    let get = |suffix: &Option<String>, base: &str| -> Option<String> {
        let name = match suffix {
            Some(s) => format!("{base}_{s}"),
            None => base.to_string(),
        };
        vars.iter()
            .find(|(k, v)| k == &name && nonempty(v))
            .map(|(_, v)| v.clone())
    };

    let mut infos: Vec<AccountInfo> = suffixes
        .iter()
        .map(|suffix| AccountInfo {
            name: suffix.clone().unwrap_or_else(|| "default".to_string()),
            email: get(suffix, "MAILCLI_EMAIL"),
            imap_host: get(suffix, "MAILCLI_IMAP_HOST"),
            smtp_host: get(suffix, "MAILCLI_SMTP_HOST"),
            has_password: get(suffix, "MAILCLI_PASSWORD").is_some(),
        })
        .collect();

    // "default" first, then numeric suffixes ascending, then the rest lexically.
    infos.sort_by_key(|a| account_sort_key(&a.name));
    infos
}

/// Sort key: default first, then numeric ascending, then lexical.
fn account_sort_key(name: &str) -> (u8, u64, String) {
    if name == "default" {
        (0, 0, String::new())
    } else if let Ok(n) = name.parse::<u64>() {
        (1, n, String::new())
    } else {
        (2, 0, name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an Env with a given suffix over the real environment. Tests here
    /// only exercise name resolution (`var`) and construction (`new`), which do
    /// not read env, so the default lookup is fine. Reader tests inject an
    /// explicit `fn` lookup (e.g. `|_| None`); captured-data cases go through the
    /// pure `scan_accounts_from`.
    fn env_with(suffix: Option<&str>) -> Env {
        Env {
            suffix: suffix.map(str::to_string),
            lookup: env_lookup,
        }
    }

    #[test]
    fn var_default_account_is_bare() {
        let env = env_with(None);
        assert_eq!(env.var("MAILCLI_IMAP_HOST"), "MAILCLI_IMAP_HOST");
    }

    #[test]
    fn var_numeric_suffix_appended() {
        let env = env_with(Some("1"));
        assert_eq!(env.var("MAILCLI_IMAP_HOST"), "MAILCLI_IMAP_HOST_1");
    }

    #[test]
    fn var_named_suffix_appended() {
        let env = env_with(Some("work"));
        assert_eq!(env.var("MAILCLI_PASSWORD"), "MAILCLI_PASSWORD_work");
    }

    #[test]
    fn new_none_is_default_account() {
        let env = Env::new(None).unwrap();
        assert_eq!(env.var("MAILCLI_EMAIL"), "MAILCLI_EMAIL");
    }

    #[test]
    fn new_rejects_empty_and_whitespace() {
        assert!(Env::new(Some(String::new())).is_err());
        assert!(Env::new(Some("   ".to_string())).is_err());
    }

    #[test]
    fn new_accepts_token() {
        assert_eq!(Env::new(Some("2".to_string())).unwrap().var("X"), "X_2");
    }

    /// A required var missing from account 2 names the *resolved* var.
    #[test]
    fn required_error_names_suffixed_var() {
        let env = Env {
            suffix: Some("2".to_string()),
            lookup: |_| None,
        };
        let err = env.required("MAILCLI_PASSWORD").unwrap_err();
        assert!(
            err.to_string().contains("MAILCLI_PASSWORD_2"),
            "error should name the suffixed var, got: {err}"
        );
    }

    fn owned(vars: &[(&str, &str)]) -> Vec<(String, String)> {
        vars.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn scan_finds_default_and_numeric_accounts() {
        let infos = scan_accounts_from(owned(&[
            ("MAILCLI_IMAP_HOST", "imap.work.com"),
            ("MAILCLI_EMAIL", "me@work.com"),
            ("MAILCLI_PASSWORD", "secret"),
            ("MAILCLI_IMAP_HOST_1", "imap.gmail.com"),
            ("MAILCLI_EMAIL_1", "me@gmail.com"),
            ("MAILCLI_PASSWORD_1", "hunter2"),
            ("UNRELATED", "ignore me"),
        ]));
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].name, "default");
        assert_eq!(infos[0].email.as_deref(), Some("me@work.com"));
        assert!(infos[0].has_password);
        assert_eq!(infos[1].name, "1");
        assert_eq!(infos[1].imap_host.as_deref(), Some("imap.gmail.com"));
    }

    #[test]
    fn scan_reports_incomplete_without_leaking_password_value() {
        let infos = scan_accounts_from(owned(&[
            ("MAILCLI_IMAP_HOST_2", "imap.example.com"),
            ("MAILCLI_EMAIL_2", "me@example.com"),
            // no password for account 2
        ]));
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "2");
        assert!(!infos[0].has_password);
    }

    #[test]
    fn scan_sorts_default_then_numeric_then_named() {
        let infos = scan_accounts_from(owned(&[
            ("MAILCLI_EMAIL_10", "ten@x.com"),
            ("MAILCLI_EMAIL_2", "two@x.com"),
            ("MAILCLI_EMAIL", "def@x.com"),
            ("MAILCLI_EMAIL_work", "work@x.com"),
        ]));
        let names: Vec<&str> = infos.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["default", "2", "10", "work"]);
    }

    #[test]
    fn scan_ignores_empty_values() {
        let infos = scan_accounts_from(owned(&[("MAILCLI_EMAIL_3", "   ")]));
        assert!(infos.is_empty());
    }
}
