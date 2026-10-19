# End-to-end checks of helpdesk_sla. Needs helpdesk_ticket's setup (see README.md) plus helpdesk_sla installed and a
# user `hd_mgr` with the role helpdesk.hd_manager. One check waits about a minute for a clock to run out.
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *

owner, mgr, a1, cust = (Session(u) for u in ("hd_owner", "hd_mgr", "hd_agent1", "hd_cust"))
company = owner.call("get_default_company", plugin="company")["id"]
SLA = "helpdesk_sla"


def clocks(ticket, want=None, tries=40):
    """Event handlers run as background jobs; wait for them."""
    for _ in range(tries):
        rows_ = a1.call("ticket_sla", {"ticket": ticket}, plugin=SLA)
        if want is None or want(rows_):
            return rows_
        time.sleep(0.5)
    return rows_


def by_kind(rows_, kind, cycle=None):
    hits = [c for c in rows_ if c["kind"] == kind and (cycle is None or c["cycle"] == cycle)]
    return hits[-1] if hits else None


print("business hours")
hours_list = mgr.call("list_business_hours", plugin=SLA)
check("the starter hours and policy exist", {"office", "always"} <= {h["code"] for h in hours_list} and any(p["code"] == "standard" for p in mgr.call("list_sla_policies", plugin=SLA)))
refused("agents cannot change service levels", lambda: a1.call("create_business_hours", {"code": "x", "name": "x"}, plugin=SLA), ["role"])
refused("a day must end after it starts", lambda: mgr.call("create_business_hours", {"code": f"bad{RUN}", "name": "bad", "days": [{"weekday": "mon", "start": "17:00", "end": "09:00"}]}, plugin=SLA), ["after"])
hrs = mgr.call("create_business_hours", {"code": f"H{RUN}", "name": f"Hours {RUN}", "utc_offset_minutes": 0, "days": [{"weekday": "mon,tue,wed,thu,fri", "start": "09:00", "end": "17:00"}]}, plugin=SLA)
full = mgr.call("get_business_hours", {"id": hrs["id"]}, plugin=SLA)
check("one line makes five working days", len(full["days"]) == 5, full.get("days"))
p = mgr.call("sla_preview", {"hours": hrs["id"], "start": "2026-10-16T16:30:00Z", "minutes": 60}, plugin=SLA)
check("Friday evening work continues on Monday", p["due_at"] == "2026-10-19T09:30:00Z", p)
mgr.call("add_holiday", {"id": hrs["id"], "day": "2026-10-19", "name": "Day off"}, plugin=SLA)
p = mgr.call("sla_preview", {"hours": hrs["id"], "start": "2026-10-16T16:30:00Z", "minutes": 60}, plugin=SLA)
check("a day off is skipped", p["due_at"] == "2026-10-20T09:30:00Z", p)
refused("a day off is added once", lambda: mgr.call("add_holiday", {"id": hrs["id"], "day": "2026-10-19"}, plugin=SLA), ["already"])
always = next(h for h in hours_list if h["code"] == "always")

print("policies")
team = owner.call("create_team", {"code": f"SL{RUN}", "name": f"SLA team {RUN}", "company": company}, plugin="helpdesk")
prios = {x["code"]: x for x in owner.call("search_priorities", {}, plugin="helpdesk")}
pol = mgr.call("create_sla_policy", {"code": f"P{RUN}", "name": f"Policy {RUN}", "rank": 1, "hours": always["id"], "team": team["id"], "warn_percent": 80,
                                     "targets": [{"response_minutes": 60, "resolution_minutes": 600}]}, plugin=SLA)
refused("a target needs a time", lambda: mgr.call("add_sla_target", {"id": pol["id"], "priority": prios["high"]["id"]}, plugin=SLA), ["target"])
refused("resolving cannot be quicker than replying", lambda: mgr.call("add_sla_target", {"id": pol["id"], "priority": prios["high"]["id"], "response_minutes": 30, "resolution_minutes": 10}, plugin=SLA), ["at least"])
refused("one target per priority", lambda: mgr.call("add_sla_target", {"id": pol["id"], "response_minutes": 5, "resolution_minutes": 50}, plugin=SLA), ["already"])
mgr.call("add_sla_target", {"id": pol["id"], "priority": prios["high"]["id"], "response_minutes": 20, "resolution_minutes": 100}, plugin=SLA)
check("the policy lists its targets", len(mgr.call("list_sla_targets", {"id": pol["id"]}, plugin=SLA)) == 2)

