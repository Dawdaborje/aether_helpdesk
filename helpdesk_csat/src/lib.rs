//! Satisfaction surveys. When a ticket is solved the customer gets a short survey by email, answered from a link with no
//! account; unanswered ones get a reminder and then expire. An answer is written to the ticket as its rating, a low one
//! alerts someone, and reports show satisfaction by agent, team and day.

mod numbers;
mod survey;
