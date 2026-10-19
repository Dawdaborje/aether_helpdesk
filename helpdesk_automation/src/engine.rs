//! Running rules: build what is known about a ticket, test each rule's conditions, and carry out its actions through the
//! ticket plugin (acting as the kernel for events and the timer, as the agent for a macro).

use aether_sdk::dates::{format_datetime, parse_datetime, Duration, NaiveDateTime};
use aether_sdk::prelude::*;
use serde_json::Map;

use crate::eval::{fill, holds};

pub type Record = aether_sdk::records::Record;

pub const ACTIONS: &[&str] = &[
    "set_priority", "set_team", "set_type", "assign_agent", "auto_assign", "unassign", "add_tag", "remove_tag", "change_state", "add_note", "send_reply", "send_email", "notify",
    "escalate", "set_due",
];

const OPEN: [&str; 4] = ["new", "open", "pending", "on_hold"];

pub fn text<'a>(r: &'a Record, k: &str) -> Option<&'a str> {
    r.get(k).and_then(Value::as_str)
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

fn moment(r: &Record, k: &str) -> Option<NaiveDateTime> {
    text(r, k).and_then(|t| parse_datetime(t).ok())
}

pub fn user_id(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains(':') {
        raw.to_string()
    } else {
        format!("users:{raw}")
    }
}

/// The helpdesk masters a rule names by code or name, looked up once per run.
#[derive(Default)]
pub struct Lookups {
    priorities: Option<Vec<Record>>,
    teams: Option<Vec<Record>>,
    types: Option<Vec<Record>>,
    tags: Option<Vec<Record>>,
}

impl Lookups {
    fn list(slot: &mut Option<Vec<Record>>, function: &str) -> Result<Vec<Record>> {
        if slot.is_none() {
            *slot = Some(plugins::call("helpdesk", function, &json!({}))?);
        }
        Ok(slot.clone().unwrap_or_default())
    }

    pub fn find(&mut self, kind: &str, value: &str) -> Result<Option<Record>> {
        let rows = match kind {
            "priority" => Self::list(&mut self.priorities, "search_priorities")?,
            "team" => Self::list(&mut self.teams, "search_teams")?,
            "ticket_type" => Self::list(&mut self.types, "search_ticket_types")?,
            "tag" => Self::list(&mut self.tags, "search_tags")?,
            _ => return Ok(None),
        };
        let v = value.trim();
        Ok(rows.into_iter().find(|r| text(r, "id") == Some(v) || text(r, "code").is_some_and(|c| c.eq_ignore_ascii_case(v)) || text(r, "name").is_some_and(|n| n.eq_ignore_ascii_case(v))))
    }

    pub fn id(&mut self, kind: &str, value: &str) -> Result<String> {
        self.find(kind, value)?.and_then(|r| text(&r, "id").map(str::to_string)).ok_or_else(|| Error::msg(format!("there is no {} `{value}`", kind.replace('_', " "))))
    }

    /// A condition names a priority, team or type by id, code or name; compare by id.
    fn canon(&mut self, field: &str, value: &str) -> String {
        let kind = match field {
            "priority" => "priority",
            "team" => "team",
            "ticket_type" => "ticket_type",
            _ => return if field == "assignee" { value.split(',').map(|v| if v.trim().is_empty() { String::new() } else { user_id(v) }).collect::<Vec<_>>().join(",") } else { value.to_string() },
        };
        value.split(',').map(|v| self.id(kind, v).unwrap_or_else(|_| v.trim().to_string())).collect::<Vec<_>>().join(",")
    }
}

fn hours(a: Option<NaiveDateTime>, b: NaiveDateTime) -> Value {
    a.map(|a| json!(((b - a).num_minutes() as f64 / 60.0 * 100.0).round() / 100.0)).unwrap_or(Value::Null)
}

