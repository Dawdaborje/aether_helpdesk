use aether_sdk::dates::{format_datetime, parse_datetime, Duration, NaiveDateTime};
use aether_sdk::db::Filter;
use aether_sdk::events::Received;
use aether_sdk::prelude::*;

use crate::numbers::{by_group, fill, summarize};

type Record = aether_sdk::records::Record;

const AGENT_ROLE: &str = "helpdesk.hd_agent";
const MANAGER_ROLE: &str = "helpdesk.hd_manager";
const ADMIN_ROLE: &str = "helpdesk.hd_admin";

#[derive(Deserialize)]
struct NewSurvey {
    code: String,
    name: String,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    send_on: Option<String>,
    #[serde(default)]
    delay_minutes: Option<i64>,
    #[serde(default)]
    expires_days: Option<i64>,
    #[serde(default)]
    reminder_days: Option<i64>,
    #[serde(default)]
    cooldown_days: Option<i64>,
    #[serde(default)]
    alert_at: Option<i64>,
    #[serde(default)]
    alert_user: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    follow_up: Option<String>,
    #[serde(default)]
    intro: Option<String>,
}

#[derive(Deserialize)]
struct SetSurvey {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    is_default: Option<bool>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    send_on: Option<String>,
    #[serde(default)]
    delay_minutes: Option<i64>,
    #[serde(default)]
    expires_days: Option<i64>,
    #[serde(default)]
    reminder_days: Option<i64>,
    #[serde(default)]
    cooldown_days: Option<i64>,
    #[serde(default)]
    alert_at: Option<i64>,
    #[serde(default)]
    alert_user: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    follow_up: Option<String>,
    #[serde(default)]
    intro: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Token {
    token: String,
}

#[derive(Deserialize)]
struct Answer {
    token: String,
    rating: i64,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize)]
struct TicketRef {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    ticket: Option<String>,
}

#[derive(Deserialize, Default)]
struct Window {
    #[serde(default)]
    days: Option<i64>,
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

fn is_agent() -> Result<bool> {
    let ctx = context::current()?;
    Ok(ctx.actor.kind != context::ActorKind::Visitor && ctx.actor.kind != context::ActorKind::Anonymous && [AGENT_ROLE, MANAGER_ROLE, ADMIN_ROLE].iter().any(|r| ctx.has_role(r)))
}

fn require_agent() -> Result<()> {
    if is_agent()? {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{AGENT_ROLE}` to do this")))
    }
}

fn is_manager() -> Result<bool> {
    let ctx = context::current()?;
    Ok(ctx.actor.kind != context::ActorKind::Visitor && (ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE)))
}

fn require_manager() -> Result<()> {
    let ctx = context::current()?;
    if ctx.actor.kind != context::ActorKind::Visitor && (ctx.has_role(MANAGER_ROLE) || ctx.has_role(ADMIN_ROLE)) {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{MANAGER_ROLE}` to do this")))
    }
}

fn now_text() -> Result<String> {
    let ctx = context::current()?;
    if ctx.now.is_empty() {
        return Err(Error::msg("the kernel did not say what time it is"));
    }
    Ok(ctx.now)
}

fn now() -> Result<NaiveDateTime> {
    parse_datetime(&now_text()?)
}

fn stamp(t: NaiveDateTime) -> String {
    format_datetime(t)
}

fn text<'a>(r: &'a Record, k: &str) -> Option<&'a str> {
    r.get(k).and_then(Value::as_str)
}

