//! Writing and running the knowledge base: categories, articles with revisions and review, suggestions for a ticket,
//! answering a ticket with an article, writing an article from a solved ticket, and where readers found nothing.

use aether_sdk::db::Filter;
use aether_sdk::prelude::*;

use crate::common::*;
use crate::markdown::plain;
use crate::ranking::{score, tokens, Fields};

#[derive(Deserialize)]
struct NewCategory {
    name: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sequence: Option<i64>,
}

#[derive(Deserialize)]
struct SetCategory {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sequence: Option<i64>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Deserialize)]
struct NewArticle {
    title: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    body: String,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    tags_text: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    featured: bool,
}

#[derive(Deserialize)]
struct SetArticle {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    tags_text: Option<String>,
    #[serde(default)]
    ticket_type: Option<String>,
    #[serde(default)]
    featured: Option<bool>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize, Default)]
struct Page {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    filter: Option<Value>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
}

/// The ticket may arrive as `id` (the page's own value) or `ticket` (renamed by a tab).
#[derive(Deserialize)]
struct TicketRef {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    ticket: Option<String>,
}

#[derive(Deserialize)]
struct SendArticle {
    #[serde(alias = "id")]
    ticket: String,
    article: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    set_state: Option<String>,
}

const VISIBILITY: &[&str] = &["public", "customers", "agents"];

fn check_visibility(v: &str) -> Result<()> {
    if VISIBILITY.contains(&v) {
        Ok(())
    } else {
        Err(Error::msg("an article is for the public, for customers or for agents"))
    }
}

fn article_of(id: &str) -> Result<Record> {
    db::find::<Record>("hd_kb_article").filter("id", id).first()?.ok_or_else(|| Error::msg("there is no such article"))
}

fn may_edit(article: &Record) -> Result<bool> {
    Ok(is_manager()? || (text(article, "author").is_some() && text(article, "author") == actor()?.as_deref()))
}

fn create_category(input: NewCategory) -> Result<Record> {
    require_manager()?;
    let name = clean("a name", &input.name)?;
    if let Some(p) = &input.parent {
        require("hd_kb_category", p, "parent category")?;
    }
    db::create(
        "hd_kb_category",
        &json!({
            "name": name, "slug": free_slug("hd_kb_category", &slugify(&name))?, "parent": opt(input.parent), "description": opt(input.description),
            "icon": opt(input.icon), "sequence": input.sequence.unwrap_or(10), "active": true,
        }),
    )
}

