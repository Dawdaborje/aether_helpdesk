//! The clocks. A ticket gets one for its first reply and one for its resolution, from the first policy that fits it.
//! A clock counts business minutes. It stops while the ticket waits on the customer or a third party, starts again
//! when the ticket does, is done when the reply goes out or the ticket is resolved, and starts over when the ticket
//! is reopened. A background sweep warns before a clock runs out and records when one did.

use std::collections::HashMap;

use aether_sdk::dates::NaiveDateTime;
use aether_sdk::db::Filter;
use aether_sdk::events::Received;
use aether_sdk::prelude::*;

use crate::common::*;
use crate::hours::Schedule;

/// Policies and their working patterns, read once per call.
struct Book {
    cache: HashMap<String, (Record, Schedule)>,
}

impl Book {
    fn new() -> Self {
        Self { cache: HashMap::new() }
    }

    fn get(&mut self, policy: &str) -> Result<&(Record, Schedule)> {
        if !self.cache.contains_key(policy) {
            let record = db::find::<Record>("hd_sla_policy").filter("id", policy).expand("targets").first()?.ok_or_else(|| Error::msg("the policy is gone"))?;
            let schedule = schedule_for(text(&record, "hours"))?;
            self.cache.insert(policy.to_string(), (record, schedule));
        }
        Ok(&self.cache[policy])
    }
}

fn policy_matches(policy: &Record, ticket: &Record) -> bool {
    ["priority", "ticket_type", "team", "customer", "channel"].iter().all(|f| match text(policy, f) {
        Some(want) => text(ticket, f) == Some(want),
        None => true,
    })
}

fn match_policy(ticket: &Record) -> Result<Option<Record>> {
    let policies: Vec<Record> = db::find("hd_sla_policy").filter("active", true).order_by("rank").expand("targets").limit(500).all()?;
    Ok(policies.into_iter().find(|p| policy_matches(p, ticket)))
}

/// (first reply, resolution) targets in business minutes for a ticket's priority.
fn targets(policy: &Record, priority: Option<&str>) -> (i64, i64) {
    let rows = policy.get("targets").and_then(Value::as_array).cloned().unwrap_or_default();
    let pick = rows.iter().find(|t| priority.is_some() && text(t, "priority") == priority).or_else(|| rows.iter().find(|t| text(t, "priority").is_none()));
    match pick {
        Some(t) => (t.get("response_minutes").and_then(Value::as_i64).unwrap_or(0), t.get("resolution_minutes").and_then(Value::as_i64).unwrap_or(0)),
        None => (0, 0),
    }
}

fn used_now(clock: &Record, schedule: &Schedule, now: NaiveDateTime) -> i64 {
    let before = clock.get("elapsed_minutes").and_then(Value::as_i64).unwrap_or(0);
    match (text(clock, "state"), moment(clock, "segment_start")) {
        (Some("running"), Some(start)) => before + schedule.between(start, now),
        _ => before,
    }
}

fn active_clocks(ticket: &str) -> Result<Vec<Record>> {
    db::find("hd_sla_clock").filter("ticket", ticket).matching(Filter::one_of("state", ["running", "paused"])).limit(20).all()
}

fn should_pause(policy: &Record, ticket_state: &str) -> bool {
    (ticket_state == "pending" && policy.get("pause_when_pending") != Some(&json!(false))) || (ticket_state == "on_hold" && policy.get("pause_when_on_hold") != Some(&json!(false)))
}

fn event_body(clock: &Record, ticket: &Record, extra: Value) -> Value {
    let mut body = json!({
        "ticket": ticket["id"], "reference": ticket["reference"], "subject": ticket["subject"], "kind": clock["kind"], "clock": clock["id"],
        "policy": clock["policy"], "target_minutes": clock["target_minutes"], "assignee": ticket.get("assignee"), "team": ticket.get("team"), "priority": ticket.get("priority"),
    });
    if let (Some(base), Some(more)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            base.insert(k.clone(), v.clone());
        }
    }
    body
}

