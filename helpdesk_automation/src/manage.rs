use aether_sdk::db::Filter;
use aether_sdk::events::Received;
use aether_sdk::prelude::*;

use crate::engine::*;
use crate::eval::{FIELDS, OPS};

const TRIGGERS: &[&str] = &["created", "replied_by_customer", "replied_by_agent", "status_changed", "updated", "assigned", "resolved", "reopened", "time"];

const AGENT_ROLE: &str = "helpdesk.hd_agent";
const MANAGER_ROLE: &str = "helpdesk.hd_manager";
const ADMIN_ROLE: &str = "helpdesk.hd_admin";

#[derive(Deserialize, Clone)]
struct CondIn {
    field: String,
    op: String,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ActIn {
    action: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Deserialize)]
struct NewRule {
    name: String,
    trigger: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    match_mode: Option<String>,
    #[serde(default)]
    rank: Option<i64>,
    #[serde(default)]
    stop_after: bool,
    #[serde(default)]
    once_per_ticket: Option<bool>,
    #[serde(default)]
    repeat_hours: Option<i64>,
    #[serde(default)]
    conditions: Vec<CondIn>,
    #[serde(default)]
    actions: Vec<ActIn>,
    /// A form with one condition and one action sends them flat.
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    condition_value: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Deserialize)]
struct SetRule {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    match_mode: Option<String>,
    #[serde(default)]
    rank: Option<i64>,
    #[serde(default)]
    stop_after: Option<bool>,
    #[serde(default)]
    once_per_ticket: Option<bool>,
    #[serde(default)]
    repeat_hours: Option<i64>,
}

#[derive(Deserialize)]
struct AddCond {
    #[serde(alias = "id")]
    rule: String,
    field: String,
    op: String,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize)]
struct AddAct {
    #[serde(alias = "id")]
    rule: String,
    action: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Deserialize)]
struct NewMacro {
    name: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    actions: Vec<ActIn>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Deserialize)]
struct AddMacroAct {
    #[serde(alias = "id")]
    macro_id: String,
    action: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    subject: Option<String>,
}

#[derive(Deserialize)]
struct Apply {
    #[serde(alias = "macro")]
    macro_id: String,
    ticket: String,
}

#[derive(Deserialize)]
struct Test {
    #[serde(alias = "id")]
    rule: String,
    ticket: String,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize, Default)]
struct Runs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    rule: Option<String>,
    #[serde(default)]
    ticket: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
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
    if is_manager()? {
        Ok(())
    } else {
        Err(Error::msg(format!("you need the role `{MANAGER_ROLE}` to do this")))
    }
}

