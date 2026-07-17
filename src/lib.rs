//! Library facade exposing the pieces of `mail-cli` that integration tests drive
//! directly. `search` is the offline IMAP SEARCH query builder; `imap` is the
//! IMAP wire layer and `smtp` builds the outgoing message, both exposed so
//! `tests/` can exercise the real send→Sent path (`smtp::build`/`formatted` +
//! `imap::append_to_sent`/`discover_sent`) against a GreenMail server. The
//! remaining modules (output/skill) stay private to the binary.

pub mod search;

pub mod config;
pub mod imap;
pub mod parse;
pub mod smtp;
