# End-to-end checks of helpdesk_automation. Needs helpdesk_ticket and helpdesk_automation installed and a user `hd_mgr`
# with the role helpdesk.hd_manager. Event-driven rules run in the background, so some checks wait for them.
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *
import hdlib
hdlib.DEFAULT_PLUGIN = "helpdesk_automation"

owner, mgr, a1, a2 = (Session(u) for u in ("hd_owner", "hd_mgr", "hd_agent1", "hd_agent2"))
T = "helpdesk_ticket"
company = owner.call("get_default_company", plugin="company")["id"]
prios = {x["code"]: x for x in owner.call("search_priorities", {}, plugin="helpdesk")}


def tk(id_):
    return a1.call("get_ticket", {"id": id_}, plugin=T)


def until(fn, tries=60):
    for _ in range(tries):
        v = fn()
        if v:
            return v
        time.sleep(0.5)
    return fn()


def make(subject, **kw):
    body = {"subject": subject, "description": kw.pop("description", "x"), "requester_email": kw.pop("email", f"u{RUN.lower()}@client.example"), "requester_name": "Pat Doe", "channel": "email"}
    body.update(kw)
    return a1.call("create_ticket", body, plugin=T)


print("making rules")
refused("agents do not make rules", lambda: a1.call("create_rule", {"name": "x", "trigger": "created", "actions": [{"action": "add_note", "value": "x"}]}), ["role"])
refused("a trigger must exist", lambda: mgr.call("create_rule", {"name": "x", "trigger": "whenever", "actions": [{"action": "add_note", "value": "x"}]}), ["trigger"])
refused("a rule does something", lambda: mgr.call("create_rule", {"name": "x", "trigger": "created"}), ["action"])
refused("a condition looks at a known thing", lambda: mgr.call("create_rule", {"name": "x", "trigger": "created", "conditions": [{"field": "colour", "op": "eq", "value": "red"}], "actions": [{"action": "add_note", "value": "x"}]}), ["look at"])
refused("a number comparison needs a number", lambda: mgr.call("create_rule", {"name": "x", "trigger": "created", "conditions": [{"field": "hours_open", "op": "gt", "value": "many"}], "actions": [{"action": "add_note", "value": "x"}]}), ["number"])
refused("an action must exist", lambda: mgr.call("create_rule", {"name": "x", "trigger": "created", "actions": [{"action": "explode"}]}), ["action"])
refused("an email action needs a message", lambda: mgr.call("create_rule", {"name": "x", "trigger": "created", "actions": [{"action": "send_email", "target": "requester"}]}), ["message"])

print("an event rule")
word = f"urgentword{RUN.lower()}"
vip_tag = owner.call("create_tag", {"code": f"auto{RUN.lower()}", "name": f"Auto {RUN}"}, plugin="helpdesk")
rule = mgr.call("create_rule", {"name": f"Urgent on arrival {RUN}", "trigger": "created", "rank": 1, "conditions": [{"field": "subject", "op": "contains", "value": word}, {"field": "channel", "op": "eq", "value": "email"}],
                                "actions": [{"action": "set_priority", "value": "urgent"}, {"action": "add_tag", "value": vip_tag["code"]}, {"action": "add_note", "value": "Auto-flagged {{reference}} for {{requester.first_name}}"}]})
check("a rule is made with its conditions and actions", rule["trigger"] == "created" and rule["active"] is True)
full = mgr.call("get_rule", {"id": rule["id"]})
check("they are kept in order", [c["field"] for c in full["conditions"]] == ["subject", "channel"] and [a["action"] for a in full["actions"]] == ["set_priority", "add_tag", "add_note"], full)
t = make(f"Printer on fire {word}")
hit = until(lambda: tk(t["id"]) if tk(t["id"]).get("priority") == prios["urgent"]["id"] else None)
check("a matching ticket is changed", hit and hit["priority"] == prios["urgent"]["id"] and vip_tag["name"] in (hit.get("tags_text") or ""), hit)
notes = [m for m in a1.call("ticket_thread", {"id": t["id"]}, plugin=T) if m["kind"] == "note"]
check("the note is filled in", any("Auto-flagged HD-" in n["body"] and n["body"].endswith("for Pat") for n in notes), notes)
quiet = make("Nothing special here")
time.sleep(4)
check("a ticket that does not match is left alone", tk(quiet["id"]).get("priority") != prios["urgent"]["id"] and not tk(quiet["id"]).get("tags_text"))
runs = until(lambda: mgr.call("list_rule_runs", {"rule": rule["id"]}))
check("the run is logged", runs and runs[0]["ok"] is True and runs[0]["ticket_reference"] == t["reference"], runs)

