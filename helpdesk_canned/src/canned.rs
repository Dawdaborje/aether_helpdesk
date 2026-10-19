use std::collections::HashMap;

use aether_sdk::prelude::*;

use crate::common::*;
use crate::merge::{render, words};

#[derive(Deserialize)]
struct NewReply {
    title: String,
    #[serde(default)]
    shortcut: Option<String>,
    body: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    then_state: Option<String>,
}

#[derive(Deserialize)]
struct SetReply {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    shortcut: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    then_state: Option<String>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Deserialize, Default)]
struct Find {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    shortcut: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Render {
    canned: String,
    #[serde(alias = "id")]
    ticket: String,
}

#[derive(Deserialize)]
struct SendIn {
    #[serde(alias = "id")]
    ticket: String,
    canned: String,
    #[serde(default)]
    extra: Option<String>,
    #[serde(default)]
    set_state: Option<String>,
    #[serde(default = "yes")]
    signature: bool,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    cc: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct SigIn {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

const STATES: &[&str] = &["none", "pending", "on_hold", "resolved"];

fn my_team_ids() -> Result<Vec<String>> {
    let teams: Vec<Record> = plugins::call("helpdesk", "my_teams", &json!({}))?;
    Ok(teams.iter().filter_map(|t| text(t, "id").map(str::to_string)).collect())
}

/// Replies the caller may use: their own, their teams', and everyone's.
fn visible(reply: &Record, me: Option<&str>, teams: &[String], manager: bool) -> bool {
    match text(reply, "scope") {
        Some("global") => true,
        Some("team") => manager || text(reply, "team").is_some_and(|t| teams.iter().any(|x| x == t)),
        _ => text(reply, "owner") == me,
    }
}

fn load_visible() -> Result<Vec<Record>> {
    require_agent()?;
    let me = actor()?;
    let teams = my_team_ids()?;
    let manager = is_manager()?;
    let all: Vec<Record> = db::find("hd_canned_reply").filter("active", true).limit(2000).all()?;
    Ok(all.into_iter().filter(|r| visible(r, me.as_deref(), &teams, manager)).collect())
}

fn check_scope(scope: &str, team: Option<&str>) -> Result<()> {
    match scope {
        "personal" => Ok(()),
        "global" => {
            if is_manager()? {
                Ok(())
            } else {
                Err(Error::msg("only a manager shares a reply with everyone"))
            }
        }
        "team" => {
            let team = team.ok_or_else(|| Error::msg("name the team that shares this reply"))?;
            if is_manager()? || my_team_ids()?.iter().any(|t| t == team) {
                Ok(())
            } else {
                Err(Error::msg("you can share a reply only with a team you belong to"))
            }
        }
        other => Err(Error::msg(format!("`{other}` is not a scope: personal, team or global"))),
    }
}

fn check_state(state: &str) -> Result<()> {
    if STATES.contains(&state) {
        Ok(())
    } else {
        Err(Error::msg("a reply can leave the ticket as it is, pending, on hold or resolved"))
    }
}

fn create(input: NewReply) -> Result<Record> {
    require_agent()?;
    let scope = input.scope.clone().unwrap_or_else(|| "personal".into());
    let team = opt(input.team.clone());
    check_scope(&scope, team.as_deref())?;
    let then = input.then_state.clone().unwrap_or_else(|| "none".into());
    check_state(&then)?;
    let shortcut = opt(input.shortcut).map(|s| s.trim_start_matches('/').to_lowercase());
    if let Some(s) = &shortcut {
        let taken: Vec<Record> = db::find("hd_canned_reply").filter("shortcut", s.as_str()).filter("active", true).limit(20).all()?;
        let me = actor()?;
        if taken.iter().any(|r| text(r, "scope") != Some("personal") || text(r, "owner") == me.as_deref()) {
            return Err(Error::msg(format!("the shortcut /{s} is already taken")));
        }
    }
    db::create(
        "hd_canned_reply",
        &json!({
            "title": clean("a title", &input.title)?, "shortcut": shortcut, "body": clean("the text", &input.body)?, "scope": scope,
            "owner": if scope == "personal" { actor()? } else { None }, "team": if scope == "team" { team } else { None },
            "ticket_type": opt(input.ticket_type), "category": opt(input.category), "then_state": then, "active": true, "use_count": 0,
        }),
    )
}

fn update(input: SetReply) -> Result<Record> {
    require_agent()?;
    let reply = require("hd_canned_reply", &input.id, "canned reply")?;
    let mine = text(&reply, "owner") == actor()?.as_deref();
    let editable = match text(&reply, "scope") {
        Some("personal") => mine,
        Some("team") => is_manager()? || my_team_ids()?.iter().any(|t| Some(t.as_str()) == text(&reply, "team")),
        _ => is_manager()?,
    };
    if !editable {
        return Err(Error::msg("you cannot change this reply"));
    }
    let mut data = json!({});
    if let Some(t) = opt(input.title) {
        data["title"] = json!(t);
    }
    if let Some(b) = opt(input.body) {
        data["body"] = json!(b);
    }
    if let Some(s) = input.shortcut {
        data["shortcut"] = if s.trim().is_empty() { Value::Null } else { json!(s.trim().trim_start_matches('/').to_lowercase()) };
    }
    for (k, v) in [("team", input.team), ("ticket_type", input.ticket_type), ("category", input.category)] {
        if let Some(v) = v {
            data[k] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    }
    if let Some(s) = input.then_state {
        check_state(&s)?;
        data["then_state"] = json!(s);
    }
    if let Some(a) = input.active {
        data["active"] = json!(a);
    }
    db::update("hd_canned_reply", &input.id, &data)?.ok_or_else(|| Error::msg("the reply is gone"))
}

fn signature_of(user: Option<&str>) -> Result<Option<Record>> {
    match user {
        Some(u) => db::find("hd_signature").filter("owner", u).first(),
        None => Ok(None),
    }
}

/// What the merge fields stand for on this ticket.
fn values_for(ticket: &Record) -> Result<HashMap<String, String>> {
    let mut v: HashMap<String, String> = HashMap::new();
    let s = |k: &str| text(ticket, k).unwrap_or("").to_string();
    v.insert("ticket.reference".into(), s("reference"));
    v.insert("ticket.subject".into(), s("subject"));
    v.insert("ticket.state".into(), s("state").replace('_', " "));
    let mut name = s("requester_name");
    if name.is_empty() {
        if let Some(c) = text(ticket, "customer") {
            if let Ok(p) = plugins::call::<Record>("party", "get_party", &json!({ "id": c })) {
                name = text(&p, "display_name").or(text(&p, "name")).unwrap_or("").to_string();
            }
        }
    }
    v.insert("requester.first_name".into(), name.split_whitespace().next().unwrap_or("").to_string());
    v.insert("requester.name".into(), name.clone());
    v.insert("customer.name".into(), name);
    v.insert("requester.email".into(), s("requester_email"));
    let sig = signature_of(actor()?.as_deref())?;
    v.insert("agent.name".into(), sig.as_ref().and_then(|g| text(g, "display_name")).unwrap_or("").to_string());
    let company = text(ticket, "company").unwrap_or_default();
    let settings: Value = plugins::call("helpdesk", "get_settings", &json!({ "id": company })).unwrap_or(Value::Null);
    v.insert("company.name".into(), settings.get("business_name").and_then(Value::as_str).unwrap_or("").to_string());
    Ok(v)
}

fn render_reply(input: &Render) -> Result<Value> {
    require_agent()?;
    let reply = require("hd_canned_reply", &input.canned, "canned reply")?;
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": input.ticket }))?;
    let (body, missing) = render(text(&reply, "body").unwrap_or(""), &values_for(&ticket)?);
    Ok(json!({ "title": reply["title"], "body": body, "missing": missing, "then_state": reply.get("then_state") }))
}

fn send(input: SendIn) -> Result<Record> {
    require_agent()?;
    let reply = require("hd_canned_reply", &input.canned, "canned reply")?;
    let me = actor()?;
    if !visible(&reply, me.as_deref(), &my_team_ids()?, is_manager()?) || reply.get("active") == Some(&json!(false)) {
        return Err(Error::msg("you cannot use this reply"));
    }
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": input.ticket }))?;
    let (mut body, missing) = render(text(&reply, "body").unwrap_or(""), &values_for(&ticket)?);
    if !missing.is_empty() {
        return Err(Error::msg(format!("these fields have no value on this ticket: {}. Edit the reply or fill them in", missing.join(", "))));
    }
    if let Some(extra) = opt(input.extra) {
        body.push_str("\n\n");
        body.push_str(&extra);
    }
    if input.signature {
        if let Some(sig) = signature_of(me.as_deref())?.and_then(|g| text(&g, "body").map(str::to_string)) {
            body.push_str("\n\n");
            body.push_str(&sig);
        }
    }
    let state = opt(input.set_state).or_else(|| text(&reply, "then_state").filter(|s| *s != "none").map(str::to_string));
    let message: Record = plugins::call(
        "helpdesk_ticket",
        "reply_ticket",
        &json!({ "id": input.ticket, "body": body, "set_state": state, "to": input.to, "cc": input.cc }),
    )?;
    db::update::<Record>(
        "hd_canned_reply",
        id_of(&reply)?,
        &json!({ "use_count": reply.get("use_count").and_then(Value::as_i64).unwrap_or(0) + 1, "last_used_at": now()? }),
    )?;
    Ok(message)
}

/// Replies likely to fit a ticket: shared words with its subject and last customer message, a matching type first.
fn suggest(ticket_id: &str) -> Result<Vec<Value>> {
    let replies = load_visible()?;
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": ticket_id }))?;
    let thread: Vec<Record> = plugins::call("helpdesk_ticket", "ticket_thread", &json!({ "id": ticket_id })).unwrap_or_default();
    let last = thread.iter().rev().find(|m| text(m, "direction") == Some("inbound")).and_then(|m| text(m, "body")).unwrap_or("");
    let wanted = words(&format!("{} {}", text(&ticket, "subject").unwrap_or(""), last));
    let mut scored: Vec<(i64, Record)> = replies
        .into_iter()
        .map(|r| {
            let have = words(&format!("{} {} {}", text(&r, "title").unwrap_or(""), text(&r, "body").unwrap_or(""), text(&r, "category").unwrap_or("")));
            let overlap = wanted.iter().filter(|w| have.contains(w)).count() as i64;
            let type_bonus = if text(&r, "ticket_type").is_some() && text(&r, "ticket_type") == text(&ticket, "ticket_type") { 5 } else { 0 };
            (overlap * 10 + type_bonus * 10 + r.get("use_count").and_then(Value::as_i64).unwrap_or(0).min(9), r)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(scored.into_iter().take(8).map(|(score, mut r)| {
        if let Some(o) = r.as_object_mut() {
            o.insert("score".into(), json!(score));
        }
        r
    }).collect())
}

const STARTERS: &[(&str, &str, &str, &str, &str)] = &[
    ("Greeting", "hello", "Hello {{requester.first_name|there}},\n\nThank you for contacting {{company.name|us}}. I am looking into {{ticket.reference}} now.", "none", "General"),
    ("Need more details", "details", "Hello {{requester.first_name|there}},\n\nTo help with \"{{ticket.subject}}\" I need a little more information: what you were doing when it happened, what you expected, and what you saw instead. Screenshots help.", "pending", "General"),
    ("Fixed, please confirm", "fixed", "Hello {{requester.first_name|there}},\n\nWe believe \"{{ticket.subject}}\" is solved. Please tell us if it still happens; otherwise we will close {{ticket.reference}} in a few days.", "resolved", "Closing"),
    ("Sorry for the delay", "sorry", "Hello {{requester.first_name|there}},\n\nI am sorry you waited for an answer on {{ticket.reference}}. It is being worked on now and I will keep you posted.", "none", "General"),
    ("Closing", "bye", "Hello {{requester.first_name|there}},\n\nI am closing {{ticket.reference}} because we have not heard back. Reply any time to reopen it.\n\n{{agent.name|The support team}}", "resolved", "Closing"),
];

fn seed() -> Result<Value> {
    let mut made = 0;
    for (title, shortcut, body, then, category) in STARTERS {
        if db::find::<Record>("hd_canned_reply").filter("shortcut", *shortcut).filter("scope", "global").first()?.is_none() {
            db::create::<Record>("hd_canned_reply", &json!({ "title": title, "shortcut": shortcut, "body": body, "scope": "global", "then_state": then, "category": category, "active": true, "use_count": 0 }))?;
            made += 1;
        }
    }
    Ok(json!({ "created": made }))
}

handler! {
    fn seed_canned(_: Empty) -> Value {
        seed()
    }

    fn create_canned_reply(input: NewReply) -> Record {
        create(input)
    }

    fn update_canned_reply(input: SetReply) -> Record {
        update(input)
    }

    fn get_canned_reply(input: Id) -> Record {
        require_agent()?;
        let reply = require("hd_canned_reply", &input.id, "canned reply")?;
        if !visible(&reply, actor()?.as_deref(), &my_team_ids()?, is_manager()?) {
            return Err(Error::msg("you cannot use this reply"));
        }
        Ok(reply)
    }

    fn list_canned_replies(input: Find) -> Vec<Record> {
        let mut rows = load_visible()?;
        if let Some(s) = opt(input.shortcut) {
            let s = s.trim_start_matches('/').to_lowercase();
            rows.retain(|r| text(r, "shortcut") == Some(s.as_str()));
        }
        if let Some(q) = opt(input.q) {
            let q = q.to_lowercase();
            rows.retain(|r| ["title", "body", "shortcut", "category"].iter().any(|f| text(r, f).is_some_and(|t| t.to_lowercase().contains(&q))));
        }
        rows.sort_by(|a, b| b.get("use_count").and_then(Value::as_i64).cmp(&a.get("use_count").and_then(Value::as_i64)));
        Ok(rows)
    }

    fn suggest_canned_replies(input: Id) -> Vec<Value> {
        suggest(&input.id)
    }

    fn render_canned_reply(input: Render) -> Value {
        render_reply(&input)
    }

    fn reply_with_canned(input: SendIn) -> Record {
        send(input)
    }

    fn set_my_signature(input: SigIn) -> Record {
        require_agent()?;
        let me = actor()?.ok_or_else(|| Error::msg("sign in first"))?;
        let data = json!({ "owner": me, "display_name": opt(input.display_name), "body": opt(input.body) });
        match signature_of(Some(&me))? {
            Some(s) => db::update("hd_signature", id_of(&s)?, &data)?.ok_or_else(|| Error::msg("the signature is gone")),
            None => db::create("hd_signature", &data),
        }
    }

    fn get_my_signature(_: Empty) -> Value {
        require_agent()?;
        Ok(signature_of(actor()?.as_deref())?.unwrap_or_else(|| json!({ "display_name": null, "body": null })))
    }
}
