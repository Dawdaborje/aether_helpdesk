//! People without an account. A visitor asks for help and gets a reference and a secret key; with the two they can
//! follow the ticket, write back and rate it. Everything here is open to anonymous callers, so each function asks
//! for the key itself and does only what the key allows. The slow parts (priority, team, owner, the thank-you email)
//! are left to a background job that runs with the plugin's own rights.

use aether_sdk::db::Filter;
use aether_sdk::prelude::*;

use crate::common::*;
use crate::thread::inbound;
use crate::tickets::{apply_rating, move_to};

#[derive(Deserialize)]
struct Submit {
    #[serde(default)]
    name: Option<String>,
    email: String,
    #[serde(default)]
    phone: Option<String>,
    subject: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    /// A field real people never see; bots fill it in.
    #[serde(default)]
    website: Option<String>,
}

#[derive(Deserialize)]
struct Open {
    reference: String,
    key: String,
}

#[derive(Deserialize)]
struct Write {
    reference: String,
    key: String,
    body: String,
}

#[derive(Deserialize)]
struct Rating {
    reference: String,
    key: String,
    rating: i64,
    #[serde(default)]
    comment: Option<String>,
}

/// A key nobody can guess: the request's own random id mixed with the time and the reference.
fn new_key(reference: &str) -> Result<String> {
    let ctx = context::current()?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{}|{}|{}", ctx.request_id, ctx.now, reference).bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let rid: String = ctx.request_id.chars().filter(char::is_ascii_alphanumeric).collect();
    Ok(format!("{rid}{hash:016x}"))
}

fn by_key(reference: &str, key: &str) -> Result<Record> {
    let ticket = db::find::<Record>("hd_ticket").filter("reference", reference.trim()).first()?;
    match ticket {
        Some(t) if !key.is_empty() && text(&t, "access_key") == Some(key.trim()) => Ok(t),
        _ => Err(Error::msg("there is no ticket with that reference and key")),
    }
}

fn plausible_email(email: &str) -> bool {
    let e = email.trim();
    e.len() <= 200 && e.split_once('@').is_some_and(|(local, host)| !local.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && !e.contains(' '))
}

fn submit(input: Submit) -> Result<Value> {
    if input.website.as_deref().is_some_and(|w| !w.trim().is_empty()) {
        return Ok(json!({ "reference": "HD-000000", "key": "" }));
    }
    let subject = clean("a subject", &input.subject)?;
    if subject.len() > 300 {
        return Err(Error::msg("keep the subject under 300 characters"));
    }
    if input.description.as_deref().is_some_and(|d| d.len() > 20_000) {
        return Err(Error::msg("keep the description under 20 000 characters"));
    }
    if !plausible_email(&input.email) {
        return Err(Error::msg("give an email address we can reply to"));
    }
    let email = input.email.trim().to_lowercase();
    let now = now()?;
    let hour_ago = stamp(aether_sdk::dates::parse_datetime(&now)? - aether_sdk::dates::Duration::hours(1));
    if db::count("hd_ticket", Filter::eq("requester_email", email.as_str()).and(Filter::gte("opened_at", hour_ago)))? >= 5 {
        return Err(Error::msg("too many requests from this address: wait a little before asking again"));
    }
    let reference = next_number("ticket", "HD-", 6)?;
    let key = new_key(&reference)?;
    let ticket: Record = db::create(
        "hd_ticket",
        &json!({
            "reference": reference, "subject": subject, "description": opt(input.description.clone()), "state": "new", "channel": "portal",
            "ticket_type": opt(input.ticket_type), "requester_name": opt(input.name), "requester_email": email, "requester_phone": opt(input.phone),
            "access_key": key, "intake_done": false, "opened_at": now, "last_activity_at": now,
        }),
    )?;
    let id = id_of(&ticket)?.to_string();
    if let Some(body) = opt(input.description) {
        db::create::<Record>(
            "hd_message",
            &json!({
                "ticket": id, "kind": "reply", "direction": "inbound", "author_name": ticket.get("requester_name"), "author_email": email,
                "subject": subject, "body": body, "channel": "portal", "posted_at": now,
            }),
        )?;
        db::update::<Record>("hd_ticket", &id, &json!({ "message_count": 1, "last_customer_message_at": now, "waiting_for_us": true }))?;
    }
    scheduler::enqueue("intake_finish").payload(&json!({ "id": id })).unique_key(&format!("intake:{id}")).send()?;
    Ok(json!({ "reference": reference, "key": key }))
}

fn view(input: Open) -> Result<Value> {
    let t = by_key(&input.reference, &input.key)?;
    let id = id_of(&t)?.to_string();
    let thread: Vec<Record> = db::find("hd_message").filter("ticket", id.as_str()).filter("kind", "reply").order_by("posted_at").limit(1000).all()?;
    let state = text(&t, "state").unwrap_or("new");
    Ok(json!({
        "reference": t["reference"], "subject": t["subject"], "state": state, "opened_at": t["opened_at"], "resolution": t.get("resolution"),
        "rating": t.get("rating"), "can_reply": !matches!(state, "closed" | "merged"), "can_rate": matches!(state, "resolved" | "closed") && t.get("rating").is_none_or(Value::is_null),
        "messages": thread.iter().map(|m| json!({
            "from": if text(m, "direction") == Some("inbound") { "you" } else { "support" },
            "when": m["posted_at"], "body": m["body"],
        })).collect::<Vec<_>>(),
    }))
}

fn reply(input: Write) -> Result<Value> {
    let t = by_key(&input.reference, &input.key)?;
    if matches!(text(&t, "state"), Some("closed" | "merged")) {
        return Err(Error::msg("this request is closed: ask again and mention the reference"));
    }
    let body = clean("a message", &input.body)?;
    if body.len() > 20_000 {
        return Err(Error::msg("keep the message under 20 000 characters"));
    }
    let id = id_of(&t)?.to_string();
    inbound(&t, &id, body, "portal", None, None, None)?;
    view(Open { reference: input.reference, key: input.key })
}

fn rate(input: Rating) -> Result<Value> {
    let t = by_key(&input.reference, &input.key)?;
    apply_rating(&t, input.rating, input.comment)?;
    view(Open { reference: input.reference, key: input.key })
}

/// A visitor may also confirm that the problem is solved (moves a resolved ticket to closed) or reopen it.
fn reopen(input: Open) -> Result<Value> {
    let t = by_key(&input.reference, &input.key)?;
    if text(&t, "state") != Some("resolved") {
        return Err(Error::msg("only a solved request can be reopened"));
    }
    move_to(id_of(&t)?, "open", None)?;
    view(input)
}

handler! {
    fn visitor_submit(input: Submit) -> Value {
        submit(input)
    }

    fn visitor_view(input: Open) -> Value {
        view(input)
    }

    fn visitor_reply(input: Write) -> Value {
        reply(input)
    }

    fn visitor_rate(input: Rating) -> Value {
        rate(input)
    }

    fn visitor_reopen(input: Open) -> Value {
        reopen(input)
    }
}
