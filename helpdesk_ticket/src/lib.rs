//! Tickets: raised over any channel, worked in one thread of public replies and internal notes, assigned by team
//! rules, merged, split and rated. Announces every step as an event for SLA, automation and reporting.

mod assign;
mod common;
mod merge;
mod stats;
mod thread;
mod tickets;
mod time;
mod visitor;
