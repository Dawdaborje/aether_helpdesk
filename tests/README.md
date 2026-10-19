# Helpdesk end-to-end checks

Plain Python 3 over HTTP. One script per plugin, all built on `hdlib.py` (sessions, checks, the command line). Every run
uses fresh codes, so any script can run again on the same organization.

## One-time setup

```sh
A="aether -c <your aether.toml>"          # set [tenancy] org_resolution = "header" so anonymous requests name their org
$A --load-plugins plugins/base plugins/helpdesk
$A --create-org "Helpdesk Test" --org-db-name hd_test --username hd_owner --email hd_owner@test.local --password Test-pass-123
$A --install-plugin helpdesk_ticket helpdesk_sla helpdesk_canned helpdesk_kb helpdesk_portal helpdesk_email helpdesk_csat helpdesk_automation --org hd_test
for u in hd_agent1 hd_agent2 hd_mgr hd_cust hd_cust2; do
  $A --create-user --username $u --email $u@test.local --password Test-pass-123 --org-db-name hd_test
done
$A --grant-role helpdesk.hd_agent   --username hd_agent1 --org hd_test
$A --grant-role helpdesk.hd_agent   --username hd_agent2 --org hd_test
$A --grant-role helpdesk.hd_manager --username hd_mgr    --org hd_test
```

`hd_owner` is the organization's administrator; `hd_cust` and `hd_cust2` are plain members (customers). Start the server,
then run any of:

```sh
python3 plugins/helpdesk/tests/helpdesk_ticket_e2e.py     --base http://localhost:7890   # 82 checks
python3 plugins/helpdesk/tests/helpdesk_sla_e2e.py        --base http://localhost:7890   # 35, waits a minute for a clock to run out
python3 plugins/helpdesk/tests/helpdesk_canned_e2e.py     --base http://localhost:7890   # 27
python3 plugins/helpdesk/tests/helpdesk_kb_e2e.py         --base http://localhost:7890   # 43
python3 plugins/helpdesk/tests/helpdesk_portal_e2e.py     --base http://localhost:7890   # 22
python3 plugins/helpdesk/tests/helpdesk_email_e2e.py      --base http://localhost:7890   # 29
python3 plugins/helpdesk/tests/helpdesk_csat_e2e.py       --base http://localhost:7890   # 28 (turns surveys on, then off)
python3 plugins/helpdesk/tests/helpdesk_automation_e2e.py --base http://localhost:7890   # 35
```

Some checks wait for background jobs (event handlers, the visitor intake, mail processing). Pure logic is also covered by
native unit tests: `cargo test --lib` in `helpdesk_sla` (business time), `helpdesk_canned` (merge fields), `helpdesk_kb`
(Markdown, ranking), `helpdesk_email` (mail parsing), `helpdesk_csat` (figures) and `helpdesk_automation` (conditions).

## Pages in a real browser, and the IMAP relay

`helpdesk_ui_check.mjs` opens every page as a manager, a customer and a visitor in headless Chromium (Playwright from
`aether/web`) and reports script errors, failed API calls and pages that show an error. The web app must be built
(`pnpm build` in `aether/web`) because the server serves `web/build`:

```sh
AETHER_URL=http://127.0.0.1:7890 BASE_PATH=/web SHOTS=/some/dir node plugins/helpdesk/tests/helpdesk_ui_check.mjs
```

`python3 plugins/helpdesk/helpdesk_email/relay/test_imap_relay.py` checks the IMAP relay against a fake mailbox and site.
