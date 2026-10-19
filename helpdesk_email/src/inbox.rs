use aether_sdk::dates::{format_datetime, parse_datetime, Duration};
use aether_sdk::db::Filter;
use aether_sdk::prelude::*;

use crate::mail::{base_address, normalize, trim_quoted, Inbound};

type Record = aether_sdk::records::Record;

const AGENT_ROLE: &str = "helpdesk.hd_agent";
const MANAGER_ROLE: &str = "helpdesk.hd_manager";
const ADMIN_ROLE: &str = "helpdesk.hd_admin";

#[derive(Deserialize)]
struct NewMailbox {
    address: String,
    name: String,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize)]
struct SetMailbox {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize)]
struct NewBlock {
    pattern: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize, Default)]
struct Page {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    filter: Option<Value>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
}

#[derive(Deserialize)]
struct Ingest {
    mailbox: String,
    #[serde(flatten)]
    message: Value,
}

fn is_manager() -> Result<bool> {
    let ctx = context::current()?;
    Ok(ctx.actor.kind != context::ActorKind::Visitor && (ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE)))
}

fn require_manager() -> Result<()> {
    if is_manager()? {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{MANAGER_ROLE}` to do this")))
    }
}

fn require_agent() -> Result<()> {
    let ctx = context::current()?;
    if ctx.actor.kind != context::ActorKind::Visitor && [AGENT_ROLE, MANAGER_ROLE, ADMIN_ROLE].iter().any(|r| ctx.has_role(r)) {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{AGENT_ROLE}` to do this")))
    }
}

fn now() -> Result<String> {
    let ctx = context::current()?;
    if ctx.now.is_empty() {
        return Err(Error::msg("the kernel did not say what time it is"));
    }
    Ok(ctx.now)
}

fn text(r: &Record, k: &str) -> Option<String> {
    r.get(k).and_then(Value::as_str).map(str::to_string)
}