/// What a rule can look at on a ticket, now.
pub fn facts(t: &Record, now: NaiveDateTime) -> Map<String, Value> {
    let mut m = Map::new();
    let s = |k: &str| t.get(k).cloned().unwrap_or(Value::Null);
    for k in ["state", "priority", "ticket_type", "team", "assignee", "channel", "customer", "requester_email", "subject", "description", "reopen_count", "message_count", "rating"] {
        m.insert(k.into(), s(k));
    }
    m.insert("tags".into(), s("tags_text"));
    m.insert("requester_domain".into(), json!(text(t, "requester_email").and_then(|e| e.split_once('@')).map(|(_, d)| d.to_lowercase())));
    m.insert("unassigned".into(), json!(text(t, "assignee").is_none()));
    let (c, a) = (moment(t, "last_customer_message_at"), moment(t, "last_agent_message_at"));
    m.insert("waiting_for_us".into(), json!(match (c, a) {
        (Some(c), Some(a)) => c > a,
        (Some(_), None) => true,
        _ => false,
    }));
    m.insert("hours_open".into(), hours(moment(t, "opened_at"), now));
    m.insert("hours_since_update".into(), hours(moment(t, "last_activity_at"), now));
    m.insert("hours_since_customer_message".into(), hours(c, now));
    m.insert("hours_since_agent_message".into(), hours(a, now));
    m.insert("hours_to_due".into(), moment(t, "due_at").map(|d| json!(((d - now).num_minutes() as f64 / 60.0 * 100.0).round() / 100.0)).unwrap_or(Value::Null));
    m
}

pub fn conditions_hold(rule: &Record, facts: &Map<String, Value>, look: &mut Lookups) -> bool {
    let list = rule.get("conditions").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        return true;
    }
    let any = text(rule, "match_mode") == Some("any");
    let mut results = list.iter().map(|c| {
        let field = text(c, "field").unwrap_or("");
        let wanted = look.canon(field, text(c, "value").unwrap_or(""));
        holds(text(c, "op").unwrap_or(""), facts.get(field).unwrap_or(&Value::Null), &wanted)
    });
    if any { results.any(|r| r) } else { results.all(|r| r) }
}

fn merge_values(t: &Record) -> Vec<(&'static str, String)> {
    let name = text(t, "requester_name").unwrap_or("").to_string();
    vec![
        ("reference", text(t, "reference").unwrap_or("").to_string()),
        ("subject", text(t, "subject").unwrap_or("").to_string()),
        ("state", text(t, "state").unwrap_or("").replace('_', " ")),
        ("requester.name", name.clone()),
        ("requester.first_name", name.split_whitespace().next().unwrap_or("").to_string()),
        ("requester.email", text(t, "requester_email").unwrap_or("").to_string()),
        ("assignee", text(t, "assignee").unwrap_or("").to_string()),
    ]
}