/// The ticket's own due date follows the soonest running clock.
fn refresh_due(ticket: &str) -> Result<()> {
    let all: Vec<Record> = db::find("hd_sla_clock").filter("ticket", ticket).limit(50).all()?;
    if all.is_empty() {
        return Ok(());
    }
    let soonest = all.iter().filter(|c| text(c, "state") == Some("running")).filter_map(|c| moment(c, "due_at")).min();
    let any_active = all.iter().any(|c| matches!(text(c, "state"), Some("running" | "paused")));
    let due = match (soonest, any_active) {
        (Some(d), _) => Some(stamp(d)),
        (None, true) => return Ok(()),
        (None, false) => None,
    };
    let _: Record = plugins::call("helpdesk_ticket", "set_ticket_due", &json!({ "id": ticket, "due_at": due }))?;
    Ok(())
}

fn new_clock(ticket: &Record, policy: &Record, kind: &str, target: i64, elapsed: i64, cycle: i64, paused: bool, schedule: &Schedule, now: NaiveDateTime) -> Result<Record> {
    let remaining = (target - elapsed).max(0);
    db::create(
        "hd_sla_clock",
        &json!({
            "ticket": ticket["id"], "ticket_reference": ticket["reference"], "policy": policy["id"], "kind": kind, "state": if paused { "paused" } else { "running" },
            "cycle": cycle, "target_minutes": target, "elapsed_minutes": elapsed, "segment_start": if paused { Value::Null } else { json!(stamp(now)) },
            "started_at": stamp(now), "due_at": if paused { Value::Null } else { json!(stamp(schedule.add(now, remaining))) },
            "breached": false, "warned": false, "assignee": ticket.get("assignee"), "team": ticket.get("team"), "priority": ticket.get("priority"),
        }),
    )
}

/// Give a ticket the clocks its policy asks for, or move the ones it has to a new policy or target. Safe to repeat.
pub fn attach(ticket_id: &str) -> Result<()> {
    let ticket = ticket(ticket_id)?;
    let state = text(&ticket, "state").unwrap_or("new").to_string();
    let now = now()?;
    let existing = active_clocks(ticket_id)?;
    if matches!(state.as_str(), "merged" | "closed") {
        return Ok(());
    }
    let mut book = Book::new();
    let Some(policy) = match_policy(&ticket)? else {
        for c in &existing {
            db::update::<Record>("hd_sla_clock", id_of(c)?, &json!({ "state": "cancelled", "segment_start": Value::Null }))?;
        }
        return refresh_due(ticket_id);
    };
    let policy_id = id_of(&policy)?.to_string();
    let schedule = book.get(&policy_id)?.1.clone();
    let (resp, res) = targets(&policy, text(&ticket, "priority"));
    let done: Vec<Record> = db::find("hd_sla_clock").filter("ticket", ticket_id).filter("state", "done").limit(50).all()?;
    for (kind, target) in [("response", resp), ("resolution", res)] {
        let have = existing.iter().find(|c| text(c, "kind") == Some(kind));
        if target <= 0 {
            if let Some(c) = have {
                db::update::<Record>("hd_sla_clock", id_of(c)?, &json!({ "state": "cancelled", "segment_start": Value::Null }))?;
            }
            continue;
        }
        if let Some(c) = have {
            if text(c, "policy") == Some(policy_id.as_str()) && c.get("target_minutes").and_then(Value::as_i64) == Some(target) {
                continue;
            }
            let old_schedule = match text(c, "policy") {
                Some(p) => book.get(p)?.1.clone(),
                None => Schedule::round_the_clock(),
            };
            let used = used_now(c, &old_schedule, now);
            let running = text(c, "state") == Some("running");
            db::update::<Record>(
                "hd_sla_clock",
                id_of(c)?,
                &json!({
                    "policy": policy_id, "target_minutes": target, "elapsed_minutes": used, "warned": false,
                    "segment_start": if running { json!(stamp(now)) } else { Value::Null },
                    "due_at": if running { json!(stamp(schedule.add(now, (target - used).max(0)))) } else { c.get("due_at").cloned().unwrap_or(Value::Null) },
                    "priority": ticket.get("priority"), "team": ticket.get("team"), "assignee": ticket.get("assignee"),
                }),
            )?;
            continue;
        }
        if done.iter().any(|c| text(c, "kind") == Some(kind)) {
            continue;
        }
        if (kind == "response" && ticket.get("first_response_at").is_some_and(|v| !v.is_null())) || (kind == "resolution" && state == "resolved") {
            continue;
        }
        let opened = moment(&ticket, "opened_at").unwrap_or(now);
        let elapsed = schedule.between(opened, now);
        new_clock(&ticket, &policy, kind, target, elapsed, 1, should_pause(&policy, &state), &schedule, now)?;
    }
    refresh_due(ticket_id)
}

