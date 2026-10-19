# End-to-end checks of helpdesk_email. Needs helpdesk_ticket and helpdesk_email installed, tenancy set to `header`, and a
# user `hd_mgr` with the role helpdesk.hd_manager.
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hdlib import *
import hdlib
hdlib.DEFAULT_PLUGIN = "helpdesk_email"

owner, mgr, a1, cust = (Session(u) for u in ("hd_owner", "hd_mgr", "hd_agent1", "hd_cust"))
guest = Session(None)
T = "helpdesk_ticket"
company = owner.call("get_default_company", plugin="company")["id"]


def settled(message_id, tries=80):
    for _ in range(tries):
        found = [r for r in rows(a1.call("list_email_inbox", {"q": message_id})) if r.get("message_id") == message_id]
        if found and found[0]["state"] != "queued":
            return found[0]
        time.sleep(0.5)
    return found[0] if found else None


print("mailboxes")
team = owner.call("create_team", {"code": f"EM{RUN}", "name": f"Mail {RUN}", "company": company}, plugin="helpdesk")
types = {t["code"]: t for t in owner.call("search_ticket_types", {}, plugin="helpdesk")}
addr = f"support{RUN.lower()}@co.example"
refused("agents do not make mailboxes", lambda: a1.call("create_mailbox", {"address": addr, "name": "x"}), ["role"])
refused("a mailbox is an address", lambda: mgr.call("create_mailbox", {"address": "nope", "name": "x"}), ["address"])
box = mgr.call("create_mailbox", {"address": addr.upper(), "name": f"Support {RUN}", "team": team["id"], "ticket_type": types["incident"]["id"]})
check("a mailbox gets a long secret", len(box["secret"]) >= 30 and box["address"] == addr, box)
refused("addresses are unique", lambda: mgr.call("create_mailbox", {"address": addr, "name": "again"}), ["already"])
check("agents never see the secret", all("secret" not in m for m in a1.call("list_mailboxes")) and "secret" not in a1.call("get_mailbox", {"id": box["id"]}))
check("managers do", mgr.call("get_mailbox", {"id": box["id"]}).get("secret") == box["secret"])
secret = box["secret"]

print("the door")
refused("no secret, no entry", lambda: guest.call("email_inbound", {"from": "a@x.org", "subject": "hi"}), ["not accepted"])
refused("a wrong secret is not accepted either", lambda: guest.call("email_inbound", {"secret": "z" * 40, "from": "a@x.org", "subject": "hi"}), ["not accepted"])
refused("the desk's functions are closed to visitors", lambda: guest.call("list_email_inbox", {}))
mid = f"<first-{RUN}@mail.example>"
r = guest.call("email_inbound", {"secret": secret, "from": f"Pat Doe <pat{RUN.lower()}@client.example>", "to": [addr], "subject": f"Printer is down {RUN}", "text": "It jams every morning.", "message_id": mid})
check("a good message is accepted", r["accepted"] is True and r.get("id"), r)
check("the same message again is not kept twice", guest.call("email_inbound", {"secret": secret, "from": f"pat{RUN.lower()}@client.example", "subject": "x", "text": "x", "message_id": mid}).get("duplicate") is True)
row = settled(mid)
check("it is processed into a ticket", row and row["state"] == "processed" and row["outcome"] == "opened a ticket" and row.get("ticket"), row)
t = a1.call("get_ticket", {"id": row["ticket"]}, plugin=T)
check("the mailbox routed it", t["team"] == team["id"] and t["ticket_type"] == types["incident"]["id"] and t["reply_to"] == addr and t["channel"] == "email", t)
check("the sender is the requester", t["requester_email"] == f"pat{RUN.lower()}@client.example" and t["requester_name"] == "Pat Doe")

print("replies")
quoted = "Yes, it is plugged in.\n\nThanks,\nPat\n\nOn Mon, 5 Oct 2026 at 10:00, Support <" + addr + "> wrote:\n> Is it plugged in?\n> Please check."
mid2 = f"<second-{RUN}@mail.example>"
guest.call("email_inbound", {"secret": secret, "from": f"pat{RUN.lower()}@client.example", "to": addr, "subject": f"Re: [{t['reference']}] Printer is down", "text": quoted, "message_id": mid2, "in_reply_to": mid})
row2 = settled(mid2)
check("a reply joins the ticket", row2 and row2["state"] == "processed" and row2["ticket"] == row["ticket"] and row2["outcome"] == "added to a ticket", row2)
inbound = [m for m in a1.call("ticket_thread", {"id": row["ticket"]}, plugin=T) if m["direction"] == "inbound"]
check("the quoted conversation is cut away", len(inbound) == 2 and "wrote:" not in inbound[-1]["body"] and inbound[-1]["body"].startswith("Yes, it is plugged in.") and inbound[-1]["body"].endswith("Pat"), [m["body"] for m in inbound])