fn update_category(input: SetCategory) -> Result<Record> {
    require_manager()?;
    require("hd_kb_category", &input.id, "category")?;
    let mut data = json!({});
    if let Some(n) = opt(input.name) {
        data["name"] = json!(n);
    }
    if let Some(p) = input.parent {
        if p.trim() == input.id {
            return Err(Error::msg("a category cannot be its own parent"));
        }
        data["parent"] = if p.trim().is_empty() { Value::Null } else { json!(p.trim()) };
    }
    for (k, v) in [("description", input.description), ("icon", input.icon)] {
        if let Some(v) = v {
            data[k] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    }
    if let Some(s) = input.sequence {
        data["sequence"] = json!(s);
    }
    if let Some(a) = input.active {
        data["active"] = json!(a);
    }
    db::update("hd_kb_category", &input.id, &data)?.ok_or_else(|| Error::msg("the category is gone"))
}

fn new_article(input: NewArticle) -> Result<Record> {
    require_agent()?;
    let title = clean("a title", &input.title)?;
    let visibility = input.visibility.clone().unwrap_or_else(|| "public".into());
    check_visibility(&visibility)?;
    if let Some(c) = &input.category {
        require("hd_kb_category", c, "category")?;
    }
    db::create(
        "hd_kb_article",
        &json!({
            "title": title, "slug": free_slug("hd_kb_article", &slugify(&title))?, "category": opt(input.category), "summary": opt(input.summary),
            "body": clean("the text", &input.body)?, "state": "draft", "visibility": visibility, "author": actor()?, "version": 1,
            "featured": input.featured && is_manager()?, "tags_text": opt(input.tags_text), "ticket_type": opt(input.ticket_type),
            "views": 0, "helpful_yes": 0, "helpful_no": 0,
        }),
    )
}

fn save_revision(article: &Record, note: Option<String>) -> Result<()> {
    db::create::<Record>(
        "hd_kb_revision",
        &json!({
            "article": article["id"], "version": article.get("version").and_then(Value::as_i64).unwrap_or(1), "title": article["title"], "summary": article.get("summary"),
            "body": article["body"], "saved_by": actor()?, "saved_at": now()?, "note": opt(note),
        }),
    )?;
    Ok(())
}

fn change_article(input: SetArticle) -> Result<Record> {
    require_agent()?;
    let article = article_of(&input.id)?;
    if !may_edit(&article)? {
        return Err(Error::msg("only the author or a manager changes an article"));
    }
    let mut data = json!({});
    let mut content_changed = false;
    if let Some(t) = opt(input.title) {
        content_changed |= text(&article, "title") != Some(t.as_str());
        data["title"] = json!(t);
    }
    if let Some(b) = opt(input.body) {
        content_changed |= text(&article, "body") != Some(b.as_str());
        data["body"] = json!(b);
    }
    if let Some(s) = input.summary {
        let s = s.trim().to_string();
        content_changed |= text(&article, "summary").unwrap_or("") != s;
        data["summary"] = if s.is_empty() { Value::Null } else { json!(s) };
    }
    if let Some(v) = input.visibility {
        check_visibility(&v)?;
        data["visibility"] = json!(v);
    }
    for (k, v) in [("category", input.category), ("tags_text", input.tags_text), ("ticket_type", input.ticket_type)] {
        if let Some(v) = v {
            if k == "category" && !v.trim().is_empty() {
                require("hd_kb_category", v.trim(), "category")?;
            }
            data[k] = if v.trim().is_empty() { Value::Null } else { json!(v.trim()) };
        }
    }
    if let Some(f) = input.featured {
        if !is_manager()? {
            return Err(Error::msg("only a manager features an article"));
        }
        data["featured"] = json!(f);
    }
    if content_changed {
        save_revision(&article, input.note)?;
        data["version"] = json!(article.get("version").and_then(Value::as_i64).unwrap_or(1) + 1);
    }
    db::update("hd_kb_article", &input.id, &data)?.ok_or_else(|| Error::msg("the article is gone"))
}

fn move_article(id: &str, to: &str) -> Result<Record> {
    let article = article_of(id)?;
    let mut data = json!({ "state": to });
    if to == "published" {
        data["reviewer"] = json!(actor()?);
        if article.get("published_at").is_none_or(Value::is_null) {
            data["published_at"] = json!(now()?);
        }
    }
    db::update("hd_kb_article", id, &data)?.ok_or_else(|| Error::msg("the article is gone"))
}

fn send_for_review(id: &str) -> Result<Record> {
    require_agent()?;
    let article = article_of(id)?;
    if !may_edit(&article)? {
        return Err(Error::msg("only the author or a manager sends an article for review"));
    }
    if text(&article, "state") != Some("draft") {
        return Err(Error::msg("only a draft is sent for review"));
    }
    move_article(id, "review")
}

fn restore(revision: &str) -> Result<Record> {
    require_agent()?;
    let rev = require("hd_kb_revision", revision, "revision")?;
    let article = article_of(text(&rev, "article").unwrap_or_default())?;
    if !may_edit(&article)? {
        return Err(Error::msg("only the author or a manager changes an article"));
    }
    save_revision(&article, Some(format!("Before restoring version {}", rev.get("version").and_then(Value::as_i64).unwrap_or(0))))?;
    db::update(
        "hd_kb_article",
        id_of(&article)?,
        &json!({ "title": rev["title"], "summary": rev.get("summary"), "body": rev["body"], "version": article.get("version").and_then(Value::as_i64).unwrap_or(1) + 1 }),
    )?
    .ok_or_else(|| Error::msg("the article is gone"))
}

fn list_articles(input: Page) -> Result<Value> {
    require_agent()?;
    let mut filter = Filter::all();
    if let Some(Value::Object(map)) = input.filter.clone() {
        if !map.is_empty() {
            filter = filter.and(Filter::from_json(Value::Object(map)));
        }
    }
    if let Some(s) = input.state.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        filter = filter.and(Filter::eq("state", s));
    }
    db::search("hd_kb_article", input.q.as_deref(), &["title", "summary", "tags_text", "slug"], filter, Some("title"), input.limit.or(Some(50)), input.offset)
}