/// Pause the clocks of a ticket that now waits on someone else, and start the paused ones when it stops waiting.
fn follow_state(ticket_id: &str, state: &str) -> Result<()> {
    let now = now()?;
    let mut book = Book::new();
    for c in active_clocks(ticket_id)? {
        let Some(pid) = text(&c, "policy").map(str::to_string) else { continue };
        let (policy, schedule) = book.get(&pid)?.clone();
        let pause = should_pause(&policy, state);
        match (text(&c, "state"), pause) {
            (Some("running"), true) => {
                let used = used_now(&c, &schedule, now);
                db::update::<Record>("hd_sla_clock", id_of(&c)?, &json!({ "state": "paused", "elapsed_minutes": used, "segment_start": Value::Null }))?;
            }
            (Some("paused"), false) => {
                let target = c.get("target_minutes").and_then(Value::as_i64).unwrap_or(0);
                let used = c.get("elapsed_minutes").and_then(Value::as_i64).unwrap_or(0);
                db::update::<Record>(
                    "hd_sla_clock",
                    id_of(&c)?,
                    &json!({ "state": "running", "segment_start": stamp(now), "due_at": stamp(schedule.add(now, (target - used).max(0))), "warned": false }),
                )?;
            }
            _ => {}
        }
    }
    refresh_due(ticket_id)
}

/// Stop the clocks of these kinds: the reply went out, or the ticket was solved.
fn finish(ticket_id: &str, kinds: &[&str]) -> Result<()> {
    let now = now()?;
    let mut book = Book::new();
    let mut ticket_row: Option<Record> = None;
    for c in active_clocks(ticket_id)? {
        if !kinds.contains(&text(&c, "kind").unwrap_or("")) {
            continue;
        }
        let schedule = match text(&c, "policy") {
            Some(p) => book.get(p)?.1.clone(),
            None => Schedule::round_the_clock(),
        };
        let used = used_now(&c, &schedule, now);
        let target = c.get("target_minutes").and_then(Value::as_i64).unwrap_or(0);
        let late = !c.get("breached").and_then(Value::as_bool).unwrap_or(false) && used > target && text(&c, "state") == Some("running");
        let mut changes = json!({ "state": "done", "met_at": stamp(now), "used_minutes": used, "segment_start": Value::Null });
        if late {
            changes["breached"] = json!(true);
            changes["breached_at"] = json!(stamp(now));
        }
        let updated: Record = db::update("hd_sla_clock", id_of(&c)?, &changes)?.ok_or_else(|| Error::msg("the clock is gone"))?;
        if late {
            let t = match &ticket_row {
                Some(t) => t.clone(),
                None => {
                    let t = ticket(ticket_id)?;
                    ticket_row = Some(t.clone());
                    t
                }
            };
            events::emit("ticket_sla_breached", &event_body(&updated, &t, json!({ "used_minutes": used, "late": true })))?;
        }
    }
    refresh_due(ticket_id)
}

fn cancel_all(ticket_id: &str) -> Result<()> {
    for c in active_clocks(ticket_id)? {
        db::update::<Record>("hd_sla_clock", id_of(&c)?, &json!({ "state": "cancelled", "segment_start": Value::Null }))?;
    }
    Ok(())
}

