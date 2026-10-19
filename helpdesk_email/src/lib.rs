//! Mail into the helpdesk. A provider (or a small relay) posts each message to a secret-protected address; the plugin
//! keeps it, drops what is blocked, machine-made or spam, routes the rest by the mailbox it came to, and hands it to
//! the ticket plugin, which threads it. Failures stay in the inbox and are retried.

mod inbox;
mod mail;
