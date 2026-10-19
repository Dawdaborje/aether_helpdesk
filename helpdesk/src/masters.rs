//! Helpdesk masters: teams, team members, priorities, ticket types, tags and settings.

use aether_sdk::prelude::*;

use crate::common::{id_of, require, require_admin, require_reader, text, Record, ASSIGNMENT_MODES};

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    active_only: Option<bool>,
}

#[derive(Deserialize)]
struct TeamInput {
    code: String,
    name: String,
    company: String,
    #[serde(default)]
    leader: Option<String>,
    #[serde(default)]
    assignment: Option<String>,
}

#[derive(Deserialize)]
struct SetTeam {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    leader: Option<String>,
    #[serde(default)]
    assignment: Option<String>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Deserialize)]
struct MemberInput {
    team: String,
    user: String,
    #[serde(default)]
    capacity: Option<i64>,
}

#[derive(Deserialize)]
struct PriorityInput {
    code: String,
    name: String,
    #[serde(default)]
    level: Option<i64>,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct SetPriority {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    level: Option<i64>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Deserialize)]
struct TypeInput {
    code: String,
    name: String,
    #[serde(default)]
    default_team: Option<String>,
}

#[derive(Deserialize)]
struct TagInput {
    code: String,
    name: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct SetSimple {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    default_team: Option<String>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Deserialize)]
struct SettingsInput {
    company: String,
    #[serde(default)]
    default_team: Option<String>,
    #[serde(default)]
    default_priority: Option<String>,
    #[serde(default)]
    auto_close_days: Option<i64>,
    #[serde(default)]
    csat_enabled: Option<bool>,
    #[serde(default)]
    send_acknowledgement: Option<bool>,
    #[serde(default)]
    restrict_by_team: Option<bool>,
    #[serde(default)]
    portal_url: Option<String>,
    #[serde(default)]
    reply_signature: Option<String>,
    #[serde(default)]
    business_name: Option<String>,
}

fn trimmed(label: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::msg(format!("{label} is required")));
    }
    Ok(value.to_string())
}

fn assignment_or(value: Option<&str>, fallback: &str) -> Result<String> {
    let mode = value.unwrap_or(fallback);
    if ASSIGNMENT_MODES.contains(&mode) {
        Ok(mode.to_string())
    } else {
        Err(Error::msg("assignment is manual, round_robin or load_balanced"))
    }
}

fn level_ok(level: i64) -> Result<i64> {
    if (1..=4).contains(&level) {
        Ok(level)
    } else {
        Err(Error::msg("level is 1 (low) to 4 (urgent)"))
    }
}

fn filter_search(mut rows: Vec<Record>, input: &Search) -> Vec<Record> {
    if input.active_only.unwrap_or(true) {
        rows.retain(|row| row.get("active") != Some(&json!(false)));
    }
    if let Some(q) = input.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        let q = q.to_lowercase();
        rows.retain(|row| {
            ["code", "name"].iter().any(|f| text(row, f).map(|v| v.to_lowercase().contains(&q)).unwrap_or(false))
        });
    }
    rows.sort_by(|a, b| text(a, "name").cmp(&text(b, "name")));
    rows
}

fn search_model(model: &str, input: Search) -> Result<Vec<Record>> {
    require_reader()?;
    let rows = db::find::<Record>(model).limit(500).all()?;
    Ok(filter_search(rows, &input))
}

fn get_one(model: &str, what: &str, input: Id) -> Result<Record> {
    require_reader()?;
    require(model, &input.id, what)
}

fn apply_simple(model: &str, what: &str, input: SetSimple) -> Result<Record> {
    require_admin()?;
    let _ = require(model, &input.id, what)?;
    let mut changes = json!({});
    if let Some(name) = input.name {
        changes["name"] = json!(trimmed("name", &name)?);
    }
    if let Some(color) = input.color {
        changes["color"] = json!(color.trim());
    }
    if let Some(team) = input.default_team {
        changes["default_team"] = if team.is_empty() {
            Value::Null
        } else {
            json!(id_of(&require("hd_team", &team, "team")?)?)
        };
    }
    if let Some(active) = input.active {
        changes["active"] = json!(active);
    }
    db::update(model, &input.id, &changes)?.ok_or_else(|| Error::msg(format!("the {what} is gone")))
}

