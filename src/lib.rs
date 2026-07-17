//! Library facade exposing the pieces of `mail-cli` that integration tests drive
//! directly. `search` is the offline IMAP SEARCH query builder; `imap` is the
//! IMAP wire layer, exposed so `tests/` can exercise the real `append_to_sent`
//! and `discover_sent` paths against a GreenMail server. The remaining modules
//! (config/smtp/parse/output/skill) stay private to the binary.

pub mod search;

pub mod config;
pub mod imap;
pub mod parse;
