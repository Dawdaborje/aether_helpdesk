# Knowledge base (`helpdesk_kb`)

Help articles for the public, for customers and for agents. Rust → WASM. Depends on `helpdesk_ticket`, `helpdesk`, `company`.

## Models

`hd_kb_category` (topic tree, address), `hd_kb_article` (Markdown text, summary, tags, state, audience, version, views,
helpful counts, a ticket type it suits, the ticket it came from), `hd_kb_revision` (every earlier version), `hd_kb_feedback`
(one vote per signed-in person, changeable), `hd_kb_use` (articles sent to or written from tickets), `hd_kb_search`
(what was searched and how many answers it found).

## Life of an article

`draft → review → published → archived`. Agents write and edit their own drafts (managers edit any), send for review; a
manager publishes, unpublishes, archives and features. Changing the title, summary or text saves the old version and
raises the version number; any version can be restored (the current one is kept first).

## Who reads what

An article is **public**, for **customers** (signed-in members) or for **agents**. Readers see published articles meant for
them; agents see every state. Hidden articles answer "there is no such article", not "forbidden". The public functions
(`kb_articles`, `kb_categories`, `kb_article`, `kb_related`, `kb_popular`, `kb_suggest`, `kb_mark_helpful`,
`kb_mark_unhelpful`, `kb_feedback`) are open to anonymous visitors and filter by audience themselves.

## Search

Words in the title count most, then tags, summary and body, with a bonus for a phrase in the title, a nudge for articles
readers found helpful and for featured ones. A search that finds nothing is remembered; `kb_gaps` and the overview list
the most frequent misses, so you know what to write. `kb_suggest` does the same ranking for a request being written
(deflection).

## With tickets

`suggest_articles` ranks published articles for a ticket by its subject and the customer's last message (its type
first); `reply_with_article` sends an article as a reply (title, summary and a link built from the site address in the
helpdesk settings); `article_from_ticket` starts a draft from the problem and the resolution or last answer.

## Text

Markdown is converted by `get_kb_article` to HTML (`body_html`) after escaping everything, so an article cannot carry its
own markup; only web, mail and site-relative links are made.

## Screens

Public: `/help` (search), `/help/topics`, `/help/topic/{topic}`, `/help/{article}` with *This helped* / *This did not
help*. Desk: overview (counts, most read, searches with no answer), articles (filter, send for review, publish, archive),
article (versions), topics, and on every ticket a *Send article* button, a *Write article* button and an *Articles* tab.
