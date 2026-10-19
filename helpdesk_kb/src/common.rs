//! Helpers shared by the parts of the plugin.

pub use aether_sdk::records::{id_of, require, text, Record};

use aether_sdk::prelude::*;

use crate::markdown::to_html;

pub const AGENT_ROLE: &str = "helpdesk.hd_agent";
pub const MANAGER_ROLE: &str = "helpdesk.hd_manager";
pub const ADMIN_ROLE: &str = "helpdesk.hd_admin";

pub fn is_agent() -> Result<bool> {
    let ctx = context::current()?;
    Ok(ctx.actor.kind != context::ActorKind::Visitor && ctx.actor.kind != context::ActorKind::Anonymous && [AGENT_ROLE, MANAGER_ROLE, ADMIN_ROLE].iter().any(|r| ctx.has_role(r)))
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
    Ok(ctx.actor.kind != context::ActorKind::Visitor && (ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE)))
}

pub fn require_manager() -> Result<()> {
    if is_manager()? {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{MANAGER_ROLE}` to do this")))
    }
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

/// Who is reading: 3 an agent, 2 a signed-in member, 1 anyone.
pub fn audience() -> Result<i64> {
    if is_agent()? {
        return Ok(3);
    }
    Ok(if context::current()?.actor.kind == context::ActorKind::User { 2 } else { 1 })
}

pub fn audience_name(level: i64) -> &'static str {
    match level {
        3 => "agent",
        2 => "customer",
        _ => "public",
    }
}

fn rank(visibility: &str) -> i64 {
    match visibility {
        "agents" => 3,
        "customers" => 2,
        _ => 1,
    }
}

/// A reader sees an article that is published and meant for them (agents see every state).
pub fn readable(article: &Record, level: i64) -> bool {
    if level >= 3 {
        return true;
    }
    text(article, "state") == Some("published") && rank(text(article, "visibility").unwrap_or("public")) <= level
}

pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    let out: String = out.chars().take(80).collect();
    if out.is_empty() { "article".into() } else { out.trim_end_matches('-').to_string() }
}

/// A slug no other article or category has.
pub fn free_slug(model: &str, base: &str) -> Result<String> {
    let mut candidate = base.to_string();
    for n in 2..200 {
        if db::find::<Record>(model).filter("slug", candidate.as_str()).first()?.is_none() {
            return Ok(candidate);
        }
        candidate = format!("{base}-{n}");
    }
    Err(Error::msg("could not find a free address for this title"))
}

/// An article as readers get it: with its text as HTML.
pub fn article_view(mut article: Record) -> Record {
    let html = to_html(text(&article, "body").unwrap_or(""));
    if let Some(o) = article.as_object_mut() {
        o.insert("body_html".into(), json!(html));
    }
    article
}

/// The site's address for links in email, from the helpdesk settings.
pub fn site_link(path: &str) -> Option<String> {
    let company = plugins::call::<Option<Record>>("company", "get_default_company", &json!({})).ok().flatten()?;
    let settings: Value = plugins::call("helpdesk", "get_settings", &json!({ "id": company["id"] })).ok()?;
    let base = settings.get("portal_url").and_then(Value::as_str).filter(|b| !b.trim().is_empty())?;
    Some(format!("{}{}", base.trim_end_matches('/'), path))
}
