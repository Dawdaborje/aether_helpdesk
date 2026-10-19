//! What readers see. These functions are open to anonymous visitors, so each one checks who is asking and shows only
//! the articles meant for them: published ones for the public, more for signed-in customers, everything for agents.

use aether_sdk::prelude::*;

use crate::common::*;
use crate::kb::suggest_for_text;
use crate::ranking::{score, tokens, Fields};

#[derive(Deserialize, Default)]
struct Browse {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    filter: Option<Value>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

#[derive(Deserialize)]
struct Slug {
    slug: String,
}

#[derive(Deserialize)]
struct Vote {
    slug: String,
    helpful: bool,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Deserialize)]
struct Words {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    q: Option<String>,
}

fn card(a: &Record) -> Value {
    json!({
        "id": a["id"], "title": a["title"], "slug": a["slug"], "summary": a.get("summary"), "category": a.get("category"), "featured": a.get("featured"),
        "views": a.get("views"), "published_at": a.get("published_at"), "score": a.get("score"),
    })
}

fn article_by_slug(slug: &str, level: i64) -> Result<Record> {
    let a = db::find::<Record>("hd_kb_article").filter("slug", slug.trim()).first()?;
    match a {
        Some(a) if readable(&a, level) => Ok(a),
        _ => Err(Error::msg("there is no such article")),
    }
}

fn category_of(input: &Browse) -> Option<String> {
    input.category.clone().or_else(|| input.filter.as_ref().and_then(|f| f.get("category")).and_then(Value::as_str).map(str::to_string)).filter(|c| !c.is_empty())
}

fn browse(input: Browse) -> Result<Value> {
    let level = audience()?;
    let category = category_of(&input);
    let mut articles: Vec<Record> = db::find("hd_kb_article").filter("state", "published").order_by("-views").limit(1500).all()?;
    articles.retain(|a| readable(a, level));
    if let Some(c) = &category {
        let c = if c.contains(':') {
            c.clone()
        } else {
            db::find::<Record>("hd_kb_category").filter("slug", c.as_str()).first()?.and_then(|x| text(&x, "id").map(str::to_string)).unwrap_or_default()
        };
        articles.retain(|a| text(a, "category") == Some(c.as_str()));
    }
    let q = input.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let mut rows: Vec<Record> = if let Some(q) = q {
        let words = tokens(q);
        let mut scored: Vec<(i64, Record)> = articles
            .into_iter()
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
                let s = score(q, &words, &f);
                (s > 0).then_some((s, a))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let hits: Vec<Record> = scored.into_iter().map(|(s, mut a)| {
            if let Some(o) = a.as_object_mut() {
                o.insert("score".into(), json!(s));
            }
            a
        }).collect();
        if q.chars().count() >= 3 {
            let _ = db::create::<Record>("hd_kb_search", &json!({ "query": q.chars().take(200).collect::<String>().to_lowercase(), "results": hits.len() as i64, "audience": audience_name(level), "at": now()? }));
        }
        hits
    } else {
        articles.sort_by(|a, b| {
            let f = |x: &Record| (x.get("featured") == Some(&json!(true)), text(x, "published_at").unwrap_or("").to_string());
            f(b).cmp(&f(a))
        });
        articles
    };
    let total = rows.len();
    let offset = input.offset.unwrap_or(0).min(total);
    rows = rows.into_iter().skip(offset).take(input.limit.unwrap_or(25).min(100)).collect();
    Ok(json!({ "rows": rows.iter().map(card).collect::<Vec<_>>(), "total": total }))
}

fn categories() -> Result<Vec<Value>> {
    let level = audience()?;
    let cats: Vec<Record> = db::find("hd_kb_category").filter("active", true).order_by("sequence").limit(500).all()?;
    let mut articles: Vec<Record> = db::find("hd_kb_article").filter("state", "published").limit(3000).all()?;
    articles.retain(|a| readable(a, level));
    Ok(cats
        .into_iter()
        .map(|c| {
            let n = articles.iter().filter(|a| text(a, "category") == text(&c, "id")).count();
            json!({ "id": c["id"], "name": c["name"], "slug": c["slug"], "parent": c.get("parent"), "description": c.get("description"), "icon": c.get("icon"), "articles": n })
        })
        .filter(|c| c["articles"].as_u64().unwrap_or(0) > 0)
        .collect())
}

