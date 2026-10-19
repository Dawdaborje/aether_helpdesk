# Service levels (`helpdesk_sla`)

## Purpose

How fast a ticket must get its first reply and its resolution, counted in **business minutes**. Clocks pause while a
ticket waits on someone else, warn before they run out, record when they did, and restart on reopen. Rust → WASM.

## Dependencies

`helpdesk_ticket` (it listens to its events and sets the ticket's due date), `helpdesk` (priorities, teams, types), `party`.

## Models

| Model | Role |
|---|---|
| `hd_hours` + `hd_hours_day` | A weekly working pattern in local time (a UTC offset in minutes). No days means round the clock. One default |
| `hd_holiday` | A day off for one set of hours |
| `hd_sla_policy` + `hd_sla_target` | When it applies (priority, type, team, customer, channel; empty means any), which hours, and targets per priority (a target with no priority covers the rest). `rank` orders policies: the first match wins. Warning point, pause flags, an extra person to tell on a miss |
| `hd_sla_clock` | One per ticket and kind (`response`, `resolution`) and cycle: target, business minutes used so far, when the current stretch started, due time, `breached` / `warned` flags, how long it took when done |

## How a clock lives

- **Start:** on `ticket_created` the first matching policy gives the ticket a first-reply clock and a resolution clock
  (a target of 0 means none). A change of priority, type, team or customer re-matches: a clock keeps the time it has
  used and moves to the new target, or is cancelled if no policy fits.
- **Pause:** `ticket_status_changed` to *pending* or *on hold* pauses clocks whose policy says so; going back to *open*
  starts them with what is left.
- **Stop:** an agent's `ticket_replied` ends the first-reply clock; `ticket_resolved` ends both. Ending after the due time
  counts as late.
- **Restart:** `ticket_reopened` starts a new resolution clock (cycle 2, 3, …); history stays.
- **Merge:** `ticket_merged` cancels the source's clocks.
- **Due date:** the ticket's `due_at` follows the soonest running clock.
- **Sweep** (every five minutes, or `sla_sweep`): a clock past `warn_percent` is warned once (`ticket_sla_warning`, a
  notification to the assignee); a clock out of time is recorded once (`ticket_sla_breached`, a notification to the
  assignee and the policy's extra person, and an internal note on the ticket).

## Business time

`hours.rs` is pure and tested natively: adding working minutes skips nights, weekends, lunch breaks (several intervals a
day) and days off, and `between` counts them; the two agree for any span.

## Functions

Setup (managers and administrators): `create_business_hours`, `set_business_hours`, `add_hours_day` (several weekdays at
once), `remove_hours_day`, `add_holiday`, `remove_holiday`, `create_sla_policy`, `update_sla_policy`, `add_sla_target`,
`remove_sla_target`, `sla_preview`, `recalculate_ticket_sla`, `seed_sla` (office hours Mon–Fri 09:00–17:00, round the
clock, and a standard policy with targets for the four priorities; on install).
Read (agents): `ticket_sla`, `list_sla_clocks` (status: on track, at risk, breached, paused), `sla_overview`
(running, at risk, breached, paused; compliance and average time by kind and by policy for the last N days), lists of
hours, days, holidays, policies and targets.

## Events announced

`ticket_sla_warning`, `ticket_sla_breached` (ticket, reference, clock kind, target, minutes used, assignee, team).

## Screens

Service levels overview, Clocks (filter by status and kind), Policies (with targets), Business hours (working days, days
off), and a *Service level* tab on every ticket.
