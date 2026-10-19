//! Numbers for the overview: what is open, what waits, how fast we answer, how people rate us.

use aether_sdk::dates::Duration;
use aether_sdk::db::{Figure, Filter};

use aether_sdk::prelude::*;

use crate::common::*;

/// Puts the name of a linked master record on each aggregate row (`priority` -> `priority_name`), so a chart can label
/// its bars with names. A record that can no longer be found keeps its key.
fn name_rows(rows: Vec<Value>, key: &str, plugin: &str, search: &str) -> Vec<Value> {
    let masters: Vec<Record> = plugins::call(plugin, search, &json!({})).unwrap_or_default();
    rows.into_iter()
        .map(|mut row| {
            let id = row.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
            let name = masters.iter().find(|m| text(m, "id") == Some(id.as_str())).and_then(|m| text(m, "name")).map(str::to_string);
            row[format!("{key}_name")] = json!(name.unwrap_or(if id.is_empty() { "None".to_string() } else { id }));
            row
        })
        .collect()
}

fn overview() -> Result<Value> {
    require_agent()?;
    let me = actor()?;
    let now_text = now()?;
    let now = aether_sdk::dates::parse_datetime(&now_text)?;
    let open = Filter::one_of("state", OPEN_STATES.iter().copied());
    let week = stamp(now - Duration::days(7));
    let by_state = db::aggregate("hd_ticket", Filter::ne("state", "merged"), &["state"], &[("tickets", Figure::Count)])?;
    let by_priority = name_rows(db::aggregate("hd_ticket", open.clone(), &["priority"], &[("tickets", Figure::Count)])?, "priority", "helpdesk", "search_priorities");
    let by_team = name_rows(db::aggregate("hd_ticket", open.clone(), &["team"], &[("tickets", Figure::Count)])?, "team", "helpdesk", "search_teams");
    let by_agent = db::aggregate("hd_ticket", open.clone().and(Filter::is_set("assignee")), &["assignee"], &[("tickets", Figure::Count)])?;
    let rating = db::aggregate("hd_ticket", Filter::is_set("rating"), &[], &[("rated", Figure::Count), ("average", Figure::avg("rating"))])?;
    let month = stamp(now - Duration::days(30));
    let means = db::aggregate("hd_ticket", Filter::gte("opened_at", month), &[], &[("first", Figure::avg("first_response_minutes")), ("solve", Figure::avg("resolution_minutes"))])?;
    let hours_of = |key: &str| means.first().and_then(|r| r.get(key)).and_then(Value::as_f64).map(|m| (m / 60.0 * 10.0).round() / 10.0);
    let mut trend = Vec::new();
    for back in (0..14).rev() {
        let from = now - Duration::days(back);
        let day = stamp(from)[..10].to_string();
        let (start, end) = (format!("{day}T00:00:00Z"), format!("{}T00:00:00Z", stamp(from + Duration::days(1))[..10].to_string()));
        let created = db::count("hd_ticket", Filter::gte("opened_at", start.as_str()).and(Filter::lt("opened_at", end.as_str())))?;
        let resolved = db::count("hd_ticket", Filter::gte("resolved_at", start.as_str()).and(Filter::lt("resolved_at", end.as_str())))?;
        trend.push(json!({ "day": day, "created": created, "resolved": resolved }));
    }
    Ok(json!({
        "open": db::count("hd_ticket", open.clone())?,
        "unassigned": db::count("hd_ticket", open.clone().and(Filter::is_empty("assignee")))?,
        "mine": match &me { Some(m) => db::count("hd_ticket", open.clone().and(Filter::eq("assignee", m.as_str())))?, None => 0 },
        "overdue": db::count("hd_ticket", open.clone().and(Filter::lt("due_at", now_text.as_str())))?,
        "created_7d": db::count("hd_ticket", Filter::gte("opened_at", week.as_str()))?,
        "resolved_7d": db::count("hd_ticket", Filter::gte("resolved_at", week.as_str()))?,
        "waiting_for_us": db::count("hd_ticket", Filter::eq("waiting_for_us", true).and(Filter::one_of("state", ["new", "open"])))?,
        "avg_first_response_hours": hours_of("first"),
        "avg_resolution_hours": hours_of("solve"),
        "rated": rating.first().and_then(|r| r.get("rated")).cloned().unwrap_or(json!(0)),
        "avg_rating": rating.first().and_then(|r| r.get("average")).cloned().unwrap_or(Value::Null),
        "by_state": by_state, "by_priority": by_priority, "by_team": by_team, "by_agent": by_agent, "trend": trend,
    }))
}

handler! {
    fn ticket_overview(_: Empty) -> Value {
        overview()
    }

    fn tickets_trend(_: Empty) -> Vec<Value> {
        let all = overview()?;
        Ok(all["trend"].as_array().cloned().unwrap_or_default())
    }

    fn tickets_by_state(_: Empty) -> Vec<Value> {
        let all = overview()?;
        Ok(all["by_state"].as_array().cloned().unwrap_or_default())
    }
}