// --- teams and members ---

fn do_create_team(input: TeamInput) -> Result<Record> {
    require_admin()?;
    let company = require("company.company", &input.company, "company")?;
    let mut data = json!({
        "code": trimmed("code", &input.code)?,
        "name": trimmed("name", &input.name)?,
        "company": id_of(&company)?,
        "assignment": assignment_or(input.assignment.as_deref(), "manual")?,
        "active": true,
    });
    if let Some(leader) = input.leader.filter(|l| !l.trim().is_empty()) {
        data["leader"] = json!(leader.trim());
    }
    db::create("hd_team", &data).map_err(|e| e.or("could not create the team (the code may be taken)"))
}

fn do_set_team(input: SetTeam) -> Result<Record> {
    require_admin()?;
    let _ = require("hd_team", &input.id, "team")?;
    let mut changes = json!({});
    if let Some(name) = input.name {
        changes["name"] = json!(trimmed("name", &name)?);
    }
    if let Some(leader) = input.leader {
        changes["leader"] = if leader.trim().is_empty() { Value::Null } else { json!(leader.trim()) };
    }
    if let Some(mode) = input.assignment {
        changes["assignment"] = json!(assignment_or(Some(&mode), "manual")?);
    }
    if let Some(active) = input.active {
        changes["active"] = json!(active);
    }
    db::update("hd_team", &input.id, &changes)?.ok_or_else(|| Error::msg("the team is gone"))
}

fn do_add_member(input: MemberInput) -> Result<Record> {
    require_admin()?;
    let team = require("hd_team", &input.team, "team")?;
    let team_id = id_of(&team)?.to_string();
    let user = trimmed("user", &input.user)?;
    if let Some(existing) = db::find::<Record>("hd_team_member")
        .filter("team", team_id.as_str())
        .filter("user", user.as_str())
        .first()?
    {
        // Re-adding a former member reactivates them.
        let mut changes = json!({ "active": true });
        if let Some(capacity) = input.capacity {
            changes["capacity"] = json!(capacity.max(0));
        }
        return db::update("hd_team_member", id_of(&existing)?, &changes)?
            .ok_or_else(|| Error::msg("the member is gone"));
    }
    db::create(
        "hd_team_member",
        &json!({ "team": team_id, "user": user, "capacity": input.capacity.unwrap_or(0).max(0), "active": true }),
    )
}

fn do_remove_member(input: Id) -> Result<Record> {
    require_admin()?;
    let _ = require("hd_team_member", &input.id, "team member")?;
    db::update("hd_team_member", &input.id, &json!({ "active": false }))?
        .ok_or_else(|| Error::msg("the member is gone"))
}

/// The teams the caller is an active member of.
fn do_my_teams() -> Result<Vec<Record>> {
    require_reader()?;
    let Some(me) = context::current()?.actor.id else { return Ok(Vec::new()) };
    let rows: Vec<Record> = db::find("hd_team_member").filter("user", me.as_str()).limit(200).all()?;
    let mut teams = Vec::new();
    for row in rows.iter().filter(|r| r.get("active") != Some(&json!(false))) {
        if let Some(team) = db::find::<Record>("hd_team").filter("id", text(row, "team").unwrap_or_default()).first()? {
            if team.get("active") != Some(&json!(false)) {
                teams.push(team);
            }
        }
    }
    Ok(teams)
}

fn do_list_members(input: Id) -> Result<Vec<Record>> {
    require_reader()?;
    let mut rows = db::find::<Record>("hd_team_member").filter("team", input.id.as_str()).limit(500).all()?;
    rows.retain(|row| row.get("active") != Some(&json!(false)));
    rows.sort_by(|a, b| text(a, "user").cmp(&text(b, "user")));
    Ok(rows)
}

// --- priorities, types, tags ---

