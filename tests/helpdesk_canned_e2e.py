# End-to-end checks of helpdesk_canned. Needs the setup in README.md plus helpdesk_canned installed and a user
# `hd_mgr` with the role helpdesk.hd_manager.
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *
import hdlib
hdlib.DEFAULT_PLUGIN = "helpdesk_canned"

owner, mgr, a1, a2, cust = (Session(u) for u in ("hd_owner", "hd_mgr", "hd_agent1", "hd_agent2", "hd_cust"))
company = owner.call("get_default_company", plugin="company")["id"]
T = "helpdesk_ticket"

print("the starters")
shared = a1.call("list_canned_replies")
check("everyone starts with shared replies", {"hello", "details", "fixed"} <= {r.get("shortcut") for r in shared})
check("a shortcut finds a reply", [r.get("shortcut") for r in a1.call("list_canned_replies", {"shortcut": "/hello"})] == ["hello"])

print("who can use what")
mine = a1.call("create_canned_reply", {"title": f"Mine {RUN}", "shortcut": f"m{RUN.lower()}", "body": "Hello {{requester.first_name|friend}}, about {{ticket.reference}}: password resets are done from the login page.", "category": "Accounts"})
check("a personal reply is the agent's own", mine["scope"] == "personal" and mine["owner"] == a1.user, mine)
check("another agent does not see it", all(r["id"] != mine["id"] for r in a2.call("list_canned_replies")))
refused("nor use it", lambda: a2.call("reply_with_canned", {"ticket": "x", "canned": mine["id"]}), ["cannot use", "no such", "ticket"])
refused("an agent cannot share with everyone", lambda: a1.call("create_canned_reply", {"title": "x", "body": "x", "scope": "global"}), ["manager"])
team = owner.call("create_team", {"code": f"CN{RUN}", "name": f"Canned {RUN}", "company": company}, plugin="helpdesk")
refused("nor with a team they are not in", lambda: a1.call("create_canned_reply", {"title": "x", "body": "x", "scope": "team", "team": team["id"]}), ["belong"])
owner.call("add_team_member", {"team": team["id"], "user": a1.user}, plugin="helpdesk")
shared_team = a1.call("create_canned_reply", {"title": f"Team {RUN}", "body": "Team text", "scope": "team", "team": team["id"]})
check("a member shares with their team", shared_team["scope"] == "team")
check("a non-member does not see it", all(r["id"] != shared_team["id"] for r in a2.call("list_canned_replies")))
check("a manager sees every team's", any(r["id"] == shared_team["id"] for r in mgr.call("list_canned_replies")))
glob = mgr.call("create_canned_reply", {"title": f"All {RUN}", "shortcut": f"g{RUN.lower()}", "body": "For everyone", "scope": "global"})
check("a manager shares with everyone", any(r["id"] == glob["id"] for r in a2.call("list_canned_replies")))
refused("a shortcut is used once among shared replies", lambda: a2.call("create_canned_reply", {"title": "y", "shortcut": f"g{RUN.lower()}", "body": "y"}), ["taken"])
refused("only the owner edits a personal reply", lambda: a2.call("update_canned_reply", {"id": mine["id"], "title": "stolen"}), ["cannot change"])
refused("a reply needs a real scope", lambda: a1.call("create_canned_reply", {"title": "x", "body": "x", "scope": "world"}), ["scope"])
refused("customers use none of it", lambda: cust.call("list_canned_replies"), ["role"])

print("filling in a ticket")
t = a1.call("create_ticket", {"subject": f"Password reset not working {RUN}", "description": "I cannot reset my password from the login page", "requester_name": "Pat Doe", "requester_email": f"pat{RUN.lower()}@example.com", "channel": "email"}, plugin=T)
r = a1.call("render_canned_reply", {"canned": mine["id"], "ticket": t["id"]})
check("merge fields are filled", r["body"].startswith("Hello Pat, about HD-") and t["reference"] in r["body"] and r["missing"] == [], r)
bad = a1.call("create_canned_reply", {"title": f"Bad {RUN}", "body": "Hi {{requester.first_name}}, {{agent.name}} here, {{ticket.nope}}"})
check("a field without a value is reported", "ticket.nope" in a1.call("render_canned_reply", {"canned": bad["id"], "ticket": t["id"]})["missing"])
refused("and stops the send", lambda: a1.call("reply_with_canned", {"ticket": t["id"], "canned": bad["id"]}), ["no value"])

print("sending")
a1.call("set_my_signature", {"display_name": "Alex", "body": "Alex, Support"})
fixed = [x for x in a1.call("list_canned_replies") if x.get("shortcut") == "fixed"][0]
m = a1.call("reply_with_canned", {"ticket": t["id"], "canned": mine["id"], "extra": "Let me know if it helps."})
check("the reply goes out through the ticket", m["direction"] == "outbound" and "Hello Pat" in m["body"] and "Let me know" in m["body"] and m["body"].rstrip().endswith("Alex, Support"), m)
check("using it counts", a1.call("get_canned_reply", {"id": mine["id"]})["use_count"] == 1)
check("the first response was stamped", a1.call("get_ticket", {"id": t["id"]}, plugin=T)["first_response_at"])
a1.call("reply_with_canned", {"ticket": t["id"], "canned": fixed["id"], "signature": False})
check("a reply can leave the ticket resolved", a1.call("get_ticket", {"id": t["id"]}, plugin=T)["state"] == "resolved")
check("a signature can be left off", not a1.call("ticket_thread", {"id": t["id"]}, plugin=T)[-1]["body"].rstrip().endswith("Alex, Support"))
check("the signature is kept", a1.call("get_my_signature")["display_name"] == "Alex")

print("suggestions")
t2 = a1.call("create_ticket", {"subject": f"Cannot reset my password {RUN}", "description": "The password reset email never arrives", "requester_email": f"q{RUN.lower()}@example.com", "channel": "email"}, plugin=T)
sug = a1.call("suggest_canned_replies", {"id": t2["id"]})
check("the reply about passwords comes first", sug and sug[0]["id"] == mine["id"], [s["title"] for s in sug])
check("suggestions are scored", all("score" in s for s in sug))
off = a1.call("update_canned_reply", {"id": mine["id"], "active": False})
check("a switched-off reply is not offered", all(r["id"] != mine["id"] for r in a1.call("list_canned_replies")))
finish()
