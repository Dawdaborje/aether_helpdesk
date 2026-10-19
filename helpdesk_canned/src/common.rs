//! Helpers shared by the parts of the plugin.

pub use aether_sdk::records::{id_of, require, text, Record};

use aether_sdk::prelude::*;

pub const AGENT_ROLE: &str = "helpdesk.hd_agent";
pub const MANAGER_ROLE: &str = "helpdesk.hd_manager";
pub const ADMIN_ROLE: &str = "helpdesk.hd_admin";

pub fn is_agent() -> Result<bool> {
    let ctx = context::current()?;
    Ok([AGENT_ROLE, MANAGER_ROLE, ADMIN_ROLE].iter().any(|r| ctx.has_role(r)))
}

pub fn require_agent() -> Result<()> {
    if is_agent()? {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{AGENT_ROLE}` to do this")))
    }
}

pub fn is_manager() -> Result<bool> {
    let ctx = context::current()?;
    Ok(ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE))
}

pub fn actor() -> Result<Option<String>> {
    Ok(context::current()?.actor.id)
}

pub fn now() -> Result<String> {
    let ctx = context::current()?;
    if ctx.now.is_empty() {
        return Err(Error::msg("the kernel did not say what time it is"));
    }
    Ok(ctx.now)
}

pub fn opt(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn clean(label: &str, value: &str) -> Result<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(Error::msg(format!("{label} is needed")));
    }
    Ok(v.to_string())
}
