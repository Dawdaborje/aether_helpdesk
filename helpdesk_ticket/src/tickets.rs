//! Creating, finding, changing and moving tickets.

use aether_sdk::db::Filter;

use crate::assign;
use aether_sdk::prelude::*;

use crate::common::*;

#[derive(Deserialize, Default)]
pub struct NewTicket {
    pub subject: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub customer: Option<String>,
    #[serde(default)]
    pub requester_name: Option<String>,
    #[serde(default)]
    pub requester_email: Option<String>,
    #[serde(default)]
    pub requester_phone: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub ticket_type: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, alias = "id")]
    pub parent: Option<String>,
    #[serde(default)]
    pub source_model: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub thread_key: Option<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Set by the plugin itself for a ticket raised for the caller.
    #[serde(skip)]
    pub requester_user: Option<String>,
    /// Set by the plugin when a ticket is made from another (split): no acknowledgement, no auto-assignment.
    #[serde(skip)]
    pub quiet: bool,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Change {
    id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    customer: Option<String>,
    #[serde(default)]
    requester_name: Option<String>,
    #[serde(default)]
    requester_email: Option<String>,
    #[serde(default)]
    requester_phone: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    due_at: Option<String>,
    #[serde(default)]
    source_model: Option<String>,
    #[serde(default)]
    source_id: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Page {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    filter: Option<Value>,
    #[serde(default)]
    order: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct StateIn {
    id: String,
    #[serde(alias = "status")]
    state: String,
    #[serde(default)]
    resolution: Option<String>,
}

/// What a board sends when a card is dropped on a column.
#[derive(Deserialize)]
struct MoveIn {
    id: String,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct DueIn {
    id: String,
    #[serde(default)]
    due_at: Option<String>,
}

#[derive(Deserialize)]
struct Resolve {
    id: String,
    #[serde(default)]
    resolution: Option<String>,
}

#[derive(Deserialize)]
struct Assign {
    id: String,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    team: Option<String>,
}

#[derive(Deserialize)]
struct TagIn {
    id: String,
    tag: String,
}

#[derive(Deserialize)]
struct Bulk {
    ids: Vec<String>,
    action: String,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize)]
struct Rate {
    id: String,
    rating: i64,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize)]
struct ForTeam {
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

// --- lookups in the masters and in party ---

pub fn settings_for(company: &str) -> Value {
    plugins::call::<Value>("helpdesk", "get_settings", &json!({ "id": company }))
        .unwrap_or_else(|_| json!({ "auto_close_days": 7, "send_acknowledgement": true }))
}

pub fn default_company() -> Result<String> {
    let found: Option<Record> = plugins::call("company", "get_default_company", &json!({}))?;
    found.and_then(|c| text(&c, "id").map(str::to_string)).ok_or_else(|| Error::msg("set up a company first"))
}

fn default_priority(settings: &Value) -> Option<String> {
    if let Some(p) = settings.get("default_priority").and_then(Value::as_str) {
        return Some(p.to_string());
    }
    let rows: Vec<Record> = plugins::call("helpdesk", "search_priorities", &json!({ "active_only": true })).ok()?;
    rows.iter().find(|p| p.get("level").and_then(Value::as_i64) == Some(2)).or(rows.first()).and_then(|p| text(p, "id").map(str::to_string))
}

fn party_by_email(email: &str) -> Option<Record> {
    let rows: Vec<Record> = plugins::call("party", "search", &json!({ "q": email })).ok()?;
    let mut hits = rows.into_iter().filter(|p| text(p, "email").is_some_and(|e| e.eq_ignore_ascii_case(email)));
    let first = hits.next()?;
    if hits.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn resolve_tags(wanted: &[String]) -> Result<Vec<Record>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let all: Vec<Record> = plugins::call("helpdesk", "search_tags", &json!({ "active_only": true }))?;
    let mut out = Vec::new();
    for w in wanted {
        let w = w.trim();
        let hit = all.iter().find(|t| {
            text(t, "id") == Some(w) || text(t, "code").is_some_and(|c| c.eq_ignore_ascii_case(w)) || text(t, "name").is_some_and(|n| n.eq_ignore_ascii_case(w))
        });
        out.push(hit.cloned().ok_or_else(|| Error::msg(format!("there is no tag `{w}`")))?);
    }
    Ok(out)
}

pub fn refresh_tags_text(ticket: &str) -> Result<()> {
    let links: Vec<Record> = db::find("hd_ticket_tag").filter("ticket", ticket).limit(100).all()?;
    let mut names = Vec::new();
    for l in links {
        if let Some(tag) = l.get("tag").and_then(Value::as_str) {
            let found: Record = plugins::call("helpdesk", "search_tags", &json!({})).ok().and_then(|rows: Vec<Record>| rows.into_iter().find(|t| text(t, "id") == Some(tag))).unwrap_or_default();
            if let Some(n) = text(&found, "name") {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    db::update::<Record>("hd_ticket", ticket, &json!({ "tags_text": names.join(", ") }))?;
    Ok(())
}

// --- creating ---

/// What a ticket gets when the request did not say: company, priority, team (from the request, the type, or the
/// settings) and the company's settings.
pub struct Defaults {
    pub company: String,
    pub settings: Value,
    pub priority: Option<String>,
    pub team: Option<String>,
}

pub fn defaults(company: Option<String>, ticket_type: Option<&str>, team: Option<String>, priority: Option<String>) -> Result<Defaults> {
    let company = match company {
        Some(c) => c,
        None => default_company()?,
    };
    let settings = settings_for(&company);
    let priority = priority.or_else(|| default_priority(&settings));
    let mut team = team;
    if team.is_none() {
        if let Some(t) = ticket_type {
            let types: Vec<Record> = plugins::call("helpdesk", "search_ticket_types", &json!({}))?;
            team = types.iter().find(|x| text(x, "id") == Some(t)).and_then(|x| text(x, "default_team")).map(str::to_string);
        }
    }
    if team.is_none() {
        team = settings.get("default_team").and_then(Value::as_str).map(str::to_string);
    }
    Ok(Defaults { company, settings, priority, team })
}

/// Link the requester to a party and fill the blanks of their contact details from it.
pub fn identify(customer: Option<String>, name: Option<String>, email: Option<String>, phone: Option<String>) -> Result<(Option<String>, Option<String>, Option<String>, Option<String>)> {
    let (mut customer, mut name, mut email, mut phone) = (customer, name, email.map(|e| e.to_lowercase()), phone);
    if let Some(id) = &customer {
        let party: Record = plugins::call("party", "get_party", &json!({ "id": id }))?;
        name = name.or_else(|| text(&party, "display_name").or(text(&party, "name")).map(str::to_string));
        email = email.or_else(|| text(&party, "email").map(str::to_lowercase));
        phone = phone.or_else(|| text(&party, "phone").or(text(&party, "mobile")).map(str::to_string));
    } else if let Some(e) = &email {
        if let Some(p) = party_by_email(e) {
            customer = text(&p, "id").map(str::to_string);
            name = name.or_else(|| text(&p, "display_name").or(text(&p, "name")).map(str::to_string));
        }
    }
    Ok((customer, name, email, phone))
}

/// Where a requester without an account follows their ticket, when the settings give the site's address.
pub fn track_link(ticket: &Record, settings: &Value) -> Option<String> {
    let base = settings.get("portal_url").and_then(Value::as_str).filter(|b| !b.trim().is_empty())?;
    let key = text(ticket, "access_key")?;
    Some(format!("{}/support/track/{}/{}", base.trim_end_matches('/'), text(ticket, "reference")?, key))
}

/// The thank-you email, best effort.
pub fn acknowledge(ticket: &Record, settings: &Value, link: Option<&str>) {
    if !settings.get("send_acknowledgement").and_then(Value::as_bool).unwrap_or(true) || matches!(text(ticket, "channel"), Some("internal" | "phone")) {
        return;
    }
    let Some(to) = text(ticket, "requester_email") else { return };
    let reference = text(ticket, "reference").unwrap_or("");
    let name = text(ticket, "requester_name").map(|n| format!(" {n}")).unwrap_or_default();
    let mut body = format!("Hello{name},\n\nWe received your request and will reply soon. Your reference is {reference}. Reply to this email to add more.");
    if let Some(l) = link {
        body.push_str(&format!("\n\nFollow it here: {l}"));
    }
    let mut mail = communication::email([to]).subject(&format!("[{reference}] We received your request: {}", text(ticket, "subject").unwrap_or(""))).text(&body);
    if let Some(r) = text(ticket, "reply_to") {
        mail = mail.reply_to(r);
    }
    let _ = mail.send();
}

pub fn make(input: NewTicket) -> Result<Record> {
    let subject = clean("a subject", &input.subject)?;
    let now = now()?;
    let (customer, name, email, phone) = identify(opt(input.customer.clone()), opt(input.requester_name.clone()), opt(input.requester_email.clone()), opt(input.requester_phone.clone()))?;
    let Defaults { company, settings, priority, team } = defaults(opt(input.company.clone()), input.ticket_type.as_deref(), opt(input.team.clone()), opt(input.priority.clone()))?;
    let tags = resolve_tags(&input.tags)?;
    let assignee = match opt(input.assignee.clone()) {
        Some(a) => Some(user_id(&a)),
        None => match (&team, input.quiet) {
            (Some(t), false) => assign::pick(t)?,
            _ => None,
        },
    };
    let channel = opt(input.channel.clone()).unwrap_or_else(|| "web".into());
    let ticket: Record = db::create(
        "hd_ticket",
        &json!({
            "reference": next_number("ticket", "HD-", 6)?, "subject": subject, "description": input.description, "state": "new", "channel": channel,
            "priority": priority, "ticket_type": input.ticket_type, "team": team, "assignee": assignee, "assigned_at": assignee.as_ref().map(|_| now.clone()),
            "customer": customer, "requester_name": name, "requester_email": email, "requester_phone": phone,
            "requester_user": input.requester_user, "company": company, "parent": input.parent, "source_model": input.source_model,
            "source_id": input.source_id, "due_at": input.due_at, "opened_at": now, "last_activity_at": now, "thread_key": input.thread_key,
            "reply_to": input.reply_to,
        }),
    )?;
    let id = id_of(&ticket)?.to_string();
    for t in &tags {
        db::create::<Record>("hd_ticket_tag", &json!({ "ticket": id, "tag": t["id"] }))?;
    }
    if !tags.is_empty() {
        refresh_tags_text(&id)?;
    }
    if let Some(body) = opt(input.description.clone()) {
        let inbound = input.requester_user.is_some() || channel == "email" || channel == "portal" || channel == "chat";
        db::create::<Record>(
            "hd_message",
            &json!({
                "ticket": id, "kind": "reply", "direction": if inbound { "inbound" } else { "internal" }, "author_user": actor()?,
                "author_name": ticket.get("requester_name"), "author_email": ticket.get("requester_email"),
                "requester_user": input.requester_user, "subject": subject, "body": body, "channel": channel, "posted_at": now,
            }),
        )?;
        db::update::<Record>("hd_ticket", &id, &json!({ "message_count": 1, "last_customer_message_at": if inbound { Some(now.clone()) } else { None }, "waiting_for_us": inbound }))?;
    }
    let ticket = get_ticket_row(&id)?;
    events::emit("ticket_created", &event_body(&ticket, json!({ "channel": channel })))?;
    if let Some(a) = &assignee {
        events::emit("ticket_assigned", &event_body(&ticket, json!({ "to": a })))?;
        tell(&[a.clone()], "A ticket was assigned to you", &subject, &id);
    }
    if !input.quiet {
        acknowledge(&ticket, &settings, None);
    }
    Ok(ticket)
}

/// Run as a background job after a visitor's ticket was stored: gives it its company, priority, team and owner,
/// thanks the requester and announces it.
fn finish_intake(id: &str) -> Result<Record> {
    let ticket = get_ticket_row(id)?;
    if ticket.get("intake_done") != Some(&json!(false)) {
        return Ok(ticket);
    }
    let (customer, name, email, phone) = identify(
        text(&ticket, "customer").map(str::to_string), text(&ticket, "requester_name").map(str::to_string),
        text(&ticket, "requester_email").map(str::to_string), text(&ticket, "requester_phone").map(str::to_string),
    )?;
    let d = defaults(None, text(&ticket, "ticket_type"), text(&ticket, "team").map(str::to_string), None)?;
    let assignee = match &d.team {
        Some(t) => assign::pick(t)?,
        None => None,
    };
    let now = now()?;
    let updated: Record = db::update(
        "hd_ticket",
        id,
        &json!({
            "company": d.company, "priority": d.priority, "team": d.team, "assignee": assignee, "assigned_at": assignee.as_ref().map(|_| now.clone()),
            "customer": customer, "requester_name": name, "requester_email": email, "requester_phone": phone, "intake_done": true,
        }),
    )?
    .ok_or_else(|| Error::msg("the ticket is gone"))?;
    events::emit("ticket_created", &event_body(&updated, json!({ "channel": updated["channel"] })))?;
    if let Some(a) = &assignee {
        events::emit("ticket_assigned", &event_body(&updated, json!({ "to": a })))?;
        tell(&[a.clone()], "A ticket was assigned to you", text(&updated, "subject").unwrap_or(""), id);
    }
    acknowledge(&updated, &d.settings, track_link(&updated, &d.settings).as_deref());
    Ok(updated)
}

// --- finding ---

fn paged(extra: Filter, page: Page) -> Result<Value> {
    let mut filter = extra;
    if let Some(teams) = team_scope()? {
        let mut visible = vec![Filter::is_empty("team")];
        if !teams.is_empty() {
            visible.push(Filter::one_of("team", teams));
        }
        if let Some(me) = actor()? {
            visible.push(Filter::eq("assignee", me));
        }
        filter = filter.and(Filter::any(visible));
    }
    if let Some(Value::Object(map)) = page.filter.clone() {
        if !map.is_empty() {
            filter = filter.and(Filter::from_json(Value::Object(map)));
        }
    }
    if let Some(s) = page.state.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        filter = filter.and(Filter::eq("state", s));
    }
    let limit = page.limit.unwrap_or(50).min(200);
    db::search(
        "hd_ticket",
        page.q.as_deref(),
        &["reference", "subject", "requester_name", "requester_email", "tags_text"],
        filter,
        Some(page.order.as_deref().unwrap_or("-last_activity_at")),
        Some(limit),
        page.offset,
    )
}

fn not_merged() -> Filter {
    Filter::ne("state", "merged")
}

fn mine_filter() -> Result<Filter> {
    Ok(Filter::eq("assignee", actor()?.ok_or_else(|| Error::msg("sign in first"))?))
}

// --- changing ---

fn change(input: Change) -> Result<Record> {
    require_agent()?;
    let mut changes = serde_json::Map::new();
    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            changes.insert(k.into(), if v.trim().is_empty() { Value::Null } else { json!(v.trim()) });
        }
    };
    if let Some(s) = input.subject.as_deref() {
        clean("a subject", s)?;
    }
    put("subject", input.subject);
    put("description", input.description);
    put("customer", input.customer);
    put("requester_name", input.requester_name);
    put("requester_email", input.requester_email.map(|e| e.to_lowercase()));
    put("requester_phone", input.requester_phone);
    put("ticket_type", input.ticket_type);
    put("priority", input.priority);
    put("due_at", input.due_at);
    put("source_model", input.source_model);
    put("source_id", input.source_id);
    if let Some(team) = input.team {
        put("team", Some(team));
    }
    if changes.is_empty() {
        return get_ticket_row(&input.id);
    }
    let before = get_ticket_row(&input.id)?;
    let changed: Vec<String> = changes.iter().filter(|(k, v)| before.get(k.as_str()).unwrap_or(&Value::Null) != *v).map(|(k, _)| k.clone()).collect();
    let updated: Record = db::update("hd_ticket", &input.id, &Value::Object(changes))?.ok_or_else(|| Error::msg("the ticket is gone"))?;
    announce_update(&updated, &changed)?;
    Ok(updated)
}

/// Tell the suite which fields of a ticket were changed by hand or by a rule.
pub fn announce_update(ticket: &Record, changed: &[String]) -> Result<()> {
    if !changed.is_empty() {
        events::emit("ticket_updated", &event_body(ticket, json!({ "changed": changed })))?;
    }
    Ok(())
}

/// The one way a ticket changes state. The kernel's workflow refuses a move that is not allowed.
pub fn move_to(id: &str, to: &str, resolution: Option<String>) -> Result<Record> {
    let ticket = get_ticket_row(id)?;
    let from = text(&ticket, "state").unwrap_or("new").to_string();
    if from == to {
        return Ok(ticket);
    }
    let now = now()?;
    let mut changes = json!({ "state": to, "last_activity_at": now });
    let put = |changes: &mut Value, k: &str, v: Value| {
        changes[k] = v;
    };
    match to {
        "resolved" => {
            put(&mut changes, "resolved_at", json!(now));
            if let (Some(opened), Ok(at)) = (moment(&ticket, "opened_at"), aether_sdk::dates::parse_datetime(&now)) {
                put(&mut changes, "resolution_minutes", json!((at - opened).num_minutes().max(0)));
            }
            if let Some(r) = opt(resolution.clone()) {
                put(&mut changes, "resolution", json!(r));
            }
        }
        "closed" => put(&mut changes, "closed_at", json!(now)),
        "open" if from == "resolved" || from == "closed" => {
            put(&mut changes, "reopen_count", json!(ticket.get("reopen_count").and_then(Value::as_i64).unwrap_or(0) + 1));
            put(&mut changes, "resolved_at", Value::Null);
            put(&mut changes, "closed_at", Value::Null);
        }
        _ => {}
    }
    let updated: Record = db::update("hd_ticket", id, &changes)?.ok_or_else(|| Error::msg("the ticket is gone"))?;
    system_line(id, &format!("Status changed from {} to {}", from.replace('_', " "), to.replace('_', " ")))?;
    events::emit("ticket_status_changed", &event_body(&updated, json!({ "from": from, "to": to })))?;
    match to {
        "resolved" => events::emit("ticket_resolved", &event_body(&updated, json!({ "resolution": updated.get("resolution") })))?,
        "closed" => events::emit("ticket_closed", &event_body(&updated, json!({})))?,
        "open" if from == "resolved" || from == "closed" => {
            events::emit("ticket_reopened", &event_body(&updated, json!({})))?;
            if let Some(a) = text(&updated, "assignee") {
                tell(&[a.to_string()], "A ticket was reopened", text(&updated, "subject").unwrap_or(""), id);
            }
        }
        _ => {}
    }
    Ok(updated)
}

fn assign_to(id: &str, assignee: Option<String>, team: Option<String>) -> Result<Record> {
    let ticket = get_ticket_row(id)?;
    if !is_open(&ticket) {
        return Err(Error::msg("only a ticket still being worked can be assigned"));
    }
    let mut changes = json!({ "last_activity_at": now()? });
    if let Some(t) = opt(team) {
        changes["team"] = json!(t);
    }
    let who = assignee.map(|a| user_id(&a));
    changes["assignee"] = json!(who);
    changes["assigned_at"] = if who.is_some() { json!(now()?) } else { Value::Null };
    let updated: Record = db::update("hd_ticket", id, &changes)?.ok_or_else(|| Error::msg("the ticket is gone"))?;
    match &who {
        Some(w) => {
            system_line(id, &format!("Assigned to {w}"))?;
            events::emit("ticket_assigned", &event_body(&updated, json!({ "to": w })))?;
            if actor()?.as_deref() != Some(w.as_str()) {
                tell(&[w.clone()], "A ticket was assigned to you", text(&updated, "subject").unwrap_or(""), id);
            }
        }
        None => system_line(id, "Unassigned")?,
    }
    if text(&updated, "state") == Some("new") && who.is_some() {
        return move_to(id, "open", None);
    }
    Ok(updated)
}

fn add_tag(input: TagIn) -> Result<Record> {
    require_agent()?;
    let tag = resolve_tags(&[input.tag])?.remove(0);
    let have = db::count("hd_ticket_tag", Filter::eq("ticket", input.id.as_str()).and(Filter::eq("tag", tag["id"].clone())))?;
    if have == 0 {
        db::create::<Record>("hd_ticket_tag", &json!({ "ticket": input.id, "tag": tag["id"] }))?;
        refresh_tags_text(&input.id)?;
    }
    get_ticket_row(&input.id)
}

fn remove_tag(input: TagIn) -> Result<Record> {
    require_agent()?;
    let tag = resolve_tags(&[input.tag])?.remove(0);
    let links: Vec<Record> = db::find("hd_ticket_tag").filter("ticket", input.id.as_str()).filter("tag", tag["id"].clone()).limit(10).all()?;
    for l in links {
        db::delete::<Record>("hd_ticket_tag", id_of(&l)?)?;
    }
    refresh_tags_text(&input.id)?;
    get_ticket_row(&input.id)
}

pub fn apply_rating(ticket: &Record, rating: i64, comment: Option<String>) -> Result<Record> {
    if !(1..=5).contains(&rating) {
        return Err(Error::msg("a rating is from 1 to 5"));
    }
    if !matches!(text(ticket, "state"), Some("resolved" | "closed")) {
        return Err(Error::msg("a ticket is rated once it is resolved"));
    }
    let id = id_of(ticket)?;
    let updated: Record = db::update("hd_ticket", id, &json!({ "rating": rating, "rating_comment": opt(comment), "rated_at": now()? }))?.ok_or_else(|| Error::msg("the ticket is gone"))?;
    events::emit("ticket_rated", &event_body(&updated, json!({ "rating": rating })))?;
    Ok(updated)
}

fn rate(input: Rate) -> Result<Record> {
    let ticket = get_ticket_row(&input.id)?;
    let mine = text(&ticket, "requester_user").is_some() && text(&ticket, "requester_user") == actor()?.as_deref();
    if !mine && !is_agent()? {
        return Err(Error::msg("only the person who asked rates a ticket"));
    }
    apply_rating(&ticket, input.rating, input.comment)
}

fn bulk(input: Bulk) -> Result<Value> {
    require_agent()?;
    if input.ids.len() > 200 {
        return Err(Error::msg("change at most 200 tickets at once"));
    }
    let (mut done, mut failed) = (0, Vec::new());
    for id in &input.ids {
        let result: Result<()> = (|| {
            match input.action.as_str() {
                "assign" => drop(assign_to(id, input.value.clone(), None)?),
                "unassign" => drop(assign_to(id, None, None)?),
                "priority" | "team" => {
                    get_ticket_row(id)?;
                    if let Some(t) = db::update::<Record>("hd_ticket", id, &json!({ input.action.as_str(): input.value }))? {
                        announce_update(&t, &[input.action.clone()])?;
                    }
                }
                "tag" => drop(add_tag(TagIn { id: id.clone(), tag: input.value.clone().unwrap_or_default() })?),
                "resolve" | "close" | "reopen" | "take" => {
                    let to = match input.action.as_str() { "resolve" => "resolved", "close" => "closed", "reopen" => "open", _ => "open" };
                    drop(move_to(id, to, input.value.clone())?)
                }
                other => return Err(Error::msg(format!("`{other}` is not something to do to many tickets"))),
            }
            Ok(())
        })();
        match result {
            Ok(()) => done += 1,
            Err(e) => failed.push(json!({ "id": id, "error": e.to_string() })),
        }
    }
    Ok(json!({ "done": done, "failed": failed }))
}

/// Daily: tickets that stayed resolved for the configured days are closed.
fn nightly() -> Result<Value> {
    let today = now()?;
    let day = aether_sdk::dates::parse_datetime(&today)?;
    let resolved: Vec<Record> = db::find("hd_ticket").filter("state", "resolved").limit(5000).all()?;
    let mut closed = 0;
    for t in resolved {
        let company = text(&t, "company").unwrap_or_default().to_string();
        let days = settings_for(&company).get("auto_close_days").and_then(Value::as_i64).unwrap_or(7);
        if days <= 0 {
            continue;
        }
        let Some(at) = moment(&t, "resolved_at") else { continue };
        if day - at >= aether_sdk::dates::Duration::days(days) {
            move_to(id_of(&t)?, "closed", None)?;
            closed += 1;
        }
    }
    Ok(json!({ "closed": closed }))
}

handler! {
    /// An agent raises a ticket for someone, over any channel.
    fn create_ticket(input: NewTicket) -> Record {
        require_agent()?;
        make(input)
    }

    /// Anyone signed in raises a ticket for themselves; they follow it afterwards.
    fn submit_ticket(input: NewTicket) -> Record {
        let mut input = input;
        let me = actor()?.ok_or_else(|| Error::msg("sign in to ask for help"))?;
        input.requester_user = Some(me);
        input.assignee = None;
        input.priority = None;
        input.parent = None;
        input.customer = None;
        input.channel = Some(if input.channel.as_deref() == Some("chat") { "chat".into() } else { "portal".into() });
        make(input)
    }

    fn create_subticket(input: NewTicket) -> Record {
        require_agent()?;
        let parent = input.parent.clone().ok_or_else(|| Error::msg("name the parent ticket"))?;
        let parent_row = get_ticket_row(&parent)?;
        let mut input = input;
        if input.customer.is_none() {
            input.customer = parent_row.get("customer").and_then(Value::as_str).map(str::to_string);
        }
        if input.requester_email.is_none() {
            input.requester_email = text(&parent_row, "requester_email").map(str::to_string);
        }
        let made = make(input)?;
        system_line(&parent, &format!("Sub-ticket {} created", text(&made, "reference").unwrap_or("")))?;
        Ok(made)
    }

    fn get_ticket(input: Id) -> Value {
        let ticket = get_ticket_row(&input.id)?;
        require_can_see(&ticket)?;
        let tags: Vec<Record> = db::find("hd_ticket_tag").filter("ticket", input.id.as_str()).limit(100).all()?;
        let minutes: i64 = db::find::<Record>("hd_time_entry").filter("ticket", input.id.as_str()).limit(5000).all()?.iter().map(|e| e.get("minutes").and_then(Value::as_i64).unwrap_or(0)).sum();
        let subs = db::count("hd_ticket", Filter::eq("parent", input.id.as_str()))?;
        let moves = db::transitions("hd_ticket", &input.id).unwrap_or_default();
        let mut out = ticket;
        if let Some(o) = out.as_object_mut() {
            o.insert("tag_links".into(), json!(tags));
            o.insert("time_minutes".into(), json!(minutes));
            o.insert("sub_tickets".into(), json!(subs));
            o.insert("moves".into(), json!(moves.iter().map(|m| m.name.clone()).collect::<Vec<_>>()));
        }
        Ok(out)
    }

    fn update_ticket(input: Change) -> Record {
        change(input)
    }

    fn list_tickets(input: Page) -> Value {
        require_agent()?;
        paged(not_merged(), input)
    }

    fn my_tickets(input: Page) -> Value {
        require_agent()?;
        paged(mine_filter()?.and(not_merged()).and(Filter::one_of("state", OPEN_STATES.iter().copied())), input)
    }

    fn unassigned_tickets(input: Page) -> Value {
        require_agent()?;
        paged(Filter::is_empty("assignee").and(Filter::one_of("state", OPEN_STATES.iter().copied())), input)
    }

    /// The tickets the caller asked for, newest activity first.
    fn my_requests(input: Page) -> Value {
        let me = actor()?.ok_or_else(|| Error::msg("sign in first"))?;
        paged(Filter::eq("requester_user", me), input)
    }

    fn sub_tickets(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_ticket").filter("parent", input.id.as_str()).limit(200).all()
    }

    fn change_ticket_state(input: StateIn) -> Record {
        require_agent()?;
        move_to(&input.id, &input.state, input.resolution)
    }

    fn move_ticket(input: MoveIn) -> Record {
        require_agent()?;
        let to = input.to.or(input.state).ok_or_else(|| Error::msg("say which column"))?;
        move_to(&input.id, &to, None)
    }

    fn take_ticket(input: Id) -> Record {
        require_agent()?;
        let me = actor()?;
        let ticket = get_ticket_row(&input.id)?;
        if text(&ticket, "assignee").is_none() {
            return assign_to(&input.id, me, None);
        }
        move_to(&input.id, "open", None)
    }

    fn wait_for_customer(input: Id) -> Record {
        require_agent()?;
        move_to(&input.id, "pending", None)
    }

    fn hold_ticket(input: Id) -> Record {
        require_agent()?;
        move_to(&input.id, "on_hold", None)
    }

    fn resume_ticket(input: Id) -> Record {
        require_agent()?;
        move_to(&input.id, "open", None)
    }

    fn resolve_ticket(input: Resolve) -> Record {
        require_agent()?;
        move_to(&input.id, "resolved", input.resolution)
    }

    fn close_ticket(input: Id) -> Record {
        require_agent()?;
        let t = get_ticket_row(&input.id)?;
        if text(&t, "state") != Some("resolved") {
            move_to(&input.id, "resolved", None)?;
        }
        move_to(&input.id, "closed", None)
    }

    fn reopen_ticket(input: Id) -> Record {
        require_agent()?;
        move_to(&input.id, "open", None)
    }

    fn assign_ticket(input: Assign) -> Record {
        require_agent()?;
        assign_to(&input.id, input.assignee, input.team)
    }

    fn assign_ticket_to_me(input: Id) -> Record {
        require_agent()?;
        assign_to(&input.id, actor()?, None)
    }

    /// Give the ticket to whoever its team's rule picks now.
    fn auto_assign_ticket(input: Id) -> Record {
        require_agent()?;
        let t = get_ticket_row(&input.id)?;
        let team = text(&t, "team").ok_or_else(|| Error::msg("the ticket has no team"))?.to_string();
        let who = assign::pick(&team)?.ok_or_else(|| Error::msg("the team assigns by hand, or nobody can take more"))?;
        assign_to(&input.id, Some(who), None)
    }

    fn unassign_ticket(input: Id) -> Record {
        require_agent()?;
        assign_to(&input.id, None, None)
    }

    fn ticket_assignees(input: ForTeam) -> Vec<Value> {
        require_agent()?;
        let team = match (input.team, input.id) {
            (Some(t), _) => Some(t),
            (None, Some(id)) => text(&get_ticket_row(&id)?, "team").map(str::to_string),
            _ => None,
        };
        assign::candidates(team.as_deref())
    }

    fn add_ticket_tag(input: TagIn) -> Record {
        add_tag(input)
    }

    fn remove_ticket_tag(input: TagIn) -> Record {
        remove_tag(input)
    }

    fn rate_ticket(input: Rate) -> Record {
        rate(input)
    }

    fn bulk_update_tickets(input: Bulk) -> Value {
        bulk(input)
    }

    /// SLA and rules set the due date from outside.
    fn set_ticket_due(input: DueIn) -> Record {
        require_agent()?;
        get_ticket_row(&input.id)?;
        db::update("hd_ticket", &input.id, &json!({ "due_at": input.due_at }))?.ok_or_else(|| Error::msg("the ticket is gone"))
    }

    /// A background job started by a visitor's submission.
    fn intake_finish(input: Id) -> Record {
        require_agent()?;
        finish_intake(&input.id)
    }

    fn tickets_nightly(_: Empty) -> Value {
        nightly()
    }
}