fn opt(v: Option<String>) -> Option<String> {
    v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

fn check_condition(c: &CondIn) -> Result<()> {
    if !FIELDS.contains(&c.field.as_str()) {
        return Err(Error::msg(format!("`{}` is not something a rule can look at: {}", c.field, FIELDS.join(", "))));
    }
    if !OPS.contains(&c.op.as_str()) {
        return Err(Error::msg(format!("`{}` is not a comparison: {}", c.op, OPS.join(", "))));
    }
    if !matches!(c.op.as_str(), "is_set" | "is_empty") && c.value.as_deref().is_none_or(|v| v.trim().is_empty()) {
        return Err(Error::msg(format!("`{}` needs a value", c.op)));
    }
    if matches!(c.op.as_str(), "gt" | "lt" | "gte" | "lte") && c.value.as_deref().and_then(|v| v.trim().parse::<f64>().ok()).is_none() {
        return Err(Error::msg("a greater-than or less-than comparison needs a number"));
    }
    Ok(())
}

fn act_json(a: &ActIn) -> Value {
    json!({ "action": a.action.trim(), "value": opt(a.value.clone()), "target": opt(a.target.clone()), "subject": opt(a.subject.clone()) })
}

fn check_rule_shape(trigger: &str, mode: Option<&str>) -> Result<()> {
    if !TRIGGERS.contains(&trigger) {
        return Err(Error::msg(format!("`{trigger}` is not a trigger: {}", TRIGGERS.join(", "))));
    }
    if mode.is_some_and(|m| !matches!(m, "all" | "any")) {
        return Err(Error::msg("conditions are matched as all or any"));
    }
    Ok(())
}

fn new_rule(mut input: NewRule) -> Result<Record> {
    require_manager()?;
    if input.conditions.is_empty() {
        if let (Some(field), Some(op)) = (input.field.take().filter(|f| !f.trim().is_empty()), input.op.take().filter(|o| !o.trim().is_empty())) {
            input.conditions.push(CondIn { field, op, value: input.condition_value.take() });
        }
    }
    if input.actions.is_empty() {
        if let Some(action) = input.action.take().filter(|a| !a.trim().is_empty()) {
            input.actions.push(ActIn { action, value: input.value.take(), target: input.target.take(), subject: input.subject.take() });
        }
    }
    check_rule_shape(&input.trigger, input.match_mode.as_deref())?;
    if input.name.trim().is_empty() {
        return Err(Error::msg("a rule has a name"));
    }
    if input.actions.is_empty() {
        return Err(Error::msg("a rule does something: give it at least one action"));
    }
    for c in &input.conditions {
        check_condition(c)?;
    }
    for a in &input.actions {
        check_action(&act_json(a))?;
    }
    let conditions: Vec<Value> = input.conditions.iter().enumerate().map(|(i, c)| json!({ "position": i, "field": c.field, "op": c.op, "value": opt(c.value.clone()) })).collect();
    let actions: Vec<Value> = input.actions.iter().enumerate().map(|(i, a)| { let mut j = act_json(a); j["position"] = json!(i); j }).collect();
    db::create(
        "hd_rule",
        &json!({
            "name": input.name.trim(), "description": opt(input.description), "active": true, "rank": input.rank.unwrap_or(100), "trigger": input.trigger,
            "match_mode": input.match_mode.unwrap_or_else(|| "all".into()), "stop_after": input.stop_after,
            "once_per_ticket": input.once_per_ticket.unwrap_or(input.trigger == "time"), "repeat_hours": input.repeat_hours.unwrap_or(0).max(0),
            "run_count": 0, "conditions": conditions, "actions": actions,
        }),
    )
}

fn change_rule(input: SetRule) -> Result<Record> {
    require_manager()?;
    check_rule_shape("created", input.match_mode.as_deref())?;
    let mut data = json!({});
    if let Some(n) = opt(input.name) {
        data["name"] = json!(n);
    }
    if let Some(d) = input.description {
        data["description"] = if d.trim().is_empty() { Value::Null } else { json!(d.trim()) };
    }
    for (k, v) in [("active", input.active), ("stop_after", input.stop_after), ("once_per_ticket", input.once_per_ticket)] {
        if let Some(v) = v {
            data[k] = json!(v);
        }
    }
    if let Some(m) = input.match_mode {
        data["match_mode"] = json!(m);
    }
    if let Some(r) = input.rank {
        data["rank"] = json!(r);
    }
    if let Some(h) = input.repeat_hours {
        data["repeat_hours"] = json!(h.max(0));
    }
    db::update("hd_rule", &input.id, &data)?.ok_or_else(|| Error::msg("there is no such rule"))
}

fn rule_with_children(id: &str) -> Result<Record> {
    db::find::<Record>("hd_rule").filter("id", id).expand("conditions").expand("actions").first()?.ok_or_else(|| Error::msg("there is no such rule"))
}

fn test_rule(input: Test) -> Result<Value> {
    require_agent()?;
    let rule = rule_with_children(&input.rule)?;
    let t: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": input.ticket }))?;
    let mut look = Lookups::default();
    let f = facts(&t, now()?);
    let matches = conditions_hold(&rule, &f, &mut look);
    let mut would = Vec::new();
    if matches {
        for a in rule.get("actions").and_then(Value::as_array).cloned().unwrap_or_default() {
            would.push(match act(&a, &input.ticket, &t, &mut look, true) {
                Ok(l) => l,
                Err(e) => format!("would fail: {e}"),
            });
        }
    }
    Ok(json!({ "matches": matches, "would": would, "facts": f }))
}

fn visible_macro(m: &Record, me: Option<&str>, teams: &[String], manager: bool) -> bool {
    match text(m, "scope") {
        Some("global") => true,
        Some("team") => manager || text(m, "team").is_some_and(|t| teams.iter().any(|x| x == t)),
        _ => text(m, "owner") == me,
    }
}

fn my_team_ids() -> Result<Vec<String>> {
    let teams: Vec<Record> = plugins::call("helpdesk", "my_teams", &json!({}))?;
    Ok(teams.iter().filter_map(|t| text(t, "id").map(str::to_string)).collect())
}

fn macros_for_me() -> Result<Vec<Record>> {
    require_agent()?;
    let me = context::current()?.actor.id;
    let teams = my_team_ids()?;
    let manager = is_manager()?;
    let all: Vec<Record> = db::find("hd_macro").filter("active", true).order_by("name").limit(1000).all()?;
    Ok(all.into_iter().filter(|m| visible_macro(m, me.as_deref(), &teams, manager)).collect())
}

fn macro_checked(id: &str, edit: bool) -> Result<Record> {
    let m = db::find::<Record>("hd_macro").filter("id", id).expand("actions").first()?.ok_or_else(|| Error::msg("there is no such macro"))?;
    let me = context::current()?.actor.id;
    let manager = is_manager()?;
    let teams = my_team_ids()?;
    if !visible_macro(&m, me.as_deref(), &teams, manager) {
        return Err(Error::msg("you cannot use this macro"));
    }
    if edit && text(&m, "scope") != Some("personal") && !manager {
        return Err(Error::msg("only a manager changes a shared macro"));
    }
    Ok(m)
}

fn new_macro(mut input: NewMacro) -> Result<Record> {
    require_agent()?;
    if input.actions.is_empty() {
        if let Some(action) = input.action.take().filter(|a| !a.trim().is_empty()) {
            input.actions.push(ActIn { action, value: input.value.take(), target: input.target.take(), subject: input.subject.take() });
        }
    }
    let scope = input.scope.clone().unwrap_or_else(|| "personal".into());
    let team = opt(input.team);
    match scope.as_str() {
        "personal" => {}
        "global" => require_manager()?,
        "team" => {
            let t = team.as_deref().ok_or_else(|| Error::msg("name the team that shares this macro"))?;
            if !is_manager()? && !my_team_ids()?.iter().any(|x| x == t) {
                return Err(Error::msg("you can share a macro only with a team you belong to"));
            }
        }
        _ => return Err(Error::msg("a macro is personal, shared with a team, or global")),
    }
    if input.name.trim().is_empty() {
        return Err(Error::msg("a macro has a name"));
    }
    if input.actions.is_empty() {
        return Err(Error::msg("a macro does something: give it at least one action"));
    }
    let mut rows = Vec::new();
    for (i, a) in input.actions.iter().enumerate() {
        let j = act_json(a);
        check_action(&j)?;
        let mut j = j;
        j["position"] = json!(i);
        rows.push(j);
    }
    db::create(
        "hd_macro",
        &json!({ "name": input.name.trim(), "scope": scope, "owner": if scope == "personal" { context::current()?.actor.id } else { None }, "team": if scope == "team" { team } else { None }, "active": true, "use_count": 0, "actions": rows }),
    )
}

fn apply_macro(input: Apply) -> Result<Value> {
    let m = macro_checked(&input.macro_id, false)?;
    let t: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": input.ticket }))?;
    let mut look = Lookups::default();
    let mut lines = Vec::new();
    for a in m.get("actions").and_then(Value::as_array).cloned().unwrap_or_default() {
        match act(&a, &input.ticket, &t, &mut look, false) {
            Ok(l) => lines.push(l),
            Err(e) => {
                lines.push(format!("stopped: {e}"));
                return Err(Error::msg(format!("the macro stopped after: {}. {e}", if lines.len() > 1 { lines[..lines.len() - 1].join("; ") } else { "nothing".into() })));
            }
        }
    }
    db::update::<Record>("hd_macro", text(&m, "id").unwrap_or_default(), &json!({ "use_count": m.get("use_count").and_then(Value::as_i64).unwrap_or(0) + 1 }))?;
    let _ = plugins::call::<Value>("helpdesk_ticket", "add_ticket_note", &json!({ "id": input.ticket, "body": format!("Macro “{}” applied: {}.", text(&m, "name").unwrap_or(""), lines.join("; ")) }));
    Ok(json!({ "applied": lines }))
}

fn ticket_of(r: &Received) -> Result<String> {
    r.payload.get("ticket").and_then(Value::as_str).map(str::to_string).ok_or_else(|| Error::msg("the event names no ticket"))
}

handler! {
    fn create_rule(input: NewRule) -> Record {
        new_rule(input)
    }

    fn update_rule(input: SetRule) -> Record {
        change_rule(input)
    }

    fn add_rule_condition(input: AddCond) -> Record {
        require_manager()?;
        let c = CondIn { field: input.field, op: input.op, value: input.value };
        check_condition(&c)?;
        db::find::<Record>("hd_rule").filter("id", input.rule.as_str()).first()?.ok_or_else(|| Error::msg("there is no such rule"))?;
        let n = db::count("hd_rule_condition", Filter::eq("rule", input.rule.as_str()))?;
        db::create("hd_rule_condition", &json!({ "rule": input.rule, "position": n as i64, "field": c.field, "op": c.op, "value": opt(c.value) }))
    }

    fn remove_rule_condition(input: Id) -> Option<Record> {
        require_manager()?;
        db::delete("hd_rule_condition", &input.id)
    }

    fn add_rule_action(input: AddAct) -> Record {
        require_manager()?;
        let a = ActIn { action: input.action, value: input.value, target: input.target, subject: input.subject };
        check_action(&act_json(&a))?;
        db::find::<Record>("hd_rule").filter("id", input.rule.as_str()).first()?.ok_or_else(|| Error::msg("there is no such rule"))?;
        let n = db::count("hd_rule_action", Filter::eq("rule", input.rule.as_str()))?;
        let mut j = act_json(&a);
        j["rule"] = json!(input.rule);
        j["position"] = json!(n as i64);
        db::create("hd_rule_action", &j)
    }

    fn remove_rule_action(input: Id) -> Option<Record> {
        require_manager()?;
        db::delete("hd_rule_action", &input.id)
    }

    fn list_rules(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_rule").order_by("rank").limit(1000).all()
    }

    fn get_rule(input: Id) -> Record {
        require_agent()?;
        rule_with_children(&input.id)
    }

    fn list_rule_conditions(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_rule_condition").filter("rule", input.id.as_str()).order_by("position").limit(200).all()
    }

    fn list_rule_actions(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_rule_action").filter("rule", input.id.as_str()).order_by("position").limit(200).all()
    }

    fn list_rule_runs(input: Runs) -> Vec<Record> {
        require_agent()?;
        let mut q = db::find::<Record>("hd_rule_run");
        // On a ticket's tab the page's own `id` is the ticket; on a rule's page it is the rule.
        match (input.ticket, input.rule.or(input.id)) {
            (Some(t), _) => q = q.filter("ticket", t.as_str()),
            (None, Some(r)) => q = q.filter("rule", r.as_str()),
            (None, None) => {}
        }
        q.order_by("-at").limit(input.limit.unwrap_or(100).min(500)).all()
    }

    /// What a rule would do to a ticket, without doing it.
    fn test_rule_on_ticket(input: Test) -> Value {
        test_rule(input)
    }

    /// Tickets to pick from when trying a rule.
    fn ticket_choices(_: Empty) -> Vec<Value> {
        require_agent()?;
        let page: Value = plugins::call("helpdesk_ticket", "list_tickets", &json!({ "limit": 200 }))?;
        Ok(page.get("rows").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    fn automation_sweep(_: Empty) -> Value {
        require_agent()?;
        sweep()
    }

    fn automation_sweep_page(input: SweepPage) -> Value {
        require_agent()?;
        sweep_page(input.offset)
    }

    fn create_macro(input: NewMacro) -> Record {
        new_macro(input)
    }

    fn update_macro(input: MacroActive) -> Record {
        require_agent()?;
        macro_checked(&input.id, true)?;
        db::update("hd_macro", &input.id, &json!({ "active": input.active }))?.ok_or_else(|| Error::msg("there is no such macro"))
    }

    fn add_macro_action(input: AddMacroAct) -> Record {
        require_agent()?;
        macro_checked(&input.macro_id, true)?;
        let a = ActIn { action: input.action, value: input.value, target: input.target, subject: input.subject };
        check_action(&act_json(&a))?;
        let n = db::count("hd_macro_action", Filter::eq("macro", input.macro_id.as_str()))?;
        let mut j = act_json(&a);
        j["macro"] = json!(input.macro_id);
        j["position"] = json!(n as i64);
        db::create("hd_macro_action", &j)
    }

    fn remove_macro_action(input: Id) -> Option<Record> {
        require_agent()?;
        let a = db::find::<Record>("hd_macro_action").filter("id", input.id.as_str()).first()?.ok_or_else(|| Error::msg("there is no such action"))?;
        macro_checked(text(&a, "macro").unwrap_or_default(), true)?;
        db::delete("hd_macro_action", &input.id)
    }

    fn list_macros(_: Empty) -> Vec<Record> {
        macros_for_me()
    }

    fn get_macro(input: Id) -> Record {
        require_agent()?;
        macro_checked(&input.id, false)
    }

    fn list_macro_actions(input: Id) -> Vec<Record> {
        require_agent()?;
        macro_checked(&input.id, false)?;
        db::find("hd_macro_action").filter("macro", input.id.as_str()).order_by("position").limit(100).all()
    }

    fn apply_macro_to_ticket(input: Apply) -> Value {
        apply_macro(input)
    }

    fn on_ticket_created(received: Received) -> Value {
        Ok(json!(on_event("created", &ticket_of(&received)?, received.depth)?))
    }

    fn on_ticket_updated(received: Received) -> Value {
        Ok(json!(on_event("updated", &ticket_of(&received)?, received.depth)?))
    }

    fn on_ticket_assigned(received: Received) -> Value {
        Ok(json!(on_event("assigned", &ticket_of(&received)?, received.depth)?))
    }

    fn on_status_changed(received: Received) -> Value {
        Ok(json!(on_event("status_changed", &ticket_of(&received)?, received.depth)?))
    }

    fn on_ticket_replied(received: Received) -> Value {
        let trigger = if received.payload.get("by").and_then(Value::as_str) == Some("customer") { "replied_by_customer" } else { "replied_by_agent" };
        Ok(json!(on_event(trigger, &ticket_of(&received)?, received.depth)?))
    }

    fn on_ticket_resolved(received: Received) -> Value {
        Ok(json!(on_event("resolved", &ticket_of(&received)?, received.depth)?))
    }

    fn on_ticket_reopened(received: Received) -> Value {
        Ok(json!(on_event("reopened", &ticket_of(&received)?, received.depth)?))
    }
}

#[derive(Deserialize)]
struct MacroActive {
    id: String,
    active: bool,
}

#[derive(Deserialize)]
struct SweepPage {
    #[serde(default)]
    offset: u32,
}
