//! Time agents spend on tickets.

use aether_sdk::prelude::*;

use crate::common::*;

#[derive(Deserialize)]
struct Log {
    #[serde(alias = "id")]
    ticket: String,
    minutes: i64,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    billable: bool,
    #[serde(default)]
    logged_on: Option<String>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

handler! {
    fn log_ticket_time(input: Log) -> Record {
        require_agent()?;
        if input.minutes <= 0 || input.minutes > 24 * 60 {
            return Err(Error::msg("log between 1 minute and 24 hours"));
        }
        get_ticket_row(&input.ticket)?;
        let day = match input.logged_on.as_deref().filter(|d| !d.is_empty()) {
            Some(d) => aether_sdk::dates::format_date(aether_sdk::dates::parse_date(d)?),
            None => aether_sdk::records::today()?,
        };
        db::create(
            "hd_time_entry",
            &json!({ "ticket": input.ticket, "agent": actor()?, "minutes": input.minutes, "logged_on": day, "note": opt(input.note), "billable": input.billable }),
        )
    }

    fn list_ticket_time(input: Id) -> Vec<Record> {
        require_agent()?;
        db::find("hd_time_entry").filter("ticket", input.id.as_str()).order_by("-logged_on").limit(1000).all()
    }

    fn delete_time_entry(input: Id) -> Option<Record> {
        require_agent()?;
        let entry = require("hd_time_entry", &input.id, "time entry")?;
        if text(&entry, "agent") != actor()?.as_deref() && !is_manager()? {
            return Err(Error::msg("only the person who logged it, or a manager, removes time"));
        }
        db::delete("hd_time_entry", &input.id)
    }
}
