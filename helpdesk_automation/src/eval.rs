//! Deciding whether a condition holds, and filling in the words of a message. Pure, so it is tested natively.

use serde_json::Value;

pub const OPS: &[&str] = &["eq", "ne", "contains", "not_contains", "in", "gt", "lt", "gte", "lte", "is_set", "is_empty"];

/// What a rule may look at on a ticket.
pub const FIELDS: &[&str] = &[
    "state", "priority", "ticket_type", "team", "assignee", "channel", "customer", "requester_email", "requester_domain", "subject", "description", "tags",
    "unassigned", "waiting_for_us", "reopen_count", "message_count", "rating", "hours_open", "hours_since_update", "hours_since_customer_message",
    "hours_since_agent_message", "hours_to_due",
];

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn same(fact: &Value, wanted: &str) -> bool {
    match (number(fact), wanted.trim().parse::<f64>().ok()) {
        (Some(a), Some(b)) if !matches!(fact, Value::String(_)) || fact.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok()) => (a - b).abs() < 1e-9,
        _ => as_text(fact).trim().eq_ignore_ascii_case(wanted.trim()),
    }
}

pub fn is_set(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.trim().is_empty(),
        _ => true,
    }
}

pub fn holds(op: &str, fact: &Value, wanted: &str) -> bool {
    match op {
        "eq" => same(fact, wanted),
        "ne" => !same(fact, wanted),
        "contains" => !wanted.trim().is_empty() && as_text(fact).to_lowercase().contains(&wanted.trim().to_lowercase()),
        "not_contains" => !(!wanted.trim().is_empty() && as_text(fact).to_lowercase().contains(&wanted.trim().to_lowercase())),
        "in" => wanted.split(',').any(|w| same(fact, w)),
        "gt" | "lt" | "gte" | "lte" => match (number(fact), wanted.trim().parse::<f64>().ok()) {
            (Some(a), Some(b)) => match op {
                "gt" => a > b,
                "lt" => a < b,
                "gte" => a >= b,
                _ => a <= b,
            },
            _ => false,
        },
        "is_set" => is_set(fact),
        "is_empty" => !is_set(fact),
        _ => false,
    }
}

/// `{{reference}}` and friends replaced by their values; an unknown name is left empty.
pub fn fill(template: &str, values: &[(&str, String)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("}}") {
            Some(close) => {
                let key = after[..close].trim();
                if let Some((_, v)) = values.iter().find(|(k, _)| *k == key) {
                    out.push_str(v);
                }
                rest = &after[close + 2..];
            }
            None => {
                out.push_str(&rest[open..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_comparisons_ignore_case() {
        assert!(holds("eq", &json!("Open"), "open"));
        assert!(holds("ne", &json!("open"), "pending"));
        assert!(holds("contains", &json!("Printer is on fire"), "FIRE"));
        assert!(holds("not_contains", &json!("hello"), "fire"));
        assert!(holds("in", &json!("pending"), "open, pending ,closed"));
    }

    #[test]
    fn numbers_compare_as_numbers() {
        assert!(holds("gt", &json!(26.5), "24"));
        assert!(holds("lte", &json!(24), "24"));
        assert!(holds("eq", &json!(3), "3.0"));
        assert!(!holds("gt", &Value::Null, "1"));
        assert!(!holds("lt", &json!("many"), "5"));
    }

    #[test]
    fn set_and_empty() {
        assert!(holds("is_set", &json!("x"), ""));
        assert!(holds("is_empty", &Value::Null, ""));
        assert!(holds("is_empty", &json!("  "), ""));
        assert!(holds("is_set", &json!(true), ""));
        assert!(holds("is_empty", &json!(false), ""));
    }

    #[test]
    fn an_empty_needle_never_matches_contains() {
        assert!(!holds("contains", &json!("anything"), ""));
    }

    #[test]
    fn messages_are_filled() {
        assert_eq!(fill("Hi {{requester.name}}, {{reference}} {{nope}}!", &[("requester.name", "Pat".into()), ("reference", "HD-1".into())]), "Hi Pat, HD-1 !");
        assert_eq!(fill("open {{ x", &[]), "open {{ x");
    }
}
