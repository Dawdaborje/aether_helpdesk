#!/usr/bin/env python3
"""Runs imap_relay.py against a tiny fake IMAP server and a fake site. No real mailbox or Aether needed."""
import json
import os
import socketserver
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
MAIL = {
    1: b"From: a@x.test\r\nSubject: first\r\nMessage-ID: <1@x>\r\n\r\nhello\r\n",
    2: b"From: b@x.test\r\nSubject: second\r\nMessage-ID: <2@x>\r\n\r\nworld\r\n",
}
seen = set()
posts = []
reject = {"on": False}


class Imap(socketserver.StreamRequestHandler):
    def reply(self, text):
        self.wfile.write(text.encode() + b"\r\n")

    def handle(self):
        self.reply("* OK ready")
        for line in self.rfile:
            parts = line.decode().strip().split(" ", 2)
            tag, cmd = parts[0], parts[1].upper()
            rest = parts[2] if len(parts) > 2 else ""
            if cmd == "CAPABILITY":
                self.reply("* CAPABILITY IMAP4rev1")
                self.reply(f"{tag} OK done")
            elif cmd == "LOGIN":
                self.reply(f"{tag} OK logged in")
            elif cmd == "SELECT":
                self.reply("* 2 EXISTS")
                self.reply(f"{tag} OK [READ-WRITE] done")
            elif cmd == "UID":
                sub, _, args = rest.partition(" ")
                if sub.upper() == "SEARCH":
                    ids = " ".join(str(i) for i in MAIL if i not in seen)
                    self.reply(f"* SEARCH {ids}".rstrip())
                    self.reply(f"{tag} OK done")
                elif sub.upper() == "FETCH":
                    uid = int(args.split(" ")[0])
                    body = MAIL[uid]
                    self.wfile.write(f"* {uid} FETCH (UID {uid} BODY[] {{{len(body)}}}\r\n".encode() + body + b")\r\n")
                    self.reply(f"{tag} OK done")
                elif sub.upper() == "STORE":
                    seen.add(int(args.split(" ")[0]))
                    self.reply(f"{tag} OK done")
                else:
                    self.reply(f"{tag} BAD")
            elif cmd == "LOGOUT":
                self.reply("* BYE")
                self.reply(f"{tag} OK bye")
                return
            else:
                self.reply(f"{tag} BAD unknown")


class Site(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if reject["on"]:
            self.send_response(503)
            self.end_headers()
            return
        posts.append((self.path, self.headers.get("X-Org-Slug"), body))
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"{}")

    def log_message(self, *a):
        pass


def run(cfg_path):
    env = dict(os.environ, IMAP_PASSWORD="pw", MAILBOX_SECRET="s3cret")
    return subprocess.run([sys.executable, os.path.join(HERE, "imap_relay.py"), "--config", cfg_path, "--once"], env=env, capture_output=True, text=True, timeout=30)


def check(ok, what):
    print(("ok   " if ok else "FAIL ") + what)
    if not ok:
        check.failed += 1


check.failed = 0

imap = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Imap)
imap.daemon_threads = True
site = HTTPServer(("127.0.0.1", 0), Site)
for s in (imap, site):
    threading.Thread(target=s.serve_forever, daemon=True).start()

cfg = f"""
[imap]
host = "127.0.0.1"
port = {imap.server_address[1]}
tls = false
user = "support@x.test"
password_env = "IMAP_PASSWORD"
[aether]
url = "http://127.0.0.1:{site.server_address[1]}"
org = "acme"
secret_env = "MAILBOX_SECRET"
"""
path = os.path.join(tempfile.mkdtemp(), "relay.toml")
open(path, "w").write(cfg)

reject["on"] = True
r = run(path)
check(r.returncode == 0 and not seen, "a site error leaves mail unread")
reject["on"] = False
r = run(path)
check(len(posts) == 2 and seen == {1, 2}, "both messages are delivered and marked read")
check(posts and posts[0][0] == "/api/plugins/helpdesk_email/email_inbound" and posts[0][1] == "acme", "path and org header")
check(posts and posts[0][2]["secret"] == "s3cret" and "Subject: first" in posts[0][2]["raw"], "secret and raw message are sent")
n = len(posts)
run(path)
check(len(posts) == n, "read mail is not sent again")
print("failed:", check.failed)
sys.exit(1 if check.failed else 0)
