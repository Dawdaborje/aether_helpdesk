# Tickets (`helpdesk_ticket`)

## Purpose

Customer tickets for the helpdesk suite: raised over any channel, worked in one thread of public replies and internal
notes, assigned by team rules, merged, split, nested, timed and rated. Rust → WASM. Every step is announced as an event
so SLA, automation, CSAT and reporting plugins can follow without this plugin knowing them.

## Dependencies

`helpdesk` (teams, priorities, types, tags, settings), `party` (customers), `company`.

## Models

| Model | Role |
|---|---|
| `hd_ticket` | The ticket: reference `HD-000001`, subject, state, channel, priority/type/team (masters), assignee, customer (party) and requester contact, parent / merged-into / split-from, link to a record in another plugin (`source_model`, `source_id`), due date, the timestamps SLA needs, rating |
| `hd_message` | One line of the thread: `reply` (public), `note` (internal) or `system`; direction in / out / internal; the email message-id for threading; delivery state of the email |
| `hd_ticket_tag` | Tag assignment (names are also kept in `tags_text` for search) |
| `hd_time_entry` | Minutes an agent spent, billable or not |
| `counter` | Numbering |

Chatter is on for notes, followers and tracked changes (state, assignee, priority, team, type, customer); the thread
itself is `hd_message` because replies must reach the requester, which chatter does not do.

## States

`new → open ⇄ pending (waiting on the customer) / on_hold (waiting on someone else) → resolved → closed`, plus `merged`.
The kernel's workflow enforces the moves (`rules/hd_ticket.json`), and *pending* and *on hold* are states of their own so
an SLA clock can pause on them. A customer reply wakes pending, on-hold and resolved tickets. Reopening counts.

## Assignment

A team assigns by hand, **round robin** (whoever was assigned one longest ago) or **load balanced** (fewest open tickets
for their capacity; a member at capacity is skipped, capacity 0 is unlimited). A ticket made with no assignee asks its
team's rule; the team comes from the request, the ticket type's default team, or the settings.

## Access

The plugin's own functions run with the plugin's rights, so each one checks the caller itself (`require_can_see`):
agents (`helpdesk.hd_agent`, `hd_manager`, `hd_admin`) see everything; anyone else only the tickets they asked for
(`requester_user`), and of those only public replies. Customers ask with `submit_ticket`, follow with `my_requests`,
`ticket_thread`, write back with `customer_reply_ticket`, and rate with `rate_ticket`. State, timestamps and rating
fields are writable only by this plugin.

## People without an account

`visitor_submit` takes a request from an anonymous visitor (name, email, subject, description; five an hour per address; a
hidden `website` field catches bots) and answers with the **reference and a secret key**. A background job
(`intake_finish`) then gives it a company, priority, team and owner, sends the thank-you email with a tracking link
(when the site address is set in the helpdesk settings) and announces `ticket_created`. With the pair, the visitor can
`visitor_view` (public replies only), `visitor_reply`, `visitor_rate` and `visitor_reopen`. These five functions are the
plugin's `public_functions`; each asks for the key itself.

## Teams

With `restrict_by_team` on in the settings an agent sees the tickets of their teams, tickets with no team and tickets
assigned to them; managers and administrators see all. The check is in every function that opens a ticket (`get_ticket_row`)
and in every list.

## Email

Replies go out with `[HD-000123]` in the subject. A failed send is recorded on the message (`delivery`,
`delivery_error`) and never undoes the reply. `ingest_email` takes a received message (from, subject, text,
message-id, in-reply-to): it joins a ticket by the reference in the subject or by the message it answers, opens a new
ticket otherwise, ignores automatic replies, does nothing twice for the same message-id, and opens a linked follow-up when
the ticket it answers is closed. Fetching mail is a separate plugin's job (`helpdesk_email`); it calls this function.

## Functions

Create: `create_ticket` (agent), `submit_ticket` (any member), `create_subticket`.
Read: `get_ticket`, `list_tickets`, `my_tickets`, `unassigned_tickets`, `my_requests`, `sub_tickets`, `ticket_thread`,
`tickets_waiting_for_agent`, `ticket_assignees`, `mergeable_tickets`.
Move: `take_ticket`, `wait_for_customer`, `hold_ticket`, `resume_ticket`, `resolve_ticket`, `close_ticket`,
`reopen_ticket`, `change_ticket_state`, `move_ticket` (board).
Work: `reply_ticket`, `add_ticket_note`, `customer_reply_ticket`, `ingest_email`, `assign_ticket`, `assign_ticket_to_me`,
`unassign_ticket`, `update_ticket`, `add_ticket_tag`, `remove_ticket_tag`, `merge_tickets`, `split_ticket`,
`split_ticket_message`, `mark_ticket_spam`, `bulk_update_tickets` (assign, unassign, priority, team, tag, take, resolve,
close, reopen; up to 200, failures named), `log_ticket_time`, `list_ticket_time`, `delete_time_entry`, `rate_ticket`.
Numbers: `ticket_overview` (open, unassigned, mine, overdue, waiting for us, created and resolved in 7 days, average first
reply and resolution hours, rating, by state / priority / team / agent, 14-day trend).

## Events

`ticket_created`, `ticket_updated` (which of priority, type, team, customer … changed), `ticket_assigned`, `ticket_replied` (`by` agent or customer), `ticket_status_changed` (`from`, `to`),
`ticket_resolved`, `ticket_closed`, `ticket_reopened`, `ticket_merged`, `ticket_rated`. The payload carries the ticket id,
reference, subject, state, customer, team, assignee, priority and company.

## Nightly

`tickets_nightly` closes tickets that have been resolved for the company's `auto_close_days`.

## Screens

Overview, Tickets (search, filters, grouping, server paging), My tickets, Unassigned, Board (drag between states), New
ticket, the ticket (header actions, conversation, time, sub-tickets, resolution), and forms to reply, note, assign,
resolve, merge, add a sub-ticket and log time.

## The rest of the suite

Service levels (`helpdesk_sla`), canned replies, the knowledge base, the portal, mail in, satisfaction surveys and
automation are plugins of their own that listen to the events above and call the functions here (`set_ticket_due`,
`ingest_email`, `auto_assign_ticket`, `rate_ticket`, …). See the README of the suite.
