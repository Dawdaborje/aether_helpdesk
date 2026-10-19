# Satisfaction (`helpdesk_csat`)

Customer satisfaction surveys. Rust → WASM. Depends on `helpdesk_ticket` and `helpdesk`.

## Flow

1. A ticket is **resolved** (or **closed**, per survey) and the company has `csat_enabled` in the helpdesk settings: the
   plugin picks a survey, creates a response with an unguessable **token**, and emails the customer a link
   (`<site address>/support/rate/{token}`, using `portal_url` from the settings).
2. The customer opens the link (no account), gives 1–5 and an optional comment (`csat_get`, `csat_submit`, open to
   visitors; the token is the key).
3. A background job writes the rating to the ticket (`rate_ticket`, so `ticket_rated` is announced), announces
   `survey_answered`, and for a rating at or below the survey's alert level notifies the agent and a named person and
   announces `ticket_low_rating` for automation to react to.
4. Every hour: one **reminder** after `reminder_days`, and surveys past `expires_days` become *expired* and refuse answers.

## Choosing and limiting

- The most specific active survey that fits wins (a team and type match beats a team match beats the default).
- Not asked when the ticket has no requester email, is spam, already has an open survey, or the same person was asked within the
  survey's cooldown. A survey can **wait** (`delay_minutes`); if the ticket was reopened meanwhile it is skipped.
- Agents can ask by hand (`send_survey_now`), ignoring the settings and cooldown but not an open survey.

## Privacy

The token is the customer's key. Agents see responses without it; only managers do (to resend a link). Answers are final.

## Reports

`csat_overview` (days): surveys sent, answered, answer rate, average, satisfied share (4 and 5), spread of ratings,
results by agent and by team, answers per day, and recent low ratings with comments. Pieces for charts: `csat_by_agent`,
`csat_distribution`, `csat_trend`, `csat_low_ratings`.

## Screens

Satisfaction (dashboard), Responses, Surveys (new / edit), the public rating page `/support/rate/{token}`, and on a
ticket a *Satisfaction* tab and an *Ask for rating* button.
