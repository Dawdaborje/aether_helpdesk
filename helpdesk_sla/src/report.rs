//! What people read: a ticket's clocks, the clocks needing attention, and how well the service levels are kept.

use aether_sdk::dates::Duration;
use aether_sdk::db::Filter;
use aether_sdk::prelude::*;

use crate::clocks::clock_view;
use crate::common::*;

#[derive(Deserialize)]
struct TicketIn {
    #[serde(alias = "id")]
    ticket: String,
}

#[derive(Deserialize, Default)]
struct Watch {
    /// `at_risk`, `breached`, `paused` or empty for every clock still open.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Deserialize, Default)]
struct Window {
    #[serde(default)]
    days: Option<i64>,
}

fn watch(input: Watch) -> Result<Vec<Value>> {
    require_agent()?;
    // The sweep marks a clock `warned` when it passes its warning point and `breached` when it runs out, so those
    // flags select the groups in the database; only the few rows shown are worked out live.
    let mut filter = match input.status.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        Some("breached") => Filter::eq("state", "running").and(Filter::eq("breached", true)),
        Some("at_risk") => Filter::eq("state", "running").and(Filter::eq("breached", false)).and(Filter::eq("warned", true)),
        Some("on_track") => Filter::eq("state", "running").and(Filter::eq("breached", false)).and(Filter::eq("warned", false)),
        Some("paused") => Filter::eq("state", "paused"),
        _ => Filter::one_of("state", ["running", "paused"]),
    };
    if let Some(k) = input.kind.as_deref().filter(|k| !k.is_empty()) {
        filter = filter.and(Filter::eq("kind", k));
    }
    let clocks: Vec<Record> = db::find("hd_sla_clock").matching(filter).order_by("due_at").limit(300).all()?;
    clock_view(clocks)
}

fn overview(input: Window) -> Result<Value> {
    require_agent()?;
    let now = now()?;
    let days = input.days.unwrap_or(30).clamp(1, 365);
    let since = stamp(now - Duration::days(days));
    let running = Filter::eq("state", "running");
    let breached_now = db::count("hd_sla_clock", running.clone().and(Filter::eq("breached", true)))?;
    let at_risk = db::count("hd_sla_clock", running.clone().and(Filter::eq("breached", false)).and(Filter::eq("warned", true)))?;
    let on_track = db::count("hd_sla_clock", running.and(Filter::eq("breached", false)).and(Filter::eq("warned", false)))?;
    let paused = db::count("hd_sla_clock", Filter::eq("state", "paused"))?;
    let done_in_period = Filter::eq("state", "done").and(Filter::gte("met_at", since));
    let mut rows = Vec::new();
    for kind in ["response", "resolution"] {
        let of_kind = done_in_period.clone().and(Filter::eq("kind", kind));
        let total = db::count("hd_sla_clock", of_kind.clone())?;
        let late = db::count("hd_sla_clock", of_kind.clone().and(Filter::eq("breached", true)))?;
        let mean = db::aggregate("hd_sla_clock", of_kind, &[], &[("average", db::Figure::avg("used_minutes"))])?;
        let average = mean.first().and_then(|r| r.get("average")).and_then(Value::as_f64).map(|m| m.round() as i64);
        rows.push(json!({
            "kind": kind, "finished": total, "met": total - late, "late": late,
            "compliance_percent": if total == 0 { Value::Null } else { json!((total - late) * 100 / total) }, "average_minutes": average,
        }));
    }
    let grouped = db::aggregate("hd_sla_clock", done_in_period, &["policy", "breached"], &[("clocks", db::Figure::Count)])?;
    let mut by_policy: std::collections::BTreeMap<String, (u64, u64)> = std::collections::BTreeMap::new();
    for g in &grouped {
        let id = g.get("policy").and_then(Value::as_str).unwrap_or("").to_string();
        let n = g.get("clocks").and_then(Value::as_u64).unwrap_or(0);
        let e = by_policy.entry(id).or_default();
        e.0 += n;
        if g.get("breached") != Some(&json!(true)) {
            e.1 += n;
        }
    }
    let mut policies = Vec::new();
    for (id, (n, met)) in by_policy {
        let name = db::find::<Record>("hd_sla_policy").filter("id", id.as_str()).first()?.and_then(|p| text(&p, "name").map(str::to_string)).unwrap_or(id.clone());
        policies.push(json!({ "policy": name, "finished": n, "met": met, "compliance_percent": if n == 0 { 0 } else { met * 100 / n } }));
    }
    Ok(json!({
        "days": days, "running": on_track + at_risk, "at_risk": at_risk, "breached": breached_now, "paused": paused,
        "by_kind": rows, "by_policy": policies,
    }))
}

handler! {
    /// The clocks of one ticket, with what is left of each.
    fn ticket_sla(input: TicketIn) -> Vec<Value> {
        require_agent()?;
        let clocks: Vec<Record> = db::find("hd_sla_clock").filter("ticket", input.ticket.as_str()).order_by("started_at").limit(50).all()?;
        clock_view(clocks)
    }

    fn list_sla_clocks(input: Watch) -> Vec<Value> {
        watch(input)
    }

    fn sla_overview(input: Window) -> Value {
        overview(input)
    }

    fn sla_by_kind(input: Window) -> Vec<Value> {
        Ok(overview(input)?["by_kind"].as_array().cloned().unwrap_or_default())
    }

    fn sla_by_policy(input: Window) -> Vec<Value> {
        Ok(overview(input)?["by_policy"].as_array().cloned().unwrap_or_default())
    }
}