fn do_create_priority(input: PriorityInput) -> Result<Record> {
    require_admin()?;
    let mut data = json!({
        "code": trimmed("code", &input.code)?,
        "name": trimmed("name", &input.name)?,
        "level": level_ok(input.level.unwrap_or(2))?,
        "active": true,
    });
    if let Some(color) = input.color.filter(|c| !c.trim().is_empty()) {
        data["color"] = json!(color.trim());
    }
    db::create("hd_priority", &data).map_err(|e| e.or("could not create the priority (the code may be taken)"))
}

fn do_set_priority(input: SetPriority) -> Result<Record> {
    require_admin()?;
    let _ = require("hd_priority", &input.id, "priority")?;
    let mut changes = json!({});
    if let Some(name) = input.name {
        changes["name"] = json!(trimmed("name", &name)?);
    }
    if let Some(level) = input.level {
        changes["level"] = json!(level_ok(level)?);
    }
    if let Some(color) = input.color {
        changes["color"] = json!(color.trim());
    }
    if let Some(active) = input.active {
        changes["active"] = json!(active);
    }
    db::update("hd_priority", &input.id, &changes)?.ok_or_else(|| Error::msg("the priority is gone"))
}

fn do_search_priorities(input: Search) -> Result<Vec<Record>> {
    let mut rows = search_model("hd_priority", input)?;
    rows.sort_by(|a, b| {
        b.get("level").and_then(Value::as_i64).cmp(&a.get("level").and_then(Value::as_i64))
    });
    Ok(rows)
}

fn do_create_ticket_type(input: TypeInput) -> Result<Record> {
    require_admin()?;
    let mut data = json!({
        "code": trimmed("code", &input.code)?,
        "name": trimmed("name", &input.name)?,
        "active": true,
    });
    if let Some(team) = input.default_team.filter(|t| !t.is_empty()) {
        data["default_team"] = json!(id_of(&require("hd_team", &team, "team")?)?);
    }
    db::create("hd_ticket_type", &data).map_err(|e| e.or("could not create the ticket type (the code may be taken)"))
}

fn do_create_tag(input: TagInput) -> Result<Record> {
    require_admin()?;
    let mut data = json!({
        "code": trimmed("code", &input.code)?,
        "name": trimmed("name", &input.name)?,
        "active": true,
    });
    if let Some(color) = input.color.filter(|c| !c.trim().is_empty()) {
        data["color"] = json!(color.trim());
    }
    db::create("hd_tag", &data).map_err(|e| e.or("could not create the tag (the code may be taken)"))
}

// --- settings ---

