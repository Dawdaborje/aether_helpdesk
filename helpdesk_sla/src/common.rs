//! Helpers shared by the parts of the plugin.

pub use aether_sdk::records::{id_of, require, text, Record};

use aether_sdk::dates::{format_datetime, parse_datetime, parse_date, NaiveDateTime};
use aether_sdk::prelude::*;

use crate::hours::{minutes_of, weekday_of, Schedule};

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

/// Service levels are set by managers and administrators.
pub fn require_manager() -> Result<()> {
    let ctx = context::current()?;
    if ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE) {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{MANAGER_ROLE}` to do this")))
    }
}

pub fn now_text() -> Result<String> {
    let ctx = context::current()?;
    if ctx.now.is_empty() {
        return Err(Error::msg("the kernel did not say what time it is"));
    }
    Ok(ctx.now)
}

pub fn now() -> Result<NaiveDateTime> {
    parse_datetime(&now_text()?)
}

pub fn moment(record: &Record, field: &str) -> Option<NaiveDateTime> {
    text(record, field).and_then(|t| parse_datetime(t).ok())
}

pub fn stamp(t: NaiveDateTime) -> String {
    format_datetime(t)
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

pub fn user_id(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains(':') {
        raw.to_string()
    } else {
        format!("users:{raw}")
    }
}

/// The working pattern of a set of business hours (none: round the clock).
pub fn schedule_for(hours: Option<&str>) -> Result<Schedule> {
    let Some(id) = hours else { return Ok(Schedule::round_the_clock()) };
    let set = db::find::<Record>("hd_hours").filter("id", id).expand("days").first()?;
    let Some(set) = set else { return Ok(Schedule::round_the_clock()) };
    let mut days = Vec::new();
    for d in set.get("days").and_then(Value::as_array).cloned().unwrap_or_default() {
        if let (Some(wd), Some(s), Some(e)) = (text(&d, "weekday").and_then(weekday_of), text(&d, "start").and_then(minutes_of), text(&d, "end").and_then(minutes_of)) {
            days.push((wd, s, e));
        }
    }
    let holidays: Vec<Record> = db::find("hd_holiday").filter("hours", id).limit(2000).all()?;
    Ok(Schedule {
        offset: set.get("utc_offset_minutes").and_then(Value::as_i64).unwrap_or(0),
        days,
        holidays: holidays.iter().filter_map(|h| text(h, "day").and_then(|d| parse_date(d).ok())).collect(),
    })
}

/// A ticket as the ticket plugin shows it. The SLA plugin acts as the kernel when it hears an event.
pub fn ticket(id: &str) -> Result<Record> {
    plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": id }))
}

pub fn tell(users: &[String], title: &str, body: &str, ticket: &str) {
    if users.is_empty() {
        return;
    }
    let key = ticket.rsplit(':').next().unwrap_or(ticket);
    let _ = Notification::new(title).body(body).link(format!("/helpdesk/tickets/{key}")).to(Audience::Actors(users.to_vec())).send();
}
