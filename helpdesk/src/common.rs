//! Helpers shared by the parts of the plugin.

pub use aether_sdk::records::{id_of, require, text, Record};

use aether_sdk::prelude::*;

pub const ADMIN_ROLE: &str = "helpdesk.hd_admin";

pub const ASSIGNMENT_MODES: &[&str] = &["manual", "round_robin", "load_balanced"];

pub fn require_admin() -> Result<()> {
    context::current()?.require_role(ADMIN_ROLE)
}

/// Anyone signed in may read the masters (a person asking for help picks a type and a priority).
pub fn require_reader() -> Result<()> {
    let ctx = context::current()?;
    if ctx.is_member() || ctx.actor.kind == context::ActorKind::System {
        Ok(())
    } else {
        Err(Error::msg("sign in first"))
    }
}
