//! The customer-facing side of the helpdesk: a help centre to search first, a form to ask for help, a page to follow a
//! request with its reference and key (no account needed) or from "my requests" (signed in), to write back, rate and
//! reopen. It holds no data of its own; it speaks to the ticket and knowledge-base plugins on the caller's behalf, so a
//! visitor can only do what those plugins let a visitor do.

use aether_sdk::prelude::*;

type Record = aether_sdk::records::Record;

#[derive(Deserialize)]
struct Submit {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    subject: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
}

#[derive(Deserialize)]
struct Open {
    reference: String,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Deserialize)]
struct Write {
    reference: String,
    #[serde(default)]
    key: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct Rating {
    reference: String,
    #[serde(default)]
    key: Option<String>,
    rating: i64,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize, Default)]
struct Browse {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    filter: Option<Value>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
}

/// How the caller is allowed to see a request: with the key from the email, or because it is theirs.
enum Access {
    Key(String),
    Account(String),
}

/// A call to another plugin. The kernel hides the reason when one plugin's refusal passes through another, so each
/// action says in its own words what a refusal there means.
fn ask<T: serde::de::DeserializeOwned>(plugin: &str, function: &str, input: &Value, refusal: &str) -> Result<T> {
    match plugins::call(plugin, function, input) {
        Err(Error::Message(m)) => Err(Error::Message(m)),
        Err(e) if e.to_string().contains("invocation failed") => Err(Error::msg(refusal)),
        other => other,
    }
}

fn signed_in() -> Result<bool> {
    Ok(context::current()?.actor.kind == context::ActorKind::User)
}

fn access(reference: &str, key: Option<&str>) -> Result<Access> {
    let key = key.map(str::trim).filter(|k| !k.is_empty() && *k != "-");
    if let Some(k) = key {
        return Ok(Access::Key(k.to_string()));
    }
    if !signed_in()? {
        return Err(Error::msg("sign in, or use the key from the email we sent you"));
    }
    let mine: Value = plugins::call("helpdesk_ticket", "my_requests", &json!({ "q": reference.trim(), "limit": 5 }))?;
    let rows = mine.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
    let hit = rows.iter().find(|t| t.get("reference").and_then(Value::as_str) == Some(reference.trim()));
    hit.and_then(|t| t.get("id").and_then(Value::as_str)).map(|id| Access::Account(id.to_string())).ok_or_else(|| Error::msg("there is no request with that reference"))
}

fn messages_of(thread: &[Value]) -> Vec<Value> {
    thread
        .iter()
        .filter(|m| m.get("kind").and_then(Value::as_str) == Some("reply"))
        .map(|m| json!({ "from": if m.get("direction").and_then(Value::as_str) == Some("inbound") { "you" } else { "support" }, "when": m["posted_at"], "body": m["body"] }))
        .collect()
}

fn view(reference: &str, key: Option<&str>) -> Result<Value> {
    match access(reference, key)? {
        Access::Key(k) => ask("helpdesk_ticket", "visitor_view", &json!({ "reference": reference, "key": k }), "there is no request with that reference and key"),
        Access::Account(id) => {
            let t: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": id }))?;
            let thread: Vec<Value> = plugins::call("helpdesk_ticket", "ticket_thread", &json!({ "id": id }))?;
            let state = t.get("state").and_then(Value::as_str).unwrap_or("new");
            Ok(json!({
                "reference": t["reference"], "subject": t["subject"], "state": state, "opened_at": t["opened_at"], "resolution": t.get("resolution"), "rating": t.get("rating"),
                "can_reply": !matches!(state, "closed" | "merged"), "can_rate": matches!(state, "resolved" | "closed") && t.get("rating").is_none_or(Value::is_null),
                "messages": messages_of(&thread),
            }))
        }
    }
}

fn submit(input: Submit) -> Result<Value> {
    if signed_in()? {
        let t: Record = plugins::call(
            "helpdesk_ticket",
            "submit_ticket",
            &json!({ "subject": input.subject, "description": input.description, "requester_name": input.name, "requester_email": input.email, "requester_phone": input.phone, "ticket_type": input.ticket_type }),
        )?;
        return Ok(json!({ "reference": t["reference"], "key": "-" }));
    }
    ask(
        "helpdesk_ticket",
        "visitor_submit",
        &json!({ "subject": input.subject, "description": input.description, "name": input.name, "email": input.email.unwrap_or_default(), "phone": input.phone, "ticket_type": input.ticket_type }),
        "we could not take this request: check your email address and subject, and wait a little if you have just sent several",
    )
}