fn ranked_for(text_in: &str, ticket_type: Option<&str>, level: i64, limit: usize) -> Result<Vec<Record>> {
    let words = tokens(text_in);
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let all: Vec<Record> = db::find("hd_kb_article").filter("state", "published").order_by("-views").limit(1500).all()?;
    let mut scored: Vec<(i64, Record)> = all
        .into_iter()
        .filter(|a| readable(a, level))
        .filter_map(|a| {
            let f = Fields {
                title: text(&a, "title").unwrap_or(""),
                tags: text(&a, "tags_text").unwrap_or(""),
                summary: text(&a, "summary").unwrap_or(""),
                body: text(&a, "body").unwrap_or(""),
                helpful_yes: a.get("helpful_yes").and_then(Value::as_i64).unwrap_or(0),
                helpful_no: a.get("helpful_no").and_then(Value::as_i64).unwrap_or(0),
                featured: a.get("featured") == Some(&json!(true)),
            };
            let s = score(text_in, &words, &f);
            if s == 0 {
                return None;
            }
            let bonus = if ticket_type.is_some() && text(&a, "ticket_type") == ticket_type { 20 } else { 0 };
            Some((s + bonus, a))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(scored.into_iter().take(limit).map(|(s, mut a)| {
        if let Some(o) = a.as_object_mut() {
            o.insert("score".into(), json!(s));
            o.remove("body");
        }
        a
    }).collect())
}

pub fn suggest_for_text(text_in: &str, level: i64, limit: usize) -> Result<Vec<Record>> {
    ranked_for(text_in, None, level, limit)
}

fn suggest_for_ticket(ticket_id: &str) -> Result<Vec<Record>> {
    require_agent()?;
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": ticket_id }))?;
    let thread: Vec<Record> = plugins::call("helpdesk_ticket", "ticket_thread", &json!({ "id": ticket_id })).unwrap_or_default();
    let last = thread.iter().rev().find(|m| text(m, "direction") == Some("inbound")).and_then(|m| text(m, "body")).unwrap_or("");
    ranked_for(&format!("{} {} {}", text(&ticket, "subject").unwrap_or(""), text(&ticket, "subject").unwrap_or(""), last), text(&ticket, "ticket_type"), 3, 8)
}

fn send_article(input: SendArticle) -> Result<Record> {
    require_agent()?;
    let article = article_of(&input.article)?;
    if text(&article, "state") != Some("published") || text(&article, "visibility") == Some("agents") {
        return Err(Error::msg("only a published article meant for customers can be sent"));
    }
    let slug = text(&article, "slug").unwrap_or("");
    let summary = text(&article, "summary").map(str::to_string).unwrap_or_else(|| plain(text(&article, "body").unwrap_or("")).chars().take(300).collect());
    let mut body = String::new();
    if let Some(n) = opt(input.note) {
        body.push_str(&n);
        body.push_str("\n\n");
    }
    body.push_str(&format!("{}\n{}", text(&article, "title").unwrap_or(""), summary));
    match site_link(&format!("/help/{slug}")) {
        Some(link) => body.push_str(&format!("\n\nRead more: {link}")),
        None => body.push_str(&format!("\n\nRead more: /help/{slug}")),
    }
    let message: Record = plugins::call("helpdesk_ticket", "reply_ticket", &json!({ "id": input.ticket, "body": body, "set_state": opt(input.set_state) }))?;
    db::create::<Record>("hd_kb_use", &json!({ "article": article["id"], "ticket": input.ticket, "kind": "sent", "by": actor()?, "at": now()? }))?;
    Ok(message)
}

fn from_ticket(ticket_id: &str) -> Result<Record> {
    require_agent()?;
    let ticket: Record = plugins::call("helpdesk_ticket", "get_ticket", &json!({ "id": ticket_id }))?;
    let thread: Vec<Record> = plugins::call("helpdesk_ticket", "ticket_thread", &json!({ "id": ticket_id })).unwrap_or_default();
    let answer = thread.iter().rev().find(|m| text(m, "direction") == Some("outbound") && text(m, "kind") == Some("reply")).and_then(|m| text(m, "body")).unwrap_or("");
    let resolution = text(&ticket, "resolution").unwrap_or("");
    let body = format!("## The problem\n\n{}\n\n## The answer\n\n{}\n", text(&ticket, "description").unwrap_or(text(&ticket, "subject").unwrap_or("")), if resolution.is_empty() { answer } else { resolution });
    let title = text(&ticket, "subject").unwrap_or("Untitled").to_string();
    let article = db::create::<Record>(
        "hd_kb_article",
        &json!({
            "title": title, "slug": free_slug("hd_kb_article", &slugify(&title))?, "body": body, "state": "draft", "visibility": "public", "author": actor()?, "version": 1,
            "ticket_type": ticket.get("ticket_type"), "from_ticket": ticket["id"], "views": 0, "helpful_yes": 0, "helpful_no": 0,
        }),
    )?;
    db::create::<Record>("hd_kb_use", &json!({ "article": article["id"], "ticket": ticket["id"], "kind": "created_from", "by": actor()?, "at": now()? }))?;
    Ok(article)
}

fn overview() -> Result<Value> {
    require_agent()?;
    let states = db::aggregate("hd_kb_article", Filter::all(), &["state"], &[("articles", db::Figure::Count)])?;
    let published: Vec<Record> = db::find("hd_kb_article").filter("state", "published").limit(3000).all()?;
    let mut by_views = published.clone();
    by_views.sort_by(|a, b| b.get("views").and_then(Value::as_i64).cmp(&a.get("views").and_then(Value::as_i64)));
    let needs_work: Vec<Value> = published
        .iter()
        .filter(|a| a.get("helpful_no").and_then(Value::as_i64).unwrap_or(0) >= 3 && a.get("helpful_no").and_then(Value::as_i64) > a.get("helpful_yes").and_then(Value::as_i64))
        .map(|a| json!({ "id": a["id"], "title": a["title"], "helpful_yes": a["helpful_yes"], "helpful_no": a["helpful_no"] }))
        .collect();
    Ok(json!({
        "by_state": states,
        "published": published.len(),
        "waiting_review": db::count("hd_kb_article", Filter::eq("state", "review"))?,
        "top_viewed": by_views.iter().take(5).map(|a| json!({ "id": a["id"], "title": a["title"], "views": a["views"] })).collect::<Vec<_>>(),
        "needs_work": needs_work,
        "gaps": gaps(30)?,
    }))
}

/// What readers searched for and found nothing, most often first.
fn gaps(days: i64) -> Result<Vec<Value>> {
    let since = stamp_ago(days)?;
    let rows: Vec<Record> = db::find("hd_kb_search").filter("results", 0).matching(Filter::gte("at", since)).limit(5000).all()?;
    let mut counts: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for r in &rows {
        *counts.entry(text(r, "query").unwrap_or("").to_lowercase()).or_insert(0) += 1;
    }
    let mut out: Vec<(String, i64)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(out.into_iter().take(20).map(|(q, n)| json!({ "query": q, "times": n })).collect())
}

fn stamp_ago(days: i64) -> Result<String> {
    let t = aether_sdk::dates::parse_datetime(&now()?)? - aether_sdk::dates::Duration::days(days);
    Ok(aether_sdk::dates::format_datetime(t))
}

handler! {
    fn create_kb_category(input: NewCategory) -> Record {
        create_category(input)
    }

    fn update_kb_category(input: SetCategory) -> Record {
        update_category(input)
    }

    fn list_kb_categories(_: Empty) -> Vec<Record> {
        require_agent()?;
        db::find("hd_kb_category").order_by("sequence").limit(1000).all()
    }

    fn create_article(input: NewArticle) -> Record {
        new_article(input)
    }

    fn update_article(input: SetArticle) -> Record {
        change_article(input)
    }

    fn submit_article(input: Id) -> Record {
        send_for_review(&input.id)
    }

    fn publish_article(input: Id) -> Record {
        require_manager()?;
        move_article(&input.id, "published")
    }

    fn unpublish_article(input: Id) -> Record {
        require_manager()?;
        move_article(&input.id, "draft")
    }

    fn archive_article(input: Id) -> Record {
        require_manager()?;
        move_article(&input.id, "archived")
    }

    fn restore_article_revision(input: Id) -> Record {
        restore(&input.id)
    }

    fn list_article_revisions(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_kb_revision").filter("article", input.id.as_str()).order_by("-version").limit(200).all()
    }

    fn get_kb_article(input: Id) -> Record {
        require_agent()?;
        Ok(article_view(article_of(&input.id)?))
    }

    fn list_kb_articles(input: Page) -> Value {
        list_articles(input)
    }

    fn suggest_articles(input: TicketRef) -> Vec<Record> {
        let t = input.ticket.or(input.id).ok_or_else(|| Error::msg("name the ticket"))?;
        suggest_for_ticket(&t)
    }

    /// A row action in the ticket's suggestion list: `id` is the article, `ticket` the ticket.
    fn send_suggested_article(input: TicketRef) -> Record {
        let t = input.ticket.ok_or_else(|| Error::msg("name the ticket"))?;
        let a = input.id.ok_or_else(|| Error::msg("name the article"))?;
        send_article(SendArticle { ticket: t, article: a, note: None, set_state: None })
    }

    /// Published articles an agent may send a customer.
    fn published_articles(_: Empty) -> Vec<Record> {
        require_agent()?;
        let mut rows: Vec<Record> = db::find("hd_kb_article").filter("state", "published").order_by("title").limit(2000).all()?;
        rows.retain(|a| text(a, "visibility") != Some("agents"));
        Ok(rows)
    }

    fn reply_with_article(input: SendArticle) -> Record {
        send_article(input)
    }

    fn article_from_ticket(input: Id) -> Record {
        from_ticket(&input.id)
    }

    fn kb_overview(_: Empty) -> Value {
        overview()
    }

    fn kb_gaps(_: Empty) -> Vec<Value> {
        require_agent()?;
        gaps(30)
    }

    fn kb_needs_work(_: Empty) -> Vec<Value> {
        Ok(overview()?["needs_work"].as_array().cloned().unwrap_or_default())
    }
}