/// A reopened ticket gets a fresh resolution clock.
fn restart_resolution(ticket_id: &str) -> Result<()> {
    let ticket = ticket(ticket_id)?;
    let now = now()?;
    if active_clocks(ticket_id)?.iter().any(|c| text(c, "kind") == Some("resolution")) {
        return Ok(());
    }
    let Some(policy) = match_policy(&ticket)? else { return Ok(()) };
    let (_, res) = targets(&policy, text(&ticket, "priority"));
    if res <= 0 {
        return Ok(());
    }
    let schedule = schedule_for(text(&policy, "hours"))?;
    let past: Vec<Record> = db::find("hd_sla_clock").filter("ticket", ticket_id).filter("kind", "resolution").limit(100).all()?;
    let cycle = past.iter().filter_map(|c| c.get("cycle").and_then(Value::as_i64)).max().unwrap_or(0) + 1;
    new_clock(&ticket, &policy, "resolution", res, 0, cycle, false, &schedule, now)?;
    refresh_due(ticket_id)
}

fn follow_assignment(ticket_id: &str) -> Result<()> {
    let t = ticket(ticket_id)?;
    for c in active_clocks(ticket_id)? {
        db::update::<Record>("hd_sla_clock", id_of(&c)?, &json!({ "assignee": t.get("assignee"), "team": t.get("team") }))?;
    }
    Ok(())
}

/// Every five minutes: warn the owner of a clock that is nearly out, and record the clocks that ran out.
fn sweep() -> Result<Value> {
    let now = now()?;
    let mut book = Book::new();
    let running: Vec<Record> = db::find("hd_sla_clock").filter("state", "running").limit(10_000).all()?;
    let (mut breached, mut warned) = (0, 0);
    for c in running {
        let Some(pid) = text(&c, "policy").map(str::to_string) else { continue };
        let (policy, schedule) = book.get(&pid)?.clone();
        let used = used_now(&c, &schedule, now);
        let target = c.get("target_minutes").and_then(Value::as_i64).unwrap_or(0);
        if target <= 0 || c.get("breached") == Some(&json!(true)) {
            continue;
        }
        let late = moment(&c, "due_at").is_some_and(|d| d <= now) || used >= target;
        let warn_at = policy.get("warn_percent").and_then(Value::as_i64).unwrap_or(80);
        let near = c.get("warned") != Some(&json!(true)) && used * 100 >= target * warn_at;
        if !late && !near {
            continue;
        }
        let t = ticket(text(&c, "ticket").unwrap_or_default())?;
        let reference = text(&t, "reference").unwrap_or("").to_string();
        let kind = if text(&c, "kind") == Some("response") { "first reply" } else { "resolution" };
        let mut people: Vec<String> = text(&t, "assignee").map(|a| vec![a.to_string()]).unwrap_or_default();
        if late {
            let updated: Record = db::update("hd_sla_clock", id_of(&c)?, &json!({ "breached": true, "breached_at": stamp(now), "warned": true }))?.ok_or_else(|| Error::msg("the clock is gone"))?;
            events::emit("ticket_sla_breached", &event_body(&updated, &t, json!({ "used_minutes": used })))?;
            people.extend(text(&policy, "escalate_to").map(str::to_string));
            people.dedup();
            tell(&people, &format!("SLA missed on {reference}"), &format!("The {kind} target of {target} minutes ran out."), text(&t, "id").unwrap_or_default());
            let _: Result<Value> = plugins::call("helpdesk_ticket", "add_ticket_note", &json!({ "id": t["id"], "body": format!("SLA missed: the {kind} target of {target} business minutes ran out.") }));
            breached += 1;
        } else {
            let updated: Record = db::update("hd_sla_clock", id_of(&c)?, &json!({ "warned": true }))?.ok_or_else(|| Error::msg("the clock is gone"))?;
            events::emit("ticket_sla_warning", &event_body(&updated, &t, json!({ "used_minutes": used, "percent": used * 100 / target })))?;
            tell(&people, &format!("SLA running out on {reference}"), &format!("{used} of {target} minutes of the {kind} target are used."), text(&t, "id").unwrap_or_default());
            warned += 1;
        }
    }
    Ok(json!({ "breached": breached, "warned": warned }))
}

fn ticket_of(received: &Received) -> Result<String> {
    received.payload.get("ticket").and_then(Value::as_str).map(str::to_string).ok_or_else(|| Error::msg("the event names no ticket"))
}