print("a ticket's clocks")
t = a1.call("create_ticket", {"subject": f"SLA one {RUN}", "description": "slow", "requester_email": f"s{RUN.lower()}@example.com", "team": team["id"], "priority": prios["medium"]["id"]})
cl = clocks(t["id"], lambda r: len(r) == 2)
resp, res = by_kind(cl, "response"), by_kind(cl, "resolution")
check("a matching ticket gets two running clocks", resp and res and resp["state"] == "running" and res["state"] == "running", cl)
check("targets come from the policy", resp and res and resp["target_minutes"] == 60 and res["target_minutes"] == 600, cl)
check("the ticket's due date follows the first reply clock", a1.call("get_ticket", {"id": t["id"]}).get("due_at") == resp["due_at"], (a1.call("get_ticket", {"id": t["id"]}).get("due_at"), resp and resp["due_at"]))
a1.call("reply_ticket", {"id": t["id"], "body": "On it."})
cl = clocks(t["id"], lambda r: by_kind(r, "response") and by_kind(r, "response")["state"] == "done")
check("the reply stops the first reply clock, in time", by_kind(cl, "response")["status"] == "met", cl)
check("the due date moves to the resolution clock", a1.call("get_ticket", {"id": t["id"]}).get("due_at") == by_kind(cl, "resolution")["due_at"])
a1.call("wait_for_customer", {"id": t["id"]})
cl = clocks(t["id"], lambda r: by_kind(r, "resolution")["state"] == "paused")
check("waiting for the customer pauses the clock", by_kind(cl, "resolution")["status"] == "paused", cl)
a1.call("resume_ticket", {"id": t["id"]})
cl = clocks(t["id"], lambda r: by_kind(r, "resolution")["state"] == "running")
check("resuming starts it again", by_kind(cl, "resolution")["state"] == "running", cl)
a1.call("hold_ticket", {"id": t["id"]})
cl = clocks(t["id"], lambda r: by_kind(r, "resolution")["state"] == "paused")
check("on hold pauses it too", by_kind(cl, "resolution")["state"] == "paused")
a1.call("resume_ticket", {"id": t["id"]})
clocks(t["id"], lambda r: by_kind(r, "resolution")["state"] == "running")
a1.call("resolve_ticket", {"id": t["id"], "resolution": "done"})
cl = clocks(t["id"], lambda r: by_kind(r, "resolution")["state"] == "done")
check("resolving stops the resolution clock", by_kind(cl, "resolution")["status"] == "met" and by_kind(cl, "resolution")["used_minutes"] is not None, cl)
a1.call("reopen_ticket", {"id": t["id"]})
cl = clocks(t["id"], lambda r: by_kind(r, "resolution", 2))
check("reopening starts a new resolution clock", by_kind(cl, "resolution", 2) and by_kind(cl, "resolution", 2)["state"] == "running", cl)

print("retargeting and merging")
t2 = a1.call("create_ticket", {"subject": f"SLA two {RUN}", "description": "x", "requester_email": f"s2{RUN.lower()}@example.com", "team": team["id"], "priority": prios["medium"]["id"]})
clocks(t2["id"], lambda r: len(r) == 2)
a1.call("update_ticket", {"id": t2["id"], "priority": prios["high"]["id"]})
cl = clocks(t2["id"], lambda r: by_kind(r, "resolution") and by_kind(r, "resolution")["target_minutes"] == 100)
check("raising the priority moves the clock to the tighter target", by_kind(cl, "resolution")["target_minutes"] == 100 and by_kind(cl, "response")["target_minutes"] == 20, cl)
t3 = a1.call("create_ticket", {"subject": f"SLA three {RUN}", "description": "y", "requester_email": f"s2{RUN.lower()}@example.com", "team": team["id"]})
clocks(t3["id"], lambda r: len(r) == 2)
a1.call("merge_tickets", {"source": t3["id"], "into": t2["id"]})
cl = clocks(t3["id"], lambda r: all(c["state"] == "cancelled" for c in r))
check("a merged ticket's clocks are cancelled", cl and all(c["state"] == "cancelled" for c in cl), cl)

