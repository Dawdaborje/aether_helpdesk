# End-to-end checks of helpdesk_ticket against a running kernel. See README.md next to this file for setup.
# Usage: python3 helpdesk_ticket_e2e.py [--base http://localhost:7890] [--org hd_test] [-v]
# Every run uses fresh codes, so it can run again on the same organization.
import argparse, json, re, secrets, sys, time, urllib.error, urllib.request

ap = argparse.ArgumentParser()
ap.add_argument("--base", default="http://localhost:7890")
ap.add_argument("--org", default="hd_test")
ap.add_argument("--password", default="Test-pass-123")
ap.add_argument("-v", action="store_true")
args = ap.parse_args()
RUN = secrets.token_hex(3).upper()


class Failed(Exception):
    def __init__(self, status, data):
        super().__init__(f"{status}: {json.dumps(data)[:400]}")
        self.status, self.data = status, data


class Session:
    def __init__(self, username):
        self.cookies = {"aether_csrf": secrets.token_hex(16)}
        self.name = username
        if username is None:  # an anonymous visitor
            self.user = None
            return
        who = self.post("/api/auth/login", {"username": username, "password": args.password})
        key = re.search(r'String\("([^"]+)"\)', who["user"]["id"])
        self.user = f"users:{key.group(1)}" if key else who["user"]["id"]
        self.post("/api/auth/org", {"org": args.org})

    def post(self, path, body):
        r = urllib.request.Request(args.base + path, data=json.dumps(body).encode(), method="POST")
        r.add_header("content-type", "application/json")
        r.add_header("origin", args.base)
        r.add_header("x-org-slug", args.org)
        r.add_header("x-csrf-token", self.cookies["aether_csrf"])
        r.add_header("cookie", "; ".join(f"{k}={v}" for k, v in self.cookies.items()))
        try:
            resp = urllib.request.urlopen(r)
            status, raw, headers = resp.status, resp.read(), resp.headers
        except urllib.error.HTTPError as e:
            status, raw, headers = e.code, e.read(), e.headers
        for c in headers.get_all("set-cookie") or []:
            k, _, rest = c.partition("=")
            self.cookies[k.strip()] = rest.split(";")[0]
        try:
            data = json.loads(raw) if raw else None
        except ValueError:
            data = {"_raw": raw.decode(errors="replace")}
        if status >= 400:
            raise Failed(status, data)
        return data

    def call(self, fn, body=None, plugin="helpdesk_ticket"):
        out = self.post(f"/api/plugins/{plugin}/{fn}", body or {})
        if args.v:
            print(f"    {self.name} {plugin}.{fn} -> {json.dumps(out)[:300]}")
        return out


results = []


def check(name, cond, detail=""):
    results.append((name, bool(cond)))
    print(("  ok    " if cond else "  FAIL  ") + name + ("" if cond else f"  [{str(detail)[:300]}]"))


def refused(name, action, words=()):
    try:
        action()
    except Failed as f:
        msg = json.dumps(f.data)
        check(name, not words or any(w in msg for w in words), msg[:300])
        return
    check(name, False, "was allowed")


def rows(page):
    return page["rows"] if isinstance(page, dict) else page


owner, a1, a2, cust, cust2 = (Session(u) for u in ("hd_owner", "hd_agent1", "hd_agent2", "hd_cust", "hd_cust2"))
guest = Session(None)
company = owner.call("get_default_company", plugin="company")["id"]

print("setup")
team = owner.call("create_team", {"code": f"RR{RUN}", "name": f"Round robin {RUN}", "company": company, "assignment": "round_robin"}, plugin="helpdesk")
for a in (a1, a2):
    owner.call("add_team_member", {"team": team["id"], "user": a.user, "capacity": 0}, plugin="helpdesk")
lb = owner.call("create_team", {"code": f"LB{RUN}", "name": f"Balanced {RUN}", "company": company, "assignment": "load_balanced"}, plugin="helpdesk")
for a in (a1, a2):
    owner.call("add_team_member", {"team": lb["id"], "user": a.user, "capacity": 0}, plugin="helpdesk")
full = owner.call("create_team", {"code": f"FU{RUN}", "name": f"Full {RUN}", "company": company, "assignment": "load_balanced"}, plugin="helpdesk")
owner.call("add_team_member", {"team": full["id"], "user": a1.user, "capacity": 1}, plugin="helpdesk")
tag = owner.call("create_tag", {"code": f"vip{RUN.lower()}", "name": f"VIP {RUN}"}, plugin="helpdesk")
types = {t["code"]: t for t in owner.call("search_ticket_types", {}, plugin="helpdesk")}

