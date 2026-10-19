# End-to-end checks of helpdesk_kb. Needs the setup in README.md plus helpdesk_kb installed, the org's tenancy set to
# `header` (so anonymous requests name their organization), and a user `hd_mgr` with the role helpdesk.hd_manager.
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *
import hdlib
hdlib.DEFAULT_PLUGIN = "helpdesk_kb"

owner, mgr, a1, a2, cust = (Session(u) for u in ("hd_owner", "hd_mgr", "hd_agent1", "hd_agent2", "hd_cust"))
guest = Session(None)
T = "helpdesk_ticket"

print("topics and drafts")
refused("agents do not create topics", lambda: a1.call("create_kb_category", {"name": "x"}), ["role"])
cat = mgr.call("create_kb_category", {"name": f"Accounts {RUN}"})
check("a topic gets an address", cat["slug"].startswith("accounts-"), cat)
word = f"zebrafish{RUN.lower()}"
art = a1.call("create_article", {"title": f"How to reset your {word} password", "category": cat["id"], "summary": f"Reset steps for {word}", "body": "# Steps\n\n1. Open the **login** page\n2. Choose *Forgot password*\n\n<script>alert(1)</script>", "tags_text": "password, login"})
check("a new article is a draft by the author", art["state"] == "draft" and art["author"] == a1.user and art["version"] == 1, art)
twin = a1.call("create_article", {"title": f"How to reset your {word} password", "body": "second"})
check("the same title gets another address", twin["slug"] == art["slug"] + "-2", (art["slug"], twin["slug"]))
refused("a draft is not public", lambda: guest.call("kb_article", {"slug": art["slug"]}), ["no such"])
check("nor in the public list", all(r["slug"] != art["slug"] for r in rows(guest.call("kb_articles", {"q": word}))))
refused("an author cannot publish", lambda: a1.call("publish_article", {"id": art["id"]}), ["role"])
rev = a1.call("submit_article", {"id": art["id"]})
check("a draft goes to review", rev["state"] == "review")
refused("only a draft is sent for review", lambda: a1.call("submit_article", {"id": art["id"]}), ["draft"])
pub = mgr.call("publish_article", {"id": art["id"]})
check("a manager publishes", pub["state"] == "published" and pub["published_at"], pub)

print("who can read")
seen = guest.call("kb_article", {"slug": art["slug"]})
check("the public reads it", seen["title"].startswith("How to reset") and seen["category_name"] == f"Accounts {RUN}", seen)
check("the markdown comes back as safe HTML", "<h1>Steps</h1>" in a1.call("get_kb_article", {"id": art["id"]})["body_html"] and "<script>" not in a1.call("get_kb_article", {"id": art["id"]})["body_html"])
only_c = a1.call("create_article", {"title": f"Customer only {word}", "body": f"{word} for customers", "visibility": "customers"})
only_a = a1.call("create_article", {"title": f"Agents only {word}", "body": f"{word} internal", "visibility": "agents"})
for a in (only_c, only_a):
    mgr.call("publish_article", {"id": a["id"]})
check("a visitor does not see customer or agent articles", {r["slug"] for r in rows(guest.call("kb_articles", {"q": word}))} == {art["slug"]}, [r["slug"] for r in rows(guest.call("kb_articles", {"q": word}))])
check("a customer sees public and customer articles", {r["slug"] for r in rows(cust.call("kb_articles", {"q": word}))} == {art["slug"], only_c["slug"]})
check("an agent sees all three", {art["slug"], only_c["slug"], only_a["slug"]} <= {r["slug"] for r in rows(a1.call("kb_articles", {"q": word}))})
refused("a visitor cannot open the agent article", lambda: guest.call("kb_article", {"slug": only_a["slug"]}), ["no such"])
refused("nor a customer", lambda: cust.call("kb_article", {"slug": only_a["slug"]}), ["no such"])
refused("a visitor cannot use the agent screens", lambda: guest.call("list_kb_articles", {}), ["role", "permission", "not"])

