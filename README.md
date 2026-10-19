# Helpdesk

Ticketing suite for Aether, built as WASM addons (Rust).

| Plugin | Purpose | State |
|---|---|---|
| [`helpdesk`](./helpdesk) | Teams, agents, priorities, ticket types, tags, settings, seed data | built |
| [`helpdesk_ticket`](./helpdesk_ticket) | Tickets, one thread of replies and notes, assignment, merge, split, time, ratings, board | built (e2e 82/82) |
| [`helpdesk_sla`](./helpdesk_sla) | Business-hours SLA policies and clocks that pause while waiting | built (e2e 35/35) |
| [`helpdesk_canned`](./helpdesk_canned) | Canned replies with merge fields, scopes, suggestions, signatures | built (e2e 27/27) |
| [`helpdesk_kb`](./helpdesk_kb) | Knowledge base: revisions, audiences, ranked search, gaps, deflection | built (e2e 43/43) |
| [`helpdesk_portal`](./helpdesk_portal) | Customer portal: help search, ask, follow, reply, rate, reopen (no account needed) | built (e2e 22/22) |
| [`helpdesk_email`](./helpdesk_email) | Mail in: secret webhook, mailboxes that route, block list, quoted-reply trimming, retrying inbox | built (e2e 29/29) |
| [`helpdesk_csat`](./helpdesk_csat) | Satisfaction surveys by link, reminders, expiry, low-rating alerts, reports | built (e2e 28/28) |
| [`helpdesk_automation`](./helpdesk_automation) | Event and clock rules, macros, dry run, run log | built (e2e 35/35) |

What you get, together: tickets from the web, email, the portal or an agent; round-robin or load-based assignment; one thread of public replies and internal notes; business-hours SLA clocks that pause while waiting; canned replies and macros; a knowledge base with ranked search that shows what people could not find; a portal that works with or without an account; mail in with routing and blocking; satisfaction surveys with alerts; and rules for events and for the clock. Every plugin is optional beyond `helpdesk` and `helpdesk_ticket`, and they talk through events.

Build a plugin with `make -C <plugin>`. Load: `aether -c project/test/aether.toml --load-plugins ../../plugins/helpdesk`.
Roles live in `helpdesk`: `hd_agent`, `hd_manager`, `hd_admin`; customers need no role. Setting up and tests: [`tests/`](./tests).