print("creating and assigning")
t1 = a1.call("create_ticket", {"subject": f"Printer {RUN}", "description": "It jams", "requester_name": "Pat", "requester_email": f"pat{RUN.lower()}@example.com", "team": team["id"], "tags": [tag["code"]], "channel": "phone"})
t2 = a1.call("create_ticket", {"subject": f"Scanner {RUN}", "description": "No power", "requester_email": f"sam{RUN.lower()}@example.com", "team": team["id"]})
check("a new ticket is numbered and new/open", t1["reference"].startswith("HD-") and t1["state"] in ("new", "open"), t1)
check("round robin gives two tickets to two agents", t1["assignee"] and t2["assignee"] and t1["assignee"] != t2["assignee"], (t1["assignee"], t2["assignee"]))
check("the first message is the description", len(a1.call("ticket_thread", {"id": t1["id"]})) >= 1)
check("tags are kept for search", a1.call("get_ticket", {"id": t1["id"]})["tags_text"] == f"VIP {RUN}", a1.call("get_ticket", {"id": t1["id"]}).get("tags_text"))
check("search finds by tag text", any(r["id"] == t1["id"] for r in rows(a1.call("list_tickets", {"q": f"VIP {RUN}"}))))
check("a priority was chosen by default", t1.get("priority"))
load = {c["user"]: c["open"] for c in a1.call("ticket_assignees", {"team": lb["id"]})}
b1 = a1.call("create_ticket", {"subject": f"B1 {RUN}", "team": lb["id"]})
check("load balancing picks the least loaded agent", b1.get("assignee") == min(load, key=load.get), (load, b1.get("assignee")))
b2 = a1.call("create_ticket", {"subject": f"B2 {RUN}", "team": lb["id"]})
b3 = a1.call("create_ticket", {"subject": f"B3 {RUN}", "team": full["id"]})
check("a member at capacity is skipped", not b3.get("assignee"), b3.get("assignee"))
refused("customers cannot create tickets for others", lambda: cust.call("create_ticket", {"subject": "x"}), ["role"])
refused("customers cannot list all tickets", lambda: cust.call("list_tickets", {}), ["role"])

print("a customer asks for help")
mine = cust.call("submit_ticket", {"subject": f"Cannot log in {RUN}", "description": "Password reset fails", "requester_email": "cust@example.com", "priority": "ignored"})
check("it is theirs", mine["requester_user"] == cust.user and mine["channel"] == "portal", mine)
check("they see it in their requests", any(r["id"] == mine["id"] for r in rows(cust.call("my_requests", {}))))
check("the other customer does not", all(r["id"] != mine["id"] for r in rows(cust2.call("my_requests", {}))))
refused("another customer cannot open it", lambda: cust2.call("get_ticket", {"id": mine["id"]}))
a1.call("add_ticket_note", {"id": mine["id"], "body": "Looks like a lockout; check the log"})
seen = cust.call("ticket_thread", {"id": mine["id"]})
check("the customer sees the question but not the note", len(seen) == 1 and seen[0]["kind"] == "reply", seen)
refused("customers cannot move tickets", lambda: cust.call("change_ticket_state", {"id": mine["id"], "state": "resolved"}), ["role"])
refused("customers cannot add notes", lambda: cust.call("add_ticket_note", {"id": mine["id"], "body": "x"}), ["role"])

print("the conversation")
r = a1.call("reply_ticket", {"id": mine["id"], "body": "Please try the reset link again.", "set_state": "pending"})
check("a reply is stored as outbound", r["direction"] == "outbound" and r["delivery"] in ("queued", "failed", "none"), r)
after = a1.call("get_ticket", {"id": mine["id"]})
check("replying set the first response and waits for the customer", after["first_response_at"] and after["state"] == "pending", after)
c = cust.call("customer_reply_ticket", {"id": mine["id"], "body": "Still failing"})
check("the customer reply reopens a pending ticket", a1.call("get_ticket", {"id": mine["id"]})["state"] == "open")
refused("another customer cannot reply", lambda: cust2.call("customer_reply_ticket", {"id": mine["id"], "body": "hi"}), ["not your"])
check("waiting for us lists it", any(x["id"] == mine["id"] for x in a1.call("tickets_waiting_for_agent")))