fn changed_matters(received: &Received) -> bool {
    received.payload.get("changed").and_then(Value::as_array).is_some_and(|c| c.iter().filter_map(Value::as_str).any(|f| matches!(f, "priority" | "ticket_type" | "team" | "customer")))
}

handler! {
    fn on_ticket_created(received: Received) -> Value {
        attach(&ticket_of(&received)?)?;
        Ok(json!({}))
    }

    fn on_ticket_updated(received: Received) -> Value {
        if changed_matters(&received) {
            attach(&ticket_of(&received)?)?;
        }
        Ok(json!({}))
    }

    fn on_ticket_assigned(received: Received) -> Value {
        follow_assignment(&ticket_of(&received)?)?;
        Ok(json!({}))
    }

    fn on_status_changed(received: Received) -> Value {
        let to = received.payload.get("to").and_then(Value::as_str).unwrap_or("");
        if matches!(to, "new" | "open" | "pending" | "on_hold") {
            follow_state(&ticket_of(&received)?, to)?;
        }
        Ok(json!({}))
    }

    fn on_ticket_replied(received: Received) -> Value {
        if received.payload.get("by").and_then(Value::as_str) == Some("agent") {
            finish(&ticket_of(&received)?, &["response"])?;
        }
        Ok(json!({}))
    }

    fn on_ticket_resolved(received: Received) -> Value {
        finish(&ticket_of(&received)?, &["response", "resolution"])?;
        Ok(json!({}))
    }

    fn on_ticket_reopened(received: Received) -> Value {
        restart_resolution(&ticket_of(&received)?)?;
        Ok(json!({}))
    }

    fn on_ticket_merged(received: Received) -> Value {
        cancel_all(&ticket_of(&received)?)?;
        refresh_due(&ticket_of(&received)?)?;
        Ok(json!({}))
    }

    fn sla_sweep(_: Empty) -> Value {
        require_manager()?;
        sweep()
    }

    /// Work the clocks out again for one ticket (after a policy change, or to back-fill).
    fn recalculate_ticket_sla(input: TicketIn) -> Value {
        require_manager()?;
        attach(&input.ticket)?;
        Ok(json!({ "ok": true }))
    }
}

#[derive(Deserialize)]
struct TicketIn {
    #[serde(alias = "id")]
    ticket: String,
}

/// A clock as people read it: what is left, and whether it is on track.
pub fn describe(clock: &Record, book_now: NaiveDateTime, schedule: &Schedule, warn_percent: i64) -> Value {
    let target = clock.get("target_minutes").and_then(Value::as_i64).unwrap_or(0);
    let used = if text(clock, "state") == Some("done") { clock.get("used_minutes").and_then(Value::as_i64).unwrap_or(0) } else { used_now(clock, schedule, book_now) };
    let breached = clock.get("breached") == Some(&json!(true));
    let status = match (text(clock, "state"), breached) {
        (Some("cancelled"), _) => "cancelled",
        (Some("done"), true) => "late",
        (Some("done"), false) => "met",
        (Some("paused"), _) => "paused",
        (_, true) => "breached",
        _ if target > 0 && used * 100 >= target * warn_percent => "at_risk",
        _ => "on_track",
    };
    let mut out = clock.clone();
    if let Some(o) = out.as_object_mut() {
        o.insert("used_minutes".into(), json!(used));
        o.insert("remaining_minutes".into(), json!(target - used));
        o.insert("percent".into(), json!(if target > 0 { used * 100 / target } else { 0 }));
        o.insert("status".into(), json!(status));
    }
    out
}

pub fn clock_view(clocks: Vec<Record>) -> Result<Vec<Value>> {
    let now = now()?;
    let mut book = Book::new();
    let mut out = Vec::new();
    for c in clocks {
        let (warn, schedule) = match text(&c, "policy") {
            Some(p) => {
                let (policy, schedule) = book.get(p)?;
                (policy.get("warn_percent").and_then(Value::as_i64).unwrap_or(80), schedule.clone())
            }
            None => (80, Schedule::round_the_clock()),
        };
        out.push(describe(&c, now, &schedule, warn));
    }
    Ok(out)
}
