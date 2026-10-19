//! Merging duplicates, splitting a conversation out, and nesting.

use aether_sdk::prelude::*;

use crate::common::*;
use crate::tickets::{make, NewTicket};

#[derive(Deserialize)]
struct Merge {
    #[serde(alias = "id")]
    source: String,
    into: String,
    #[serde(default)]
    force: bool,
}

#[derive(Deserialize)]
struct MessageId {
    id: String,
}

#[derive(Deserialize)]
struct Split {
    id: String,
    message: String,
    #[serde(default)]
    subject: Option<String>,
}

fn merge(input: Merge) -> Result<Record> {
    require_agent()?;
    if input.source == input.into {
        return Err(Error::msg("a ticket cannot be merged into itself"));
    }
    let source = get_ticket_row(&input.source)?;
    let target = get_ticket_row(&input.into)?;
    let (sid, tid) = (id_of(&source)?.to_string(), id_of(&target)?.to_string());
    if matches!(text(&source, "state"), Some("merged" | "closed")) {
        return Err(Error::msg("only a ticket that is not closed or merged can be merged away"));
    }
    if matches!(text(&target, "state"), Some("merged" | "closed")) {
        return Err(Error::msg("merge into a ticket that is still being worked"));
    }
    if let (Some(a), Some(b)) = (text(&source, "customer"), text(&target, "customer")) {
        if a != b && !input.force {
            return Err(Error::msg("these tickets are from different customers: merge only if you are sure"));
        }
    }
    system_line(&tid, &format!("Merged {} into this ticket", text(&source, "reference").unwrap_or("")))?;
    let messages: Vec<Record> = db::find("hd_message").filter("ticket", sid.as_str()).limit(5000).all()?;
    let moved = messages.len() as i64;
    for m in &messages {
        db::update::<Record>("hd_message", id_of(m)?, &json!({ "ticket": tid }))?;
    }
    for e in db::find::<Record>("hd_time_entry").filter("ticket", sid.as_str()).limit(5000).all()? {
        db::update::<Record>("hd_time_entry", id_of(&e)?, &json!({ "ticket": tid }))?;
    }
    for t in db::find::<Record>("hd_ticket_tag").filter("ticket", sid.as_str()).limit(200).all()? {
        let have = db::count("hd_ticket_tag", db::Filter::eq("ticket", tid.as_str()).and(db::Filter::eq("tag", t["tag"].clone())))?;
        if have == 0 {
            db::update::<Record>("hd_ticket_tag", id_of(&t)?, &json!({ "ticket": tid }))?;
        }
    }
    for child in db::find::<Record>("hd_ticket").filter("parent", sid.as_str()).limit(500).all()? {
        db::update::<Record>("hd_ticket", id_of(&child)?, &json!({ "parent": tid }))?;
    }
    crate::tickets::refresh_tags_text(&tid)?;
    let now = now()?;
    let kept = target.get("message_count").and_then(Value::as_i64).unwrap_or(0) + moved;
    let mut tchanges = json!({ "message_count": kept, "last_activity_at": now });
    for field in ["first_response_at"] {
        let (a, b) = (moment(&source, field), moment(&target, field));
        if let (Some(a), Some(b)) = (a, b) {
            if a < b {
                tchanges[field] = json!(stamp(a));
            }
        } else if let (Some(a), None) = (a, b) {
            tchanges[field] = json!(stamp(a));
        }
    }
    db::update::<Record>("hd_ticket", &tid, &tchanges)?;
    let merged: Record = db::update("hd_ticket", &sid, &json!({ "state": "merged", "merged_into": tid, "message_count": 0, "last_activity_at": now }))?
        .ok_or_else(|| Error::msg("the ticket is gone"))?;
    system_line(&sid, &format!("Merged into {}", text(&target, "reference").unwrap_or("")))?;
    events::emit("ticket_merged", &event_body(&merged, json!({ "into": tid, "messages_moved": moved })))?;
    get_ticket_row(&tid)
}

fn split(input: Split) -> Result<Record> {
    require_agent()?;
    let ticket = get_ticket_row(&input.id)?;
    let message = require("hd_message", &input.message, "message")?;
    if text(&message, "ticket") != Some(input.id.as_str()) || text(&message, "kind") != Some("reply") {
        return Err(Error::msg("pick a reply of this ticket"));
    }
    let subject = opt(input.subject).unwrap_or_else(|| format!("{} (split)", text(&ticket, "subject").unwrap_or("")));
    let str_of = |k: &str| ticket.get(k).and_then(Value::as_str).map(str::to_string);
    let made = make(NewTicket {
        subject,
        customer: str_of("customer"),
        requester_name: str_of("requester_name"),
        requester_email: str_of("requester_email"),
        requester_phone: str_of("requester_phone"),
        team: str_of("team"),
        ticket_type: str_of("ticket_type"),
        priority: str_of("priority"),
        channel: str_of("channel"),
        requester_user: str_of("requester_user"),
        company: str_of("company"),
        quiet: true,
        ..Default::default()
    })?;
    let nid = id_of(&made)?.to_string();
    db::update::<Record>("hd_message", &input.message, &json!({ "ticket": nid }))?;
    db::update::<Record>("hd_ticket", &nid, &json!({ "split_from": input.id, "message_count": 1, "description": message.get("body") }))?;
    let left = ticket.get("message_count").and_then(Value::as_i64).unwrap_or(1) - 1;
    db::update::<Record>("hd_ticket", &input.id, &json!({ "message_count": left.max(0), "last_activity_at": now()? }))?;
    system_line(&input.id, &format!("A message was split into {}", text(&made, "reference").unwrap_or("")))?;
    system_line(&nid, &format!("Split from {}", text(&ticket, "reference").unwrap_or("")))?;
    get_ticket_row(&nid)
}

/// A list action on the conversation sends the message's id.
fn split_message(id: String) -> Result<Record> {
    require_agent()?;
    let message = require("hd_message", &id, "message")?;
    let ticket = text(&message, "ticket").unwrap_or_default().to_string();
    split(Split { id: ticket, message: id, subject: None })
}

handler! {
    fn split_ticket_message(input: MessageId) -> Record {
        split_message(input.id)
    }

    fn mergeable_tickets(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_ticket").matching(db::Filter::one_of("state", ["new", "open", "pending", "on_hold", "resolved"])).order_by("-last_activity_at").limit(500).all()
    }

    fn merge_tickets(input: Merge) -> Record {
        merge(input)
    }

    fn split_ticket(input: Split) -> Record {
        split(input)
    }
}