fn read(input: Slug) -> Result<Value> {
    let level = audience()?;
    let a = article_by_slug(&input.slug, level)?;
    if level < 3 {
        let _ = db::increment::<Record>("hd_kb_article", id_of(&a)?, "views", 1.0);
    }
    let category = match text(&a, "category") {
        Some(c) => db::find::<Record>("hd_kb_category").filter("id", c).first()?,
        None => None,
    };
    let related: Vec<Value> = {
        let words = format!("{} {}", text(&a, "title").unwrap_or(""), text(&a, "tags_text").unwrap_or(""));
        suggest_for_text(&words, level, 6)?.iter().filter(|r| r["id"] != a["id"]).take(5).map(card).collect()
    };
    let mut out = article_view(a);
    if let Some(o) = out.as_object_mut() {
        o.insert("category_name".into(), json!(category.as_ref().and_then(|c| text(c, "name"))));
        o.insert("related".into(), json!(related));
    }
    Ok(out)
}

fn vote(slug: &str, helpful: bool, comment: Option<String>) -> Result<Value> {
    let level = audience()?;
    let a = article_by_slug(slug, level)?;
    let id = id_of(&a)?.to_string();
    let who = actor()?.unwrap_or_else(|| "anonymous".into());
    let previous = if who == "anonymous" { None } else { db::find::<Record>("hd_kb_feedback").filter("article", id.as_str()).filter("who", who.as_str()).first()? };
    match previous {
        Some(p) if p.get("helpful") == Some(&json!(helpful)) => {}
        Some(p) => {
            db::update::<Record>("hd_kb_feedback", id_of(&p)?, &json!({ "helpful": helpful, "comment": opt(comment), "at": now()? }))?;
            db::increment::<Record>("hd_kb_article", &id, if helpful { "helpful_yes" } else { "helpful_no" }, 1.0)?;
            db::increment::<Record>("hd_kb_article", &id, if helpful { "helpful_no" } else { "helpful_yes" }, -1.0)?;
        }
        None => {
            db::create::<Record>("hd_kb_feedback", &json!({ "article": id, "helpful": helpful, "comment": opt(comment), "who": who, "at": now()? }))?;
            db::increment::<Record>("hd_kb_article", &id, if helpful { "helpful_yes" } else { "helpful_no" }, 1.0)?;
        }
    }
    let fresh = article_by_slug(slug, level)?;
    Ok(json!({ "helpful_yes": fresh["helpful_yes"], "helpful_no": fresh["helpful_no"] }))
}

handler! {
    fn kb_articles(input: Browse) -> Value {
        browse(input)
    }

    fn kb_categories(_: Empty) -> Vec<Value> {
        categories()
    }

    fn kb_article(input: Slug) -> Value {
        read(input)
    }

    fn kb_mark_helpful(input: Slug) -> Value {
        vote(&input.slug, true, None)
    }

    fn kb_mark_unhelpful(input: Slug) -> Value {
        vote(&input.slug, false, None)
    }

    fn kb_feedback(input: Vote) -> Value {
        vote(&input.slug, input.helpful, input.comment)
    }

    /// While someone writes a request: the articles that may already answer it.
    fn kb_suggest(input: Words) -> Vec<Value> {
        let text_in = input.text.or(input.q).unwrap_or_default();
        Ok(suggest_for_text(&text_in, audience()?, 5)?.iter().map(card).collect())
    }

    fn kb_related(input: Slug) -> Vec<Value> {
        let level = audience()?;
        let a = article_by_slug(&input.slug, level)?;
        let words = format!("{} {}", text(&a, "title").unwrap_or(""), text(&a, "tags_text").unwrap_or(""));
        Ok(suggest_for_text(&words, level, 6)?.iter().filter(|r| r["id"] != a["id"]).take(5).map(card).collect())
    }

    fn kb_popular(_: Empty) -> Vec<Value> {
        let level = audience()?;
        let mut rows: Vec<Record> = db::find("hd_kb_article").filter("state", "published").order_by("-views").limit(60).all()?;
        rows.retain(|a| readable(a, level));
        Ok(rows.iter().take(8).map(card).collect())
    }
}
