# helpdesk

## Purpose
Masters for the helpdesk suite: teams and agents, priorities, ticket types, tags and per-company settings.

## Dependencies
`party`, `company`, `number_sequence`.

## Models
`hd_team` (assignment: manual / round_robin / load_balanced), `hd_team_member` (user, capacity), `hd_priority` (level 1–4),
`hd_ticket_type`, `hd_tag`, `hd_settings` (one per company: default team and priority, auto-close days, CSAT on/off, acknowledgement, **restrict agents to their teams**, the site's public address for emailed links, a reply signature and the business name).

## Functions
`create_/set_/get_/search_` for each master, `add_team_member`, `remove_team_member` (deactivates), `list_team_members`,
`set_settings`, `get_settings` (id = company id), `my_teams` (the teams the caller belongs to), `public_ticket_types` (open to visitors: the kinds of request they can pick), `seed_helpdesk` (idempotent, on install and upgrade).

## Roles
Any signed-in member reads; `hd_admin` writes. Other plugins write through `via:*`.

## Boundaries
No tickets here; `helpdesk_ticket` owns them and links these masters.
