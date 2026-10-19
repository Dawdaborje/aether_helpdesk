# Automation (`helpdesk_automation`)

Rules that act on tickets, and macros agents apply with one click. Rust → WASM. Depends on `helpdesk_ticket`, `helpdesk`.

## A rule

- **When:** a ticket is created, a customer replies, an agent replies, its state changes, it is updated (priority, type,
  team or customer), assigned, resolved or reopened — or **time**: every ten minutes, every open ticket is checked.
- **Only if** (all, or any): state, priority, type, team, assignee, channel, customer, requester email or domain, subject,
  description, tags, *unassigned*, *waiting for us* (the customer spoke last), reopen and message counts, rating, and the
  clock: hours open, hours since the last update, since the customer wrote, since an agent wrote, and hours to the due
  date (negative once overdue — so SLA misses are a condition). Comparisons: is, is not, contains, does not contain, in a
  list, greater / less (or equal), is set, is empty. Priority, team and type may be named by id, code or name.
- **Then:** set priority, team or type; assign to a person or **by the team's rule**; unassign; add or remove a tag; move
  the state; add an internal note; send a public reply; email an address or the requester; notify people (the assignee,
  the team, named users); **escalate** (raise the priority one level and tell someone); set a due date. Messages may use
  `{{reference}}`, `{{subject}}`, `{{requester.name}}`, `{{requester.first_name}}`, `{{requester.email}}`, `{{state}}`.
- Rules run **in order**; one can **stop the others**; one can fire **once per ticket** (the default for clock rules), or
  again after some hours. If an action fails the rule stops there and the run is logged as failed.

## Safety

- A rule can set off more events (it changed the priority, so *updated* fires); a chain of rules more than three deep is cut
  off, and a rule that has already fired three times for a ticket within two minutes is held back, so rules that undo each other
  cannot loop.
- Setting up rules, and sharing macros, is for managers and administrators. Rules act as the kernel, so they are not held
  back by an agent's team limits.
- **Try it first:** `test_rule_on_ticket` says whether a rule matches a ticket and lists, line by line, what it would do —
  nothing is changed.
- Every run is logged with the ticket, the trigger and what happened (on the rule, on the ticket's *Automation* tab and in
  the run log).

## The clock

`automation_sweep` does not walk the tickets itself: it counts the open ones and hands them out in jobs of 40, each a
separate background call with its own budget, so any number of tickets is covered.

## Macros

A macro is a named list of the same actions, personal, shared with a team, or global. Applying one (`Macro` button on a
ticket, as the agent, so team limits and roles apply) runs the actions, stops at the first that fails and says where,
leaves a note on the ticket and counts the use.

## Screens

Rules (filter by trigger), a rule (conditions, actions, runs, *Try on a ticket*), run log, macros, and on every ticket a
*Macro* button and an *Automation* tab.