fn opt(v: Option<String>) -> Option<String> {
    v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

fn int(r: &Record, k: &str, default: i64) -> i64 {
    r.get(k).and_then(Value::as_i64).unwrap_or(default)
}

fn new_token(what: &str) -> Result<String> {
    let ctx = context::current()?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{}|{}|{}", ctx.request_id, ctx.now, what).bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let rid: String = ctx.request_id.chars().filter(char::is_ascii_alphanumeric).collect();
    Ok(format!("{rid}{hash:016x}"))
}

fn check_send_on(v: &str) -> Result<()> {
    if matches!(v, "resolved" | "closed") {
        Ok(())
    } else {
        Err(Error::msg("a survey goes out when a ticket is resolved or closed"))
    }
}

fn clamp_days(label: &str, v: i64, max: i64) -> Result<i64> {
    if (0..=max).contains(&v) {
        Ok(v)
    } else {
        Err(Error::msg(format!("{label} is between 0 and {max}")))
    }
}

fn clear_default(keep: &str) -> Result<()> {
    for s in db::find::<Record>("hd_survey").filter("is_default", true).limit(50).all()? {
        if text(&s, "id") != Some(keep) {
            db::update::<Record>("hd_survey", text(&s, "id").unwrap_or_default(), &json!({ "is_default": false }))?;
        }
    }
    Ok(())
}

fn new_survey(input: NewSurvey) -> Result<Record> {
    require_manager()?;
    let send_on = input.send_on.clone().unwrap_or_else(|| "resolved".into());
    check_send_on(&send_on)?;
    let mut data = json!({
        "code": input.code.trim(), "name": input.name.trim(), "active": true, "is_default": input.is_default, "team": opt(input.team), "ticket_type": opt(input.ticket_type),
        "send_on": send_on, "delay_minutes": clamp_days("the delay", input.delay_minutes.unwrap_or(0), 10_080)?,
        "expires_days": clamp_days("the expiry", input.expires_days.unwrap_or(14), 365)?, "reminder_days": clamp_days("the reminder", input.reminder_days.unwrap_or(0), 365)?,
        "cooldown_days": clamp_days("the cooldown", input.cooldown_days.unwrap_or(7), 365)?, "alert_at": input.alert_at.unwrap_or(2).clamp(0, 5), "alert_user": opt(input.alert_user),
    });
    for (k, v) in [("subject", input.subject), ("question", input.question), ("follow_up", input.follow_up), ("intro", input.intro)] {
        if let Some(v) = opt(v) {
            data[k] = json!(v);
        }
    }
    let s: Record = db::create("hd_survey", &data)?;
    if input.is_default {
        clear_default(text(&s, "id").unwrap_or_default())?;
    }
    Ok(s)
}

fn change_survey(input: SetSurvey) -> Result<Record> {
    require_manager()?;
    let mut data = json!({});
    if let Some(n) = opt(input.name) {
        data["name"] = json!(n);
    }
    if let Some(a) = input.active {
        data["active"] = json!(a);
    }
    if let Some(d) = input.is_default {
        data["is_default"] = json!(d);
    }
    if let Some(s) = input.send_on {
        check_send_on(&s)?;
        data["send_on"] = json!(s);
    }
    for (k, v, max) in [("delay_minutes", input.delay_minutes, 10_080), ("expires_days", input.expires_days, 365), ("reminder_days", input.reminder_days, 365), ("cooldown_days", input.cooldown_days, 365)] {
        if let Some(v) = v {
            data[k] = json!(clamp_days(k, v, max)?);
        }
    }
    if let Some(a) = input.alert_at {
        data["alert_at"] = json!(a.clamp(0, 5));
    }
    for (k, v) in [("team", input.team), ("ticket_type", input.ticket_type), ("alert_user", input.alert_user), ("subject", input.subject), ("question", input.question), ("follow_up", input.follow_up), ("intro", input.intro)] {
        if let Some(v) = v {
            data[k] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    }
    let s: Record = db::update("hd_survey", &input.id, &data)?.ok_or_else(|| Error::msg("there is no such survey"))?;
    if input.is_default == Some(true) {
        clear_default(&input.id)?;
    }
    Ok(s)
}

/// The survey for a ticket: the most specific active one that fits (team and type), else the default.
fn survey_for(ticket: &Record) -> Result<Option<Record>> {
    let all: Vec<Record> = db::find("hd_survey").filter("active", true).limit(200).all()?;
    let mut best: Option<(i32, Record)> = None;
    for s in all {
        let team_ok = text(&s, "team").is_none_or(|t| text(ticket, "team") == Some(t));
        let type_ok = text(&s, "ticket_type").is_none_or(|t| text(ticket, "ticket_type") == Some(t));
        if !team_ok || !type_ok {
            continue;
        }
        let specificity = text(&s, "team").is_some() as i32 * 2 + text(&s, "ticket_type").is_some() as i32;
        let rank = if specificity == 0 && s.get("is_default") != Some(&json!(true)) { -1 } else { specificity };
        if rank >= 0 && best.as_ref().is_none_or(|(b, _)| rank > *b) {
            best = Some((rank, s));
        }
    }
    Ok(best.map(|(_, s)| s))
}

fn csat_enabled(ticket: &Record) -> bool {
    let company = text(ticket, "company").unwrap_or_default();
    plugins::call::<Value>("helpdesk", "get_settings", &json!({ "id": company })).map(|s| s.get("csat_enabled") == Some(&json!(true))).unwrap_or(false)
}

fn link(token: &str, ticket: &Record) -> String {
    let company = text(ticket, "company").unwrap_or_default();
    let base = plugins::call::<Value>("helpdesk", "get_settings", &json!({ "id": company })).ok().and_then(|s| s.get("portal_url").and_then(Value::as_str).map(|b| b.trim_end_matches('/').to_string())).unwrap_or_default();
    format!("{base}/support/rate/{token}")
}

fn deliver(response: &Record, survey: &Record, ticket: &Record, reminder: bool) -> Option<String> {
    let to = text(response, "requester_email")?;
    let reference = text(ticket, "reference").unwrap_or("");
    let values = [("reference", reference), ("subject", text(ticket, "subject").unwrap_or("")), ("name", text(response, "requester_name").unwrap_or(""))];
    let subject = fill(text(survey, "subject").unwrap_or("How did we do on {{reference}}?"), &values);
    let intro = fill(text(survey, "intro").unwrap_or(""), &values);
    let body = format!("{}\n\n{}\n\n{}\n\nThank you.", if reminder { "A gentle reminder:" } else { "Hello," }, intro, link(text(response, "token").unwrap_or(""), ticket));
    let mut mail = communication::email([to]).subject(&if reminder { format!("Reminder: {subject}") } else { subject }).text(&body);
    if let Some(r) = text(ticket, "reply_to") {
        mail = mail.reply_to(r);
    }
    mail.send().err().map(|e| e.to_string())
}

/// Ask for a rating now. Returns the response, or nothing when this ticket should not be surveyed.
fn ask(ticket_id: &str, on: Option<&str>, force: bool) -> Result<Option<Record>> {
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": ticket_id }))?;
    if !force && !csat_enabled(&ticket) {
        return Ok(None);
    }
    let Some(email) = text(&ticket, "requester_email").map(str::to_string) else { return Ok(None) };
    if ticket.get("spam") == Some(&json!(true)) {
        return Ok(None);
    }
    let Some(survey) = survey_for(&ticket)? else { return Ok(None) };
    if !force && on.is_some_and(|o| text(&survey, "send_on") != Some(o)) {
        return Ok(None);
    }
    if db::count("hd_survey_response", Filter::eq("ticket", ticket_id).and(Filter::eq("state", "sent")))? > 0 {
        return Ok(None);
    }
    let cooldown = int(&survey, "cooldown_days", 7);
    if !force && cooldown > 0 {
        let since = stamp(now()? - Duration::days(cooldown));
        if db::count("hd_survey_response", Filter::eq("requester_email", email.as_str()).and(Filter::gte("sent_at", since)))? > 0 {
            return Ok(None);
        }
    }
    let expires = stamp(now()? + Duration::days(int(&survey, "expires_days", 14)));
    let response: Record = db::create(
        "hd_survey_response",
        &json!({
            "survey": survey["id"], "ticket": ticket["id"], "reference": ticket["reference"], "token": new_token(ticket_id)?, "requester_name": ticket.get("requester_name"),
            "requester_email": email, "agent": ticket.get("assignee"), "team": ticket.get("team"), "state": "sent", "sent_at": now_text()?, "expires_at": expires, "applied": false, "alerted": false,
        }),
    )?;
    let delay = int(&survey, "delay_minutes", 0);
    if delay > 0 && !force {
        scheduler::enqueue("survey_send").payload(&json!({ "id": response["id"] })).delay_secs((delay * 60) as u64).unique_key(&format!("survey:{}", text(&response, "id").unwrap_or_default())).send()?;
    } else if let Some(err) = deliver(&response, &survey, &ticket, false) {
        db::update::<Record>("hd_survey_response", text(&response, "id").unwrap_or_default(), &json!({ "send_error": err.chars().take(300).collect::<String>() }))?;
    }
    events::emit("survey_sent", &json!({ "ticket": ticket["id"], "reference": ticket["reference"], "response": response["id"] }))?;
    Ok(Some(response))
}

fn send_later(id: &str) -> Result<Value> {
    let r = db::find::<Record>("hd_survey_response").filter("id", id).first()?.ok_or_else(|| Error::msg("there is no such survey response"))?;
    if text(&r, "state") != Some("sent") {
        return Ok(json!({ "skipped": true }));
    }
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": r["ticket"] }))?;
    let survey = db::find::<Record>("hd_survey").filter("id", text(&r, "survey").unwrap_or_default()).first()?.ok_or_else(|| Error::msg("the survey is gone"))?;
    if !matches!(text(&ticket, "state"), Some("resolved" | "closed")) {
        db::update::<Record>("hd_survey_response", id, &json!({ "state": "skipped" }))?;
        return Ok(json!({ "skipped": "reopened meanwhile" }));
    }
    if let Some(err) = deliver(&r, &survey, &ticket, false) {
        db::update::<Record>("hd_survey_response", id, &json!({ "send_error": err.chars().take(300).collect::<String>() }))?;
    }
    Ok(json!({ "sent": true }))
}

fn by_token(token: &str) -> Result<Record> {
    if token.len() < 20 {
        return Err(Error::msg("this survey link is not valid"));
    }
    db::find::<Record>("hd_survey_response").filter("token", token).first()?.ok_or_else(|| Error::msg("this survey link is not valid"))
}

fn expired(r: &Record) -> bool {
    text(r, "state") == Some("expired") || (text(r, "state") == Some("sent") && text(r, "expires_at").and_then(|t| parse_datetime(t).ok()).zip(now().ok()).is_some_and(|(e, n)| e < n))
}

fn read_survey(token: &str) -> Result<Value> {
    let r = by_token(token)?;
    let survey = db::find::<Record>("hd_survey").filter("id", text(&r, "survey").unwrap_or_default()).first()?.unwrap_or(Value::Null);
    Ok(json!({
        "reference": r["reference"], "question": survey.get("question"), "follow_up": survey.get("follow_up"),
        "answered": text(&r, "state") == Some("answered"), "expired": expired(&r), "rating": r.get("rating"),
    }))
}

fn submit(input: Answer) -> Result<Value> {
    let r = by_token(&input.token)?;
    if text(&r, "state") == Some("answered") {
        return Err(Error::msg("this survey was already answered, thank you"));
    }
    if expired(&r) || text(&r, "state") == Some("skipped") {
        return Err(Error::msg("this survey is no longer open"));
    }
    if !(1..=5).contains(&input.rating) {
        return Err(Error::msg("a rating is from 1 to 5"));
    }
    let id = text(&r, "id").unwrap_or_default().to_string();
    let comment = opt(input.comment).map(|c| c.chars().take(2000).collect::<String>());
    db::update::<Record>("hd_survey_response", &id, &json!({ "state": "answered", "rating": input.rating, "comment": comment, "answered_at": now_text()? }))?;
    scheduler::enqueue("survey_apply").payload(&json!({ "id": id })).unique_key(&format!("apply:{id}")).send()?;
    Ok(json!({ "thanks": true, "rating": input.rating }))
}

/// Background: write the rating to the ticket and alert on a low one.
fn apply(id: &str) -> Result<Value> {
    let r = db::find::<Record>("hd_survey_response").filter("id", id).first()?.ok_or_else(|| Error::msg("there is no such survey response"))?;
    if r.get("applied") == Some(&json!(true)) || text(&r, "state") != Some("answered") {
        return Ok(json!({ "skipped": true }));
    }
    let rating = int(&r, "rating", 0);
    let ticket_id = text(&r, "ticket").unwrap_or_default().to_string();
    let wrote: Result<Value> = plugins::call("helpdesk_ticket", "rate_ticket", &json!({ "id": ticket_id, "rating": rating, "comment": r.get("comment") }));
    db::update::<Record>("hd_survey_response", id, &json!({ "applied": wrote.is_ok() }))?;
    let survey = db::find::<Record>("hd_survey").filter("id", text(&r, "survey").unwrap_or_default()).first()?.unwrap_or(Value::Null);
    let threshold = int(&survey, "alert_at", 2);
    events::emit("survey_answered", &json!({ "ticket": ticket_id, "reference": r["reference"], "rating": rating, "agent": r.get("agent"), "team": r.get("team") }))?;
    if threshold > 0 && rating <= threshold && r.get("alerted") != Some(&json!(true)) {
        let mut people: Vec<String> = text(&r, "agent").map(|a| vec![a.to_string()]).unwrap_or_default();
        if let Some(u) = text(&survey, "alert_user") {
            people.push(if u.contains(':') { u.to_string() } else { format!("users:{u}") });
        }
        if !people.is_empty() {
            let _ = Notification::new(format!("Low rating ({rating}/5) on {}", text(&r, "reference").unwrap_or("")))
                .body(text(&r, "comment").unwrap_or("The customer left no comment."))
                .link(format!("/helpdesk/tickets/{}", ticket_id.rsplit(':').next().unwrap_or(&ticket_id)))
                .to(Audience::Actors(people))
                .send();
        }
        db::update::<Record>("hd_survey_response", id, &json!({ "alerted": true }))?;
        events::emit("ticket_low_rating", &json!({ "ticket": ticket_id, "reference": r["reference"], "rating": rating, "comment": r.get("comment"), "agent": r.get("agent"), "team": r.get("team") }))?;
    }
    Ok(json!({ "applied": wrote.is_ok() }))
}

/// Hourly: remind once, then let unanswered surveys expire.
fn sweep() -> Result<Value> {
    let n = now()?;
    let open: Vec<Record> = db::find("hd_survey_response").filter("state", "sent").limit(5000).all()?;
    let (mut reminded, mut expired_n) = (0, 0);
    for r in open {
        let id = text(&r, "id").unwrap_or_default().to_string();
        if expired(&r) {
            db::update::<Record>("hd_survey_response", &id, &json!({ "state": "expired" }))?;
            expired_n += 1;
            continue;
        }
        let survey = db::find::<Record>("hd_survey").filter("id", text(&r, "survey").unwrap_or_default()).first()?.unwrap_or(Value::Null);
        let days = int(&survey, "reminder_days", 0);
        let sent = text(&r, "sent_at").and_then(|t| parse_datetime(t).ok());
        if days > 0 && r.get("reminded_at").is_none_or(Value::is_null) && sent.is_some_and(|s| n - s >= Duration::days(days)) {
            let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": r["ticket"] }))?;
            if matches!(text(&ticket, "state"), Some("resolved" | "closed")) {
                let _ = deliver(&r, &survey, &ticket, true);
                db::update::<Record>("hd_survey_response", &id, &json!({ "reminded_at": now_text()? }))?;
                reminded += 1;
            }
        }
    }
    Ok(json!({ "reminded": reminded, "expired": expired_n }))
}

fn overview(input: Window) -> Result<Value> {
    require_agent()?;
    let days = input.days.unwrap_or(30).clamp(1, 365);
    let n = now()?;
    let since = stamp(n - Duration::days(days));
    let asked: Vec<Record> = db::find("hd_survey_response").matching(Filter::gte("sent_at", since.as_str())).order_by("-sent_at").limit(4000).all()?;
    let answered: Vec<&Record> = asked.iter().filter(|r| text(r, "state") == Some("answered") && r.get("rating").is_some_and(|v| !v.is_null())).collect();
    let ratings: Vec<i64> = answered.iter().map(|r| int(r, "rating", 0)).collect();
    let s = summarize(&ratings);
    let group = |key: &str| -> Vec<Value> {
        let pairs: Vec<(String, i64)> = answered.iter().filter_map(|r| text(r, key).map(|k| (k.to_string(), int(r, "rating", 0)))).collect();
        by_group(&pairs).into_iter().map(|(k, s)| json!({ "key": k, "answers": s.answered, "average": s.average, "satisfied_percent": s.satisfied_percent })).collect()
    };
    let mut trend = Vec::new();
    for back in (0..days.min(30)).rev() {
        let day = stamp(n - Duration::days(back))[..10].to_string();
        let day_ratings: Vec<i64> = answered.iter().filter(|r| text(r, "answered_at").is_some_and(|d| d.starts_with(&day))).map(|r| int(r, "rating", 0)).collect();
        let ds = summarize(&day_ratings);
        trend.push(json!({ "day": day, "answers": ds.answered, "average": ds.average }));
    }
    let mut low: Vec<&&Record> = answered.iter().filter(|r| int(r, "rating", 0) <= 2).collect();
    low.sort_by(|a, b| text(b, "answered_at").cmp(&text(a, "answered_at")));
    Ok(json!({
        "days": days, "sent": asked.len(), "answered": s.answered,
        "response_rate_percent": if asked.is_empty() { Value::Null } else { json!(s.answered * 100 / asked.len()) },
        "average": s.average, "satisfied_percent": s.satisfied_percent,
        "distribution": s.distribution.iter().enumerate().map(|(i, n)| json!({ "rating": (i + 1).to_string(), "answers": n })).collect::<Vec<_>>(),
        "by_agent": group("agent"), "by_team": group("team"), "trend": trend,
        "low": low.iter().take(10).map(|r| json!({ "reference": r["reference"], "rating": r["rating"], "comment": r.get("comment"), "agent": r.get("agent") })).collect::<Vec<_>>(),
    }))
}

fn list(input: Page) -> Result<Value> {
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
    let mut out = db::search("hd_survey_response", input.q.as_deref(), &["reference", "requester_email", "requester_name", "comment"], filter, Some("-sent_at"), input.limit.or(Some(50)), input.offset)?;
    // The link's token is the customer's key: only managers (who may need to resend a link) see it.
    if !is_manager()? {
        if let Some(rows) = out.get_mut("rows").and_then(Value::as_array_mut) {
            for r in rows {
                if let Some(o) = r.as_object_mut() {
                    o.remove("token");
                }
            }
        }
    }
    Ok(out)
}

fn seed() -> Result<Value> {
    if db::count("hd_survey", Filter::all())? > 0 {
        return Ok(json!({ "created": 0 }));
    }
    db::create::<Record>("hd_survey", &json!({ "code": "standard", "name": "Standard satisfaction survey", "active": true, "is_default": true, "send_on": "resolved", "delay_minutes": 0, "expires_days": 14, "reminder_days": 3, "cooldown_days": 7, "alert_at": 2 }))?;
    Ok(json!({ "created": 1 }))
}

fn ticket_of(r: &Received) -> Result<String> {
    r.payload.get("ticket").and_then(Value::as_str).map(str::to_string).ok_or_else(|| Error::msg("the event names no ticket"))
}

handler! {
    fn seed_csat(_: Empty) -> Value {
        seed()
    }

    fn create_survey(input: NewSurvey) -> Record {
        new_survey(input)
    }

    fn update_survey(input: SetSurvey) -> Record {
        change_survey(input)
    }

    fn list_surveys(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_survey").order_by("name").limit(200).all()
    }

    fn get_survey(input: Id) -> Record {
        require_agent()?;
        db::find::<Record>("hd_survey").filter("id", input.id.as_str()).first()?.ok_or_else(|| Error::msg("there is no such survey"))
    }

    fn on_ticket_resolved(received: Received) -> Value {
        ask(&ticket_of(&received)?, Some("resolved"), false)?;
        Ok(json!({}))
    }

    fn on_ticket_closed(received: Received) -> Value {
        ask(&ticket_of(&received)?, Some("closed"), false)?;
        Ok(json!({}))
    }

    /// An agent asks for a rating now, whatever the settings.
    fn send_survey_now(input: TicketRef) -> Value {
        require_agent()?;
        let t = input.ticket.or(input.id).ok_or_else(|| Error::msg("name the ticket"))?;
        match ask(&t, None, true)? {
            Some(r) => Ok(json!({ "sent": true, "response": r["id"] })),
            None => Err(Error::msg("this ticket cannot be surveyed: it has no email address, no survey fits it, or one is already waiting")),
        }
    }

    fn survey_send(input: Id) -> Value {
        require_agent()?;
        send_later(&input.id)
    }

    fn survey_apply(input: Id) -> Value {
        require_agent()?;
        apply(&input.id)
    }

    fn survey_sweep(_: Empty) -> Value {
        require_agent()?;
        sweep()
    }

    /// The survey page's data: open to anyone holding the link.
    fn csat_get(input: Token) -> Value {
        read_survey(&input.token)
    }

    fn csat_submit(input: Answer) -> Value {
        submit(input)
    }

    fn list_survey_responses(input: Page) -> Value {
        list(input)
    }

    fn ticket_survey(input: TicketRef) -> Vec<Record> {
        require_agent()?;
        let t = input.ticket.or(input.id).ok_or_else(|| Error::msg("name the ticket"))?;
        let mut rows: Vec<Record> = db::find("hd_survey_response").filter("ticket", t.as_str()).order_by("-sent_at").limit(20).all()?;
        if !is_manager()? {
            for r in &mut rows {
                if let Some(o) = r.as_object_mut() {
                    o.remove("token");
                }
            }
        }
        Ok(rows)
    }

    fn csat_overview(input: Window) -> Value {
        overview(input)
    }

    fn csat_by_agent(input: Window) -> Vec<Value> {
        Ok(overview(input)?["by_agent"].as_array().cloned().unwrap_or_default())
    }

    fn csat_distribution(input: Window) -> Vec<Value> {
        Ok(overview(input)?["distribution"].as_array().cloned().unwrap_or_default())
    }

    fn csat_trend(input: Window) -> Vec<Value> {
        Ok(overview(input)?["trend"].as_array().cloned().unwrap_or_default())
    }

    fn csat_low_ratings(input: Window) -> Vec<Value> {
        Ok(overview(input)?["low"].as_array().cloned().unwrap_or_default())
    }
}
