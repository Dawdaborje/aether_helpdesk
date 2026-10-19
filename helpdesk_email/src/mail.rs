//! Reading mail: addresses, the shapes in which providers hand a message over, a plain RFC 822 message, and cutting a
//! reply down to what the person just wrote. Pure text work, tested natively.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Inbound {
    pub from_email: String,
    pub from_name: Option<String>,
    pub to: Vec<String>,
    pub subject: String,
    pub text: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub auto_submitted: Option<String>,
    pub precedence: Option<String>,
    pub spam: bool,
}

/// `Ada Lovelace <ada@example.com>` or `ada@example.com`: the name, if any, and the lower-case address.
pub fn parse_address(raw: &str) -> Option<(Option<String>, String)> {
    let raw = raw.trim();
    if let (Some(open), Some(close)) = (raw.rfind('<'), raw.rfind('>')) {
        if open < close {
            let email = raw[open + 1..close].trim().to_lowercase();
            if email.contains('@') {
                let name = raw[..open].trim().trim_matches('"').trim().to_string();
                return Some(((!name.is_empty()).then_some(name), email));
            }
        }
    }
    let bare = raw.split_whitespace().find(|w| w.contains('@'))?.trim_matches(|c| c == '<' || c == '>' || c == ',' || c == ';' || c == '"');
    bare.contains('@').then(|| (None, bare.to_lowercase()))
}

pub fn parse_addresses(raw: &str) -> Vec<String> {
    raw.split([',', ';']).filter_map(|p| parse_address(p).map(|(_, e)| e)).collect()
}

/// `support+abc@example.com` is the mailbox `support@example.com`.
pub fn base_address(address: &str) -> String {
    let a = address.trim().to_lowercase();
    match a.split_once('@') {
        Some((local, host)) => format!("{}@{}", local.split('+').next().unwrap_or(local), host),
        None => a,
    }
}

fn is_quote_start(line: &str, next: Option<&str>) -> bool {
    let t = line.trim();
    let lower = t.to_lowercase();
    (lower.starts_with("on ") && (lower.ends_with("wrote:") || next.is_some_and(|n| n.trim().to_lowercase().ends_with("wrote:"))))
        || lower.starts_with("-----original message")
        || lower.starts_with("-----ursprüngliche")
        || (lower.starts_with("from:") && next.is_some_and(|n| n.trim().to_lowercase().starts_with("sent:")))
        || (t.len() > 20 && t.chars().all(|c| c == '_'))
}

/// What the person wrote this time, without the earlier messages quoted under it and without their signature.
pub fn trim_quoted(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = lines.len();
    for (i, line) in lines.iter().enumerate() {
        if is_quote_start(line, lines.get(i + 1).copied()) {
            keep = i;
            break;
        }
    }
    let mut kept: Vec<&str> = lines[..keep].to_vec();
    while kept.last().is_some_and(|l| l.trim().is_empty() || l.trim_start().starts_with('>')) {
        kept.pop();
    }
    if let Some(sig) = kept.iter().rposition(|l| l.trim_end() == "--" || l.trim_end() == "-- ") {
        kept.truncate(sig);
        while kept.last().is_some_and(|l| l.trim().is_empty()) {
            kept.pop();
        }
    }
    let out = kept.join("\n").trim().to_string();
    if out.is_empty() { text.trim().to_string() } else { out }
}

// --- encodings ---

fn decode_qp(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'=' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => i += 2,
            b'=' if i + 2 < bytes.len() && bytes[i + 1] == b'\r' && bytes[i + 2] == b'\n' => i += 3,
            b'=' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'=');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn decode_b64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

/// `=?UTF-8?B?...?=` and `=?UTF-8?Q?...?=` in a header.
fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("=?") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let parts: Vec<&str> = after.splitn(3, '?').collect();
        if parts.len() == 3 {
            if let Some(end) = parts[2].find("?=") {
                let data = &parts[2][..end];
                let bytes = match parts[1].to_ascii_uppercase().as_str() {
                    "B" => decode_b64(data),
                    _ => decode_qp(&data.replace('_', " ")),
                };
                out.push_str(&String::from_utf8_lossy(&bytes));
                let consumed = start + 2 + parts[0].len() + 1 + parts[1].len() + 1 + end + 2;
                rest = &rest[consumed..];
                if rest.trim_start().starts_with("=?") {
                    rest = rest.trim_start();
                }
                continue;
            }
        }
        out.push_str("=?");
        rest = after;
    }
    out.push_str(rest);
    out
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").split_whitespace().collect::<Vec<_>>().join(" ")
}

