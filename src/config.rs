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

/// Read an optional env var, treating empty/whitespace as absent.
fn opt(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

/// Read a required env var, failing loud with the variable name.
fn required(name: &str) -> Result<String> {
    opt(name).with_context(|| format!("required environment variable {name} is not set"))
}

/// Parse a port, defaulting when unset, failing loud when malformed.
fn port(name: &str, default: u16) -> Result<u16> {
    match opt(name) {
        None => Ok(default),
        Some(v) => v
            .parse::<u16>()
            .with_context(|| format!("environment variable {name} is not a valid port: {v:?}")),
    }
}

fn tls_mode() -> Result<TlsMode> {
    match opt("MAILCLI_TLS").as_deref() {
        None | Some("implicit") => Ok(TlsMode::Implicit),
        Some("starttls") => Ok(TlsMode::StartTls),
        Some(other) => bail!("MAILCLI_TLS must be 'implicit' or 'starttls', got {other:?}"),
    }
}

fn auth_method() -> Result<AuthMethod> {
    match opt("MAILCLI_AUTH").as_deref() {
        None | Some("login") => Ok(AuthMethod::Login),
        Some("xoauth2") => Ok(AuthMethod::Xoauth2),
        Some(other) => bail!("MAILCLI_AUTH must be 'login' or 'xoauth2', got {other:?}"),
    }
}

/// Login username: `MAILCLI_USER` if set, else the account address.
fn login_user() -> Result<String> {
    match opt("MAILCLI_USER") {
        Some(u) => Ok(u),
        None => required("MAILCLI_EMAIL"),
    }
}

impl ImapConfig {
    /// Build IMAP settings from the environment, validating only what read
    /// commands require.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: required("MAILCLI_IMAP_HOST")?,
            port: port("MAILCLI_IMAP_PORT", 993)?,
            user: login_user()?,
            password: required("MAILCLI_PASSWORD")?,
            tls: tls_mode()?,
            auth: auth_method()?,
        })
    }
}

impl SmtpConfig {
    /// Build SMTP settings from the environment, validating only what `send`
    /// requires.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: required("MAILCLI_SMTP_HOST")?,
            port: port("MAILCLI_SMTP_PORT", 465)?,
            user: login_user()?,
            password: required("MAILCLI_PASSWORD")?,
            email: required("MAILCLI_EMAIL")?,
            tls: tls_mode()?,
            auth: auth_method()?,
        })
    }
}
