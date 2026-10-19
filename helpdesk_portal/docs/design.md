# Customer portal (`helpdesk_portal`)

The customer side of the helpdesk. Rust → WASM. Depends on `helpdesk_ticket`, `helpdesk_kb`, `helpdesk`, `company`.
It keeps **no data of its own**: every function passes the call to the ticket or knowledge-base plugin as the caller, so
a visitor can do only what those plugins allow visitors, and a signed-in customer only what they could do there.

## Flow

1. `/support` is the help centre's search (public). People look for an answer first; **Ask for help** is on the same page.
2. `/support/new` takes a subject, kind, description and contact details. A visitor gets a **reference and a secret
   key** (also in the acknowledgement email when the site address is set in the helpdesk settings); a signed-in customer
   gets the reference only.
3. `/support/track/{reference}/{key}` shows the request and its conversation (public replies only, never internal
   notes or system lines), with *Write to us*, *Rate our help* (once solved) and *It is not solved*. For a signed-in
   customer the key is `-`: the reference alone opens their own request; nobody else's.
4. `/support/find` opens a request from a reference and key; `/support/my` lists a signed-in customer's requests.

## Access

Anonymous visitors reach the portal functions in `public_functions`; each forwards to a ticket function that checks the
key. A wrong key, another customer's reference and a closed request all answer with a plain sentence. The portal tells
a visitor that a request does not exist or the key is wrong, never which.

## Functions

`portal_info`, `portal_articles`, `portal_ticket_types`, `portal_submit`, `portal_find`, `portal_view`, `portal_messages`,
`portal_reply`, `portal_rate`, `portal_reopen`, `portal_my`. The same function serves keyed and signed-in access.

## Notes

- A visitor's ticket is completed by a background job in the ticket plugin (company, priority, team, owner, the thank-you
  email), so submitting is quick and cannot be slowed by assignment.
- One address may open five requests an hour; a hidden field in the API (`website`) catches simple bots.
- The article body is shown as Markdown text; `kb_article` also returns `body_html`, escaped, for a front end that
  renders HTML.