fn do_set_settings(input: SettingsInput) -> Result<Record> {
    require_admin()?;
    let company = require("company.company", &input.company, "company")?;
    let company_id = id_of(&company)?.to_string();
    let mut data = json!({});
    if let Some(team) = input.default_team {
        data["default_team"] = if team.is_empty() {
            Value::Null
        } else {
            json!(id_of(&require("hd_team", &team, "team")?)?)
        };
    }
    if let Some(priority) = input.default_priority {
        data["default_priority"] = if priority.is_empty() {
            Value::Null
        } else {
            json!(id_of(&require("hd_priority", &priority, "priority")?)?)
        };
    }
    if let Some(days) = input.auto_close_days {
        if days < 0 {
            return Err(Error::msg("auto_close_days cannot be negative"));
        }
        data["auto_close_days"] = json!(days);
    }
    if let Some(flag) = input.csat_enabled {
        data["csat_enabled"] = json!(flag);
    }
    if let Some(flag) = input.send_acknowledgement {
        data["send_acknowledgement"] = json!(flag);
    }
    if let Some(flag) = input.restrict_by_team {
        data["restrict_by_team"] = json!(flag);
    }
    for (name, value) in [("portal_url", input.portal_url), ("reply_signature", input.reply_signature), ("business_name", input.business_name)] {
        if let Some(v) = value {
            data[name] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    }
    match db::find::<Record>("hd_settings").filter("company", company_id.as_str()).first()? {
        Some(existing) => db::update("hd_settings", id_of(&existing)?, &data)?
            .ok_or_else(|| Error::msg("the settings are gone")),
        None => {
            data["company"] = json!(company_id);
            db::create("hd_settings", &data)
        }
    }
}

fn do_get_settings(input: Id) -> Result<Value> {
    require_reader()?;
    // `id` is the company id.
    Ok(db::find::<Record>("hd_settings")
        .filter("company", input.id.as_str())
        .first()?
        .unwrap_or_else(|| json!({ "company": input.id, "auto_close_days": 7, "csat_enabled": false, "send_acknowledgement": true, "restrict_by_team": false })))
}

// --- seed ---

/// (code, name, level)
const SEED_PRIORITIES: &[(&str, &str, i64)] =
    &[("low", "Low", 1), ("medium", "Medium", 2), ("high", "High", 3), ("urgent", "Urgent", 4)];

const SEED_TYPES: &[(&str, &str)] = &[
    ("question", "Question"),
    ("incident", "Incident"),
    ("problem", "Problem"),
    ("feature-request", "Feature request"),
];

/// Create the starter priorities and ticket types that are missing. Safe to run again.
fn do_seed() -> Result<Value> {
    let mut priorities = 0;
    for (code, name, level) in SEED_PRIORITIES {
        if db::find::<Record>("hd_priority").filter("code", *code).first()?.is_some() {
            continue;
        }
        db::create::<Record>(
            "hd_priority",
            &json!({ "code": code, "name": name, "level": level, "active": true }),
        )?;
        priorities += 1;
    }
    let mut types = 0;
    for (code, name) in SEED_TYPES {
        if db::find::<Record>("hd_ticket_type").filter("code", *code).first()?.is_some() {
            continue;
        }
        db::create::<Record>("hd_ticket_type", &json!({ "code": code, "name": name, "active": true }))?;
        types += 1;
    }
    Ok(json!({ "priorities": priorities, "ticket_types": types }))
}

handler! {
    /// Starter priorities and ticket types; runs on install and upgrade.
    fn seed_helpdesk(_: Empty) -> Value { do_seed() }

    fn create_team(input: TeamInput) -> Record { do_create_team(input) }
    fn set_team(input: SetTeam) -> Record { do_set_team(input) }
    fn get_team(input: Id) -> Record { get_one("hd_team", "team", input) }
    fn search_teams(input: Search) -> Vec<Record> { search_model("hd_team", input) }
    fn add_team_member(input: MemberInput) -> Record { do_add_member(input) }
    fn remove_team_member(input: Id) -> Record { do_remove_member(input) }
    fn list_team_members(input: Id) -> Vec<Record> { do_list_members(input) }
    fn my_teams(_: Empty) -> Vec<Record> { do_my_teams() }

    fn create_priority(input: PriorityInput) -> Record { do_create_priority(input) }
    fn set_priority(input: SetPriority) -> Record { do_set_priority(input) }
    fn search_priorities(input: Search) -> Vec<Record> { do_search_priorities(input) }

    fn create_ticket_type(input: TypeInput) -> Record { do_create_ticket_type(input) }
    fn set_ticket_type(input: SetSimple) -> Record { apply_simple("hd_ticket_type", "ticket type", input) }
    fn search_ticket_types(input: Search) -> Vec<Record> { search_model("hd_ticket_type", input) }

    fn create_tag(input: TagInput) -> Record { do_create_tag(input) }
    fn set_tag(input: SetSimple) -> Record { apply_simple("hd_tag", "tag", input) }
    fn search_tags(input: Search) -> Vec<Record> { search_model("hd_tag", input) }

    /// The kinds of request a visitor can pick from, without anything else of the masters.
    fn public_ticket_types(_: Empty) -> Vec<Value> {
        let rows: Vec<Record> = db::find("hd_ticket_type").filter("active", true).order_by("name").limit(100).all()?;
        Ok(rows.iter().map(|t| json!({ "id": t["id"], "name": t["name"], "code": t["code"] })).collect())
    }

    fn set_settings(input: SettingsInput) -> Record { do_set_settings(input) }
    fn get_settings(input: Id) -> Value { do_get_settings(input) }
}