fn people(target: &str, t: &Record) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for part in target.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "assignee" => out.extend(text(t, "assignee").map(str::to_string)),
            "team" => {
                if let Some(team) = text(t, "team") {
                    let members: Vec<Record> = plugins::call("helpdesk", "list_team_members", &json!({ "id": team }))?;
                    out.extend(members.iter().filter_map(|m| text(m, "user").map(user_id)));
                }
            }
            other => out.push(user_id(other)),
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn check_action(spec: &Value) -> Result<()> {
    let a = text(spec, "action").unwrap_or("");
    if !ACTIONS.contains(&a) {
        return Err(Error::msg(format!("`{a}` is not an action: {}", ACTIONS.join(", "))));
    }
    let value = text(spec, "value").unwrap_or("").trim();
    let target = text(spec, "target").unwrap_or("").trim();
    let need = |what: &str, v: &str| if v.is_empty() { Err(Error::msg(format!("{a} needs {what}"))) } else { Ok(()) };
    match a {
        "set_priority" | "set_team" | "set_type" | "assign_agent" | "add_tag" | "remove_tag" | "add_note" | "send_reply" => need("a value", value)?,
        "change_state" => {
            if !["open", "pending", "on_hold", "resolved", "closed"].contains(&value) {
                return Err(Error::msg("change_state moves a ticket to open, pending, on_hold, resolved or closed"));
            }
        }
        "send_email" => {
            need("a target (requester, or addresses)", target)?;
            need("the message", value)?;
        }
        "notify" => {
            need("a target (assignee, team, or users)", target)?;
            need("the message", value)?;
        }
        "set_due" => {
            if value.parse::<i64>().map(|m| m < 0).unwrap_or(true) {
                return Err(Error::msg("set_due takes a number of minutes from now"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Carry one action out (or, in a dry run, describe it).
pub fn act(spec: &Value, ticket_id: &str, t: &Record, look: &mut Lookups, dry: bool) -> Result<String> {
    let action = text(spec, "action").unwrap_or("");
    let value = text(spec, "value").unwrap_or("").trim().to_string();
    let target = text(spec, "target").unwrap_or("").trim().to_string();
    let values = merge_values(t);
    let say = fill(&value, &values);
    let call = |f: &str, input: Value| -> Result<Value> { plugins::call("helpdesk_ticket", f, &input) };
    let line = match action {
        "set_priority" => {
            let id = look.id("priority", &value)?;
            if !dry {
                call("update_ticket", json!({ "id": ticket_id, "priority": id }))?;
            }
            format!("priority set to {value}")
        }
        "set_team" => {
            let id = look.id("team", &value)?;
            if !dry {
                call("update_ticket", json!({ "id": ticket_id, "team": id }))?;
            }
            format!("team set to {value}")
        }
        "set_type" => {
            let id = look.id("ticket_type", &value)?;
            if !dry {
                call("update_ticket", json!({ "id": ticket_id, "ticket_type": id }))?;
            }
            format!("type set to {value}")
        }
        "assign_agent" => {
            if !dry {
                call("assign_ticket", json!({ "id": ticket_id, "assignee": user_id(&value) }))?;
            }
            format!("assigned to {value}")
        }
        "auto_assign" => {
            if !dry {
                call("auto_assign_ticket", json!({ "id": ticket_id }))?;
            }
            "assigned by the team's rule".into()
        }
        "unassign" => {
            if !dry {
                call("unassign_ticket", json!({ "id": ticket_id }))?;
            }
            "unassigned".into()
        }
        "add_tag" | "remove_tag" => {
            let tag = look.find("tag", &value)?.and_then(|r| text(&r, "code").map(str::to_string)).ok_or_else(|| Error::msg(format!("there is no tag `{value}`")))?;
            if !dry {
                call(if action == "add_tag" { "add_ticket_tag" } else { "remove_ticket_tag" }, json!({ "id": ticket_id, "tag": tag }))?;
            }
            format!("{} tag {value}", if action == "add_tag" { "added" } else { "removed" })
        }
        "change_state" => {
            if !dry {
                call("change_ticket_state", json!({ "id": ticket_id, "state": value, "resolution": if value == "resolved" { Some(fill(&target, &values)).filter(|s| !s.is_empty()) } else { None } }))?;
            }
            format!("moved to {}", value.replace('_', " "))
        }
        "add_note" => {
            if !dry {
                call("add_ticket_note", json!({ "id": ticket_id, "body": say }))?;
            }
            "added a note".into()
        }
        "send_reply" => {
            if !dry {
                call("reply_ticket", json!({ "id": ticket_id, "body": say }))?;
            }
            "sent a reply".into()
        }
        "send_email" => {
            let to: Vec<String> = if target == "requester" { text(t, "requester_email").map(|e| vec![e.to_string()]).unwrap_or_default() } else { target.split(',').map(|a| a.trim().to_string()).filter(|a| a.contains('@')).collect() };
            if to.is_empty() {
                return Err(Error::msg("there is nobody to email"));
            }
            if !dry {
                let subject = fill(text(spec, "subject").filter(|s| !s.is_empty()).unwrap_or("[{{reference}}] {{subject}}"), &values);
                communication::email(to.iter().map(String::as_str)).subject(&subject).text(&say).send()?;
            }
            format!("emailed {}", to.join(", "))
        }
        "notify" => {
            let who = people(&target, t)?;
            if !dry && !who.is_empty() {
                let title = fill(text(spec, "subject").filter(|s| !s.is_empty()).unwrap_or("Ticket {{reference}}"), &values);
                Notification::new(title).body(say).link(format!("/helpdesk/tickets/{}", ticket_id.rsplit(':').next().unwrap_or(ticket_id))).to(Audience::Actors(who.clone())).send()?;
            }
            format!("told {} {}", who.len(), if who.len() == 1 { "person" } else { "people" })
        }
        "escalate" => {
            let all: Vec<Record> = plugins::call("helpdesk", "search_priorities", &json!({ "active_only": true }))?;
            let level = |id: Option<&str>| all.iter().find(|p| text(p, "id") == id).and_then(|p| p.get("level").and_then(Value::as_i64)).unwrap_or(0);
            let now_level = level(text(t, "priority"));
            let next = all.iter().filter(|p| p.get("level").and_then(Value::as_i64).unwrap_or(0) > now_level).min_by_key(|p| p.get("level").and_then(Value::as_i64).unwrap_or(0)).cloned();
            match next {
                Some(p) => {
                    if !dry {
                        call("update_ticket", json!({ "id": ticket_id, "priority": p["id"] }))?;
                        let who = people(&target, t)?;
                        if !who.is_empty() {
                            let _ = Notification::new(format!("Escalated: {}", text(t, "reference").unwrap_or(""))).body(say).link(format!("/helpdesk/tickets/{}", ticket_id.rsplit(':').next().unwrap_or(ticket_id))).to(Audience::Actors(who)).send();
                        }
                    }
                    format!("priority raised to {}", text(&p, "name").unwrap_or("the next level"))
                }
                None => "already at the highest priority".into(),
            }
        }
        "set_due" => {
            let minutes: i64 = value.parse().unwrap_or(0);
            if !dry {
                call("set_ticket_due", json!({ "id": ticket_id, "due_at": format_datetime(now()? + Duration::minutes(minutes)) }))?;
            }
            format!("due in {minutes} minutes")
        }
        other => return Err(Error::msg(format!("`{other}` is not an action"))),
    };
    Ok(line)
}

fn rules_for(trigger: &str) -> Result<Vec<Record>> {
    db::find("hd_rule").filter("active", true).filter("trigger", trigger).order_by("rank").expand("conditions").expand("actions").limit(500).all()
}

fn log(rule: &Record, t: &Record, trigger: &str, ok: bool, summary: &str) -> Result<()> {
    let id = text(rule, "id").unwrap_or_default();
    db::create::<Record>("hd_rule_run", &json!({ "rule": id, "ticket": t.get("id"), "ticket_reference": t.get("reference"), "trigger": trigger, "at": now_text()?, "ok": ok, "summary": summary.chars().take(1000).collect::<String>() }))?;
    let n = rule.get("run_count").and_then(Value::as_i64).unwrap_or(0) + 1;
    db::update::<Record>("hd_rule", id, &json!({ "run_count": n, "last_run_at": now_text()? }))?;
    Ok(())
}

/// May this rule fire for this ticket now? A rule that fires once per ticket waits for its repeat hours; any rule that
/// has fired three times for a ticket within two minutes is held back, so rules that undo each other cannot loop.
fn may_fire(rule: &Record, ticket: &str) -> Result<bool> {
    let id = text(rule, "id").unwrap_or_default();
    let n = now()?;
    let recent = db::count("hd_rule_run", db::Filter::eq("rule", id).and(db::Filter::eq("ticket", ticket)).and(db::Filter::gte("at", format_datetime(n - Duration::minutes(2)))))?;
    if recent >= 3 {
        return Ok(false);
    }
    if rule.get("once_per_ticket") == Some(&json!(true)) {
        let last: Vec<Record> = db::find("hd_rule_run").filter("rule", id).filter("ticket", ticket).filter("ok", true).order_by("-at").limit(1).all()?;
        if let Some(prev) = last.first() {
            let repeat = rule.get("repeat_hours").and_then(Value::as_i64).unwrap_or(0);
            return Ok(repeat > 0 && moment(prev, "at").is_some_and(|p| n - p >= Duration::hours(repeat)));
        }
    }
    Ok(true)
}

/// Run one rule on one ticket whose facts are known; returns the lines of what it did when it fired.
fn fire_if_matching(rule: &Record, ticket_id: &str, t: &Record, trigger: &str, look: &mut Lookups) -> Result<Option<Vec<String>>> {
    let facts = facts(t, now()?);
    if !conditions_hold(rule, &facts, look) || !may_fire(rule, ticket_id)? {
        return Ok(None);
    }
    let actions = rule.get("actions").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut lines = Vec::new();
    for a in &actions {
        match act(a, ticket_id, t, look, false) {
            Ok(l) => lines.push(l),
            Err(e) => {
                lines.push(format!("stopped: {e}"));
                log(rule, t, trigger, false, &lines.join("; "))?;
                return Ok(Some(lines));
            }
        }
    }
    log(rule, t, trigger, true, &lines.join("; "))?;
    Ok(Some(lines))
}

/// Something happened to a ticket: run the rules for it, in order.
pub fn on_event(trigger: &str, ticket_id: &str, depth: u32) -> Result<Vec<Value>> {
    if depth > 3 {
        return Ok(Vec::new());
    }
    let rules = rules_for(trigger)?;
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let mut look = Lookups::default();
    let mut done = Vec::new();
    for rule in rules {
        let t: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": ticket_id }))?;
        if let Some(lines) = fire_if_matching(&rule, ticket_id, &t, trigger, &mut look)? {
            done.push(json!({ "rule": rule["name"], "did": lines }));
            if rule.get("stop_after") == Some(&json!(true)) {
                break;
            }
        }
    }
    Ok(done)
}

const PAGE: u32 = 40;

/// Every ten minutes: the rules that watch the clock look at every open ticket. The tickets are checked in small
/// background jobs, so no single call has to walk them all.
pub fn sweep() -> Result<Value> {
    if rules_for("time")?.is_empty() {
        return Ok(json!({ "rules": 0, "pages": 0 }));
    }
    let first: Value = plugins::call("helpdesk_ticket", "list_tickets", &json!({ "limit": 1, "filter": { "state": { "in": OPEN } } }))?;
    let total = first.get("total").and_then(Value::as_u64).unwrap_or(0) as u32;
    let pages = total.div_ceil(PAGE).min(250);
    let stamp = now_text()?.chars().take(15).collect::<String>();
    for p in 0..pages {
        scheduler::enqueue("automation_sweep_page").payload(&json!({ "offset": p * PAGE })).unique_key(&format!("sweep:{stamp}:{p}")).send()?;
    }
    Ok(json!({ "rules": 1, "tickets": total, "pages": pages }))
}

/// One slice of the open tickets, against every clock rule.
pub fn sweep_page(offset: u32) -> Result<Value> {
    let rules = rules_for("time")?;
    if rules.is_empty() {
        return Ok(json!({ "fired": 0 }));
    }
    let mut look = Lookups::default();
    let page: Value = plugins::call("helpdesk_ticket", "list_tickets", &json!({ "limit": PAGE, "offset": offset, "order": "reference", "filter": { "state": { "in": OPEN } } }))?;
    let rows = page.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut fired = 0;
    for t in &rows {
        let id = text(t, "id").unwrap_or_default().to_string();
        for rule in &rules {
            if fire_if_matching(rule, &id, t, "time", &mut look)?.is_some() {
                fired += 1;
                if rule.get("stop_after") == Some(&json!(true)) {
                    break;
                }
            }
        }
    }
    Ok(json!({ "checked": rows.len(), "fired": fired }))
}