fn reply(input: Write) -> Result<Value> {
    match access(&input.reference, input.key.as_deref())? {
        Access::Key(k) => ask("helpdesk_ticket", "visitor_reply", &json!({ "reference": input.reference, "key": k, "body": input.body }), "your message could not be sent: the request may be closed or the key wrong"),
        Access::Account(id) => {
            let _: Record = ask("helpdesk_ticket", "customer_reply_ticket", &json!({ "id": id, "body": input.body }), "your message could not be sent: the request may be closed")?;
            view(&input.reference, input.key.as_deref())
        }
    }
}

fn rate(input: Rating) -> Result<Value> {
    match access(&input.reference, input.key.as_deref())? {
        Access::Key(k) => ask("helpdesk_ticket", "visitor_rate", &json!({ "reference": input.reference, "key": k, "rating": input.rating, "comment": input.comment }), "a request can be rated once it is solved, with 1 to 5"),
        Access::Account(id) => {
            let _: Record = ask("helpdesk_ticket", "rate_ticket", &json!({ "id": id, "rating": input.rating, "comment": input.comment }), "a request can be rated once it is solved, with 1 to 5")?;
            view(&input.reference, input.key.as_deref())
        }
    }
}

fn reopen(input: Open) -> Result<Value> {
    match access(&input.reference, input.key.as_deref())? {
        Access::Key(k) => ask("helpdesk_ticket", "visitor_reopen", &json!({ "reference": input.reference, "key": k }), "only a solved request can be reopened"),
        Access::Account(id) => {
            let _: Record = plugins::call("helpdesk_ticket", "customer_reply_ticket", &json!({ "id": id, "body": "I still need help with this." }))?;
            view(&input.reference, input.key.as_deref())
        }
    }
}

handler! {
    /// Name of the business, for the page heading.
    fn portal_info(_: Empty) -> Value {
        let company: Option<Record> = plugins::call("company", "get_default_company", &json!({})).unwrap_or(None);
        let name = company.and_then(|c| plugins::call::<Value>("helpdesk", "get_settings", &json!({ "id": c["id"] })).ok()).and_then(|s| s.get("business_name").and_then(Value::as_str).map(str::to_string));
        Ok(json!({ "business_name": name, "signed_in": signed_in()? }))
    }

    /// The help centre's search, as the caller is allowed to see it.
    fn portal_articles(input: Browse) -> Value {
        plugins::call("helpdesk_kb", "kb_articles", &json!({ "q": input.q, "filter": input.filter, "category": input.category, "limit": input.limit, "offset": input.offset }))
    }

    fn portal_ticket_types(_: Empty) -> Vec<Value> {
        plugins::call("helpdesk", "public_ticket_types", &json!({}))
    }

    fn portal_submit(input: Submit) -> Value {
        submit(input)
    }

    /// Check a reference and key; answers them back for the page to open.
    fn portal_find(input: Open) -> Value {
        let seen = view(&input.reference, input.key.as_deref())?;
        Ok(json!({ "reference": seen["reference"], "key": input.key.unwrap_or_else(|| "-".into()) }))
    }

    fn portal_view(input: Open) -> Value {
        view(&input.reference, input.key.as_deref())
    }

    fn portal_messages(input: Open) -> Vec<Value> {
        Ok(view(&input.reference, input.key.as_deref())?["messages"].as_array().cloned().unwrap_or_default())
    }

    fn portal_reply(input: Write) -> Value {
        reply(input)
    }

    fn portal_rate(input: Rating) -> Value {
        rate(input)
    }

    fn portal_reopen(input: Open) -> Value {
        reopen(input)
    }

    /// Requests the signed-in caller asked for.
    fn portal_my(input: Browse) -> Value {
        if !signed_in()? {
            return Err(Error::msg("sign in to see your requests"));
        }
        plugins::call("helpdesk_ticket", "my_requests", &json!({ "q": input.q, "filter": input.filter, "limit": input.limit, "offset": input.offset }))
    }
}
