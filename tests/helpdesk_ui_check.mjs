// Opens every helpdesk page in a real browser and reports script errors, failed requests and pages that show an error.
// Needs the setup in README.md and the server running; the web app is either served by the server or by `vite dev`.
//
//   AETHER_URL=http://127.0.0.1:5199 BASE_PATH=/web node helpdesk_ui_check.mjs      (SHOTS=/dir keeps screenshots)
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.join(here, '../../../aether/web/package.json'));
const { chromium } = require('playwright');

const BASE = process.env.AETHER_URL ?? 'http://127.0.0.1:17893';
const PREFIX = process.env.BASE_PATH ?? '';
const ORG = process.env.ORG ?? 'hd_test';
const PASSWORD = 'Test-pass-123';
const SHOTS = process.env.SHOTS ?? '';
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? '/usr/bin/chromium', headless: true, args: ['--no-sandbox'] });

async function login(username) {
	const context = await browser.newContext({ viewport: { width: 1280, height: 860 }, extraHTTPHeaders: { 'X-Org-Slug': ORG } });
	if (!username) return { context, call: null };
	// The CSRF cookie is whatever the client picks; the server only compares it with the header.
	let csrf = 'c'.repeat(32);
	await context.addCookies([{ name: 'aether_csrf', value: csrf, url: BASE }]);
	const headers = async () => ({ 'X-CSRF-Token': (await context.cookies()).find((c) => c.name === 'aether_csrf')?.value ?? csrf, Origin: BASE });
	const r = await context.request.post(BASE + '/api/auth/login', { data: { username, password: PASSWORD }, headers: await headers() });
	if (!r.ok()) throw new Error(`login ${username}: ${r.status()}`);
	await context.request.post(BASE + '/api/auth/org', { data: { org: ORG }, headers: await headers() });
	const call = async (plugin, fn, data = {}) => {
		const res = await context.request.post(`${BASE}/api/plugins/${plugin}/${fn}`, { data, headers: await headers() });
		if (!res.ok()) throw new Error(`${plugin}.${fn}: ${res.status()} ${await res.text()}`);
		return res.json();
	};
	return { context, call };
}
const rows = (d) => (Array.isArray(d) ? d : (d?.rows ?? d?.items ?? []));

const mgr = await login('hd_mgr');
const first = async (plugin, fn) => rows(await mgr.call(plugin, fn, { limit: 5 }))[0];
const ids = {
	ticket: (await first('helpdesk_ticket', 'list_tickets'))?.id,
	macro: (await first('helpdesk_automation', 'list_macros'))?.id,
	rule: (await first('helpdesk_automation', 'list_rules'))?.id,
	canned: (await first('helpdesk_canned', 'list_canned_replies'))?.id,
	survey: (await first('helpdesk_csat', 'list_surveys'))?.id,
	mailbox: (await first('helpdesk_email', 'list_mailboxes'))?.id,
	article: (await first('helpdesk_kb', 'list_kb_articles'))?.id,
	hours: (await first('helpdesk_sla', 'list_business_hours'))?.id,
	policy: (await first('helpdesk_sla', 'list_sla_policies'))?.id
};
const published = rows(await mgr.call('helpdesk_kb', 'list_kb_articles', { state: 'published', limit: 50 })).find((a) => a.state === 'published' && a.visibility === 'public');
const slug = published?.slug;
const topic = rows(await mgr.call('helpdesk_kb', 'list_kb_categories'))[0];
const trail = ids.ticket ? await mgr.call('helpdesk_ticket', 'get_ticket', { id: ids.ticket }) : null;
console.log('ids', JSON.stringify({ ...ids, slug, topic: topic?.slug, reference: trail?.reference }));

