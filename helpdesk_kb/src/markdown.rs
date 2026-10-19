//! A small, safe Markdown to HTML: headings, paragraphs, lists, code, bold, italic and links. Everything is escaped
//! first, so an article can never carry markup of its own; links must be web, mail or site-relative addresses.

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn pair(s: &str, marker: &str, open: &str, close: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find(marker) {
        let after = &rest[a + marker.len()..];
        match after.find(marker) {
            Some(b) if b > 0 => {
                out.push_str(&rest[..a]);
                out.push_str(open);
                out.push_str(&after[..b]);
                out.push_str(close);
                rest = &after[b + marker.len()..];
            }
            _ => {
                out.push_str(&rest[..a + marker.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('[') {
        let after = &rest[a + 1..];
        let parsed = after.find("](").and_then(|m| {
            let url_part = &after[m + 2..];
            url_part.find(')').map(|e| (&after[..m], &url_part[..e], m + 2 + e + 1))
        });
        match parsed {
            Some((text, url, used)) if ["http://", "https://", "mailto:", "/"].iter().any(|p| url.starts_with(p)) && !url.contains(' ') => {
                out.push_str(&rest[..a]);
                out.push_str(&format!("<a href=\"{url}\" rel=\"noopener\">{text}</a>"));
                rest = &after[used..];
            }
            _ => {
                out.push_str(&rest[..a + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn inline(s: &str) -> String {
    let escaped = escape(s);
    let mut out = String::new();
    for (i, part) in escaped.split('`').enumerate() {
        if i % 2 == 1 {
            out.push_str(&format!("<code>{part}</code>"));
        } else {
            out.push_str(&pair(&pair(&links(part), "**", "<strong>", "</strong>"), "*", "<em>", "</em>"));
        }
    }
    out
}

pub fn to_html(markdown: &str) -> String {
    let mut out = String::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut list: Option<&str> = None;
    let mut fence = false;
    let flush = |out: &mut String, paragraph: &mut Vec<&str>| {
        if !paragraph.is_empty() {
            out.push_str(&format!("<p>{}</p>\n", inline(&paragraph.join(" "))));
            paragraph.clear();
        }
    };
    let close_list = |out: &mut String, list: &mut Option<&str>| {
        if let Some(tag) = list.take() {
            out.push_str(&format!("</{tag}>\n"));
        }
    };
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            flush(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            out.push_str(if fence { "</pre>\n" } else { "<pre>" });
            fence = !fence;
            continue;
        }
        if fence {
            out.push_str(&escape(line));
            out.push('\n');
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            flush(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
        } else if let Some(h) = t.strip_prefix("### ") {
            flush(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            out.push_str(&format!("<h3>{}</h3>\n", inline(h)));
        } else if let Some(h) = t.strip_prefix("## ") {
            flush(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            out.push_str(&format!("<h2>{}</h2>\n", inline(h)));
        } else if let Some(h) = t.strip_prefix("# ") {
            flush(&mut out, &mut paragraph);
            close_list(&mut out, &mut list);
            out.push_str(&format!("<h1>{}</h1>\n", inline(h)));
        } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush(&mut out, &mut paragraph);
            if list != Some("ul") {
                close_list(&mut out, &mut list);
                out.push_str("<ul>\n");
                list = Some("ul");
            }
            out.push_str(&format!("<li>{}</li>\n", inline(item)));
        } else if let Some(item) = t.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).map(|(_, i)| i) {
            flush(&mut out, &mut paragraph);
            if list != Some("ol") {
                close_list(&mut out, &mut list);
                out.push_str("<ol>\n");
                list = Some("ol");
            }
            out.push_str(&format!("<li>{}</li>\n", inline(item)));
        } else {
            close_list(&mut out, &mut list);
            paragraph.push(t);
        }
    }
    flush(&mut out, &mut paragraph);
    close_list(&mut out, &mut list);
    if fence {
        out.push_str("</pre>\n");
    }
    out
}

/// The text of an article without the markup, for summaries.
pub fn plain(markdown: &str) -> String {
    markdown.lines().map(|l| l.trim().trim_start_matches(['#', '-', '*', '>', ' '])).filter(|l| !l.is_empty() && !l.starts_with("```")).collect::<Vec<_>>().join(" ").replace(['*', '`'], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_in_the_text_is_escaped() {
        assert_eq!(to_html("<script>alert(1)</script>"), "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>\n");
    }

    #[test]
    fn headings_lists_and_emphasis() {
        let html = to_html("# Title\n\nSome **bold** and *soft* text with `code`.\n\n- one\n- two\n\n1. first\n2. second");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>") && html.contains("<em>soft</em>") && html.contains("<code>code</code>"));
        assert!(html.contains("<ul>\n<li>one</li>\n<li>two</li>\n</ul>"));
        assert!(html.contains("<ol>\n<li>first</li>\n<li>second</li>\n</ol>"));
    }

    #[test]
    fn only_safe_links_are_made() {
        assert!(to_html("[ok](https://example.com/a)").contains("<a href=\"https://example.com/a\""));
        assert!(to_html("[site](/help/reset)").contains("<a href=\"/help/reset\""));
        assert!(!to_html("[bad](javascript:alert(1))").contains("<a "));
    }

    #[test]
    fn code_blocks_keep_their_text() {
        assert_eq!(to_html("```\na < b\n```"), "<pre>a &lt; b\n</pre>\n");
    }

    #[test]
    fn plain_text_drops_markup() {
        assert_eq!(plain("# Title\n\n**Bold** text\n- item"), "Title Bold text item");
    }
}