print("trying a rule")
dry = a1.call("test_rule_on_ticket", {"id": rule["id"], "ticket": quiet["id"]})
check("a dry run says it would not fire", dry["matches"] is False and dry["would"] == [])
dry = a1.call("test_rule_on_ticket", {"id": rule["id"], "ticket": t["id"]})
check("or lists what it would do, changing nothing", dry["matches"] is True and len(dry["would"]) == 3 and "priority set to urgent" in dry["would"][0], dry)

print("conditions and order")
any_rule = mgr.call("create_rule", {"name": f"Any {RUN}", "trigger": "created", "match_mode": "any", "rank": 5, "stop_after": True,
                                    "conditions": [{"field": "subject", "op": "contains", "value": f"alpha{RUN.lower()}"}, {"field": "subject", "op": "contains", "value": f"beta{RUN.lower()}"}],
                                    "actions": [{"action": "set_priority", "value": "low"}, {"action": "change_state", "value": "pending"}]})
later = mgr.call("create_rule", {"name": f"Later {RUN}", "trigger": "created", "rank": 50, "conditions": [{"field": "subject", "op": "contains", "value": f"beta{RUN.lower()}"}], "actions": [{"action": "add_note", "value": "should not run"}]})
tb = make(f"about beta{RUN.lower()}")
done = until(lambda: tk(tb["id"]) if tk(tb["id"])["state"] == "pending" else None)
check("any-of conditions fire, and the actions run in order", done and done["priority"] == prios["low"]["id"] and done["state"] == "pending", done)
time.sleep(3)
check("a rule that stops the others does", not any("should not run" in m["body"] for m in a1.call("ticket_thread", {"id": tb["id"]}, plugin=T)))

print("other triggers")
cust_rule = mgr.call("create_rule", {"name": f"Customer replied {RUN}", "trigger": "replied_by_customer", "conditions": [{"field": "subject", "op": "contains", "value": f"reply{RUN.lower()}"}], "actions": [{"action": "set_priority", "value": "high"}, {"action": "add_note", "value": "Customer is waiting"}]})
tr = make(f"Question reply{RUN.lower()}")
a1.call("reply_ticket", {"id": tr["id"], "body": "Hi"}, plugin=T)
a1.call("ingest_email", {"from_email": f"u{RUN.lower()}@client.example", "subject": f"Re: [{tr['reference']}] Question", "text": "Any news?", "message_id": f"<ar-{RUN}>"}, plugin=T)
got = until(lambda: tk(tr["id"]) if tk(tr["id"]).get("priority") == prios["high"]["id"] else None)
check("a customer's reply triggers its rule", got is not None)
res_rule = mgr.call("create_rule", {"name": f"When solved {RUN}", "trigger": "resolved", "conditions": [{"field": "subject", "op": "contains", "value": f"solve{RUN.lower()}"}], "actions": [{"action": "add_note", "value": "Solved: {{subject}}"}]})
ts = make(f"to solve{RUN.lower()}")
a1.call("resolve_ticket", {"id": ts["id"], "resolution": "ok"}, plugin=T)
until(lambda: any("Solved:" in m["body"] for m in a1.call("ticket_thread", {"id": ts["id"]}, plugin=T) if m["kind"] == "note"))
check("a resolved ticket triggers its rule", any("Solved:" in m["body"] for m in a1.call("ticket_thread", {"id": ts["id"]}, plugin=T) if m["kind"] == "note"))

