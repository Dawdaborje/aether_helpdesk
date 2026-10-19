# Email (`helpdesk_email`)

Mail into the helpdesk. Rust → WASM. Depends on `helpdesk_ticket` and `helpdesk`.

## Why it is shaped like this

Aether sends mail (`communication::send`) but has no connection that fetches it. So mail **arrives by being posted**: a
provider's inbound webhook, a small relay in front of any mail server, or a command that pipes a whole raw message. This
plugin is the secure, forgiving place where those posts land.

## The door

`email_inbound` is open to anyone, and refuses everything without a valid **mailbox secret**: the secret, sent in a
`secret` field (or an `X-Aether-Secret` header of the message), *names* the mailbox the message is for. Nothing is kept
for a wrong secret and the answer never says why. A message is stored in the inbox and handled by a background job, so
the provider gets its answer at once and a slow ticket plugin cannot lose mail.

Accepted shapes (all JSON): a generic object (`from`, `to`, `subject`, `text`, `message_id`, `in_reply_to`, `references`,
`headers`), Resend's `email.received` event, Postmark's inbound JSON, and `{ "raw": "<whole RFC 822 message>" }`, which
is parsed here (headers, folded lines, encoded words, multipart, quoted-printable, base64, HTML-only mail).

## What happens to a message

1. **Block list** (an address, or `@domain.com` for a domain and its subdomains): dropped and counted.
2. **Spam flag** from the mail server, **mail from our own mailboxes** (a loop), and **floods** (20 processed messages
   from one sender in an hour): dropped and recorded with the reason.
3. A reply is **cut down to what the person just wrote** (the quoted conversation, "On … wrote:", Outlook headers and the
   signature go; if nothing is left the whole text is kept).
4. The ticket plugin's `ingest_email` threads it (reference in the subject, or the message it answers), opens a ticket
   routed to the mailbox's team, type and priority, ignores automatic replies, and starts a follow-up when the ticket is
   closed. Replies leave from the mailbox's address (`reply_to`).

Every message stays in the inbox with its state (waiting, done, ignored, blocked, failed), what happened and the ticket
it ended in. A failure is retried every ten minutes up to five times and can be retried by hand.

## Models

`hd_mailbox` (address, team / type / priority, secret, counters), `hd_email_block`, `hd_email_inbox`.

## Functions

Managers: `create_mailbox`, `update_mailbox`, `rotate_mailbox_secret`, `add_email_block`, `remove_email_block`.
Agents: `list_mailboxes` (without secrets), `get_mailbox`, `list_email_blocks`, `list_email_inbox`, `email_overview`,
`retry_email`, `email_ingest` (feed a message in by hand through the same checks). Public: `email_inbound`.

## Setting it up

1. Create a mailbox for the address customers write to; copy its secret.
2. Point the provider's inbound webhook (or your relay) at `/api/plugins/helpdesk_email/email_inbound` on the site, adding
   the secret to each message.
3. Set the site's public address in the helpdesk settings so emails carry working links.

## Fetching from an IMAP mailbox

For a mail server with no inbound webhook, `relay/imap_relay.py` (Python standard library only) polls a mailbox and posts
every unread message, whole, to `email_inbound` with the mailbox secret. A message is marked read (or moved to another
folder) only after the site accepted it, so a failure leaves it for the next pass; repeats are dropped by Message-ID. It
runs as a service (`imap_relay.py --config relay.toml`) or from cron (`--once`). The options are described at the top of
the script, and `relay/test_imap_relay.py` checks it against a fake mailbox and site.
