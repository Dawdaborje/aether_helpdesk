# End-to-end checks of helpdesk_csat. Needs helpdesk_ticket and helpdesk_csat installed, tenancy set to `header`, and a
# user `hd_mgr` with the role helpdesk.hd_manager (managers see survey links; agents do not).
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *
import hdlib
hdlib.DEFAULT_PLUGIN = "helpdesk_csat"

owner, mgr, a1, cust = (Session(u) for u in ("hd_owner", "hd_mgr", "hd_agent1", "hd_cust"))
guest = Session(None)
T = "helpdesk_ticket"
company = owner.call("get_default_company", plugin="company")["id"]


def survey_of(ticket, tries=60):
    for _ in range(tries):
        rs = mgr.call("ticket_survey", {"ticket": ticket})
        if rs:
            return rs[0]
        time.sleep(0.5)
    return None


def make(subject, email, team=None):
    body = {"subject": subject, "description": "x", "requester_email": email, "channel": "email"}
    if team:
        body["team"] = team
    return a1.call("create_ticket", body, plugin=T)


print("switched off")
owner.call("set_settings", {"company": company, "csat_enabled": False}, plugin="helpdesk")
off = make(f"No survey {RUN}", f"off{RUN.lower()}@client.example")
a1.call("resolve_ticket", {"id": off["id"], "resolution": "done"}, plugin=T)
time.sleep(4)
check("no survey goes out while surveys are off", mgr.call("ticket_survey", {"ticket": off["id"]}) == [])
owner.call("set_settings", {"company": company, "csat_enabled": True}, plugin="helpdesk")

print("setup")
check("a starter survey exists", any(s["code"] == "standard" for s in a1.call("list_surveys")))
refused("agents do not edit surveys", lambda: a1.call("create_survey", {"code": "x", "name": "x"}), ["role"])
refused("a survey goes out on resolve or close", lambda: mgr.call("create_survey", {"code": f"bad{RUN}", "name": "bad", "send_on": "never"}), ["resolved or closed"])
team = owner.call("create_team", {"code": f"CS{RUN}", "name": f"CSAT {RUN}", "company": company}, plugin="helpdesk")
mine = mgr.call("create_survey", {"code": f"T{RUN}", "name": f"Team survey {RUN}", "team": team["id"], "question": f"Rate team {RUN}", "alert_at": 2, "alert_user": "boss", "reminder_days": 3})
check("a survey for one team", mine["team"] == team["id"] and mine["is_default"] is False, mine)

print("asking")
t1 = make(f"Survey one {RUN}", f"one{RUN.lower()}@client.example", team["id"])
a1.call("resolve_ticket", {"id": t1["id"], "resolution": "fixed"}, plugin=T)
r1 = survey_of(t1["id"])
check("resolving sends a survey", r1 and r1["state"] == "sent" and len(r1["token"]) >= 30 and r1["survey"] == mine["id"], r1)
check("the team's own survey was chosen", r1 and r1["survey"] == mine["id"])
check("agents never see the link's token", all("token" not in x for x in a1.call("ticket_survey", {"ticket": t1["id"]})))
t_same = make(f"Survey again {RUN}", f"one{RUN.lower()}@client.example", team["id"])
a1.call("resolve_ticket", {"id": t_same["id"], "resolution": "fixed"}, plugin=T)
time.sleep(5)
check("the same person is not asked again within the cooldown", mgr.call("ticket_survey", {"ticket": t_same["id"]}) == [])
forced = a1.call("send_survey_now", {"id": t_same["id"]})
check("an agent can ask anyway", forced["sent"] is True)
refused("but not twice for one ticket", lambda: a1.call("send_survey_now", {"id": t_same["id"]}), ["cannot be surveyed"])

print("answering")
tok = r1["token"]
refused("a bad link is refused", lambda: guest.call("csat_get", {"token": "nope"}), ["not valid"])
g = guest.call("csat_get", {"token": tok})
check("the link shows the question", g["question"] == f"Rate team {RUN}" and g["answered"] is False and g["expired"] is False, g)
refused("a rating is 1 to 5", lambda: guest.call("csat_submit", {"token": tok, "rating": 9}), ["1 to 5"])
ok = guest.call("csat_submit", {"token": tok, "rating": 1, "comment": "Slow and rude"})
check("a visitor answers", ok["thanks"] is True)
refused("an answer is final", lambda: guest.call("csat_submit", {"token": tok, "rating": 5}), ["already"])
done = None
for _ in range(60):
    done = mgr.call("ticket_survey", {"ticket": t1["id"]})[0]
    if done.get("applied") and done.get("alerted"):
        break
    time.sleep(0.5)
check("the rating is written to the ticket, a low one alerts", done["applied"] and done["alerted"] and done["rating"] == 1, done)
tk = a1.call("get_ticket", {"id": t1["id"]}, plugin=T)
check("the ticket carries the rating and comment", tk["rating"] == 1 and tk["rating_comment"] == "Slow and rude", tk)

print("running out")
quick = mgr.call("create_survey", {"code": f"Q{RUN}", "name": f"Quick {RUN}", "team": owner.call("create_team", {"code": f"QT{RUN}", "name": f"Quick team {RUN}", "company": company}, plugin="helpdesk")["id"], "expires_days": 0})
qt = make(f"Expiring {RUN}", f"exp{RUN.lower()}@client.example", quick["team"])
a1.call("resolve_ticket", {"id": qt["id"]}, plugin=T)
rq = survey_of(qt["id"])
time.sleep(1)
gq = guest.call("csat_get", {"token": rq["token"]})
check("a survey past its time says so", gq["expired"] is True, gq)
refused("and takes no answer", lambda: guest.call("csat_submit", {"token": rq["token"], "rating": 5}), ["no longer open"])
sw = mgr.call("survey_sweep")
check("the sweep expires it", sw["expired"] >= 1, sw)

print("reports")
t3 = make(f"Survey three {RUN}", f"three{RUN.lower()}@client.example", team["id"])
a1.call("resolve_ticket", {"id": t3["id"]}, plugin=T)
r3 = survey_of(t3["id"])
guest.call("csat_submit", {"token": r3["token"], "rating": 5, "comment": "Great"})
time.sleep(3)
o = a1.call("csat_overview")
check("the overview counts", o["sent"] >= 3 and o["answered"] >= 2 and o["average"] is not None and len(o["distribution"]) == 5, o)
check("the spread adds up", sum(d["answers"] for d in o["distribution"]) == o["answered"])
check("results come by agent and team", o["by_team"] and all("average" in x for x in o["by_team"]))
check("low comments are listed", any(l["comment"] == "Slow and rude" for l in o["low"]))
lst = rows(a1.call("list_survey_responses", {"state": "answered"}))
check("the responses list hides tokens from agents", lst and all("token" not in r for r in lst))
refused("customers see no results", lambda: cust.call("csat_overview"), ["role"])
refused("customers cannot sweep", lambda: cust.call("survey_sweep"), ["role"])
owner.call("set_settings", {"company": company, "csat_enabled": False}, plugin="helpdesk")
finish()