// --- RFC 822 ---

fn split_message(raw: &str) -> (Vec<(String, String)>, &str) {
    let raw_norm = raw;
    let (head, body) = match raw_norm.find("\r\n\r\n").map(|i| (i, 4)).or_else(|| raw_norm.find("\n\n").map(|i| (i, 2))) {
        Some((i, n)) => (&raw_norm[..i], &raw_norm[i + n..]),
        None => (raw_norm, ""),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_lowercase(), v.trim().to_string()));
        }
    }
    (headers, body)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn param(value: &str, name: &str) -> Option<String> {
    let lower = value.to_lowercase();
    let at = lower.find(&format!("{name}="))?;
    let rest = &value[at + name.len() + 1..];
    let rest = rest.trim_start();
    Some(if let Some(q) = rest.strip_prefix('"') { q.split('"').next()?.to_string() } else { rest.split(';').next()?.trim().to_string() })
}

fn body_text(headers: &[(String, String)], body: &str) -> String {
    let ctype = header(headers, "content-type").unwrap_or("text/plain");
    let lower = ctype.to_lowercase();
    if lower.starts_with("multipart/") {
        if let Some(boundary) = param(ctype, "boundary") {
            let marker = format!("--{boundary}");
            let mut plain = None;
            let mut html = None;
            for part in body.split(&marker).skip(1) {
                if part.starts_with("--") {
                    break;
                }
                let (h, b) = split_message(part.trim_start_matches(['\r', '\n']));
                let pt = header(&h, "content-type").unwrap_or("text/plain").to_lowercase();
                let text = body_text(&h, b);
                if pt.starts_with("multipart/") {
                    if plain.is_none() && !text.is_empty() {
                        plain = Some(text);
                    }
                } else if pt.starts_with("text/plain") && plain.is_none() {
                    plain = Some(text);
                } else if pt.starts_with("text/html") && html.is_none() {
                    html = Some(text);
                }
            }
            return plain.or(html).unwrap_or_default();
        }
    }
    let encoded = header(headers, "content-transfer-encoding").unwrap_or("7bit").to_lowercase();
    let bytes = match encoded.as_str() {
        "quoted-printable" => decode_qp(body),
        "base64" => decode_b64(body),
        _ => body.as_bytes().to_vec(),
    };
    let text = String::from_utf8_lossy(&bytes).to_string();
    if lower.starts_with("text/html") { strip_tags(&text) } else { text }
}

pub fn parse_rfc822(raw: &str) -> Inbound {
    let (headers, body) = split_message(raw);
    let from = header(&headers, "from").map(decode_words).unwrap_or_default();
    let (from_name, from_email) = parse_address(&from).unwrap_or((None, String::new()));
    let mut to = Vec::new();
    for h in ["to", "cc", "delivered-to", "x-original-to"] {
        if let Some(v) = header(&headers, h) {
            to.extend(parse_addresses(v));
        }
    }
    let flag = |name: &str| header(&headers, name).is_some_and(|v| v.trim_start().to_lowercase().starts_with("yes"));
    Inbound {
        from_email,
        from_name,
        to,
        subject: header(&headers, "subject").map(decode_words).unwrap_or_default(),
        text: body_text(&headers, body),
        message_id: header(&headers, "message-id").map(|s| s.to_string()),
        in_reply_to: header(&headers, "in-reply-to").map(|s| s.to_string()),
        references: header(&headers, "references").map(|s| s.to_string()),
        auto_submitted: header(&headers, "auto-submitted").map(|s| s.to_string()),
        precedence: header(&headers, "precedence").map(|s| s.to_string()),
        spam: flag("x-spam-flag") || flag("x-spam-status"),
    }
}

// --- provider payloads ---

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string).filter(|x| !x.is_empty())
}

fn first(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| s(v, k))
}