print("provider shapes and raw mail")
mid3 = f"pm-{RUN}"
guest.call("email_inbound", {"secret": secret, "From": f"lee{RUN.lower()}@client.example", "FromName": "Lee", "ToFull": [{"Email": addr}], "Subject": f"Postmark {RUN}", "TextBody": "From Postmark", "MessageID": mid3, "Headers": [{"Name": "X-Spam-Status", "Value": "No"}]})
row3 = settled(mid3)
check("a Postmark message is understood", row3 and row3["state"] == "processed" and row3["from_email"] == f"lee{RUN.lower()}@client.example", row3)
mid4 = f"rs-{RUN}"
guest.call("email_inbound", {"secret": secret, "type": "email.received", "data": {"from": f"Kim <kim{RUN.lower()}@client.example>", "to": [addr], "subject": f"Resend {RUN}", "text": "From Resend", "message_id": mid4}})
check("a Resend message is understood", (settled(mid4) or {}).get("state") == "processed")
mid5 = f"<raw-{RUN}@mail.example>"
raw = f"From: =?UTF-8?B?SsO8cmdlbg==?= <jurgen{RUN.lower()}@client.example>\nTo: {addr}\nSubject: Raw mail {RUN}\nMessage-ID: {mid5}\nContent-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\nCaf=C3=A9 machine broken\n"
guest.call("email_inbound", {"secret": secret, "raw": raw})
row5 = settled(mid5)
rt = a1.call("get_ticket", {"id": row5["ticket"]}, plugin=T) if row5 and row5.get("ticket") else {}
check("a raw message is parsed, name and text decoded", row5 and row5["state"] == "processed" and rt.get("requester_name") == "Jürgen" and "Café" in rt.get("description", ""), (row5, rt.get("requester_name")))

print("what is kept out")
mgr.call("add_email_block", {"pattern": f"@spam{RUN.lower()}.example", "reason": "spammers"})
refused("a block is an address or domain", lambda: mgr.call("add_email_block", {"pattern": "nonsense"}), ["block"])
mid6 = f"blk-{RUN}"
guest.call("email_inbound", {"secret": secret, "from": f"x{RUN.lower()}@mail.spam{RUN.lower()}.example", "to": addr, "subject": "Win", "text": "money", "message_id": mid6})
row6 = settled(mid6)
check("a blocked domain (and its subdomains) is stopped", row6 and row6["state"] == "blocked" and row6.get("ticket") is None, row6)
check("the block counts what it stopped", any(b["pattern"] == f"@spam{RUN.lower()}.example" and b["hits"] >= 1 for b in a1.call("list_email_blocks")))
mid7 = f"sp-{RUN}"
guest.call("email_inbound", {"secret": secret, "from": f"s{RUN.lower()}@client.example", "to": addr, "subject": "Cheap", "text": "buy", "message_id": mid7, "headers": {"X-Spam-Flag": "YES"}})
check("mail the server flagged as spam is ignored", (settled(mid7) or {}).get("state") == "ignored")
mid8 = f"au-{RUN}"
guest.call("email_inbound", {"secret": secret, "from": f"r{RUN.lower()}@client.example", "to": addr, "subject": "Away", "text": "away", "message_id": mid8, "headers": {"Auto-Submitted": "auto-replied"}})
check("an automatic reply is ignored", (settled(mid8) or {}).get("state") == "ignored")
mid9 = f"lp-{RUN}"
guest.call("email_inbound", {"secret": secret, "from": addr, "to": addr, "subject": "Loop", "text": "loop", "message_id": mid9})
loop = settled(mid9)
check("mail from our own mailbox is a loop and is dropped", loop and loop["state"] == "ignored" and "loop" in loop["outcome"], loop)

print("by hand")
done = a1.call("email_ingest", {"mailbox": box["id"], "from": f"hand{RUN.lower()}@client.example", "subject": f"By hand {RUN}", "text": "Forwarded to us", "message_id": f"hand-{RUN}"})
check("an agent can feed a message in and see the result at once", done["state"] == "processed" and done["ticket"], done)
again = a1.call("retry_email", {"id": row["id"]})
check("trying again a finished message does nothing", again["state"] == "processed")
o = a1.call("email_overview")
check("the overview adds up", o["mailboxes"] >= 1 and o["blocked_senders"] >= 1 and any(s["state"] == "processed" for s in o["by_state"]), o)
rot = mgr.call("rotate_mailbox_secret", {"id": box["id"]})
refused("a rotated secret ends the old one", lambda: guest.call("email_inbound", {"secret": secret, "from": "a@x.org", "subject": "x", "text": "x"}), ["not accepted"])
finish()
