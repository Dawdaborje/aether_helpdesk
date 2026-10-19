//! Choosing who gets a ticket: by hand, round robin, or by load.
//!
//! Round robin gives the next ticket to whoever was assigned one longest ago (or never). Load balanced gives it to
//! whoever has the fewest tickets still open compared with their capacity; a member at capacity is skipped.

use aether_sdk::prelude::*;

use crate::common::*;

struct Member {
    user: String,
    capacity: i64,
}

fn members(team: &str) -> Result<Vec<Member>> {
    let rows: Vec<Record> = plugins::call("helpdesk", "list_team_members", &json!({ "id": team }))?;
    Ok(rows
        .iter()
        .filter(|m| m.get("active") != Some(&json!(false)))
        .filter_map(|m| {
            Some(Member { user: user_id(text(m, "user")?), capacity: m.get("capacity").and_then(Value::as_i64).unwrap_or(0) })
        })
        .collect())
}

fn open_count(user: &str) -> Result<u64> {
    db::count("hd_ticket", db::Filter::eq("assignee", user).and(db::Filter::one_of("state", OPEN_STATES.iter().copied())))
}

fn last_assigned(user: &str) -> Result<String> {
    let rows: Vec<Record> = db::find("hd_ticket").filter("assignee", user).order_by("-assigned_at").limit(1).all()?;
    Ok(rows.first().and_then(|t| text(t, "assigned_at")).unwrap_or("").to_string())
}

/// Who the team's rule picks now, or none when the team assigns by hand or nobody can take more.
pub fn pick(team: &str) -> Result<Option<String>> {
    let team_row: Record = plugins::call("helpdesk", "get_team", &json!({ "id": team }))?;
    let mode = text(&team_row, "assignment").unwrap_or("manual").to_string();
    if mode == "manual" {
        return Ok(None);
    }
    let people = members(team)?;
    let mut best: Option<(String, (u64, String))> = None;
    for person in people {
        let open = open_count(&person.user)?;
        if person.capacity > 0 && open >= person.capacity as u64 {
            continue;
        }
        let key = if mode == "round_robin" { (0, last_assigned(&person.user)?) } else { (open, last_assigned(&person.user)?) };
        if best.as_ref().is_none_or(|(_, b)| key < *b) {
            best = Some((person.user, key));
        }
    }
    Ok(best.map(|(user, _)| user))
}

/// The agents of a team (or of every team when none is named), for a picker.
pub fn candidates(team: Option<&str>) -> Result<Vec<Value>> {
    let teams: Vec<String> = match team {
        Some(t) => vec![t.to_string()],
        None => {
            let rows: Vec<Record> = plugins::call("helpdesk", "search_teams", &json!({ "active_only": true }))?;
            rows.iter().filter_map(|t| text(t, "id").map(str::to_string)).collect()
        }
    };
    let mut out: Vec<Value> = Vec::new();
    for t in teams {
        for m in members(&t)? {
            if !out.iter().any(|o| o["id"] == m.user) {
                out.push(json!({ "id": m.user, "user": m.user, "capacity": m.capacity, "open": open_count(&m.user)? }));
            }
        }
    }
    Ok(out)
}