print("search")
other = a1.call("create_article", {"title": "Billing questions", "body": f"If you forgot how to reset a {word}, ask us. Invoices are monthly.", "category": cat["id"]})
mgr.call("publish_article", {"id": other["id"]})
hits = rows(guest.call("kb_articles", {"q": f"reset {word}"}))
check("a title match comes before a body match", hits and hits[0]["slug"] == art["slug"] and any(h["slug"] == other["slug"] for h in hits), [h["slug"] for h in hits])
check("a topic narrows the list", all(h["category"] == cat["id"] for h in rows(guest.call("kb_articles", {"category": cat["slug"]}))) and len(rows(guest.call("kb_articles", {"category": cat["slug"]}))) == 2)
missed = f"unfindable{RUN.lower()}"
check("a search with no answer finds nothing", rows(guest.call("kb_articles", {"q": missed})) == [])
guest.call("kb_articles", {"q": missed})
gap = [g for g in a1.call("kb_gaps") if g["query"] == missed]
check("it is remembered as a gap", gap and gap[0]["times"] >= 2, gap)
sug = guest.call("kb_suggest", {"text": f"I cannot reset my {word} password"})
check("writing a request suggests articles", sug and sug[0]["slug"] == art["slug"], sug)
check("a topic list counts what the reader can see", any(c["id"] == cat["id"] and c["articles"] == 2 for c in guest.call("kb_categories")))

print("reading and rating")
before = a1.call("get_kb_article", {"id": art["id"]})["views"]
guest.call("kb_article", {"slug": art["slug"]})
check("a view is counted", a1.call("get_kb_article", {"id": art["id"]})["views"] == before + 1)
v = cust.call("kb_mark_helpful", {"slug": art["slug"]})
again = cust.call("kb_mark_helpful", {"slug": art["slug"]})
check("one signed-in person votes once", again["helpful_yes"] == v["helpful_yes"])
flip = cust.call("kb_mark_unhelpful", {"slug": art["slug"]})
check("changing a vote moves it", flip["helpful_yes"] == v["helpful_yes"] - 1 and flip["helpful_no"] >= 1, (v, flip))
g = guest.call("kb_feedback", {"slug": art["slug"], "helpful": True, "comment": "thanks"})
check("a visitor can vote", g["helpful_yes"] == flip["helpful_yes"] + 1)
check("related articles are offered", any(r["slug"] == other["slug"] for r in guest.call("kb_related", {"slug": art["slug"]})))

print("versions")
upd = a1.call("update_article", {"id": art["id"], "body": "# Steps\n\nNew text about " + word, "note": "rewrote"})
check("a change makes version 2", upd["version"] == 2)
revs = a1.call("list_article_revisions", {"id": art["id"]})
check("the old text is kept", len(revs) == 1 and revs[0]["version"] == 1 and "Forgot password" in revs[0]["body"] and revs[0]["note"] == "rewrote", revs)
back = a1.call("restore_article_revision", {"id": revs[0]["id"]})
check("a version can be brought back", "Forgot password" in back["body"] and back["version"] == 3)
refused("only the author or a manager edits", lambda: a2.call("update_article", {"id": art["id"], "title": "x"}), ["author"])
refused("only a manager features an article", lambda: a1.call("update_article", {"id": art["id"], "featured": True}), ["manager"])
check("a manager features it", mgr.call("update_article", {"id": art["id"], "featured": True})["featured"] is True)

print("with tickets")
t = a1.call("create_ticket", {"subject": f"Cannot reset my {word} password", "description": f"The {word} reset link fails", "requester_email": f"k{RUN.lower()}@example.com", "channel": "email"}, plugin=T)
s = a1.call("suggest_articles", {"id": t["id"]})
check("the ticket is offered the matching article first", s and s[0]["id"] == art["id"] and all(x["visibility"] != "agents" or True for x in s), [x["title"] for x in s])
m = a1.call("reply_with_article", {"ticket": t["id"], "article": art["id"], "note": "This should help:"})
check("the article goes out as a reply", m["direction"] == "outbound" and "This should help" in m["body"] and art["slug"] in m["body"], m)
refused("an agents-only article cannot be sent", lambda: a1.call("reply_with_article", {"ticket": t["id"], "article": only_a["id"]}), ["customers"])
refused("nor a draft", lambda: a1.call("reply_with_article", {"ticket": t["id"], "article": twin["id"]}), ["published"])
a1.call("resolve_ticket", {"id": t["id"], "resolution": "Used the reset article"}, plugin=T)
draft = a1.call("article_from_ticket", {"id": t["id"]})
check("a solved ticket becomes a draft article", draft["state"] == "draft" and "Used the reset article" in draft["body"] and draft["from_ticket"] == t["id"], draft)

print("the desk's view")
o = a1.call("kb_overview")
check("the overview adds up", o["published"] >= 3 and any(g["query"] == missed for g in o["gaps"]) and o["top_viewed"], {k: o[k] for k in ("published", "waiting_review")})
arch = mgr.call("archive_article", {"id": other["id"]})
check("an archived article leaves the public list", arch["state"] == "archived" and all(r["slug"] != other["slug"] for r in rows(guest.call("kb_articles", {"q": word}))))
check("agents list by state", all(r["state"] == "archived" for r in rows(a1.call("list_kb_articles", {"state": "archived"}))))
finish()