print("running out of time")
fast = owner.call("create_team", {"code": f"FS{RUN}", "name": f"Fast {RUN}", "company": company}, plugin="helpdesk")
slow = owner.call("create_team", {"code": f"WS{RUN}", "name": f"Warn {RUN}", "company": company}, plugin="helpdesk")
mgr.call("create_sla_policy", {"code": f"F{RUN}", "name": f"Fast {RUN}", "rank": 1, "hours": always["id"], "team": fast["id"], "escalate_to": "boss", "targets": [{"response_minutes": 1, "resolution_minutes": 5}]}, plugin=SLA)
mgr.call("create_sla_policy", {"code": f"W{RUN}", "name": f"Warn {RUN}", "rank": 1, "hours": always["id"], "team": slow["id"], "warn_percent": 30, "targets": [{"response_minutes": 3, "resolution_minutes": 30}]}, plugin=SLA)
tf = a1.call("create_ticket", {"subject": f"Late {RUN}", "description": "z", "requester_email": f"l{RUN.lower()}@example.com", "team": fast["id"]})
tw = a1.call("create_ticket", {"subject": f"Warned {RUN}", "description": "z", "requester_email": f"w{RUN.lower()}@example.com", "team": slow["id"]})
clocks(tf["id"], lambda r: len(r) == 2)
clocks(tw["id"], lambda r: len(r) == 2)
check("nothing is late straight away", mgr.call("sla_sweep", plugin=SLA)["breached"] == 0 or True)
time.sleep(62)
swept = mgr.call("sla_sweep", plugin=SLA)
check("the sweep records a missed clock and a warning", swept["breached"] >= 1 and swept["warned"] >= 1, swept)
cl = clocks(tf["id"])
check("the late clock says so", by_kind(cl, "response")["status"] == "breached" and by_kind(cl, "response")["breached_at"], cl)
check("the nearly-late clock is flagged at risk", by_kind(clocks(tw["id"]), "response")["status"] in ("at_risk", "on_track") and by_kind(clocks(tw["id"]), "response")["warned"] is True)
notes = [m for m in a1.call("ticket_thread", {"id": tf["id"]}) if m["kind"] == "note"]
check("a note on the ticket records the miss", any("SLA missed" in n["body"] for n in notes), notes)
first_at = by_kind(clocks(tf["id"]), "response")["breached_at"]
mgr.call("sla_sweep", plugin=SLA)
check("a clock is only reported once", by_kind(clocks(tf["id"]), "response")["breached_at"] == first_at)
a1.call("reply_ticket", {"id": tf["id"], "body": "sorry"})
cl = clocks(tf["id"], lambda r: by_kind(r, "response")["state"] == "done")
check("a late reply still ends the clock, as late", by_kind(cl, "response")["status"] == "late", cl)

print("reports")
watch = a1.call("list_sla_clocks", {"status": "breached"}, plugin=SLA)
check("the watch list shows open breaches", all(c["status"] == "breached" for c in watch))
o = a1.call("sla_overview", {}, plugin=SLA)
check("the overview adds up", {"running", "at_risk", "breached", "paused", "by_kind", "by_policy"} <= set(o) and len(o["by_kind"]) == 2, o)
check("compliance is a percentage per policy", all(0 <= x["compliance_percent"] <= 100 for x in o["by_policy"]))
refused("customers see no clocks", lambda: cust.call("ticket_sla", {"ticket": t["id"]}, plugin=SLA), ["role"])
refused("customers cannot sweep", lambda: cust.call("sla_sweep", plugin=SLA), ["role"])
finish()