const key = (id) => String(id).split(':').pop();
const agentPages = [
	'/helpdesk', '/helpdesk/overview', '/helpdesk/tickets', '/helpdesk/tickets/mine', '/helpdesk/tickets/unassigned', '/helpdesk/board', '/helpdesk/tickets/new',
	`/helpdesk/tickets/${ids.ticket}`, `/helpdesk/tickets/${ids.ticket}/reply`, `/helpdesk/tickets/${ids.ticket}/note`, `/helpdesk/tickets/${ids.ticket}/assign`,
	`/helpdesk/tickets/${ids.ticket}/resolve`, `/helpdesk/tickets/${ids.ticket}/time`, `/helpdesk/tickets/${ids.ticket}/merge`, `/helpdesk/tickets/${ids.ticket}/subticket`,
	`/helpdesk/tickets/${ids.ticket}/canned`, `/helpdesk/tickets/${ids.ticket}/macro`, `/helpdesk/tickets/${ids.ticket}/article`,
	'/helpdesk/setup', '/helpdesk/priorities', '/helpdesk/priorities/new', '/helpdesk/tags', '/helpdesk/tags/new', '/helpdesk/teams', '/helpdesk/teams/new', '/helpdesk/ticket-types', '/helpdesk/ticket-types/new',
	'/helpdesk/sla', '/helpdesk/sla/clocks', '/helpdesk/sla/hours', '/helpdesk/sla/hours/new', `/helpdesk/sla/hours/${ids.hours}`, `/helpdesk/sla/hours/${ids.hours}/day`, `/helpdesk/sla/hours/${ids.hours}/holiday`,
	'/helpdesk/sla/policies', '/helpdesk/sla/policies/new', `/helpdesk/sla/policies/${ids.policy}`, `/helpdesk/sla/policies/${ids.policy}/target`,
	'/helpdesk/canned', '/helpdesk/canned/new', `/helpdesk/canned/${ids.canned}`, '/helpdesk/signature',
	'/helpdesk/kb', '/helpdesk/kb/articles', '/helpdesk/kb/articles/new', `/helpdesk/kb/articles/${ids.article}`, '/helpdesk/kb/topics', '/helpdesk/kb/topics/new',
	'/helpdesk/email', '/helpdesk/email/inbox', '/helpdesk/email/mailboxes', '/helpdesk/email/mailboxes/new', `/helpdesk/email/mailboxes/${ids.mailbox}`, '/helpdesk/email/blocked', '/helpdesk/email/blocked/new',
	'/helpdesk/csat', '/helpdesk/csat/responses', '/helpdesk/csat/surveys', '/helpdesk/csat/surveys/new', `/helpdesk/csat/surveys/${ids.survey}`,
	'/helpdesk/automation', '/helpdesk/automation/log', '/helpdesk/automation/macros', '/helpdesk/automation/macros/new', `/helpdesk/automation/macros/${ids.macro}`, `/helpdesk/automation/macros/${ids.macro}/action`,
	'/helpdesk/automation/rules/new', `/helpdesk/automation/rules/${ids.rule}`, `/helpdesk/automation/rules/${ids.rule}/action`, `/helpdesk/automation/rules/${ids.rule}/condition`, `/helpdesk/automation/rules/${ids.rule}/test`
];
const customerPages = ['/support', '/support/my', '/support/new', '/support/find', '/help', '/help/topics', slug && `/help/${slug}`, topic && `/help/topic/${topic.slug}`];
const guestPages = ['/support', '/support/new', '/support/find', '/help', '/help/topics', slug && `/help/${slug}`];

const problems = [];
async function visit(who, pages, user) {
	const { context } = user;
	for (const route of pages.filter(Boolean)) {
		if (route.includes('undefined')) { problems.push([who, route, 'no data to open it with']); continue; }
		const page = await context.newPage();
		const issues = [];
		page.on('pageerror', (e) => issues.push('script error: ' + e.message));
		page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) issues.push('console: ' + m.text().slice(0, 200)); });
		// A visitor is not signed in, so the app's own questions about the session are refused, as they should be.
		const quiet = (url) => who === 'guest' && /\/api\/(auth\/(me|orgs)|ui\/prefs)/.test(url);
		page.on('response', (r) => { if (r.status() >= 400 && r.url().includes('/api/') && !quiet(r.url())) issues.push(`${r.status()} ${new URL(r.url()).pathname}`); });
		try {
			await page.goto(BASE + PREFIX + route, { waitUntil: 'load', timeout: 30000 });
			await page.waitForLoadState('networkidle', { timeout: 4000 }).catch(() => {});
			await page.waitForTimeout(1200);
			const text = (await page.locator('body').innerText()).replace(/\s+/g, ' ').trim();
			if (/page not found|something went wrong|forbidden|not allowed|unexpected error|undefined|\[object/i.test(text)) issues.push('page says: ' + text.slice(0, 160));
			if (text.length < 40) issues.push('page looks empty: ' + text);
			if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `${who}${route.replace(/[^a-z0-9]+/gi, '_')}.png`) });
		} catch (e) {
			issues.push('did not load: ' + e.message.split('\n')[0]);
		}
		console.log((issues.length ? 'FAIL ' : 'ok   ') + who + ' ' + route + (issues.length ? '\n       ' + [...new Set(issues)].join('\n       ') : ''));
		if (issues.length) problems.push([who, route, issues.join('; ')]);
		await page.close();
	}
}
await visit('manager', agentPages, mgr);
await visit('customer', customerPages, await login('hd_cust'));
await visit('guest', guestPages, await login(null));
await browser.close();
console.log(`\n${problems.length} page(s) with problems`);
process.exit(problems.length ? 1 : 0);