print("email in")
mid = f"<m1-{RUN}@mail.example>"
e1 = a1.call("ingest_email", {"from_email": f"Lee{RUN}@Example.com", "from_name": "Lee", "subject": f"Invoice question {RUN}", "text": "Where is my invoice?", "message_id": mid})
check("email opens a ticket", e1["created"] is True)
dup = a1.call("ingest_email", {"from_email": f"lee{RUN}@example.com", "subject": f"Invoice question {RUN}", "text": "Where is my invoice?", "message_id": mid})
check("the same message again is ignored", dup.get("duplicate") is True and dup["ticket"] == e1["ticket"], dup)
ref = a1.call("get_ticket", {"id": e1["ticket"]})["reference"]
e2 = a1.call("ingest_email", {"from_email": f"lee{RUN}@example.com", "subject": f"Re: [{ref}] Invoice question", "text": "Any news?", "message_id": f"<m2-{RUN}@mail.example>"})
check("a reply with the reference joins the ticket", e2["created"] is False and e2["ticket"] == e1["ticket"], e2)
e3 = a1.call("ingest_email", {"from_email": f"lee{RUN}@example.com", "subject": "Re: something", "text": "Thanks", "message_id": f"<m3-{RUN}@mail.example>", "in_reply_to": mid})
check("a reply to the first message joins by message id", e3["created"] is False and e3["ticket"] == e1["ticket"], e3)
check("auto replies are ignored", "ignored" in a1.call("ingest_email", {"from_email": "x@example.com", "subject": "Out of office: hi", "text": "away", "message_id": f"<auto-{RUN}>"}))
check("the thread has three customer messages", len([m for m in a1.call("ticket_thread", {"id": e1["ticket"]}) if m["direction"] == "inbound"]) == 3)

print("moves")
refused("the workflow refuses a jump", lambda: a1.call("change_ticket_state", {"id": b1["id"], "state": "closed"}))
res = a1.call("resolve_ticket", {"id": mine["id"], "resolution": "Reset the lockout"})
check("resolving stamps time and resolution", res["state"] == "resolved" and res["resolved_at"] and res["resolution"] == "Reset the lockout", res)
r1 = cust.call("rate_ticket", {"id": mine["id"], "rating": 5, "comment": "Fast"})
check("the requester rates it", r1["rating"] == 5)
refused("a rating is 1 to 5", lambda: cust.call("rate_ticket", {"id": mine["id"], "rating": 9}), ["1 to 5"])
refused("the other customer cannot rate", lambda: cust2.call("rate_ticket", {"id": mine["id"], "rating": 1}))
cl = a1.call("close_ticket", {"id": mine["id"]})
check("closing works from resolved", cl["state"] == "closed" and cl["closed_at"])
refused("a closed ticket takes no replies", lambda: a1.call("reply_ticket", {"id": mine["id"], "body": "late"}), ["closed"])
ro = a1.call("reopen_ticket", {"id": mine["id"]})
check("reopening counts", ro["state"] == "open" and ro["reopen_count"] == 1, ro)
a1.call("close_ticket", {"id": mine["id"]})
fu = a1.call("ingest_email", {"from_email": "cust@example.com", "subject": f"Re: [{ro['reference']}] Cannot log in", "text": "It happened again", "message_id": f"<fu-{RUN}>"})
check("email to a closed ticket opens a follow-up", fu["created"] is True and fu.get("follow_up_of") == mine["id"], fu)

print("merge and split")
m1 = a1.call("create_ticket", {"subject": f"Dup A {RUN}", "description": "first", "requester_email": f"dup{RUN.lower()}@example.com"})
m2 = a1.call("create_ticket", {"subject": f"Dup B {RUN}", "description": "second", "requester_email": f"dup{RUN.lower()}@example.com", "tags": [tag["code"]]})
a1.call("log_ticket_time", {"ticket": m2["id"], "minutes": 30, "note": "call", "billable": True})
merged = a1.call("merge_tickets", {"source": m2["id"], "into": m1["id"]})
check("messages and tags moved to the kept ticket", merged["message_count"] == 2 and "VIP" in (a1.call("get_ticket", {"id": m1["id"]}).get("tags_text") or ""), merged)
src = a1.call("get_ticket", {"id": m2["id"]})
check("the other is merged and points at it", src["state"] == "merged" and src["merged_into"] == m1["id"], src)
check("time moved with it", a1.call("get_ticket", {"id": m1["id"]})["time_minutes"] == 30)
refused("a merged ticket takes no replies", lambda: a1.call("reply_ticket", {"id": m2["id"], "body": "x"}), ["merged"])
refused("a ticket cannot merge into itself", lambda: a1.call("merge_tickets", {"source": m1["id"], "into": m1["id"]}), ["itself"])
other = a1.call("create_ticket", {"subject": f"Other customer {RUN}", "requester_email": f"o{RUN.lower()}@example.com"})
c1 = a1.call("create_ticket", {"subject": f"Cust one {RUN}", "customer": None, "requester_email": f"c1{RUN.lower()}@example.com"})
thread = a1.call("ticket_thread", {"id": m1["id"]})
pick = [m for m in thread if m["kind"] == "reply"][-1]
new = a1.call("split_ticket", {"id": m1["id"], "message": pick["id"], "subject": f"Split {RUN}"})
check("a message becomes its own ticket", new["split_from"] == m1["id"] and new["message_count"] == 1, new)
check("the old one has one message fewer", a1.call("get_ticket", {"id": m1["id"]})["message_count"] == 1)

