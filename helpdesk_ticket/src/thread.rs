//! The conversation: public replies (emailed to the requester), internal notes, what the requester writes back, and
//! email that arrives from outside.

use aether_sdk::db::Filter;

use aether_sdk::prelude::*;

use crate::common::*;
use crate::tickets::{make, move_to, NewTicket};

#[derive(Deserialize)]
struct Reply {
    id: String,
    body: String,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    cc: Option<String>,
    /// `pending`, `on_hold` or `resolved`: where the ticket goes once the reply is sent.
    #[serde(default, alias = "next_state")]
    set_state: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
}

#[derive(Deserialize)]
struct Note {
    id: String,
    body: String,
    #[serde(default)]
    mentions: Vec<String>,
}

#[derive(Deserialize)]
struct CustomerReply {
    id: String,
    body: String,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
pub struct Email {
    from_email: String,
    #[serde(default)]
    from_name: Option<String>,
    subject: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    in_reply_to: Option<String>,
    #[serde(default)]
    references: Option<String>,
    /// `Auto-Submitted` header: anything but `no` is a machine.
    #[serde(default)]
    auto_submitted: Option<String>,
    /// `Precedence` header: `bulk`, `list` and `junk` are not people.
    #[serde(default)]
    precedence: Option<String>,
    /// Routing chosen by the mailbox the message arrived at.
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    reply_to: Option<String>,
}

const AUTO_SUBJECTS: &[&str] = &["auto:", "automatic reply", "out of office", "undeliverable", "delivery status notification", "mail delivery failed", "autoreply"];

fn addresses(list: Option<&str>) -> Vec<String> {
    list.unwrap_or("").split([',', ';']).map(|a| a.trim().to_string()).filter(|a| a.contains('@')).collect()
}

/// `HD-000123` in a subject line, if any.
fn reference_in(subject: &str) -> Option<String> {
    let at = subject.find("HD-")?;
    let digits: String = subject[at + 3..].chars().take_while(char::is_ascii_digit).collect();
    if digits.len() >= 6 {
        Some(format!("HD-{digits}"))
    } else {
        None
    }
}

fn blocked(ticket: &Record) -> Result<()> {
    match text(ticket, "state") {
        Some("merged") => Err(Error::msg("this ticket was merged into another: continue there")),
        Some("closed") => Err(Error::msg("this ticket is closed: reopen it or start a new one")),
        _ => Ok(()),
    }
}

fn bump(ticket: &Record, id: &str, mut changes: Value) -> Result<()> {
    let count = ticket.get("message_count").and_then(Value::as_i64).unwrap_or(0) + 1;
    changes["message_count"] = json!(count);
    changes["last_activity_at"] = json!(now()?);
    db::update::<Record>("hd_ticket", id, &changes)?;
    Ok(())
}

fn reply(input: Reply) -> Result<Record> {
    require_agent()?;
    let ticket = get_ticket_row(&input.id)?;
    blocked(&ticket)?;
    let body = clean("a reply", &input.body)?;
    let now = now()?;
    let to = match opt(input.to.clone()) {
        Some(t) => addresses(Some(&t)),
        None => text(&ticket, "requester_email").map(|e| vec![e.to_string()]).unwrap_or_default(),
    };
    let cc = addresses(input.cc.as_deref());
    let subject = text(&ticket, "subject").unwrap_or("").to_string();
    let reference = text(&ticket, "reference").unwrap_or("").to_string();
    let mut all: Vec<String> = to.clone();
    all.extend(cc.clone());
    let (delivery, error) = if all.is_empty() || text(&ticket, "channel") == Some("internal") {
        ("none", None)
    } else {
        let signature = crate::tickets::settings_for(text(&ticket, "company").unwrap_or_default()).get("reply_signature").and_then(Value::as_str).map(str::to_string);
        let mailed = match signature {
            Some(sig) => format!("{body}\n\n-- \n{sig}"),
            None => body.clone(),
        };
        let mut mail = communication::email(all.iter().map(String::as_str)).subject(&format!("[{reference}] {subject}")).text(&mailed);
        if let Some(r) = text(&ticket, "reply_to") {
            mail = mail.reply_to(r);
        }
        match mail.send() {
            Ok(_) => ("queued", None),
            Err(e) => ("failed", Some(e.to_string())),
        }
    };
    let message: Record = db::create(
        "hd_message",
        &json!({
            "ticket": input.id, "kind": "reply", "direction": "outbound", "author_user": actor()?, "requester_user": ticket.get("requester_user"),
            "subject": subject, "body": body, "recipients": all.join(", "), "channel": text(&ticket, "channel"),
            "delivery": delivery, "delivery_error": error, "posted_at": now,
        }),
    )?;
    let mut changes = json!({ "last_agent_message_at": now, "waiting_for_us": false });
    if ticket.get("first_response_at").is_none_or(Value::is_null) {
        changes["first_response_at"] = json!(now);
        if let (Some(opened), Ok(at)) = (moment(&ticket, "opened_at"), aether_sdk::dates::parse_datetime(&now)) {
            changes["first_response_minutes"] = json!((at - opened).num_minutes().max(0));
        }
    }
    bump(&ticket, &input.id, changes)?;
    let mut updated = get_ticket_row(&input.id)?;
    match input.set_state.as_deref().filter(|s| !s.is_empty()) {
        Some(next @ ("pending" | "on_hold" | "resolved")) => updated = move_to(&input.id, next, input.resolution)?,
        Some(other) => return Err(Error::msg(format!("a reply can leave the ticket pending, on hold or resolved, not `{other}`"))),
        None if text(&updated, "state") == Some("new") => updated = move_to(&input.id, "open", None)?,
        None => {}
    }
    events::emit("ticket_replied", &event_body(&updated, json!({ "by": "agent", "message": message["id"], "public": true })))?;
    Ok(message)
}

fn note(input: Note) -> Result<Record> {
    require_agent()?;
    let ticket = get_ticket_row(&input.id)?;
    let body = clean("a note", &input.body)?;
    let message: Record = db::create(
        "hd_message",
        &json!({
            "ticket": input.id, "kind": "note", "direction": "internal", "author_user": actor()?, "requester_user": ticket.get("requester_user"),
            "subject": ticket.get("subject"), "body": body, "posted_at": now()?,
        }),
    )?;
    bump(&ticket, &input.id, json!({}))?;
    let people: Vec<String> = input.mentions.iter().map(|m| user_id(m)).collect();
    tell(&people, "You were mentioned on a ticket", &body, &input.id);
    Ok(message)
}

/// The requester writes back from the portal.
fn customer_reply(input: CustomerReply) -> Result<Record> {
    let ticket = get_ticket_row(&input.id)?;
    let me = actor()?.ok_or_else(|| Error::msg("sign in first"))?;
    if text(&ticket, "requester_user") != Some(me.as_str()) {
        return Err(Error::msg("this is not your ticket"));
    }
    blocked(&ticket)?;
    inbound(&ticket, &input.id, clean("a message", &input.body)?, "portal", None, None, None)
}

/// An inbound message from the requester: stores it, wakes the ticket and tells whoever has it.
pub fn inbound(ticket: &Record, id: &str, body: String, channel: &str, message_id: Option<String>, in_reply_to: Option<String>, from: Option<(String, Option<String>)>) -> Result<Record> {
    let now = now()?;
    let message: Record = db::create(
        "hd_message",
        &json!({
            "ticket": id, "kind": "reply", "direction": "inbound", "author_user": actor()?, "author_email": from.as_ref().map(|f| f.0.clone()).or(text(ticket, "requester_email").map(str::to_string)),
            "author_name": from.as_ref().and_then(|f| f.1.clone()).or(text(ticket, "requester_name").map(str::to_string)), "requester_user": ticket.get("requester_user"),
            "subject": ticket.get("subject"), "body": body, "channel": channel, "email_message_id": message_id, "in_reply_to": in_reply_to, "posted_at": now,
        }),
    )?;
    bump(ticket, id, json!({ "last_customer_message_at": now, "waiting_for_us": true }))?;
    let mut updated = get_ticket_row(id)?;
    match text(&updated, "state") {
        Some("pending" | "on_hold") => updated = move_to(id, "open", None)?,
        Some("resolved") => updated = move_to(id, "open", None)?,
        _ => {}
    }
    events::emit("ticket_replied", &event_body(&updated, json!({ "by": "customer", "message": message["id"], "public": true })))?;
    if let Some(a) = text(&updated, "assignee") {
        tell(&[a.to_string()], "The customer replied", text(&updated, "subject").unwrap_or(""), id);
    }
    Ok(message)
}

/// Email that arrived: a reply to a ticket is added to it (found by the reference in the subject, or by the message
/// it answers); anything else opens a ticket. Running it twice for one message-id changes nothing.
fn ingest(input: Email) -> Result<Value> {
    require_agent()?;
    let lowered = input.subject.to_lowercase();
    if AUTO_SUBJECTS.iter().any(|a| lowered.trim_start().starts_with(a)) {
        return Ok(json!({ "ignored": "automatic reply" }));
    }
    if input.auto_submitted.as_deref().is_some_and(|a| !a.trim().eq_ignore_ascii_case("no") && !a.trim().is_empty())
        || input.precedence.as_deref().is_some_and(|p| matches!(p.trim().to_lowercase().as_str(), "bulk" | "list" | "junk" | "auto_reply"))
    {
        return Ok(json!({ "ignored": "sent by a machine" }));
    }
    let from = input.from_email.trim().to_lowercase();
    if !from.contains('@') {
        return Err(Error::msg("the message has no sender address"));
    }
    if let Some(mid) = opt(input.message_id.clone()) {
        if let Some(seen) = db::find::<Record>("hd_message").filter("email_message_id", mid.as_str()).first()? {
            return Ok(json!({ "ticket": seen["ticket"], "message": seen["id"], "created": false, "duplicate": true }));
        }
    }
    let mut found: Option<Record> = None;
    if let Some(r) = reference_in(&input.subject) {
        found = db::find::<Record>("hd_ticket").filter("reference", r.as_str()).first()?;
    }
    if found.is_none() {
        let mut ids: Vec<String> = Vec::new();
        ids.extend(opt(input.in_reply_to.clone()));
        ids.extend(input.references.as_deref().unwrap_or("").split_whitespace().map(str::to_string));
        for mid in ids {
            if let Some(m) = db::find::<Record>("hd_message").filter("email_message_id", mid.as_str()).first()? {
                found = db::find::<Record>("hd_ticket").filter("id", text(&m, "ticket").unwrap_or_default()).first()?;
            } else if let Some(t) = db::find::<Record>("hd_ticket").filter("thread_key", mid.as_str()).first()? {
                found = Some(t);
            }
            if found.is_some() {
                break;
            }
        }
    }
    let body = input.text.clone().unwrap_or_default();
    if let Some(mut ticket) = found {
        if text(&ticket, "state") == Some("merged") {
            if let Some(into) = text(&ticket, "merged_into") {
                ticket = get_ticket_row(into)?;
            }
        }
        let id = id_of(&ticket)?.to_string();
        if text(&ticket, "state") == Some("closed") {
            // A reply to a closed ticket starts a follow-up that points back at it.
            let made = make(NewTicket {
                subject: input.subject.clone(),
                description: Some(body.clone()),
                customer: ticket.get("customer").and_then(Value::as_str).map(str::to_string),
                requester_name: input.from_name.clone(),
                requester_email: Some(from.clone()),
                team: ticket.get("team").and_then(Value::as_str).map(str::to_string),
                priority: ticket.get("priority").and_then(Value::as_str).map(str::to_string),
                ticket_type: ticket.get("ticket_type").and_then(Value::as_str).map(str::to_string),
                channel: Some("email".into()),
                parent: Some(id.clone()),
                thread_key: input.message_id.clone(),
                ..Default::default()
            })?;
            return Ok(json!({ "ticket": made["id"], "created": true, "follow_up_of": id }));
        }
        let message = inbound(&ticket, &id, body, "email", opt(input.message_id), opt(input.in_reply_to), Some((from, input.from_name)))?;
        return Ok(json!({ "ticket": id, "message": message["id"], "created": false }));
    }
    let made = make(NewTicket {
        subject: input.subject.trim().to_string(),
        description: Some(body),
        requester_name: input.from_name,
        requester_email: Some(from),
        channel: Some("email".into()),
        thread_key: input.message_id.clone(),
        team: input.team.clone(),
        ticket_type: input.ticket_type.clone(),
        priority: input.priority.clone(),
        reply_to: input.reply_to.clone(),
        ..Default::default()
    })?;
    if let Some(mid) = opt(input.message_id) {
        let first: Vec<Record> = db::find("hd_message").filter("ticket", id_of(&made)?).limit(1).all()?;
        if let Some(m) = first.first() {
            db::update::<Record>("hd_message", id_of(m)?, &json!({ "email_message_id": mid }))?;
        }
    }
    Ok(json!({ "ticket": made["id"], "created": true }))
}

fn thread(input: Id) -> Result<Vec<Value>> {
    let ticket = get_ticket_row(&input.id)?;
    require_can_see(&ticket)?;
    let agent = is_agent()?;
    let rows: Vec<Record> = db::find("hd_message").filter("ticket", input.id.as_str()).order_by("posted_at").limit(2000).all()?;
    Ok(rows
        .into_iter()
        .filter(|m| agent || text(m, "kind") == Some("reply"))
        .map(|m| {
            let label = text(&m, "author_name").or(text(&m, "author_user")).unwrap_or(if text(&m, "kind") == Some("system") { "System" } else { "" }).to_string();
            let visibility = if text(&m, "kind") == Some("note") { "internal" } else if text(&m, "kind") == Some("system") { "system" } else { "public" };
            let mut v = m;
            if let Some(o) = v.as_object_mut() {
                o.insert("author_label".into(), json!(label));
                o.insert("visibility".into(), json!(visibility));
            }
            v
        })
        .collect())
}

fn spam(input: Id) -> Result<Record> {
    require_agent()?;
    let ticket = get_ticket_row(&input.id)?;
    blocked(&ticket)?;
    if text(&ticket, "state") != Some("resolved") {
        move_to(&input.id, "resolved", Some("Marked as spam".into()))?;
    }
    db::update::<Record>("hd_ticket", &input.id, &json!({ "spam": true }))?;
    move_to(&input.id, "closed", None)
}

fn waiting_for_agent() -> Result<Vec<Record>> {
    require_agent()?;
    // Open tickets whose last word is the customer's: they wait on us. The flag is kept as messages arrive.
    db::find("hd_ticket").matching(Filter::eq("waiting_for_us", true).and(Filter::one_of("state", ["new", "open"]))).order_by("last_customer_message_at").limit(500).all()
}

handler! {
    fn reply_ticket(input: Reply) -> Record {
        reply(input)
    }

    fn add_ticket_note(input: Note) -> Record {
        note(input)
    }

    fn customer_reply_ticket(input: CustomerReply) -> Record {
        customer_reply(input)
    }

    fn ingest_email(input: Email) -> Value {
        ingest(input)
    }

    fn ticket_thread(input: Id) -> Vec<Value> {
        thread(input)
    }

    fn mark_ticket_spam(input: Id) -> Record {
        spam(input)
    }

    fn tickets_waiting_for_agent(_: Empty) -> Vec<Record> {
        waiting_for_agent()
    }
}
