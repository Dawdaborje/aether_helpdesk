# Canned replies (`helpdesk_canned`)

Reusable answers for agents, sent through the ticket plugin. Rust → WASM. Depends on `helpdesk_ticket`, `helpdesk`, `party`.

- **Models:** `hd_canned_reply` (title, /shortcut, text, scope personal / team / global, owner, team, ticket type, category,
  state to leave the ticket in, use count) and `hd_signature` (one per agent: the name `{{agent.name}}` stands for, and a
  signature added under replies).
- **Scope:** an agent keeps personal replies and shares with a team they belong to; a manager shares with everyone or
  any team. Only the owner edits a personal reply. Shortcuts are unique among shared replies.
- **Merge fields:** `{{requester.first_name|there}}`, `{{requester.name}}`, `{{requester.email}}`, `{{customer.name}}`,
  `{{ticket.reference}}`, `{{ticket.subject}}`, `{{ticket.state}}`, `{{agent.name}}`, `{{company.name}}`. A fallback after `|`
  covers an empty value; a field with no value and no fallback is **reported and stops the send** so a blank never
  reaches a customer.
- **Sending:** `reply_with_canned` renders, adds an extra line and the signature if asked, and calls `reply_ticket`, so
  email, first-response stamps, SLA clocks and events behave as for any reply. The reply's own *then* state (pending,
  on hold, resolved) applies unless the agent picks another. Using it counts.
- **Suggestions:** `suggest_canned_replies` ranks visible replies by words shared with the ticket's subject and the
  customer's last message, a matching ticket type first, then popularity.
- **Starters** (on install): greeting, need more details, fixed, sorry for the delay, closing.
- **Screens:** list, new, edit, "Reply with a canned reply" on the ticket (a header button and a *Suggested replies* tab),
  and *My signature*.