print("sub-tickets, time and bulk")
sub = a1.call("create_subticket", {"parent": m1["id"], "subject": f"Sub {RUN}"})
check("a sub-ticket points at its parent", sub["parent"] == m1["id"] and a1.call("get_ticket", {"id": m1["id"]})["sub_tickets"] == 1)
t = a1.call("log_ticket_time", {"ticket": m1["id"], "minutes": 15})
check("time is summed", a1.call("get_ticket", {"id": m1["id"]})["time_minutes"] == 45)
refused("time is positive", lambda: a1.call("log_ticket_time", {"ticket": m1["id"], "minutes": 0}), ["minute"])
refused("others' time is theirs to remove", lambda: a2.call("delete_time_entry", {"id": t["id"]}), ["only the person"])
done = a1.call("bulk_update_tickets", {"ids": [b1["id"], b2["id"], "nope"], "action": "resolve", "value": "bulk"})
check("bulk resolves what it can and names the failure", done["done"] == 2 and len(done["failed"]) == 1, done)
a1.call("assign_ticket", {"id": b3["id"], "assignee": a2.user})
check("assign by hand and notify", a1.call("get_ticket", {"id": b3["id"]})["assignee"] == a2.user)
check("my tickets lists them for the agent", any(r["id"] == b3["id"] for r in rows(a2.call("my_tickets", {}))))
a1.call("unassign_ticket", {"id": b3["id"]})
check("unassigned tickets lists it", any(r["id"] == b3["id"] for r in rows(a1.call("unassigned_tickets", {}))))
a1.call("add_ticket_tag", {"id": b3["id"], "tag": tag["code"]})
a1.call("remove_ticket_tag", {"id": b3["id"], "tag": tag["code"]})
check("tags can be removed", not a1.call("get_ticket", {"id": b3["id"]}).get("tags_text"))
check("a spam ticket is closed and flagged", a1.call("mark_ticket_spam", {"id": other["id"]})["state"] == "closed")

print("people without an account")
who = f"visitor{RUN.lower()}@example.com"
bot = guest.call("visitor_submit", {"email": who, "subject": "buy now", "website": "spam.example"})
check("a bot is turned away quietly", bot["key"] == "")
refused("a bad address is refused", lambda: guest.call("visitor_submit", {"email": "nope", "subject": "x"}), ["email"])
v = guest.call("visitor_submit", {"name": "Vic", "email": who, "subject": f"Where is my order {RUN}", "description": "Ordered last week"})
check("a visitor gets a reference and a long key", v["reference"].startswith("HD-") and len(v["key"]) >= 30, v)
vt = None
for _ in range(60):
    found = [r for r in rows(a1.call("list_tickets", {"q": v["reference"]})) if r["reference"] == v["reference"]]
    if found and found[0].get("priority"):
        vt = found[0]
        break
    time.sleep(0.5)
