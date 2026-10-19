# Shared by the helpdesk end-to-end scripts: sessions, checks and the command line.
import argparse, json, re, secrets, sys, time, urllib.error, urllib.request

ap = argparse.ArgumentParser()
ap.add_argument("--base", default="http://localhost:7890")
ap.add_argument("--org", default="hd_test")
ap.add_argument("--password", default="Test-pass-123")
ap.add_argument("-v", action="store_true")
args = ap.parse_args()
RUN = secrets.token_hex(3).upper()
DEFAULT_PLUGIN = "helpdesk_ticket"


class Failed(Exception):
    def __init__(self, status, data):
        super().__init__(f"{status}: {json.dumps(data)[:400]}")
        self.status, self.data = status, data


class Session:
    def __init__(self, username):
        self.cookies = {"aether_csrf": secrets.token_hex(16)}
        self.name = username
        if username is None:  # an anonymous visitor
            self.user = None
            return
        who = self.post("/api/auth/login", {"username": username, "password": args.password})
        key = re.search(r'String\("([^"]+)"\)', who["user"]["id"])
        self.user = f"users:{key.group(1)}" if key else who["user"]["id"]
        self.post("/api/auth/org", {"org": args.org})

    def post(self, path, body):
        r = urllib.request.Request(args.base + path, data=json.dumps(body).encode(), method="POST")
        r.add_header("content-type", "application/json")
        r.add_header("origin", args.base)
        r.add_header("x-org-slug", args.org)
        r.add_header("x-csrf-token", self.cookies["aether_csrf"])
        r.add_header("cookie", "; ".join(f"{k}={v}" for k, v in self.cookies.items()))
        try:
            resp = urllib.request.urlopen(r)
            status, raw, headers = resp.status, resp.read(), resp.headers
        except urllib.error.HTTPError as e:
            status, raw, headers = e.code, e.read(), e.headers
        for c in headers.get_all("set-cookie") or []:
            k, _, rest = c.partition("=")
            self.cookies[k.strip()] = rest.split(";")[0]
        try:
            data = json.loads(raw) if raw else None
        except ValueError:
            data = {"_raw": raw.decode(errors="replace")}
        if status >= 400:
            raise Failed(status, data)
        return data

    def call(self, fn, body=None, plugin=None):
        plugin = plugin or DEFAULT_PLUGIN
        out = self.post(f"/api/plugins/{plugin}/{fn}", body or {})
        if args.v:
            print(f"    {self.name} {plugin}.{fn} -> {json.dumps(out)[:300]}")
        return out


results = []


def check(name, cond, detail=""):
    results.append((name, bool(cond)))
    print(("  ok    " if cond else "  FAIL  ") + name + ("" if cond else f"  [{str(detail)[:300]}]"))


def refused(name, action, words=()):
    try:
        action()
    except Failed as f:
        msg = json.dumps(f.data)
        check(name, not words or any(w in msg for w in words), msg[:300])
        return
    check(name, False, "was allowed")


def rows(page):
    return page["rows"] if isinstance(page, dict) else page




def finish():
    bad = [n for n, ok in results if not ok]
    print(f"\n{len(results) - len(bad)}/{len(results)} checks passed")
    sys.exit(1 if bad else 0)