print("assignment and escalation")
team = owner.call("create_team", {"code": f"AU{RUN}", "name": f"Auto {RUN}", "company": company, "assignment": "round_robin"}, plugin="helpdesk")
owner.call("add_team_member", {"team": team["id"], "user": a2.user, "capacity": 0}, plugin="helpdesk")
as_rule = mgr.call("create_rule", {"name": f"Route {RUN}", "trigger": "created", "conditions": [{"field": "subject", "op": "contains", "value": f"route{RUN.lower()}"}], "actions": [{"action": "set_team", "value": team["code"]}, {"action": "auto_assign"}]})
tt = make(f"please route{RUN.lower()}")
rt = until(lambda: tk(tt["id"]) if tk(tt["id"]).get("assignee") else None)
check("a rule sets the team and assigns by its rule", rt and rt["team"] == team["id"] and rt["assignee"] == a2.user, rt)
esc = mgr.call("create_rule", {"name": f"Escalate {RUN}", "trigger": "created", "conditions": [{"field": "subject", "op": "contains", "value": f"climb{RUN.lower()}"}], "actions": [{"action": "escalate", "target": "assignee", "value": "Raised for {{reference}}"}]})
tc = make(f"needs to climb{RUN.lower()}", priority=prios["low"]["id"])
until(lambda: tk(tc["id"]).get("priority") != prios["low"]["id"])
check("escalating raises the priority a level", tk(tc["id"])["priority"] == prios["medium"]["id"], tk(tc["id"]).get("priority"))

print("the clock")
idle = mgr.call("create_rule", {"name": f"Idle {RUN}", "trigger": "time", "conditions": [{"field": "subject", "op": "contains", "value": f"idle{RUN.lower()}"}, {"field": "hours_open", "op": "gte", "value": "0"}, {"field": "waiting_for_us", "op": "eq", "value": "true"}],
                                 "actions": [{"action": "add_note", "value": "Nobody answered {{reference}}"}]})
check("a clock rule fires once per ticket by default", idle["once_per_ticket"] is True)
ti = make(f"stuck idle{RUN.lower()}")
s1 = mgr.call("automation_sweep")
check("the sweep hands the tickets out in small jobs", s1["pages"] >= 1 and s1["tickets"] >= 1, s1)
nn = lambda: [m for m in a1.call("ticket_thread", {"id": ti["id"]}, plugin=T) if m["kind"] == "note" and "Nobody answered" in m["body"]]
until(nn, tries=80)
check("the rule fires on the stuck ticket", len(nn()) == 1)
mgr.call("automation_sweep")
time.sleep(8)
check("and not a second time", len(nn()) == 1, len(nn()))

print("macros")
refused("agents cannot share macros with everyone", lambda: a1.call("create_macro", {"name": "x", "scope": "global", "action": "add_note", "value": "x"}), ["role"])
mac = a1.call("create_macro", {"name": f"Take and note {RUN}", "action": "change_state", "value": "pending"})
a1.call("add_macro_action", {"id": mac["id"], "action": "add_note", "value": "Waiting for {{requester.first_name}}"})
check("a macro lists its actions", len(a1.call("list_macro_actions", {"id": mac["id"]})) == 2)
check("another agent does not see a personal macro", all(m["id"] != mac["id"] for m in a2.call("list_macros")))
tm = make(f"macro target {RUN}")
out = a1.call("apply_macro_to_ticket", {"macro": mac["id"], "ticket": tm["id"]})
check("applying it does every action", len(out["applied"]) == 2 and tk(tm["id"])["state"] == "pending", out)
check("it leaves a note saying so", any("Macro" in m["body"] and mac["id"] and "applied" in m["body"] for m in a1.call("ticket_thread", {"id": tm["id"]}, plugin=T) if m["kind"] == "note"))
check("and counts the use", a1.call("get_macro", {"id": mac["id"]})["use_count"] == 1)
refused("another agent cannot use it", lambda: a2.call("apply_macro_to_ticket", {"macro": mac["id"], "ticket": tm["id"]}), ["cannot use"])
glob = mgr.call("create_macro", {"name": f"Shared {RUN}", "scope": "global", "action": "add_note", "value": "shared"})
check("a manager's global macro is everyone's", any(m["id"] == glob["id"] for m in a2.call("list_macros")))

print("switching off")
mgr.call("update_rule", {"id": rule["id"], "active": False})
t2 = make(f"Off switch {word}")
time.sleep(5)
check("a switched-off rule does nothing", tk(t2["id"]).get("priority") != prios["urgent"]["id"])
check("the log can be read by agents", len(a1.call("list_rule_runs", {"limit": 50})) >= 3)
finish()
