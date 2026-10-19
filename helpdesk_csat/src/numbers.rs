//! The arithmetic of the reports, kept pure.

use std::collections::BTreeMap;

pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in values {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

#[derive(Debug, PartialEq)]
pub struct Summary {
    pub answered: usize,
    pub average: Option<f64>,
    /// Share of 4s and 5s among the answers, in percent.
    pub satisfied_percent: Option<i64>,
    /// Count of each rating 1 to 5.
    pub distribution: [usize; 5],
}

pub fn summarize(ratings: &[i64]) -> Summary {
    let mut distribution = [0usize; 5];
    for r in ratings {
        if (1..=5).contains(r) {
            distribution[(*r - 1) as usize] += 1;
        }
    }
    let answered: usize = distribution.iter().sum();
    if answered == 0 {
        return Summary { answered: 0, average: None, satisfied_percent: None, distribution };
    }
    let total: usize = distribution.iter().enumerate().map(|(i, n)| (i + 1) * n).sum();
    Summary {
        answered,
        average: Some((total as f64 / answered as f64 * 100.0).round() / 100.0),
        satisfied_percent: Some(((distribution[3] + distribution[4]) * 100 / answered) as i64),
        distribution,
    }
}

/// (key, ratings) grouped, best average first.
pub fn by_group(pairs: &[(String, i64)]) -> Vec<(String, Summary)> {
    let mut groups: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for (k, r) in pairs {
        groups.entry(k.as_str()).or_default().push(*r);
    }
    let mut out: Vec<(String, Summary)> = groups.into_iter().map(|(k, v)| (k.to_string(), summarize(&v))).collect();
    out.sort_by(|a, b| b.1.average.partial_cmp(&a.1.average).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_filled() {
        assert_eq!(fill("How did we do on {{reference}}? {{reference}}!", &[("reference", "HD-1")]), "How did we do on HD-1? HD-1!");
    }

    #[test]
    fn a_summary_counts_and_averages() {
        let s = summarize(&[5, 5, 4, 2, 1, 0, 9]);
        assert_eq!(s.answered, 5);
        assert_eq!(s.distribution, [1, 1, 0, 1, 2]);
        assert_eq!(s.average, Some(3.4));
        assert_eq!(s.satisfied_percent, Some(60));
    }

    #[test]
    fn nothing_answered_has_no_average() {
        assert_eq!(summarize(&[]), Summary { answered: 0, average: None, satisfied_percent: None, distribution: [0; 5] });
    }

    #[test]
    fn groups_come_best_first() {
        let g = by_group(&[("ann".into(), 5), ("bob".into(), 2), ("ann".into(), 4), ("bob".into(), 3)]);
        assert_eq!(g[0].0, "ann");
        assert_eq!(g[0].1.average, Some(4.5));
        assert_eq!(g[1].1.average, Some(2.5));
    }
}