fn addresses_of(v: &Value) -> Vec<String> {
    match v {
        Value::String(t) => parse_addresses(t),
        Value::Array(a) => a
            .iter()
            .flat_map(|x| match x {
                Value::String(t) => parse_addresses(t),
                Value::Object(_) => first(x, &["Email", "email", "address"]).map(|e| vec![e.to_lowercase()]).unwrap_or_default(),
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A header from a list of `{Name, Value}` rows (Postmark) or an object (generic).
fn header_of(headers: &Value, name: &str) -> Option<String> {
    match headers {
        Value::Array(rows) => rows.iter().find(|r| first(r, &["Name", "name"]).is_some_and(|n| n.eq_ignore_ascii_case(name))).and_then(|r| first(r, &["Value", "value"])),
        Value::Object(map) => map.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).and_then(|(_, v)| v.as_str().map(str::to_string)),
        _ => None,
    }
}

fn from_parts(v: &Value, headers: &Value) -> Inbound {
    let from_raw = first(v, &["from_email", "from", "From", "sender", "Sender"]).unwrap_or_default();
    let (parsed_name, email) = parse_address(&from_raw).unwrap_or((None, String::new()));
    let email = if email.is_empty() { first(v, &["from_email", "FromEmail"]).unwrap_or_default().to_lowercase() } else { email };
    let mut to = Vec::new();
    for k in ["to", "To", "ToFull", "recipient", "OriginalRecipient", "cc", "Cc", "CcFull"] {
        if let Some(x) = v.get(k) {
            to.extend(addresses_of(x));
        }
    }
    let spam = ["X-Spam-Flag", "X-Spam-Status"].iter().any(|h| header_of(headers, h).is_some_and(|x| x.trim_start().to_lowercase().starts_with("yes")));
    Inbound {
        from_email: email,
        from_name: first(v, &["from_name", "FromName", "name"]).or(parsed_name),
        to,
        subject: first(v, &["subject", "Subject"]).unwrap_or_default(),
        text: first(v, &["text", "text_body", "TextBody", "body", "body_plain", "StrippedTextReply"]).or_else(|| first(v, &["html", "HtmlBody"]).map(|h| strip_tags(&h))).unwrap_or_default(),
        message_id: first(v, &["message_id", "messageId", "MessageID", "Message-Id"]).or_else(|| header_of(headers, "Message-ID")),
        in_reply_to: first(v, &["in_reply_to", "InReplyTo"]).or_else(|| header_of(headers, "In-Reply-To")),
        references: first(v, &["references", "References"]).or_else(|| header_of(headers, "References")),
        auto_submitted: header_of(headers, "Auto-Submitted"),
        precedence: header_of(headers, "Precedence"),
        spam,
    }
}

/// A message as it arrived, whatever the sender of the webhook calls things.
pub fn normalize(payload: &Value) -> Result<Inbound, String> {
    if let Some(raw) = s(payload, "raw") {
        return Ok(parse_rfc822(&raw));
    }
    let inner = if s(payload, "type").as_deref() == Some("email.received") { payload.get("data").unwrap_or(payload) } else { payload };
    let headers = inner.get("headers").or_else(|| inner.get("Headers")).cloned().unwrap_or(Value::Null);
    let message = from_parts(inner, &headers);
    if message.from_email.is_empty() || !message.from_email.contains('@') {
        return Err("the message has no sender address".into());
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn addresses_are_read() {
        assert_eq!(parse_address("Ada Lovelace <Ada@Example.com>"), Some((Some("Ada Lovelace".into()), "ada@example.com".into())));
        assert_eq!(parse_address("\"Doe, Jo\" <jo@x.org>"), Some((Some("Doe, Jo".into()), "jo@x.org".into())));
        assert_eq!(parse_address("plain@x.org"), Some((None, "plain@x.org".into())));
        assert_eq!(parse_address("nobody"), None);
        assert_eq!(parse_addresses("a@x.org, B <b@x.org>"), vec!["a@x.org", "b@x.org"]);
        assert_eq!(base_address("Support+k3j@Example.com"), "support@example.com");
    }

    #[test]
    fn a_reply_is_cut_to_what_was_just_written() {
        let t = "Thanks, that worked.\n\nBest,\nAda\n\nOn Mon, 5 Oct 2026 at 10:00, Support <s@x.org> wrote:\n> Did you try it?\n> Please say.";
        assert_eq!(trim_quoted(t), "Thanks, that worked.\n\nBest,\nAda");
        let t = "Yes.\n\n-----Original Message-----\nFrom: s@x.org\nSent: Monday";
        assert_eq!(trim_quoted(t), "Yes.");
        let t = "Hello\n\nbody\n\n-- \nAda, ACME";
        assert_eq!(trim_quoted(t), "Hello\n\nbody");
        assert_eq!(trim_quoted("> only quoted"), "> only quoted");
    }

    #[test]
    fn encodings_decode() {
        assert_eq!(decode_words("=?UTF-8?B?SGVsbG8gd29ybGQ=?="), "Hello world");
        assert_eq!(decode_words("=?UTF-8?Q?Caf=C3=A9_menu?="), "Café menu");
        assert_eq!(String::from_utf8(decode_qp("caf=C3=A9 soft=\nbreak")).unwrap(), "café softbreak");
    }

    #[test]
    fn a_plain_message_parses() {
        let raw = "From: Ada <ada@x.org>\nTo: support@co.com\nSubject: Printer\nMessage-ID: <a1@x.org>\nAuto-Submitted: no\n\nIt jams.\n";
        let m = parse_rfc822(raw);
        assert_eq!((m.from_email.as_str(), m.subject.as_str(), m.text.trim(), m.message_id.as_deref()), ("ada@x.org", "Printer", "It jams.", Some("<a1@x.org>")));
        assert_eq!(m.to, vec!["support@co.com"]);
    }

    #[test]
    fn a_multipart_message_prefers_plain_text() {
        let raw = "From: a@x.org\nSubject: =?UTF-8?B?SGk=?=\nContent-Type: multipart/alternative; boundary=\"B1\"\n\n--B1\nContent-Type: text/html\n\n<p>Hello <b>HTML</b></p>\n--B1\nContent-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\nHello caf=C3=A9\n--B1--\n";
        let m = parse_rfc822(raw);
        assert_eq!(m.subject, "Hi");
        assert_eq!(m.text.trim(), "Hello café");
    }

    #[test]
    fn html_only_mail_becomes_text() {
        let raw = "From: a@x.org\nSubject: s\nContent-Type: text/html\n\n<div>Hello&nbsp;<b>there</b></div>";
        assert_eq!(parse_rfc822(raw).text, "Hello there");
    }

    #[test]
    fn provider_shapes_are_understood() {
        let generic = normalize(&json!({ "from": "Pat <pat@x.org>", "to": ["support@co.com"], "subject": "Hi", "text": "Body", "message_id": "<m1>", "headers": { "Auto-Submitted": "auto-replied" } })).unwrap();
        assert_eq!((generic.from_email.as_str(), generic.from_name.as_deref(), generic.auto_submitted.as_deref()), ("pat@x.org", Some("Pat"), Some("auto-replied")));
        let resend = normalize(&json!({ "type": "email.received", "data": { "from": "Pat <pat@x.org>", "to": ["support@co.com"], "subject": "Hi", "text": "Body", "message_id": "<m2>" } })).unwrap();
        assert_eq!((resend.from_email.as_str(), resend.to.clone(), resend.message_id.as_deref()), ("pat@x.org", vec!["support@co.com".to_string()], Some("<m2>")));
        let postmark = normalize(&json!({ "From": "pat@x.org", "FromName": "Pat", "ToFull": [{ "Email": "Support@co.com" }], "Subject": "Hi", "TextBody": "Body", "MessageID": "abc", "Headers": [{ "Name": "In-Reply-To", "Value": "<m1>" }, { "Name": "X-Spam-Status", "Value": "Yes, score=9" }] })).unwrap();
        assert_eq!((postmark.to.clone(), postmark.in_reply_to.as_deref(), postmark.spam), (vec!["support@co.com".to_string()], Some("<m1>"), true));
        assert!(normalize(&json!({ "subject": "no sender" })).is_err());
    }

    #[test]
    fn a_raw_message_inside_a_payload_is_parsed() {
        let m = normalize(&json!({ "raw": "From: a@x.org\nTo: s@co.com\nSubject: Raw\n\nText" })).unwrap();
        assert_eq!((m.subject.as_str(), m.text.trim()), ("Raw", "Text"));
    }
}
