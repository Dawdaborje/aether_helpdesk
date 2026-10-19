# End-to-end checks of helpdesk_portal. Needs helpdesk_ticket, helpdesk_kb and helpdesk_portal installed, the org's
# tenancy set to `header`, and a user `hd_mgr` with the role helpdesk.hd_manager.
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *
import hdlib
hdlib.DEFAULT_PLUGIN = "helpdesk_portal"

mgr, a1, cust, cust2 = (Session(u) for u in ("hd_mgr", "hd_agent1", "hd_cust", "hd_cust2"))
guest = Session(None)
T = "helpdesk_ticket"
KB = "helpdesk_kb"

print("the help centre")
word = f"quokka{RUN.lower()}"
art = a1.call("create_article", {"title": f"All about {word}", "summary": f"What a {word} is", "body": f"A {word} is small."}, plugin=KB)
mgr.call("publish_article", {"id": art["id"]}, plugin=KB)
hits = rows(guest.call("portal_articles", {"q": word}))
check("a visitor searches the help centre", hits and hits[0]["slug"] == art["slug"], hits)
check("a visitor sees the kinds of request", len(guest.call("portal_ticket_types")) >= 1)

print("a visitor asks for help")
who = f"portal{RUN.lower()}@example.com"
sent = guest.call("portal_submit", {"name": "Pat", "email": who, "subject": f"Cannot print {RUN}", "description": "The printer says offline"})
check("a reference and key come back", sent["reference"].startswith("HD-") and len(sent["key"]) >= 30, sent)
ref, key = sent["reference"], sent["key"]
refused("without the key a visitor sees nothing", lambda: guest.call("portal_view", {"reference": ref}), ["sign in"])
refused("a wrong key shows nothing", lambda: guest.call("portal_view", {"reference": ref, "key": "x" * 40}), ["no ticket"])
v = guest.call("portal_view", {"reference": ref, "key": key})
check("the right key shows the request", v["subject"].startswith("Cannot print") and v["can_reply"] and not v["can_rate"], v)
found = guest.call("portal_find", {"reference": ref, "key": key})
check("the find page answers the pair back", found == {"reference": ref, "key": key}, found)
tid = None
for _ in range(60):
    hit = [r for r in rows(a1.call("list_tickets", {"q": ref}, plugin=T)) if r["reference"] == ref]
    if hit and hit[0].get("priority"):
        tid = hit[0]["id"]
        break
    time.sleep(0.5)
check("the desk receives it", tid)
a1.call("add_ticket_note", {"id": tid, "body": "internal only"}, plugin=T)
a1.call("reply_ticket", {"id": tid, "body": "Is the printer switched on?"}, plugin=T)
msgs = guest.call("portal_messages", {"reference": ref, "key": key})
check("the visitor reads the answer, never the note", [m["from"] for m in msgs] == ["you", "support"] and all("internal" not in m["body"] for m in msgs), msgs)
v = guest.call("portal_reply", {"reference": ref, "key": key, "body": "Yes, it is on."})
check("the visitor writes back", len(v["messages"]) == 3)
a1.call("resolve_ticket", {"id": tid, "resolution": "Restarted the spooler"}, plugin=T)
v = guest.call("portal_rate", {"reference": ref, "key": key, "rating": 5, "comment": "quick"})
check("the visitor rates a solved request", v["rating"] == 5)
v = guest.call("portal_reopen", {"reference": ref, "key": key})
check("and can say it is not solved", v["state"] == "open", v)

print("a signed-in customer")
mine = cust.call("portal_submit", {"subject": f"My account {RUN}", "description": "I cannot sign in", "email": "cust@example.com"})
check("the form works with an account, no key needed", mine["key"] == "-" and mine["reference"].startswith("HD-"), mine)
lst = rows(cust.call("portal_my", {}))
check("it is listed under my requests", any(r["reference"] == mine["reference"] for r in lst))
check("the other customer does not see it", all(r["reference"] != mine["reference"] for r in rows(cust2.call("portal_my", {}))))
view = cust.call("portal_view", {"reference": mine["reference"], "key": "-"})
check("the owner opens it by reference alone", view["subject"].startswith("My account") and view["can_reply"], view)
refused("another customer cannot", lambda: cust2.call("portal_view", {"reference": mine["reference"], "key": "-"}), ["no request"])
refused("a visitor cannot list requests", lambda: guest.call("portal_my", {}), ["sign in"])
r = cust.call("portal_reply", {"reference": mine["reference"], "key": "-", "body": "More detail"})
check("the owner writes back", len(r["messages"]) == 2, r)
mt = [t for t in rows(a1.call("list_tickets", {"q": mine["reference"]}, plugin=T)) if t["reference"] == mine["reference"]][0]
a1.call("add_ticket_note", {"id": mt["id"], "body": "agents only"}, plugin=T)
a1.call("resolve_ticket", {"id": mt["id"], "resolution": "done"}, plugin=T)
r = cust.call("portal_rate", {"reference": mine["reference"], "key": "-", "rating": 4})
check("the owner rates it", r["rating"] == 4 and all("agents only" not in m["body"] for m in r["messages"]), r)
r = cust.call("portal_reopen", {"reference": mine["reference"], "key": "-"})
check("and reopens it", r["state"] == "open", r)

print("the page's info")
check("the heading info is open to all", "business_name" in guest.call("portal_info") and guest.call("portal_info")["signed_in"] is False and cust.call("portal_info")["signed_in"] is True)
finish()
