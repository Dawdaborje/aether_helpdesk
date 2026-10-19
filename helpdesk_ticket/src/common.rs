//! Helpers shared by the parts of the plugin.

pub use aether_sdk::records::{id_of, next_number, require, text, Record};

use aether_sdk::dates::{format_datetime, parse_datetime, NaiveDateTime};
use aether_sdk::prelude::*;

pub const AGENT_ROLE: &str = "helpdesk.hd_agent";
pub const MANAGER_ROLE: &str = "helpdesk.hd_manager";
pub const ADMIN_ROLE: &str = "helpdesk.hd_admin";

/// States a ticket is still being worked in.
pub const OPEN_STATES: &[&str] = &["new", "open", "pending", "on_hold"];

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

/// The caller's account, `users:abc`.
pub fn actor() -> Result<Option<String>> {
    Ok(context::current()?.actor.id)
}

/// The request's time, `2026-10-05T08:03:00Z`.
pub fn now() -> Result<String> {
    let ctx = context::current()?;
    if ctx.now.is_empty() {
        return Err(Error::msg("the kernel did not say what time it is"));
    }
    Ok(ctx.now)
}

pub fn moment(record: &Record, field: &str) -> Option<NaiveDateTime> {
    text(record, field).and_then(|t| parse_datetime(t).ok())
}

pub fn stamp(moment: NaiveDateTime) -> String {
    format_datetime(moment)
}

/// Account ids are `users:abc`; a bare `abc` typed by a person is completed.
pub fn user_id(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains(':') {
        raw.to_string()
    } else {
        format!("users:{raw}")
    }
}

pub fn clean(label: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::msg(format!("{label} is needed")));
    }
    Ok(value.to_string())
}

pub fn opt(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn get_ticket_row(id: &str) -> Result<Record> {
    let ticket = db::find::<Record>("hd_ticket").filter("id", id).first()?.ok_or_else(|| Error::msg("there is no such ticket"))?;
    check_scope(&ticket)?;
    Ok(ticket)
}

/// The teams an agent is limited to, or none when the company lets agents see every team (or the caller is a
/// manager, an administrator or the kernel itself).
pub fn team_scope() -> Result<Option<Vec<String>>> {
    let ctx = context::current()?;
    if ctx.actor.kind != context::ActorKind::User || ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE) || !ctx.has_role(AGENT_ROLE) {
        return Ok(None);
    }
    let company = crate::tickets::default_company()?;
    let settings: Value = plugins::call("helpdesk", "get_settings", &json!({ "id": company })).unwrap_or(Value::Null);
    if settings.get("restrict_by_team") != Some(&json!(true)) {
        return Ok(None);
    }
    let teams: Vec<Record> = plugins::call("helpdesk", "my_teams", &json!({}))?;
    Ok(Some(teams.iter().filter_map(|t| text(t, "id").map(str::to_string)).collect()))
}

/// A restricted agent sees tickets of their teams, tickets with no team, and tickets assigned to them.
fn check_scope(ticket: &Record) -> Result<()> {
    let Some(teams) = team_scope()? else { return Ok(()) };
    let mine = actor()?;
    let visible = match text(ticket, "team") {
        None => true,
        Some(t) => teams.iter().any(|x| x == t),
    } || (text(ticket, "assignee").is_some() && text(ticket, "assignee") == mine.as_deref());
    if visible {
        Ok(())
    } else {
        Err(Error::msg("this ticket belongs to another team"))
    }
}

/// Agents see every ticket; anyone else only the ones they asked for. The plugin's own functions run with the
/// plugin's rights (`via:self`), so this check, not the record rule, keeps one customer out of another's ticket.
pub fn require_can_see(ticket: &Record) -> Result<()> {
    if is_agent()? {
        return Ok(());
    }
    match (text(ticket, "requester_user"), actor()?) {
        (Some(owner), Some(me)) if owner == me => Ok(()),
        _ => Err(Error::msg("this is not your ticket")),
    }
}

pub fn is_open(ticket: &Record) -> bool {
    text(ticket, "state").is_some_and(|s| OPEN_STATES.contains(&s))
}

/// What other plugins hear about a ticket.
pub fn event_body(ticket: &Record, extra: Value) -> Value {
    let mut body = json!({
        "ticket": ticket["id"], "reference": ticket["reference"], "subject": ticket["subject"], "state": ticket["state"],
        "customer": ticket["customer"], "team": ticket["team"], "assignee": ticket["assignee"], "priority": ticket["priority"],
        "company": ticket["company"],
    });
    if let (Some(base), Some(more)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            base.insert(k.clone(), v.clone());
        }
    }
    body
}

/// A line in the thread written by the plugin itself ("assigned to …", "merged into …").
pub fn system_line(ticket: &str, body: &str) -> Result<()> {
    let ticket_row = get_ticket_row(ticket)?;
    db::create::<Record>(
        "hd_message",
        &json!({
            "ticket": ticket, "kind": "system", "direction": "internal", "body": body, "author_user": actor()?,
            "requester_user": ticket_row.get("requester_user"), "posted_at": now()?,
        }),
    )?;
    Ok(())
}

/// Tell people something happened, best effort: a notification that fails never undoes the work.
pub fn tell(users: &[String], title: &str, body: &str, ticket: &str) {
    if users.is_empty() {
        return;
    }
    let _ = Notification::new(title)
        .body(body)
        .link(format!("/helpdesk/tickets/{}", ticket.rsplit(':').next().unwrap_or(ticket)))
        .to(Audience::Actors(users.to_vec()))
        .send();
}