fn opt(v: Option<String>) -> Option<String> {
    v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

/// A secret nobody can guess: the request's random id mixed with the time and what it is for.
fn new_secret(what: &str) -> Result<String> {
    let ctx = context::current()?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{}|{}|{}", ctx.request_id, ctx.now, what).bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let rid: String = ctx.request_id.chars().filter(char::is_ascii_alphanumeric).collect();
    Ok(format!("{rid}{hash:016x}"))
}

fn safe(mut mailbox: Record, manager: bool) -> Record {
    if !manager {
        if let Some(o) = mailbox.as_object_mut() {
            o.remove("secret");
        }
    }
    mailbox
}

fn make_mailbox(input: NewMailbox) -> Result<Record> {
    require_manager()?;
    let address = input.address.trim().to_lowercase();
    if !address.contains('@') || address.contains(' ') {
        return Err(Error::msg("a mailbox is an email address"));
    }
    if db::find::<Record>("hd_mailbox").filter("address", address.as_str()).first()?.is_some() {
        return Err(Error::msg("that address is already a mailbox"));
    }
    db::create(
        "hd_mailbox",
        &json!({
            "address": address, "name": input.name.trim(), "team": opt(input.team), "ticket_type": opt(input.ticket_type), "priority": opt(input.priority),
            "secret": new_secret(&address)?, "active": true, "received_count": 0, "notes": opt(input.notes),
        }),
    )
}

fn set_mailbox(input: SetMailbox) -> Result<Record> {
    require_manager()?;
    let mut data = json!({});
    if let Some(n) = opt(input.name) {
        data["name"] = json!(n);
    }
    for (k, v) in [("team", input.team), ("ticket_type", input.ticket_type), ("priority", input.priority), ("notes", input.notes)] {
        if let Some(v) = v {
            data[k] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    }
    if let Some(a) = input.active {
        data["active"] = json!(a);
    }
    db::update("hd_mailbox", &input.id, &data)?.ok_or_else(|| Error::msg("there is no such mailbox"))
}

fn pattern_blocks(pattern: &str, email: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    let e = email.to_lowercase();
    if let Some(domain) = p.strip_prefix('@') {
        e.ends_with(&format!("@{domain}")) || e.ends_with(&format!(".{domain}"))
    } else {
        e == p
    }
}

fn blocked_by(email: &str) -> Result<Option<Record>> {
    let blocks: Vec<Record> = db::find("hd_email_block").filter("active", true).limit(2000).all()?;
    Ok(blocks.into_iter().find(|b| text(b, "pattern").is_some_and(|p| pattern_blocks(&p, email))))
}

fn store(mailbox: &Record, message: &Inbound) -> Result<Record> {
    db::create(
        "hd_email_inbox",
        &json!({
            "mailbox": mailbox["address"], "from_email": message.from_email, "from_name": message.from_name, "subject": message.subject.chars().take(300).collect::<String>(),
            "message_id": message.message_id, "payload": serde_json::to_value(message)?, "state": "queued", "attempts": 0, "received_at": now()?,
        }),
    )
}

/// The public door. The secret in the message names the mailbox it is for; without a valid one nothing is kept.
fn inbound(payload: Value) -> Result<Value> {
    let secret = payload.get("secret").and_then(Value::as_str).map(str::to_string).or_else(|| payload.get("headers").or_else(|| payload.get("Headers")).and_then(|h| match h {
        Value::Object(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case("x-aether-secret")).and_then(|(_, v)| v.as_str().map(str::to_string)),
        Value::Array(rows) => rows.iter().find(|r| r.get("Name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case("x-aether-secret"))).and_then(|r| r.get("Value").and_then(Value::as_str).map(str::to_string)),
        _ => None,
    }));
    let Some(secret) = secret.filter(|s| s.len() >= 20) else { return Err(Error::msg("not accepted")) };
    let mailbox = db::find::<Record>("hd_mailbox").filter("secret", secret.as_str()).filter("active", true).first()?;
    let Some(mailbox) = mailbox else { return Err(Error::msg("not accepted")) };
    let message = normalize(&payload).map_err(Error::msg)?;
    if let Some(mid) = &message.message_id {
        if db::find::<Record>("hd_email_inbox").filter("message_id", mid.as_str()).filter("mailbox", text(&mailbox, "address").unwrap_or_default()).first()?.is_some() {
            return Ok(json!({ "accepted": true, "duplicate": true }));
        }
    }
    let row = store(&mailbox, &message)?;
    let id = text(&row, "id").unwrap_or_default();
    scheduler::enqueue("email_process").payload(&json!({ "id": id })).unique_key(&format!("inbox:{id}")).send()?;
    Ok(json!({ "accepted": true, "id": id }))
}

fn finish(id: &str, state: &str, outcome: &str, ticket: Option<&str>, error: Option<String>) -> Result<Record> {
    db::update("hd_email_inbox", id, &json!({ "state": state, "outcome": outcome, "ticket": ticket, "error": error.map(|e| e.chars().take(500).collect::<String>()), "processed_at": now()? }))?.ok_or_else(|| Error::msg("the message is gone"))
}

fn process(id: &str) -> Result<Record> {
    let row = db::find::<Record>("hd_email_inbox").filter("id", id).first()?.ok_or_else(|| Error::msg("there is no such message"))?;
    if matches!(text(&row, "state").as_deref(), Some("processed" | "ignored" | "blocked")) {
        return Ok(row);
    }
    let attempts = row.get("attempts").and_then(Value::as_i64).unwrap_or(0) + 1;
    db::update::<Record>("hd_email_inbox", id, &json!({ "attempts": attempts }))?;
    let message: Inbound = serde_json::from_value(row.get("payload").cloned().unwrap_or(Value::Null)).map_err(|e| Error::msg(format!("the stored message is unreadable: {e}")))?;
    if let Some(block) = blocked_by(&message.from_email)? {
        db::increment::<Record>("hd_email_block", text(&block, "id").unwrap_or_default().as_str(), "hits", 1.0)?;
        return finish(id, "blocked", &format!("blocked sender ({})", text(&block, "pattern").unwrap_or_default()), None, None);
    }
    if message.spam {
        return finish(id, "ignored", "marked as spam by the mail server", None, None);
    }
    let boxes: Vec<Record> = db::find("hd_mailbox").limit(500).all()?;
    if boxes.iter().any(|b| text(b, "address").is_some_and(|a| a == base_address(&message.from_email))) {
        return finish(id, "ignored", "sent from one of our own mailboxes (a mail loop)", None, None);
    }
    let hour_ago = format_datetime(parse_datetime(&now()?)? - Duration::hours(1));
    let recent = db::count("hd_email_inbox", Filter::eq("from_email", message.from_email.as_str()).and(Filter::eq("state", "processed")).and(Filter::gte("received_at", hour_ago)))?;
    if recent >= 20 {
        return finish(id, "ignored", "too many messages from this sender in an hour", None, None);
    }
    let mailbox = boxes.iter().find(|b| text(b, "address") == text(&row, "mailbox")).cloned().unwrap_or(Value::Null);
    let is_reply = message.in_reply_to.is_some() || message.subject.to_lowercase().trim_start().starts_with("re:") || message.subject.contains("[HD-");
    let body: String = if is_reply { trim_quoted(&message.text) } else { message.text.trim().to_string() };
    let body: String = body.chars().take(20_000).collect();
    let sent: Result<Value> = plugins::call(
        "helpdesk_ticket",
        "ingest_email",
        &json!({
            "from_email": message.from_email, "from_name": message.from_name, "subject": message.subject, "text": body, "message_id": message.message_id,
            "in_reply_to": message.in_reply_to, "references": message.references, "auto_submitted": message.auto_submitted, "precedence": message.precedence,
            "team": mailbox.get("team"), "ticket_type": mailbox.get("ticket_type"), "priority": mailbox.get("priority"), "reply_to": mailbox.get("address"),
        }),
    );
    match sent {
        Ok(answer) => {
            if let Some(why) = answer.get("ignored").and_then(Value::as_str) {
                return finish(id, "ignored", why, None, None);
            }
            if let Some(mid) = text(&mailbox, "id") {
                let n = mailbox.get("received_count").and_then(Value::as_i64).unwrap_or(0) + 1;
                db::update::<Record>("hd_mailbox", &mid, &json!({ "received_count": n, "last_received_at": now()? }))?;
            }
            let outcome = if answer.get("duplicate") == Some(&json!(true)) { "already received" } else if answer.get("follow_up_of").is_some() { "follow-up to a closed ticket" } else if answer.get("created") == Some(&json!(true)) { "opened a ticket" } else { "added to a ticket" };
            finish(id, "processed", outcome, answer.get("ticket").and_then(Value::as_str), None)
        }
        Err(e) => finish(id, "failed", "could not be handed to the ticket plugin", None, Some(e.to_string())),
    }
}

fn retry_failed() -> Result<Value> {
    let rows: Vec<Record> = db::find("hd_email_inbox").matching(Filter::one_of("state", ["failed", "queued"])).limit(500).all()?;
    let (mut tried, mut fixed) = (0, 0);
    let cutoff = format_datetime(parse_datetime(&now()?)? - Duration::minutes(5));
    for r in rows {
        let attempts = r.get("attempts").and_then(Value::as_i64).unwrap_or(0);
        let old = text(&r, "received_at").is_some_and(|t| t < cutoff);
        if attempts >= 5 || !old {
            continue;
        }
        tried += 1;
        if text(&process(&text(&r, "id").unwrap_or_default())?, "state").as_deref() == Some("processed") {
            fixed += 1;
        }
    }
    Ok(json!({ "tried": tried, "processed": fixed }))
}

fn list_inbox(input: Page) -> Result<Value> {
    require_agent()?;
    let mut filter = Filter::all();
    if let Some(Value::Object(map)) = input.filter.clone() {
        if !map.is_empty() {
            filter = filter.and(Filter::from_json(Value::Object(map)));
        }
    }
    if let Some(s) = input.state.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        filter = filter.and(Filter::eq("state", s));
    }
    db::search("hd_email_inbox", input.q.as_deref(), &["subject", "from_email", "from_name", "mailbox", "message_id"], filter, Some("-received_at"), input.limit.or(Some(50)), input.offset)
}

fn overview() -> Result<Value> {
    require_agent()?;
    let week = format_datetime(parse_datetime(&now()?)? - Duration::days(7));
    let by_state = db::aggregate("hd_email_inbox", Filter::gte("received_at", week.as_str()), &["state"], &[("messages", db::Figure::Count)])?;
    let by_mailbox = db::aggregate("hd_email_inbox", Filter::gte("received_at", week.as_str()), &["mailbox"], &[("messages", db::Figure::Count)])?;
    Ok(json!({
        "mailboxes": db::count("hd_mailbox", Filter::eq("active", true))?,
        "blocked_senders": db::count("hd_email_block", Filter::eq("active", true))?,
        "failed": db::count("hd_email_inbox", Filter::eq("state", "failed"))?,
        "waiting": db::count("hd_email_inbox", Filter::eq("state", "queued"))?,
        "by_state": by_state, "by_mailbox": by_mailbox,
    }))
}

handler! {
    fn create_mailbox(input: NewMailbox) -> Record {
        make_mailbox(input)
    }

    fn update_mailbox(input: SetMailbox) -> Record {
        set_mailbox(input)
    }

    fn rotate_mailbox_secret(input: Id) -> Record {
        require_manager()?;
        let m = db::find::<Record>("hd_mailbox").filter("id", input.id.as_str()).first()?.ok_or_else(|| Error::msg("there is no such mailbox"))?;
        db::update("hd_mailbox", &input.id, &json!({ "secret": new_secret(&text(&m, "address").unwrap_or_default())? }))?.ok_or_else(|| Error::msg("the mailbox is gone"))
    }

    fn list_mailboxes(_: Empty) -> Vec<Record> {
        require_agent()?;
        let manager = is_manager()?;
        let rows: Vec<Record> = db::find("hd_mailbox").order_by("address").limit(500).all()?;
        Ok(rows.into_iter().map(|m| safe(m, manager)).collect())
    }

    fn get_mailbox(input: Id) -> Record {
        require_agent()?;
        let m = db::find::<Record>("hd_mailbox").filter("id", input.id.as_str()).first()?.ok_or_else(|| Error::msg("there is no such mailbox"))?;
        Ok(safe(m, is_manager()?))
    }

    fn add_email_block(input: NewBlock) -> Record {
        require_manager()?;
        let p = input.pattern.trim().to_lowercase();
        if !(p.contains('@')) {
            return Err(Error::msg("block an address, or @domain.com for a whole domain"));
        }
        if db::find::<Record>("hd_email_block").filter("pattern", p.as_str()).first()?.is_some() {
            return Err(Error::msg("that is already blocked"));
        }
        db::create("hd_email_block", &json!({ "pattern": p, "reason": opt(input.reason), "active": true, "hits": 0, "created_by": context::current()?.actor.id }))
    }

    fn remove_email_block(input: Id) -> Option<Record> {
        require_manager()?;
        db::delete("hd_email_block", &input.id)
    }

    fn list_email_blocks(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_email_block").order_by("pattern").limit(2000).all()
    }

    /// The address a provider or relay posts each message to. Open to anyone; the secret in the message is the key.
    fn email_inbound(input: Value) -> Value {
        inbound(input)
    }

    /// An agent feeds a message in by hand (a forwarded mail, a test), through the same checks.
    fn email_ingest(input: Ingest) -> Value {
        require_agent()?;
        let m = db::find::<Record>("hd_mailbox").filter("id", input.mailbox.as_str()).first()?.ok_or_else(|| Error::msg("there is no such mailbox"))?;
        let message = normalize(&input.message).map_err(Error::msg)?;
        let row = store(&m, &message)?;
        let done = process(&text(&row, "id").unwrap_or_default())?;
        Ok(json!({ "state": done["state"], "outcome": done.get("outcome"), "ticket": done.get("ticket"), "error": done.get("error") }))
    }

    /// The background job for one stored message.
    fn email_process(input: Id) -> Record {
        require_agent()?;
        process(&input.id)
    }

    fn retry_email(input: Id) -> Record {
        require_agent()?;
        db::update::<Record>("hd_email_inbox", &input.id, &json!({ "state": "queued", "attempts": 0 }))?;
        process(&input.id)
    }

    fn email_retry_sweep(_: Empty) -> Value {
        require_agent()?;
        retry_failed()
    }

    fn list_email_inbox(input: Page) -> Value {
        list_inbox(input)
    }

    fn email_overview(_: Empty) -> Value {
        overview()
    }
}

