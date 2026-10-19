//! Business hours, days off and service policies with their targets.

use aether_sdk::prelude::*;

use crate::common::*;
use crate::hours::{minutes_of, weekday_of};

#[derive(Deserialize)]
struct DayIn {
    /// One day, or several separated by commas: `mon,tue,wed,thu,fri`.
    weekday: String,
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct NewHours {
    code: String,
    name: String,
    #[serde(default)]
    utc_offset_minutes: Option<i64>,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    days: Vec<DayIn>,
}

#[derive(Deserialize)]
struct SetHours {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    utc_offset_minutes: Option<i64>,
    #[serde(default)]
    is_default: Option<bool>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Deserialize)]
struct AddDay {
    #[serde(alias = "id")]
    hours: String,
    weekday: String,
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct AddHoliday {
    #[serde(alias = "id")]
    hours: String,
    day: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct TargetIn {
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    response_minutes: Option<i64>,
    #[serde(default)]
    resolution_minutes: Option<i64>,
}

#[derive(Deserialize)]
struct NewPolicy {
    code: String,
    name: String,
    #[serde(default)]
    rank: Option<i64>,
    #[serde(default)]
    hours: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    customer: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    warn_percent: Option<i64>,
    #[serde(default)]
    pause_when_pending: Option<bool>,
    #[serde(default)]
    pause_when_on_hold: Option<bool>,
    #[serde(default)]
    escalate_to: Option<String>,
    #[serde(default)]
    targets: Vec<TargetIn>,
}

#[derive(Deserialize)]
struct SetPolicy {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    rank: Option<i64>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    hours: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    customer: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    warn_percent: Option<i64>,
    #[serde(default)]
    pause_when_pending: Option<bool>,
    #[serde(default)]
    pause_when_on_hold: Option<bool>,
    #[serde(default)]
    escalate_to: Option<String>,
}

#[derive(Deserialize)]
struct AddTarget {
    #[serde(alias = "id")]
    policy: String,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    response_minutes: Option<i64>,
    #[serde(default)]
    resolution_minutes: Option<i64>,
}

#[derive(Deserialize)]
struct Preview {
    #[serde(default)]
    hours: Option<String>,
    start: String,
    minutes: i64,
}

fn check_day(d: &DayIn) -> Result<Vec<String>> {
    let (s, e) = (minutes_of(&d.start).ok_or_else(|| Error::msg(format!("`{}` is not a time like 09:00", d.start)))?, minutes_of(&d.end).ok_or_else(|| Error::msg(format!("`{}` is not a time like 17:00", d.end)))?);
    if e <= s {
        return Err(Error::msg("the working day ends after it starts"));
    }
    let names: Vec<String> = d.weekday.split(',').map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        return Err(Error::msg("name the weekday"));
    }
    let mut out = Vec::new();
    for n in names {
        weekday_of(&n).ok_or_else(|| Error::msg(format!("`{n}` is not a weekday")))?;
        out.push(n[..3].to_string());
    }
    Ok(out)
}

fn clear_default_except(keep: &str) -> Result<()> {
    for h in db::find::<Record>("hd_hours").filter("is_default", true).limit(50).all()? {
        if id_of(&h)? != keep {
            db::update::<Record>("hd_hours", id_of(&h)?, &json!({ "is_default": false }))?;
        }
    }
    Ok(())
}

fn create_hours(input: NewHours) -> Result<Record> {
    require_manager()?;
    let code = clean("a code", &input.code)?;
    let name = clean("a name", &input.name)?;
    let mut rows = Vec::new();
    for d in &input.days {
        for wd in check_day(d)? {
            rows.push(json!({ "weekday": wd, "start": d.start.trim(), "end": d.end.trim(), "position": rows.len() as i64 }));
        }
    }
    let hours: Record = db::create("hd_hours", &json!({ "code": code, "name": name, "utc_offset_minutes": input.utc_offset_minutes.unwrap_or(0), "is_default": input.is_default, "active": true, "days": rows }))?;
    if input.is_default {
        clear_default_except(id_of(&hours)?)?;
    }
    Ok(hours)
}

fn set_hours(input: SetHours) -> Result<Record> {
    require_manager()?;
    require("hd_hours", &input.id, "business hours")?;
    let mut data = json!({});
    if let Some(n) = opt(input.name) {
        data["name"] = json!(n);
    }
    if let Some(o) = input.utc_offset_minutes {
        if o.abs() > 14 * 60 {
            return Err(Error::msg("a UTC offset is within 14 hours"));
        }
        data["utc_offset_minutes"] = json!(o);
    }
    if let Some(d) = input.is_default {
        data["is_default"] = json!(d);
    }
    if let Some(a) = input.active {
        data["active"] = json!(a);
    }
    let out: Record = db::update("hd_hours", &input.id, &data)?.ok_or_else(|| Error::msg("the hours are gone"))?;
    if input.is_default == Some(true) {
        clear_default_except(&input.id)?;
    }
    Ok(out)
}

fn add_day(input: AddDay) -> Result<Vec<Record>> {
    require_manager()?;
    require("hd_hours", &input.hours, "business hours")?;
    let existing = db::count("hd_hours_day", db::Filter::eq("hours", input.hours.as_str()))?;
    let mut made = Vec::new();
    for (i, wd) in check_day(&DayIn { weekday: input.weekday.clone(), start: input.start.clone(), end: input.end.clone() })?.into_iter().enumerate() {
        made.push(db::create::<Record>("hd_hours_day", &json!({ "hours": input.hours, "weekday": wd, "start": input.start.trim(), "end": input.end.trim(), "position": existing as i64 + i as i64 }))?);
    }
    Ok(made)
}

fn new_holiday(input: AddHoliday) -> Result<Record> {
    require_manager()?;
    require("hd_hours", &input.hours, "business hours")?;
    let day = aether_sdk::dates::format_date(aether_sdk::dates::parse_date(&input.day)?);
    if db::count("hd_holiday", db::Filter::eq("hours", input.hours.as_str()).and(db::Filter::eq("day", day.as_str())))? > 0 {
        return Err(Error::msg("that day is already a day off"));
    }
    db::create("hd_holiday", &json!({ "hours": input.hours, "day": day, "name": opt(input.name) }))
}

fn target_row(t: &TargetIn) -> Result<Value> {
    let (r, s) = (t.response_minutes.unwrap_or(0), t.resolution_minutes.unwrap_or(0));
    if r < 0 || s < 0 {
        return Err(Error::msg("a target is not negative"));
    }
    if r == 0 && s == 0 {
        return Err(Error::msg("a target has a response time, a resolution time, or both"));
    }
    if r > 0 && s > 0 && s < r {
        return Err(Error::msg("resolving takes at least as long as the first reply"));
    }
    Ok(json!({ "priority": opt(t.priority.clone()), "response_minutes": r, "resolution_minutes": s }))
}

fn percent(p: Option<i64>) -> Result<Option<i64>> {
    match p {
        Some(v) if !(1..=100).contains(&v) => Err(Error::msg("the warning point is a percentage from 1 to 100")),
        other => Ok(other),
    }
}

fn create_policy(input: NewPolicy) -> Result<Record> {
    require_manager()?;
    let mut rows = Vec::new();
    for (i, t) in input.targets.iter().enumerate() {
        let mut row = target_row(t)?;
        row["position"] = json!(i as i64);
        rows.push(row);
    }
    db::create(
        "hd_sla_policy",
        &json!({
            "code": clean("a code", &input.code)?, "name": clean("a name", &input.name)?, "active": true, "rank": input.rank.unwrap_or(100),
            "hours": opt(input.hours), "priority": opt(input.priority), "ticket_type": opt(input.ticket_type), "team": opt(input.team),
            "customer": opt(input.customer), "channel": opt(input.channel), "warn_percent": percent(input.warn_percent)?.unwrap_or(80),
            "pause_when_pending": input.pause_when_pending.unwrap_or(true), "pause_when_on_hold": input.pause_when_on_hold.unwrap_or(true),
            "escalate_to": opt(input.escalate_to).map(|u| user_id(&u)), "targets": rows,
        }),
    )
}

fn set_policy(input: SetPolicy) -> Result<Record> {
    require_manager()?;
    require("hd_sla_policy", &input.id, "policy")?;
    let mut data = json!({});
    let put = |data: &mut Value, k: &str, v: Option<String>| {
        if let Some(v) = v {
            data[k] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    };
    if let Some(n) = opt(input.name) {
        data["name"] = json!(n);
    }
    if let Some(r) = input.rank {
        data["rank"] = json!(r);
    }
    if let Some(a) = input.active {
        data["active"] = json!(a);
    }
    for (k, v) in [("hours", input.hours), ("priority", input.priority), ("ticket_type", input.ticket_type), ("team", input.team), ("customer", input.customer), ("channel", input.channel)] {
        put(&mut data, k, v);
    }
    if let Some(p) = percent(input.warn_percent)? {
        data["warn_percent"] = json!(p);
    }
    if let Some(b) = input.pause_when_pending {
        data["pause_when_pending"] = json!(b);
    }
    if let Some(b) = input.pause_when_on_hold {
        data["pause_when_on_hold"] = json!(b);
    }
    if let Some(u) = input.escalate_to {
        data["escalate_to"] = if u.trim().is_empty() { Value::Null } else { json!(user_id(&u)) };
    }
    db::update("hd_sla_policy", &input.id, &data)?.ok_or_else(|| Error::msg("the policy is gone"))
}

fn add_target(input: AddTarget) -> Result<Record> {
    require_manager()?;
    require("hd_sla_policy", &input.policy, "policy")?;
    let mut row = target_row(&TargetIn { priority: input.priority.clone(), response_minutes: input.response_minutes, resolution_minutes: input.resolution_minutes })?;
    if db::count("hd_sla_target", db::Filter::eq("policy", input.policy.as_str()).and(match opt(input.priority) {
        Some(p) => db::Filter::eq("priority", p),
        None => db::Filter::is_empty("priority"),
    }))? > 0 {
        return Err(Error::msg("this policy already has a target for that priority"));
    }
    row["policy"] = json!(input.policy);
    row["position"] = json!(db::count("hd_sla_target", db::Filter::eq("policy", input.policy.as_str()))? as i64);
    db::create("hd_sla_target", &row)
}

/// The hours and policy a fresh organization starts with. Safe to run again.
fn seed() -> Result<Value> {
    let mut made = json!({ "hours": 0, "policies": 0 });
    let office = match db::find::<Record>("hd_hours").filter("code", "office").first()? {
        Some(h) => h,
        None => {
            made["hours"] = json!(1);
            let days: Vec<Value> = ["mon", "tue", "wed", "thu", "fri"].iter().enumerate().map(|(i, d)| json!({ "weekday": d, "start": "09:00", "end": "17:00", "position": i })).collect();
            db::create("hd_hours", &json!({ "code": "office", "name": "Office hours", "utc_offset_minutes": 0, "is_default": true, "active": true, "days": days }))?
        }
    };
    if db::find::<Record>("hd_hours").filter("code", "always").first()?.is_none() {
        db::create::<Record>("hd_hours", &json!({ "code": "always", "name": "Round the clock", "utc_offset_minutes": 0, "is_default": false, "active": true }))?;
        made["hours"] = json!(made["hours"].as_i64().unwrap_or(0) + 1);
    }
    if db::count("hd_sla_policy", db::Filter::all())? == 0 {
        let priorities: Vec<Record> = plugins::call("helpdesk", "search_priorities", &json!({ "active_only": true }))?;
        let mut targets = Vec::new();
        for (code, resp, res) in [("urgent", 30, 240), ("high", 120, 480), ("medium", 240, 960), ("low", 480, 2400)] {
            if let Some(p) = priorities.iter().find(|p| text(p, "code") == Some(code)) {
                targets.push(json!({ "priority": p["id"], "response_minutes": resp, "resolution_minutes": res, "position": targets.len() }));
            }
        }
        db::create::<Record>("hd_sla_policy", &json!({ "code": "standard", "name": "Standard support", "active": true, "rank": 100, "hours": office["id"], "warn_percent": 80, "pause_when_pending": true, "pause_when_on_hold": true, "targets": targets }))?;
        made["policies"] = json!(1);
    }
    Ok(made)
}

handler! {
    fn seed_sla(_: Empty) -> Value {
        seed()
    }

    fn create_business_hours(input: NewHours) -> Record {
        create_hours(input)
    }

    fn set_business_hours(input: SetHours) -> Record {
        set_hours(input)
    }

    fn add_hours_day(input: AddDay) -> Vec<Record> {
        add_day(input)
    }

    fn remove_hours_day(input: Id) -> Option<Record> {
        require_manager()?;
        db::delete("hd_hours_day", &input.id)
    }

    fn add_holiday(input: AddHoliday) -> Record {
        new_holiday(input)
    }

    fn remove_holiday(input: Id) -> Option<Record> {
        require_manager()?;
        db::delete("hd_holiday", &input.id)
    }

    fn list_business_hours(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_hours").limit(200).all()
    }

    fn get_business_hours(input: Id) -> Record {
        require_agent()?;
        db::find::<Record>("hd_hours").filter("id", input.id.as_str()).expand("days").first()?.ok_or_else(|| Error::msg("there are no such hours"))
    }

    fn list_hours_days(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_hours_day").filter("hours", input.id.as_str()).order_by("position").limit(100).all()
    }

    fn list_holidays(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_holiday").filter("hours", input.id.as_str()).order_by("day").limit(1000).all()
    }

    fn create_sla_policy(input: NewPolicy) -> Record {
        create_policy(input)
    }

    fn update_sla_policy(input: SetPolicy) -> Record {
        set_policy(input)
    }

    fn list_sla_policies(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_sla_policy").order_by("rank").limit(500).all()
    }

    fn get_sla_policy(input: Id) -> Record {
        require_agent()?;
        db::find::<Record>("hd_sla_policy").filter("id", input.id.as_str()).expand("targets").first()?.ok_or_else(|| Error::msg("there is no such policy"))
    }

    fn list_sla_targets(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_sla_target").filter("policy", input.id.as_str()).order_by("position").limit(100).all()
    }

    fn add_sla_target(input: AddTarget) -> Record {
        add_target(input)
    }

    fn remove_sla_target(input: Id) -> Option<Record> {
        require_manager()?;
        db::delete("hd_sla_target", &input.id)
    }

    /// When would a clock started at `start` with `minutes` of working time end?
    fn sla_preview(input: Preview) -> Value {
        require_agent()?;
        let schedule = schedule_for(opt(input.hours).as_deref())?;
        let start = aether_sdk::dates::parse_datetime(&input.start)?;
        Ok(json!({ "due_at": stamp(schedule.add(start, input.minutes)) }))
    }
}
