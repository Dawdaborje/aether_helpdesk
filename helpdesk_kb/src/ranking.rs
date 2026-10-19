//! Which articles answer a question best: words in the title count most, then tags, summary and body, with a nudge for
//! articles readers found helpful.

pub fn tokens(text: &str) -> Vec<String> {
    const FILLER: &[&str] = &["the", "and", "for", "how", "can", "with", "not", "are", "you", "does", "why", "what", "when", "where", "this", "that", "from", "have", "has", "was", "will", "get", "cant", "dont"];
    let mut out: Vec<String> = text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3 && !FILLER.contains(w)).map(str::to_string).collect();
    out.sort();
    out.dedup();
    out
}

fn count(haystack: &str, needle: &str) -> i64 {
    haystack.to_lowercase().matches(needle).count() as i64
}

pub struct Fields<'a> {
    pub title: &'a str,
    pub tags: &'a str,
    pub summary: &'a str,
    pub body: &'a str,
    pub helpful_yes: i64,
    pub helpful_no: i64,
    pub featured: bool,
}

/// Zero means the article does not answer the question at all.
pub fn score(query: &str, words: &[String], f: &Fields) -> i64 {
    let mut total = 0;
    for w in words {
        total += count(f.title, w).min(3) * 8 + count(f.tags, w).min(3) * 5 + count(f.summary, w).min(3) * 3 + count(f.body, w).min(5);
    }
    if total == 0 {
        return 0;
    }
    if !query.trim().is_empty() && f.title.to_lowercase().contains(&query.trim().to_lowercase()) {
        total += 15;
    }
    total + (f.helpful_yes - f.helpful_no).clamp(-3, 3) + if f.featured { 2 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article<'a>(title: &'a str, body: &'a str) -> Fields<'a> {
        Fields { title, tags: "", summary: "", body, helpful_yes: 0, helpful_no: 0, featured: false }
    }

    #[test]
    fn filler_and_short_words_are_dropped() {
        assert_eq!(tokens("How can I reset my password?"), vec!["password", "reset"]);
    }

    #[test]
    fn a_title_match_beats_a_body_match() {
        let q = "reset password";
        let w = tokens(q);
        assert!(score(q, &w, &article("Reset your password", "steps")) > score(q, &w, &article("Account tips", "you can reset a password here")));
    }

    #[test]
    fn no_shared_word_scores_nothing() {
        assert_eq!(score("refund", &tokens("refund"), &article("Reset your password", "steps")), 0);
    }

    #[test]
    fn helpful_articles_rise() {
        let w = tokens("invoice");
        let mut good = article("Invoice help", "x");
        good.helpful_yes = 5;
        let mut bad = article("Invoice help", "x");
        bad.helpful_no = 5;
        assert!(score("invoice", &w, &good) > score("invoice", &w, &bad));
    }
}
