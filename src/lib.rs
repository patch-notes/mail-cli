//! Library facade exposing the pure, dependency-light pieces of `mail-cli` for
//! integration testing. Only `search` — the offline IMAP SEARCH query builder —
//! is public here; the I/O modules (imap/smtp/…) remain private to the binary.

pub mod search;