check("the background job gave it a company and priority", vt and vt.get("company") and vt.get("priority"), vt)
check("agents see the visitor's ticket", vt and vt["channel"] == "portal" and vt["requester_email"] == who)
refused("a wrong key shows nothing", lambda: guest.call("visitor_view", {"reference": v["reference"], "key": "x" * 40}), ["no ticket"])
seen = guest.call("visitor_view", {"reference": v["reference"], "key": v["key"]})
check("the key shows the conversation", seen["subject"].startswith("Where is") and len(seen["messages"]) == 1 and seen["messages"][0]["from"] == "you", seen)
a1.call("add_ticket_note", {"id": vt["id"], "body": "internal only"})
a1.call("reply_ticket", {"id": vt["id"], "body": "It ships today."})
seen = guest.call("visitor_view", {"reference": v["reference"], "key": v["key"]})
check("notes never reach the visitor", all("internal only" not in m["body"] for m in seen["messages"]) and len(seen["messages"]) == 2, seen)
seen = guest.call("visitor_reply", {"reference": v["reference"], "key": v["key"], "body": "Thanks!"})
check("a visitor can write back", len(seen["messages"]) == 3)
refused("a visitor cannot rate an open ticket", lambda: guest.call("visitor_rate", {"reference": v["reference"], "key": v["key"], "rating": 5}), ["resolved"])
a1.call("resolve_ticket", {"id": vt["id"], "resolution": "Shipped"})
seen = guest.call("visitor_rate", {"reference": v["reference"], "key": v["key"], "rating": 4, "comment": "ok"})
check("a visitor rates once solved", seen["rating"] == 4 and seen["can_rate"] is False, seen)
seen = guest.call("visitor_reopen", {"reference": v["reference"], "key": v["key"]})
check("a visitor can reopen a solved request", seen["state"] == "open", seen)
refused("a visitor cannot call agent functions", lambda: guest.call("list_tickets", {}))
many = 0
for i in range(6):
    try:
        guest.call("visitor_submit", {"email": f"flood{RUN.lower()}@example.com", "subject": f"flood {i}"})
        many += 1
    except Failed:
        pass
check("one address cannot flood the desk", many == 5, many)

print("machines and mailboxes")
robot = a1.call("ingest_email", {"from_email": "list@example.com", "subject": "Newsletter", "text": "buy", "message_id": f"<bulk-{RUN}>", "precedence": "bulk"})
check("bulk mail is ignored", "ignored" in robot)
robot = a1.call("ingest_email", {"from_email": "bot@example.com", "subject": "Hello", "text": "x", "message_id": f"<as-{RUN}>", "auto_submitted": "auto-replied"})
check("auto-submitted mail is ignored", "ignored" in robot)
routed = a1.call("ingest_email", {"from_email": f"route{RUN.lower()}@example.com", "subject": "Routed", "text": "hi", "message_id": f"<rt-{RUN}>", "team": lb["id"], "ticket_type": types["incident"]["id"], "reply_to": "support@example.com"})
rt = a1.call("get_ticket", {"id": routed["ticket"]})
check("a mailbox routes team, type and reply address", rt["team"] == lb["id"] and rt["ticket_type"] == types["incident"]["id"] and rt["reply_to"] == "support@example.com", rt)

print("agents limited to their teams")
other_team = owner.call("create_team", {"code": f"OT{RUN}", "name": f"Other {RUN}", "company": company}, plugin="helpdesk")
owner.call("add_team_member", {"team": other_team["id"], "user": a2.user}, plugin="helpdesk")
walled = a2.call("create_ticket", {"subject": f"Walled {RUN}", "team": other_team["id"], "assignee": a2.user})
owner.call("set_settings", {"company": company, "restrict_by_team": True}, plugin="helpdesk")
try:
    refused("an agent outside the team cannot open its ticket", lambda: a1.call("get_ticket", {"id": walled["id"]}), ["another team"])
    check("the list leaves it out", all(r["id"] != walled["id"] for r in rows(a1.call("list_tickets", {"q": f"Walled {RUN}"}))))
    check("a member still sees it", any(r["id"] == walled["id"] for r in rows(a2.call("list_tickets", {"q": f"Walled {RUN}"}))))
    refused("nor can they act on it", lambda: a1.call("add_ticket_note", {"id": walled["id"], "body": "x"}), ["another team"])
    check("tickets with no team stay visible", a1.call("get_ticket", {"id": e1["ticket"]})["id"] == e1["ticket"])
    check("administrators see every team", owner.call("get_ticket", {"id": walled["id"]})["id"] == walled["id"])
finally:
    owner.call("set_settings", {"company": company, "restrict_by_team": False}, plugin="helpdesk")
check("with the limit off, everyone sees it", a1.call("get_ticket", {"id": walled["id"]})["id"] == walled["id"])

print("overview")
o = a1.call("ticket_overview")
check("the overview adds up", o["open"] >= 1 and o["created_7d"] >= 5 and isinstance(o["by_state"], list) and len(o["trend"]) == 14, {k: o[k] for k in ("open", "created_7d")})
check("average rating is known", o["avg_rating"] is not None)
refused("customers do not see the overview", lambda: cust.call("ticket_overview"), ["role"])
check("the nightly job runs", "closed" in owner.call("tickets_nightly"))

bad = [n for n, ok in results if not ok]
print(f"\n{len(results) - len(bad)}/{len(results)} checks passed")
sys.exit(1 if bad else 0)
