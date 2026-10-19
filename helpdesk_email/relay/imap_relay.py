#!/usr/bin/env python3
"""Fetch mail from an IMAP mailbox and post it to the helpdesk_email plugin.

Aether receives mail only by being posted to (see docs/design.md); this is the small relay in front of a mail server
that has no inbound webhook. It uses nothing but the Python standard library.

    imap_relay.py --config relay.toml            # keep polling
    imap_relay.py --config relay.toml --once     # one pass, for cron

relay.toml:

    [imap]
    host = "imap.example.com"
    port = 993                 # 993 = TLS (default); 143 with starttls = true
    starttls = false           # true: connect plain, then upgrade
    tls = true                 # false: no encryption at all (only for a mail server on the same machine)
    user = "support@example.com"
    password_env = "IMAP_PASSWORD"   # the password is read from this environment variable (or set `password`)
    folder = "INBOX"
    move_to = ""               # optional folder for delivered mail; otherwise it is only marked as read
    poll_seconds = 60
    max_bytes = 20000000       # larger messages are marked read and skipped

    [aether]
    url = "https://help.example.com"          # the site
    org = "acme"                              # sent as X-Org-Slug (needed when organizations are told apart by header)
    secret_env = "MAILBOX_SECRET"             # the mailbox secret from helpdesk_email (or set `secret`)

Only unread messages are fetched. A message is marked read (or moved) after the site accepted it, so a failure leaves it
for the next pass; the site drops repeats by Message-ID, so resending is harmless.
"""
import argparse
import imaplib
import json
import os
import re
import sys
import time
import tomllib
import urllib.error
import urllib.request


def log(msg):
    print(time.strftime("%Y-%m-%d %H:%M:%S"), msg, flush=True)


def connect(cfg):
    port = int(cfg.get("port", 993))
    if cfg.get("tls", True) is False or cfg.get("starttls") or port == 143:
        conn = imaplib.IMAP4(cfg["host"], port)
        if cfg.get("starttls", port == 143):
            conn.starttls()
    else:
        conn = imaplib.IMAP4_SSL(cfg["host"], port)
    password = cfg.get("password") or os.environ.get(cfg.get("password_env", ""), "")
    if not password:
        raise SystemExit("no IMAP password: set password_env to an environment variable that holds it")
    conn.login(cfg["user"], password)
    return conn


def post(site, raw):
    secret = site.get("secret") or os.environ.get(site.get("secret_env", ""), "")
    if not secret:
        raise SystemExit("no mailbox secret: set secret_env to an environment variable that holds it")
    body = json.dumps({"raw": raw.decode("utf-8", "replace"), "secret": secret}).encode()
    headers = {"Content-Type": "application/json"}
    if site.get("org"):
        headers["X-Org-Slug"] = site["org"]
    url = site["url"].rstrip("/") + "/api/plugins/helpdesk_email/email_inbound"
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.status


def pass_once(cfg):
    imap_cfg, site = cfg["imap"], cfg["aether"]
    max_bytes = int(imap_cfg.get("max_bytes", 20_000_000))
    move_to = imap_cfg.get("move_to") or ""
    conn = connect(imap_cfg)
    sent = failed = skipped = 0
    try:
        status, _ = conn.select(imap_cfg.get("folder", "INBOX"))
        if status != "OK":
            raise RuntimeError("cannot open the folder")
        status, data = conn.uid("SEARCH", None, "UNSEEN")
        uids = data[0].split() if status == "OK" and data and data[0] else []
        for uid in uids:
            # PEEK keeps the message unread until the site has it.
            status, parts = conn.uid("FETCH", uid, "(BODY.PEEK[])")
            raw = next((p[1] for p in parts or [] if isinstance(p, tuple)), None)
            if status != "OK" or raw is None:
                failed += 1
                continue
            if len(raw) > max_bytes:
                log(f"uid {uid.decode()} is {len(raw)} bytes, over the limit: skipped")
                skipped += 1
            else:
                try:
                    post(site, raw)
                except urllib.error.HTTPError as e:
                    # Left unread: a wrong secret is fixed by the operator, a server error clears by itself.
                    log(f"uid {uid.decode()}: the site answered {e.code}")
                    failed += 1
                    continue
                except (urllib.error.URLError, TimeoutError, OSError) as e:
                    log(f"uid {uid.decode()}: the site could not be reached ({e})")
                    failed += 1
                    continue
                sent += 1
            conn.uid("STORE", uid, "+FLAGS", "(\\Seen)")
            if move_to:
                conn.uid("COPY", uid, move_to)
                conn.uid("STORE", uid, "+FLAGS", "(\\Deleted)")
        if move_to:
            conn.expunge()
    finally:
        try:
            conn.logout()
        except Exception:
            pass
    return sent, failed, skipped


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--config", required=True)
    ap.add_argument("--once", action="store_true")
    args = ap.parse_args()
    with open(args.config, "rb") as f:
        cfg = tomllib.load(f)
    delay = int(cfg["imap"].get("poll_seconds", 60))
    while True:
        try:
            sent, failed, skipped = pass_once(cfg)
            if sent or failed or skipped:
                log(f"delivered {sent}, failed {failed}, skipped {skipped}")
        except (imaplib.IMAP4.error, OSError, RuntimeError) as e:
            log(f"mailbox error: {e}")
            if args.once:
                return 1
        if args.once:
            return 0
        time.sleep(delay)


if __name__ == "__main__":
    sys.exit(main())
