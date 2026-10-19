//! Merge fields: `{{requester.first_name}}` is replaced by a value; `{{requester.first_name|there}}` falls back to
//! `there` when the value is empty. A name nobody knows is left empty and reported, so the agent can see it before
//! sending. Text with `{{` and no closing `}}` is left as it is.

use std::collections::HashMap;

pub fn render(template: &str, values: &HashMap<String, String>) -> (String, Vec<String>) {
    let mut out = String::with_capacity(template.len());
    let mut missing: Vec<String> = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return (out, missing);
        };
        let spec = after[..close].trim();
        let (key, fallback) = match spec.split_once('|') {
            Some((k, f)) => (k.trim(), Some(f.trim())),
            None => (spec, None),
        };
        match values.get(key).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            Some(v) => out.push_str(v),
            None => match fallback {
                Some(f) => out.push_str(f),
                None => {
                    if !missing.iter().any(|m| m == key) {
                        missing.push(key.to_string());
                    }
                }
            },
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    (out, missing)
}

/// Words worth comparing: lower case, at least four letters, not a filler word.
pub fn words(text: &str) -> Vec<String> {
    const FILLER: &[&str] = &["that", "this", "with", "have", "from", "your", "will", "what", "when", "there", "about", "would", "could", "please", "thank", "thanks", "hello"];
    let mut out: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && !FILLER.contains(w))
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> HashMap<String, String> {
        [("requester.first_name", "Ada"), ("ticket.reference", "HD-000007"), ("agent.name", "")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn fields_are_filled() {
        let (text, missing) = render("Hi {{ requester.first_name }}, about {{ticket.reference}}.", &values());
        assert_eq!(text, "Hi Ada, about HD-000007.");
        assert!(missing.is_empty());
    }

    #[test]
    fn a_fallback_covers_an_empty_value() {
        assert_eq!(render("Hi {{agent.name|the team}}", &values()).0, "Hi the team");
        assert_eq!(render("Hi {{nobody|friend}}", &values()).0, "Hi friend");
    }

    #[test]
    fn unknown_fields_are_reported_once() {
        let (text, missing) = render("{{nope}} and {{nope}} and {{agent.name}}", &values());
        assert_eq!(text, " and  and ");
        assert_eq!(missing, vec!["nope", "agent.name"]);
    }

    #[test]
    fn an_unclosed_field_is_left_alone() {
        assert_eq!(render("Total {{ open", &values()).0, "Total {{ open");
    }

    #[test]
    fn words_skip_short_and_filler() {
        assert_eq!(words("Please reset the Password, thanks!"), vec!["password", "reset"]);
    }
}
