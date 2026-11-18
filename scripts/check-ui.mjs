// Headless verification of the unified list, account detail tabs and import flow.

import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import http from 'node:http'
import { dirname, extname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { resolveTheme } from '../frontend/node_modules/@codex-proxy/ui/dist/theme/index.js'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const distDir = join(root, 'frontend/dist')
const outDir = process.env.SCREENSHOT_DIR || '/tmp/cpr-chatgpt-account-panel-screenshots'
const chromeBin = process.env.CHROME_BIN
  || '/home/huangtao/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux64/chrome-headless-shell'

const themes = Object.fromEntries(['light', 'dark'].map(mode => [mode, {
  ...resolveTheme(mode, 'relay-blue', '#3b82f6').tokens,
  '--cp-font-family': 'system-ui,sans-serif',
}]))

const now = Date.UTC(2026, 10, 6, 8, 0, 0)
const dayMs = 86400_000

const mockAccounts = [
  {
    id: 'acc-work',
    name: 'team-alpha',
    email: 'alpha@acme.io',
    plan_type: 'team',
    kind: 'team',
    enabled: true,
    credential_state: 'ready',
    access_token_expires_at_ms: now + 6 * dayMs,
    quota: {
      windows: [
        { window_seconds: 18000, used_percent: 42, reset_at_ms: now + 7200_000 },
        { window_seconds: 604800, used_percent: 78, reset_at_ms: now + 3 * dayMs },
      ],
    },
    usage: {
      checked_at_ms: now - 180_000,
      days: 7,
      work: {
        total: 18.4,
        share: 0.34,
        days_with_use: 4,
        last_day: '2026-11-05',
        surfaces: [
          { key: 'work_web', value: 12.1 },
          { key: 'work_desktop', value: 6.3 },
        ],
      },
      surfaces: [
        { key: 'cli', value: 22.0 },
        { key: 'work_web', value: 12.1 },
        { key: 'vscode', value: 9.8 },
        { key: 'work_desktop', value: 6.3 },
        { key: 'desktop_app', value: 3.9 },
      ],
      total: 58.0,
      daily_tokens: [
        { date: '2026-10-31', tokens: 820000 },
        { date: '2026-11-01', tokens: 1430000 },
        { date: '2026-11-02', tokens: 0 },
        { date: '2026-11-03', tokens: 2610000 },
        { date: '2026-11-04', tokens: 1180000 },
        { date: '2026-11-05', tokens: 1960000 },
        { date: '2026-11-06', tokens: 540000 },
      ],
      windows: [[300, 42], [10080, 78]],
      problems: [],
    },
    payment: {
      status: 'past_due',
      created_at_ms: now - 3 * dayMs,
      checked_at_ms: now - 600_000,
      subscription_status: 'past_due',
      delinquent_since_ms: now - 3 * dayMs,
      grace_ends_at_ms: Date.now() + 4 * dayMs,
    },
    blocker: null,
  },
  {
    id: 'acc-codex',
    name: 'plus-codex-only',
    email: 'dev@PersonalLongDomainExample.org',
    plan_type: 'plus',
    kind: 'personal',
    enabled: true,
    credential_state: 'ready',
    access_token_expires_at_ms: now + 9 * dayMs,
    quota: {
      windows: [
        { window_seconds: 18000, used_percent: 18, reset_at_ms: now + 9000_000 },
        { window_seconds: 604800, used_percent: 93, reset_at_ms: now + dayMs },
      ],
    },
    usage: {
      checked_at_ms: now - 3600_000,
      days: 7,
      work: { total: 0, share: 0, days_with_use: 0, surfaces: [] },
      surfaces: [
        { key: 'cli', value: 64 },
        { key: 'vscode', value: 21 },
        { key: 'exec', value: 8 },
      ],
      total: 93,
      daily_tokens: [
        { date: '2026-10-31', tokens: 3100000 },
        { date: '2026-11-01', tokens: 2900000 },
        { date: '2026-11-02', tokens: 4200000 },
        { date: '2026-11-03', tokens: 3800000 },
        { date: '2026-11-04', tokens: 5100000 },
        { date: '2026-11-05', tokens: 4700000 },
        { date: '2026-11-06', tokens: 2300000 },
      ],
      windows: [[300, 18], [10080, 93]],
      problems: [],
    },
    payment: {
      status: 'paid',
      created_at_ms: now - 12 * dayMs,
      checked_at_ms: now - 3600_000,
    },
    blocker: null,
  },
  {
    id: 'acc-expired-saved',
    name: 'expired-with-saved-pw',
    email: 'saved-pw@example.com',
    plan_type: 'pro',
    kind: 'personal',
    enabled: true,
    credential_state: 'expired',
    access_token_expires_at_ms: now - 2 * dayMs,
    quota: {
      windows: [
        { window_seconds: 18000, used_percent: 5 },
        { window_seconds: 604800, used_percent: 12 },
      ],
    },
    usage: null,
    payment: null,
    blocker: {
      code: 'token_expired',
      message: '访问令牌已过期，需要重新登录',
    },
  },
  {
    id: 'acc-expired',
    name: 'expired-account',
    email: 'old@example.com',
    plan_type: 'plus',
    kind: 'personal',
    enabled: true,
    credential_state: 'expired',
    access_token_expires_at_ms: now - dayMs,
    quota: null,
    usage: null,
    payment: null,
    blocker: {
      code: 'token_expired',
      message: '访问令牌已过期，请先重新登录',
    },
  },
]

const mockUsageDetail = {
  checked_at_ms: now,
  days: 7,
  limits: {
    plan_type: 'team',
    limit_reached: false,
    windows: [
      { name: 'primary', window_minutes: 300, used_percent: 42, resets_at_ms: now + 7200_000 },
      { name: 'secondary', window_minutes: 10080, used_percent: 78, resets_at_ms: now + 3 * dayMs },
    ],
  },
  tokens: {
    daily: [
      { date: '2026-10-31', tokens: 120_000 },
      { date: '2026-11-01', tokens: 340_000 },
      { date: '2026-11-02', tokens: 80_000 },
      { date: '2026-11-03', tokens: 590_000 },
      { date: '2026-11-04', tokens: 410_000 },
      { date: '2026-11-05', tokens: 730_000 },
      { date: '2026-11-06', tokens: 260_000 },
    ],
    lifetime: 18_400_000,
    threads: 142,
  },
  breakdown: {
    unit: 'relative',
    updated_at_ms: now - 300_000,
    days: [
      { date: '2026-10-31', total: 6.2, surfaces: { cli: 4.0, vscode: 2.2 }, features: {}, models: {} },
      { date: '2026-11-01', total: 9.5, surfaces: { cli: 3.1, work_web: 4.4, vscode: 2.0 }, features: {}, models: {} },
      { date: '2026-11-02', total: 3.0, surfaces: { cli: 2.0, desktop_app: 1.0 }, features: {}, models: {} },
      { date: '2026-11-03', total: 11.4, surfaces: { cli: 4.2, work_web: 4.2, work_desktop: 3.0 }, features: {}, models: {} },
      { date: '2026-11-04', total: 7.8, surfaces: { cli: 3.8, vscode: 2.5, desktop_app: 1.5 }, features: {}, models: {} },
      { date: '2026-11-05', total: 10.6, surfaces: { cli: 2.9, work_web: 3.5, work_desktop: 3.3, vscode: 0.9 }, features: {}, models: {} },
      { date: '2026-11-06', total: 5.6, surfaces: { cli: 2.0, vscode: 2.2, desktop_app: 1.4 }, features: {}, models: {} },
    ],
    surfaces: [
      { key: 'cli', value: 22.0 },
      { key: 'work_web', value: 12.1 },
      { key: 'vscode', value: 9.8 },
      { key: 'work_desktop', value: 6.3 },
      { key: 'desktop_app', value: 3.9 },
    ],
    features: [
      { key: 'user', value: 38.2 },
      { key: 'subagent', value: 11.4 },
      { key: 'guardian_review', value: 4.5 },
    ],
    models: [
      { key: 'gpt-5.3-codex', value: 34.1 },
      { key: 'gpt-5.4', value: 15.0 },
      { key: 'gpt-5.4-mini', value: 5.0 },
    ],
  },
  work: mockAccounts[0].usage.work,
  problems: {},
}

const mockSubscription = {
  checked_at_ms: now,
  plan_type: 'team',
  status: 'past_due',
  expires_at_ms: now + 18 * dayMs,
  renews_at_ms: now + 18 * dayMs,
  will_renew: true,
  grace_ends_at_ms: Date.now() + 4 * dayMs,
  delinquent_since_ms: now - 3 * dayMs,
  billing_Interval: 'month',
  billing_currency: 'USD',
  seats: 5,
}

const mockBilling = {
  history: {
    checked_at_ms: now,
    subscription: mockSubscription,
    transactions: [
      {
        id: 'in_1',
        created_at_ms: now - 3 * dayMs,
        period_start_ms: now - 3 * dayMs,
        period_end_ms: now + 27 * dayMs,
        amount: 15000,
        currency: 'USD',
        status: 'past_due',
        plan: 'CHATGPT TEAM (5 SEATS)',
        document_url: 'https://invoice.stripe.com/i/acct_mock/test_inv_001',
      },
      {
        id: 'in_0',
        created_at_ms: now - 33 * dayMs,
        period_start_ms: now - 33 * dayMs,
        period_end_ms: now - 3 * dayMs,
        amount: 15000,
        currency: 'USD',
        status: 'paid',
        plan: 'CHATGPT TEAM (5 SEATS)',
        document_url: 'https://invoice.stripe.com/i/acct_mock/test_inv_000',
      },
    ],
  },
  payment: mockAccounts[0].payment,
}

const mockDevices = {
  checked_at_ms: now,
  in_use_known: true,
  devices: [
    {
      id: 'dev-cpr',
      name: 'Codex CLI · Linux x86_64',
      platform: 'Linux · Ubuntu 24.04',
      apps: ['Codex CLI'],
      last_signed_in_ms: now - 1200_000,
      city: 'Tokyo',
      region: 'Tokyo',
      country: 'Japan',
      current: true,
      in_use: true,
      trusted: true,
      signed_out: false,
    },
    {
      id: 'dev-mac',
      name: 'MacBook Pro (Chrome 131)',
      platform: 'macOS · Sequoia 15.1',
      apps: ['ChatGPT Web', 'ChatGPT Desktop'],
      last_signed_in_ms: now - 3 * 3600_000,
      city: 'San Francisco',
      region: 'CA',
      country: 'United States',
      current: false,
      in_use: false,
      trusted: true,
      signed_out: false,
    },
    {
      id: 'dev-ios',
      name: 'iPhone 16 Pro',
      platform: 'iOS 18.1',
      apps: ['ChatGPT iOS'],
      last_signed_in_ms: now - 2 * dayMs,
      city: 'Singapore',
      region: '',
      country: 'Singapore',
      current: false,
      in_use: false,
      trusted: false,
      signed_out: false,
    },
  ],
}

const mockLoginAccounts = [
  {
    accountId: 'acc-work',
    name: 'team-alpha',
    email: 'alpha@acme.io',
    kind: 'team',
    enabled: true,
    credentialState: 'ready',
    hasPassword: true,
    hasTotp: true,
    lastLoginAtMs: now - 2 * dayMs,
    lastError: null,
    autoLoginEnabled: true,
    autoLoginLast: {
      atMs: now - 3 * dayMs,
      trigger: '访问令牌已过期或即将过期',
      success: true,
    },
    pendingOauth: null,
  },
  {
    accountId: 'acc-codex',
    name: 'plus-codex-only',
    email: 'dev@PersonalLongDomainExample.org',
    kind: 'personal',
    enabled: true,
    credentialState: 'ready',
    hasPassword: true,
    hasTotp: false,
    lastLoginAtMs: now - 5 * dayMs,
    lastError: null,
    pendingOauth: null,
  },
  {
    accountId: 'acc-expired-saved',
    name: 'expired-with-saved-pw',
    email: 'saved-pw@example.com',
    kind: 'personal',
    enabled: true,
    credentialState: 'expired',
    hasPassword: true,
    hasTotp: true,
    lastLoginAtMs: now - 8 * dayMs,
    lastError: null,
    pendingOauth: null,
  },
  {
    accountId: 'acc-expired',
    name: 'expired-account',
    email: 'old@example.com',
    kind: 'personal',
    enabled: true,
    credentialState: 'expired',
    hasPassword: false,
    hasTotp: false,
    lastLoginAtMs: null,
    lastError: {
      code: 'browser_challenge',
      message: '遇到 Cloudflare 浏览器验证拦截，请使用浏览器手动登录',
      atMs: now - 1800_000,
    },
    pendingOauth: null,
  },
]

const mockPastePreview = {
  applied: false,
  lines: 3,
  created: 2,
  updated: 1,
  skipped: 0,
  existingInCpr: 1,
  items: [
    {
      line: 1,
      email: 'new-plus@acme.io',
      kind: 'personal',
      hasPassword: true,
      hasTotp: true,
      action: 'create',
      inCpr: false,
      message: '新个人账号，待执行 OAuth 登录建入 CPR',
    },
    {
      line: 2,
      email: 'new-team@acme.io',
      kind: 'team',
      hasPassword: true,
      hasTotp: false,
      action: 'create',
      inCpr: false,
      message: '新 Team 账号，待执行 OAuth 登录建入 CPR',
    },
    {
      line: 3,
      email: 'alpha@acme.io',
      kind: 'team',
      hasPassword: true,
      hasTotp: true,
      action: 'update',
      inCpr: true,
      existingAccountId: 'acc-work',
      message: '已在 CPR (acc-work)，勾选更新后可覆盖重登',
    },
  ],
}

const bridgePayload = {
  accounts: { checked_at_ms: now, accounts: mockAccounts },
  usage: { usage: mockUsageDetail, summary: mockAccounts[0].usage },
  subscription: { subscription: mockSubscription },
  billing: mockBilling,
  devices: { devices: mockDevices },
  signOutVoided: { signed_out: 3, remaining: 0, cpr_voided: true },
  probe: (() => {
    const account = { ...mockAccounts[0], supported: true, running: null, blocked: null }
    const health = { level: 'degraded', last_run_id: 'probe-1', model: 'gpt-6-luna', changed_ms: now }
    const turnstate = { schedule: { enabled: false, model: 'gpt-6-luna', interval_minutes: 60 }, health, runs: [], daily: [] }
    const modeltrace = { ...turnstate, health: null, schedule: { ...turnstate.schedule, model: 'gpt-6.1-sol' } }
    const config = { probe_enabled: false, probe_maintenance_enabled: false, min_interval_minutes: 30, fingerprint_sync_hours: 6 }
    const rate = { judged: 1, ok: 0 }
    return {
      status: { accounts: { [account.id]: { level: health.level, blocked: false, running: false } } },
      account: { account: { ...account, methods: { turnstate, modeltrace } }, config },
      overview: {
        now_ms: now, range: { hours: 24, start_ms: now - dayMs, slot_ms: 1800_000, slots: 48 }, config,
        accounts: [{ ...account, health, schedule: { turnstate: turnstate.schedule, modeltrace: modeltrace.schedule }, latest: {}, rate, rate_24h: rate, rate_7d: rate, strip: [{ i: 40, degraded: 1 }] }],
      },
      models: { bank: [], cpr: ['gpt-6-luna', 'gpt-6.1-sol'], aliases: {} },
    }
  })(),
  loginOverview: {
    oauthRedirect: 'http://localhost:1455/auth/callback',
    accounts: mockLoginAccounts,
  },
  loginAccountStatus: {
    oauthRedirect: 'http://localhost:1455/auth/callback',
    account: mockLoginAccounts[0],
  },
  pastePreview: mockPastePreview,
  oauthStart: {
    authorizeUrl: 'https://auth.openai.com/oauth/authorize?client_id=app_EMoamEEZ73f0CkXaXp7hrann&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&state=mock_state_12345',
    state: 'mock_state_12345',
    expiresAtMs: now + 600_000,
  },
}

const bridgeScript = `<script>
const __MOCK__ = ${JSON.stringify(bridgePayload)};
const __THEMES__ = ${JSON.stringify(themes)};
window.__LOGIN_DELAY_MS__ = 0;
window.__LOGIN_PROGRESS__ = null;
window.__LOGIN_ERROR__ = null;
window.__setHostTheme = (name) => {
  const root = document.documentElement;
  root.dataset.theme = name;
  root.classList.toggle('dark', name === 'dark');
  root.style.colorScheme = name;
  const payload = __THEMES__[name] || __THEMES__.light;
  for (const [k, v] of Object.entries(payload)) {
    root.style.setProperty(k, v);
  }
  document.body.style.background = 'var(--cp-color-bg-layout, var(--cp-color-bg-container))';
  window.dispatchEvent(new CustomEvent('codex-proxy-themechange', { detail: { theme: name } }));
};
function __reply(data, status = 200) {
  return {
    status,
    contentType: 'application/json',
    body: new TextEncoder().encode(JSON.stringify(data)).buffer,
  };
}
window.codexProxyPlugin = {
  version: 2,
  get theme() { return document.documentElement.dataset.theme || 'light'; },
  request: async (input) => {
    const fn = input.path;
    const query = Object.fromEntries(new URLSearchParams(input.query || ''));
    const payload = input.body ? JSON.parse(input.body) : {};
    if (fn === 'api/accounts') return __reply(__MOCK__.accounts);
    if (fn.startsWith('api/probe/') && __MOCK__.probe[fn.slice('api/probe/'.length)])
      return __reply(__MOCK__.probe[fn.slice('api/probe/'.length)]);
    if (fn === 'api/usage') return __reply(__MOCK__.usage);
    if (fn === 'api/subscription') {
      if (query.id === 'acc-work') return __reply({ ...__MOCK__.subscription, payment: __MOCK__.billing.payment });
      const existing = __MOCK__.accounts.accounts.find(a => a.id === query.id)?.payment;
      return __reply({ payment: { ...(existing || {}), status: existing?.created_at_ms ? existing.status : 'paid', subscription_status: 'active', checked_at_ms: Date.now(), subscription_expires_at_ms: Date.now() + 604800000, subscription_will_renew: true } });
    }
    if (fn === 'api/billing') return __reply(__MOCK__.billing);
    if (fn === 'api/devices') return __reply(__MOCK__.devices);
    if (fn === 'api/devices/sign-out') return __reply(__MOCK__.signOutVoided);
    if (fn === 'api/login/status') {
      const id = query.id || payload.accountId;
      if (id) {
        const found = __MOCK__.loginOverview.accounts.find(a => a.accountId === id) || __MOCK__.loginOverview.accounts[0];
        return __reply({ oauthRedirect: 'http://localhost:1455/auth/callback', account: found });
      }
      return __reply(__MOCK__.loginOverview);
    }
    if (fn === 'api/login/progress') {
      return __reply({ progress: window.__LOGIN_PROGRESS__ });
    }
    if (fn === 'api/login/auto-login') {
      const found = __MOCK__.loginOverview.accounts.find(a => a.accountId === payload.accountId);
      found.autoLoginEnabled = payload.enabled;
      return __reply({ ok: true, account: found });
    }
    if (fn === 'api/login/paste') return __reply(__MOCK__.pastePreview);
    if (fn === 'api/login/proxies') return __reply({ proxies: [
      { id: 'px-off', name: '旧代理', enabled: false },
      { id: 'px-new', name: '新加坡 新线路', enabled: true },
      { id: 'px-tokyo', name: '东京 住宅', enabled: true },
    ] });
    if (fn === 'api/login/import') {
      (window.__IMPORTS__ ||= []).push(payload);
      if (payload.email === 'new-team@acme.io' && !payload.workspaceId) {
        return __reply({ error: { code: 'workspace_ambiguous', message: '账号有 2 个工作区：个人、Acme', details: { choices: [
          { kind: 'workspace', id: 'ws-p', name: '个人' },
          { kind: 'workspace', id: 'ws-acme', name: 'Acme' },
        ] } } }, 409);
      }
      return __reply({ ok: true, accountId: 'acct_new_' + window.__IMPORTS__.length, email: payload.email, kind: payload.kind });
    }
    if (fn === 'api/login/oauth/start') return __reply(__MOCK__.oauthStart);
    if (fn === 'api/login/run') {
      const acctId = payload?.accountId || 'acc-work';
      if (window.__LOGIN_DELAY_MS__ > 0) {
        window.__LOGIN_PROGRESS__ = {
          accountId: acctId,
          stage: 'password',
          label: '验证登录密码',
          startedAtMs: Date.now(),
          updatedAtMs: Date.now(),
        };
        await new Promise(r => setTimeout(r, window.__LOGIN_DELAY_MS__));
      }
      window.__LOGIN_PROGRESS__ = null;
      if (window.__LOGIN_ERROR__) {
        return __reply({ error: window.__LOGIN_ERROR__ }, window.__LOGIN_ERROR__.status || 401);
      }
      const found = __MOCK__.loginOverview.accounts.find(a => a.accountId === acctId) || __MOCK__.loginOverview.accounts[0];
      return __reply({
        ok: true,
        accountId: acctId,
        email: found.email,
        planType: found.kind === 'team' ? 'team' : 'pro',
        expiresAtMs: Date.now() + 7 * 86400000,
        account: { ...found, credentialState: 'ready' },
      });
    }
    return __reply({ error: { code: 'not_found', message: 'Unhandled mock route: ' + fn } }, 404);
  }
};
addEventListener('DOMContentLoaded', () => window.__setHostTheme('light'), { once: true });
</script>`

const rawHtml = await readFile(join(distDir, 'index.html'), 'utf8')
const builtCss = await readFile(join(distDir, 'app.css'), 'utf8')
assert.doesNotMatch(builtCss, /url\s*\(/i, '打包后的 CSS 不能包含 url(...)，CPR 插件沙箱会拒绝')
assert.doesNotMatch(`${rawHtml}\n${builtCss}`, /data:image\//i, '打包产物不能包含 data:image/ 资源地址')
const html = rawHtml.replace('<head>', `<head>${bridgeScript}`)
const mime = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
}

const server = http.createServer(async (req, res) => {
  const path = (req.url ?? '/').split('?')[0]
  if (path === '/' || path === '/index.html') {
    res.writeHead(200, { 'content-type': mime['.html'] }).end(html)
    return
  }
  try {
    const body = await readFile(join(distDir, path))
    res.writeHead(200, { 'content-type': mime[extname(path)] ?? 'application/octet-stream' }).end(body)
  }
  catch {
    res.writeHead(404).end()
  }
})

await new Promise(r => server.listen(0, '127.0.0.1', r))
const { port } = server.address()
const baseUrl = `http://127.0.0.1:${port}/`

await mkdir(outDir, { recursive: true })

const chrome = spawn(chromeBin, [
  '--headless',
  '--no-sandbox',
  '--disable-gpu',
  '--remote-debugging-port=0',
  '--window-size=1440,960',
  'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] })

const wsUrl = await new Promise((resolveUrl, reject) => {
  let stderr = ''
  chrome.stderr.on('data', (chunk) => {
    stderr += chunk.toString()
    const match = stderr.match(/DevTools listening on (ws:\/\/\S+)/)
    if (match)
      resolveUrl(match[1])
  })
  chrome.on('exit', code => reject(new Error(`chrome exited (${code}): ${stderr}`)))
})

const browserWs = new WebSocket(wsUrl)
await new Promise((resolveOpen, reject) => {
  browserWs.onopen = resolveOpen
  browserWs.onerror = reject
})

let nextId = 0
const pending = new Map()
browserWs.onmessage = (event) => {
  const msg = JSON.parse(event.data)
  if (msg.id && pending.has(msg.id)) {
    const { resolve: res, reject: rej } = pending.get(msg.id)
    pending.delete(msg.id)
    if (msg.error)
      rej(new Error(msg.error.message))
    else
      res(msg.result)
  }
}

function cdp(method, params = {}, sessionId) {
  const id = ++nextId
  browserWs.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }))
  return new Promise((res, rej) => pending.set(id, { resolve: res, reject: rej }))
}

const screenshotPaths = []

try {
  const { targetInfos } = await cdp('Target.getTargets')
  const page = targetInfos.find(t => t.type === 'page')
  const { sessionId } = await cdp('Target.attachToTarget', { targetId: page.targetId, flatten: true })
  await cdp('Page.enable', {}, sessionId)
  await cdp('Runtime.enable', {}, sessionId)

  const errors = []
  browserWs.addEventListener('message', (event) => {
    const msg = JSON.parse(event.data)
    if (msg.method === 'Runtime.exceptionThrown') {
      errors.push(msg.params.exceptionDetails.text)
    }
  })

  async function setViewport(width, height, theme = 'light') {
    await cdp('Emulation.setDeviceMetricsOverride', {
      width,
      height,
      deviceScaleFactor: 2,
      mobile: width <= 480,
    }, sessionId)
    await evaluate(`window.__setHostTheme(${JSON.stringify(theme)});`)
    await sleep(160)
  }

  async function evaluate(expression) {
    const { result, exceptionDetails } = await cdp('Runtime.evaluate', {
      expression: `(async () => { ${expression} })()`,
      awaitPromise: true,
      returnByValue: true,
    }, sessionId)
    if (exceptionDetails) {
      throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text)
    }
    return result.value
  }

  async function assertNoClippedButtons(contextLabel) {
    const check = await evaluate(`
      const overflowX = document.documentElement.scrollWidth > window.innerWidth + 1;
      const clipped = [];
      for (const btn of document.querySelectorAll('button')) {
        const style = getComputedStyle(btn);
        if (style.display === 'none' || style.visibility === 'hidden') continue;
        const r = btn.getBoundingClientRect();
        if (r.width === 0 || r.height === 0) continue;
        if (r.left < -1 || r.right > window.innerWidth + 1) {
          clipped.push({ text: btn.textContent.trim(), left: Math.round(r.left), right: Math.round(r.right), vw: window.innerWidth });
        }
      }
      return { overflowX, clipped };
    `)
    assert.equal(check.overflowX, false, `${contextLabel} 不能出现横向溢出`)
    assert.deepEqual(check.clipped, [], `${contextLabel} 不能有被裁切的按钮: ${JSON.stringify(check.clipped)}`)
  }

  async function capture(name, width, height, theme) {
    await setViewport(width, height, theme)
    const { data } = await cdp('Page.captureScreenshot', { format: 'png' }, sessionId)
    const filePath = join(outDir, `${name}.png`)
    await writeFile(filePath, Buffer.from(data, 'base64'))
    screenshotPaths.push(filePath)
    return filePath
  }

  await cdp('Page.navigate', { url: baseUrl }, sessionId)
  await sleep(600)

  const listStats = await evaluate(`
    const rows = [...document.querySelectorAll('.probe-monitor-row')];
    const work = rows.find(row => row.dataset.account === 'acc-work');
    const paid = rows.find(row => row.dataset.account === 'acc-codex');
    return {
      count: rows.length, work: work?.innerText, paid: paid?.innerText,
      cells: work?.querySelectorAll('.probe-cell').length,
      stripHeight: work?.querySelector('.probe-strip').getBoundingClientRect().height,
      lightBg: getComputedStyle(document.querySelector('.probe-monitor-list')).backgroundColor,
      text: document.body.innerText,
    };
  `)
  assert.equal(listStats.count, 4, '探测、订阅共用一个账号列表')
  assert.match(listStats.work, /降智/)
  assert.match(listStats.work, /付款逾期/)
  assert.match(listStats.work, /宽限期至 .*剩 4 天/)
  assert.match(listStats.work, /账单日 \d{4}\/\d{2}\/\d{2}/, '宽限期账号主界面应同时显示账单日')
  assert.match(listStats.paid, /未查订阅/, '旧的已支付账单不能当作订阅有效')
  assert.match(listStats.paid, /账单日 2026\/10\/25/, '已查账单账号主界面应显示账单日')
  assert.match(listStats.text, /免输重登/)
  assert.match(listStats.text, /去重新登录/)
  assert.doesNotMatch(listStats.text, /额度|周额度/)
  assert.ok(listStats.cells > 0, '默认保留可点开的探针时间条')
  assert.ok(listStats.stripHeight <= 14, '探针默认采用紧凑高度')

  const expanded = await evaluate(`
    const toggle = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === '展开探针');
    toggle.click();
    await new Promise(r => setTimeout(r, 80));
    const height = document.querySelector('.probe-strip').getBoundingClientRect().height;
    toggle.click();
    return height;
  `)
  assert.ok(expanded > listStats.stripHeight)
  await capture('01-unified-light', 1440, 900, 'light')
  await capture('02-unified-dark', 1440, 900, 'dark')
  assert.notEqual(await evaluate(`return getComputedStyle(document.querySelector('.probe-monitor-list')).backgroundColor;`), listStats.lightBg)
  await capture('03-unified-390px', 390, 960, 'light')
  await assertNoClippedButtons('390px 合并列表')

  const filtered = await evaluate(`
    const select = label => [...document.querySelectorAll('.probe-filter-chip')].find(b => b.textContent.includes(label)).click();
    select('订阅宽限期');
    await new Promise(r => setTimeout(r, 70));
    const grace = document.querySelectorAll('.probe-monitor-row').length;
    select('降智');
    await new Promise(r => setTimeout(r, 70));
    const degraded = document.querySelectorAll('.probe-monitor-row').length;
    select('全部');
    return { grace, degraded };
  `)
  assert.deepEqual(filtered, { grace: 1, degraded: 1 })

  const refreshed = await evaluate(`
    [...document.querySelectorAll('button')].find(b => b.textContent.includes('查询全部订阅')).click();
    await new Promise(r => setTimeout(r, 140));
    return document.querySelector('[data-account="acc-codex"] .subscription-cell').innerText;
  `)
  assert.match(refreshed, /有效.*账单日 2026\/10\/25.*续费日 \d{4}\/\d{2}\/\d{2}/s)

  // 验证免输重登弹窗：展示实时进度、失败原因与阶段，并支持重试成功
  const reloginRunning = await evaluate(`
    window.__LOGIN_DELAY_MS__ = 700;
    window.__LOGIN_ERROR__ = {
      status: 401,
      code: 'password_rejected',
      message: '密码校验未通过，请核对后重试',
      details: { stage: 'password' },
    };
    const rowBtn = [...document.querySelectorAll('[data-account="acc-expired-saved"] button')]
      .find(b => b.textContent.trim() === '免输重登');
    rowBtn.click();
    await new Promise(r => setTimeout(r, 520));
    const dlg = [...document.querySelectorAll('[role="dialog"]')].find(el => !el.closest('[inert]') && el.querySelector('.relogin-list'));
    return {
      open: Boolean(dlg),
      title: dlg?.innerText || '',
      hasProgressTrack: Boolean(dlg?.querySelector('.relogin-progress-track')),
      stepChips: dlg?.querySelectorAll('.relogin-step-chip').length || 0,
      hasStageLabel: dlg?.innerText.includes('验证登录密码') || false,
    };
  `)
  assert.equal(reloginRunning.open, true, '点击免输重登应立即打开进度弹窗')
  assert.equal(reloginRunning.hasProgressTrack, true, '弹窗应显示总进度条')
  assert.ok(reloginRunning.stepChips >= 6, '运行中应显示分阶段进度胶囊')
  assert.equal(reloginRunning.hasStageLabel, true, '轮询进度应展示当前阶段文字')

  await sleep(320)
  const reloginFailed = await evaluate(`
    const dlg = [...document.querySelectorAll('[role="dialog"]')].find(el => !el.closest('[inert]') && el.querySelector('.relogin-list'));
    return {
      text: dlg?.innerText || '',
      state: dlg?.querySelector('[data-relogin-state]')?.getAttribute('data-relogin-state'),
      hasErrorBox: Boolean(dlg?.querySelector('.relogin-error-box')),
    };
  `)
  assert.equal(reloginFailed.state, 'failed', '失败后列表项应进入 failed 状态')
  assert.equal(reloginFailed.hasErrorBox, true, '失败后应在弹窗内展示错误原因卡片')
  assert.match(reloginFailed.text, /验证密码阶段/)
  assert.match(reloginFailed.text, /\[密码被拒绝\] 密码校验未通过，请核对后重试/)
  await capture('16-relogin-error-light', 1440, 900, 'light')
  await capture('17-relogin-error-dark', 1440, 900, 'dark')
  await setViewport(390, 844, 'light')
  await assertNoClippedButtons('390px 免输重登弹窗')
  await setViewport(1440, 900, 'light')

  const reloginRetried = await evaluate(`
    window.__LOGIN_DELAY_MS__ = 0;
    window.__LOGIN_ERROR__ = null;
    const dlg = [...document.querySelectorAll('[role="dialog"]')].find(el => !el.closest('[inert]') && el.querySelector('.relogin-list'));
    const retryBtn = [...dlg.querySelectorAll('button')].find(b => b.textContent.includes('重试'));
    retryBtn.click();
    await new Promise(r => setTimeout(r, 150));
    const afterRetry = {
      state: dlg.querySelector('[data-relogin-state]')?.getAttribute('data-relogin-state'),
      text: dlg.innerText,
    };
    const closeBtn = [...dlg.querySelectorAll('button')].find(b => b.textContent.trim() === '关闭');
    closeBtn.click();
    await new Promise(r => setTimeout(r, 250));
    const stillOpen = [...document.querySelectorAll('[role="dialog"]')].some(el => !el.closest('[inert]') && el.querySelector('.relogin-list'));
    return {
      ...afterRetry,
      closed: !stillOpen,
    };
  `)
  assert.equal(reloginRetried.state, 'done', '弹窗内点重试应重新执行并成功')
  assert.match(reloginRetried.text, /新令牌已写回 CPR/)
  assert.equal(reloginRetried.closed, true, '完成后应可正常关闭免输重登弹窗')

  const probeDetail = await evaluate(`
    const row = document.querySelector('[data-account="acc-work"]');
    row.querySelector('.monitor-account').click();
    await new Promise(r => setTimeout(r, 140));
    return document.body.innerText;
  `)
  assert.match(probeDetail, /alpha@acme\.io/)
  assert.match(probeDetail, /测票当前判定.*降智/s)
  await assertNoClippedButtons('390px 探测详情')

  await evaluate(`
    document.querySelector('button[aria-label="返回列表"]').click();
    await new Promise(r => setTimeout(r, 100));
    document.querySelector('[data-account="acc-work"] .subscription-cell').click();
    await new Promise(r => setTimeout(r, 200));
  `)
  assert.equal(await evaluate(`return document.querySelectorAll('.billing-row').length;`), 2, '进入订阅与账单页应自动加载 2 条账单记录')
  assert.equal(await evaluate(`return [...document.querySelectorAll('button')].some(b => b.textContent.trim() === '刷新');`), true, '自动加载完成后应显示刷新按钮')
  await capture('04-billing-390px', 390, 1300, 'light')
  await assertNoClippedButtons('390px 订阅与账单')

  await evaluate(`
    const tabs = [...document.querySelectorAll('[aria-label="账号详情分类"] [role="radio"], [aria-label="账号详情分类"] .z-segmented-item')];
    tabs.find(el => el.textContent.includes('官方用量')).click();
    await new Promise(r => setTimeout(r, 200));
  `)
  assert.equal(await evaluate(`return document.querySelectorAll('[aria-labelledby="usage-title"] .trend-chart').length;`), 2, '进入官方用量页应自动加载 2 张走势图')
  assert.equal(await evaluate(`return document.querySelector('[aria-labelledby="usage-title"] .meters');`), null)
  await evaluate(`
    const dayBtns = [...document.querySelectorAll('[aria-label="天数"] [role="radio"], [aria-label="天数"] .z-segmented-item')];
    dayBtns.find(el => el.textContent.includes('14 天')).click();
    await new Promise(r => setTimeout(r, 160));
  `)
  assert.equal(await evaluate(`return document.querySelectorAll('[aria-labelledby="usage-title"] .trend-chart').length;`), 2, '切换天数应自动刷新且不破坏走势图')

  await evaluate(`
    const tabs = [...document.querySelectorAll('[aria-label="账号详情分类"] [role="radio"], [aria-label="账号详情分类"] .z-segmented-item')];
    tabs.find(el => el.textContent.includes('登录与设备')).click();
    await new Promise(r => setTimeout(r, 200));
  `)

  // 验证账号详情顶栏「下一个 / 上一个账号」切换并保持当前标签页
  const pagerCheck = await evaluate(`
    const nextBtn = document.querySelector('button[aria-label="下一个账号"]');
    nextBtn.click();
    await new Promise(r => setTimeout(r, 180));
    const afterNext = {
      title: document.querySelector('.detail-header-bar h1')?.textContent.trim(),
      hasLoginSection: Boolean(document.querySelector('#account-login-section')),
    };
    document.querySelector('button[aria-label="上一个账号"]').click();
    await new Promise(r => setTimeout(r, 180));
    return {
      afterNext,
      restoredTitle: document.querySelector('.detail-header-bar h1')?.textContent.trim(),
    };
  `)
  assert.notEqual(pagerCheck.afterNext.title, 'alpha@acme.io', '点击下一个账号应切换到其他账号')
  assert.equal(pagerCheck.afterNext.hasLoginSection, true, '切换账号后应保持在登录与设备标签页')
  assert.equal(pagerCheck.restoredTitle, 'alpha@acme.io', '点击上一个账号应切回原账号')

  // ── 3. 详情页（重新登录 + 登录设备）验收与截图 ──
  await setViewport(1440, 1100, 'light')
  await evaluate(`
    const buttons = [...document.querySelectorAll('button')];
    buttons.find(b => b.textContent.includes('浏览器手动登录'))?.click();
    await new Promise(r => setTimeout(r, 200));
    document.querySelector('#account-login-section')?.scrollIntoView({ behavior: 'instant', block: 'start' });
    await new Promise(r => setTimeout(r, 150));
  `)

  const deviceStats = await evaluate(`
    return {
      deviceRows: document.querySelectorAll('.device-row').length,
      loginSection: Boolean(document.querySelector('#account-login-section')),
      hasAuthorizeUrl: Boolean(document.querySelector('input[aria-label="OAuth 授权链接"]')?.value),
      inputBorder: getComputedStyle(document.querySelector('#account-login-section .cp-input')).borderTopWidth,
      hasAutoLoginToggle: document.body.innerText.includes('401 / 令牌过期时自动重登一次'),
      hasAutoLoginResult: document.body.innerText.includes('自动重登成功'),
    };
  `)
  assert.equal(deviceStats.deviceRows, 3, '进入登录与设备页应自动加载 3 台登录设备行')
  assert.equal(deviceStats.loginSection, true, '详情页应包含重新登录卡片')
  assert.equal(deviceStats.hasAuthorizeUrl, true, '展开浏览器手动登录应自动生成授权链接')
  assert.equal(deviceStats.inputBorder, '1px', '输入框应具备可见边框（未被 @layer base 重置）')
  assert.equal(deviceStats.hasAutoLoginToggle, true, '重新登录卡片应包含账号级自动重登开关')
  assert.equal(deviceStats.hasAutoLoginResult, true, '应显示最近一次自动重登结果')

  const autoLoginToggle = await evaluate(`
    const label = [...document.querySelectorAll('#account-login-section label')].find(el => el.textContent.includes('401 / 令牌过期时自动重登一次'));
    const input = label.querySelector('input');
    input.click();
    await new Promise(r => setTimeout(r, 100));
    const off = {
      checked: input.checked,
      status: document.body.innerText.includes('已关闭自动重登'),
    };
    input.click();
    await new Promise(r => setTimeout(r, 100));
    return { off, onChecked: input.checked };
  `)
  assert.equal(autoLoginToggle.off.checked, false, '关闭后账号级自动重登开关应取消')
  assert.equal(autoLoginToggle.off.status, true, '关闭后应显示结果提示')
  assert.equal(autoLoginToggle.onChecked, true, '可以重新开启账号级自动重登')

  await capture('07-detail-devices-login-light', 1440, 1100, 'light')
  await capture('08-detail-devices-login-dark', 1440, 1100, 'dark')
  await capture('09-detail-devices-login-390px', 390, 1380, 'light')
  await assertNoClippedButtons('390px 详情页（重登与设备）')

  // 验证登出所有设备后显示「免输重登」与「去重新登录」入口
  await setViewport(1440, 1000, 'light')
  const voidedBannerCheck = await evaluate(`
    const signOutAllBtn = [...document.querySelectorAll('button')].find(b => b.textContent.includes('登出所有设备'));
    signOutAllBtn.click();
    await new Promise(r => setTimeout(r, 150));
    const dialogConfirm = [...document.querySelectorAll('.z-dialog button, [role="dialog"] button')].find(b => b.textContent.trim() === '登出');
    dialogConfirm.click();
    await new Promise(r => setTimeout(r, 200));
    const text = document.body.innerText;
    return {
      hasVoidedNotice: text.includes('需要重新登录写回 CPR'),
      hasQuickReloginBtn: text.includes('免输重登') && text.includes('去重新登录'),
    };
  `)
  assert.equal(voidedBannerCheck.hasVoidedNotice, true, '登出所有设备后应提示需要重新登录写回 CPR')
  assert.equal(voidedBannerCheck.hasQuickReloginBtn, true, '提示旁应直接提供免输重登与去重新登录入口')

  // 验证设备卡片上的「免输重登」也会打开进度弹窗并明确展示失败原因
  const deviceReloginCheck = await evaluate(`
    window.__LOGIN_ERROR__ = {
      status: 409,
      code: 'proxy_unavailable',
      message: '绑定的出口代理已停用',
      details: { stage: 'prepare' },
    };
    const quickBtn = [...document.querySelectorAll('[aria-labelledby="devices-title"] .banner-ok button')]
      .find(b => b.textContent.includes('免输重登'));
    quickBtn.click();
    await new Promise(r => setTimeout(r, 150));
    const dlg = [...document.querySelectorAll('[role="dialog"]')].find(el => !el.closest('[inert]') && el.querySelector('.relogin-list'));
    const dlgText = dlg?.innerText || '';
    const closeBtn = [...(dlg?.querySelectorAll('button') || [])].find(b => b.textContent.trim() === '关闭');
    closeBtn?.click();
    await new Promise(r => setTimeout(r, 150));
    window.__LOGIN_ERROR__ = null;
    const bannerError = document.querySelector('[aria-labelledby="devices-title"] .banner-error')?.innerText || '';
    return {
      hasDialog: Boolean(dlg),
      dlgText,
      bannerError,
    };
  `)
  assert.equal(deviceReloginCheck.hasDialog, true, '设备面板点击免输重登应打开进度弹窗')
  assert.match(deviceReloginCheck.dlgText, /读取出口阶段/)
  assert.match(deviceReloginCheck.dlgText, /\[代理不可用\] 绑定的出口代理已停用/)
  assert.match(deviceReloginCheck.bannerError, /免输重登失败：\[代理不可用\] 绑定的出口代理已停用/)

  // ── 4. 导入账号页验收与截图 ──
  await evaluate(`
    document.querySelector('button[aria-label="返回列表"]').click();
    await new Promise(r => setTimeout(r, 100));
    const loginTab = [...document.querySelectorAll('.monitor-header button')].find(el => el.textContent.includes('导入账号'));
    loginTab.click();
    await new Promise(r => setTimeout(r, 200));
    const ta = document.querySelector('textarea');
    ta.value = 'new-plus@acme.io----pass123----JBSWY3DPEHPK3PXP----personal\\nnew-team@acme.io----pass456--------team\\nalpha@acme.io----pass789----JBSWY3DPEHPK3PXP----team';
    ta.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise(r => setTimeout(r, 550));
  `)

  const loginPageStats = await evaluate(`
    const buttons = [...document.querySelectorAll('button')].map(b => b.textContent.trim());
    return {
      previewRows: document.querySelectorAll('.import-preview-row').length,
      existingRows: document.querySelectorAll('.login-account-row').length,
      hasImportCountBtn: buttons.some(b => b.includes('导入 2 个')),
      hasPreviewBtn: buttons.some(b => b.includes('解析并预览')),
    };
  `)
  assert.equal(loginPageStats.previewRows, 3, '输入后防抖应自动渲染 3 行导入预览')
  assert.equal(loginPageStats.existingRows, 0, '已有账号快速重新登录列表应已删除')
  assert.equal(loginPageStats.hasImportCountBtn, true, '主按钮应显示「导入 2 个」')
  assert.equal(loginPageStats.hasPreviewBtn, false, '「解析并预览」按钮应已删除')

  const proxyPick = await evaluate(`
    const sel = document.querySelector('select[aria-label="新账号出口"]');
    const ta = document.querySelector('textarea.cp-textarea');
    return {
      value: sel.value,
      selectBorder: getComputedStyle(sel).borderTopWidth,
      textareaBorder: getComputedStyle(ta).borderTopWidth,
      options: [...sel.options].map(o => ({ text: o.textContent.trim(), disabled: o.disabled })),
    };
  `)
  assert.equal(proxyPick.value, 'px-new', '默认选第一个启用的代理')
  assert.equal(proxyPick.selectBorder, '1px', '下拉框应具备可见边框')
  assert.equal(proxyPick.textareaBorder, '1px', '多行文本框应具备可见边框')
  assert.match(proxyPick.options[0].text, /直连/)
  assert.deepEqual(proxyPick.options.map(o => o.disabled), [false, true, false, false], '直连可选，停用的代理不能选')

  await capture('10-login-import-light', 1440, 1100, 'light')
  await capture('11-login-import-dark', 1440, 1100, 'dark')
  await capture('12-login-import-390px', 390, 1380, 'light')
  await assertNoClippedButtons('390px 导入账号页')

  // 导入：多工作区的行给出候选，点一下带着工作区重试
  await evaluate(`
    const start = [...document.querySelectorAll('button')].find(b => b.textContent.includes('导入 2 个'));
    start.click();
    await new Promise(r => setTimeout(r, 400));
  `)
  const choiceState = await evaluate(`
    const buttons = [...document.querySelectorAll('.import-preview-row button')].map(b => b.textContent.trim());
    return { buttons, imports: window.__IMPORTS__ };
  `)
  assert.ok(choiceState.imports.every(i => i.proxyId === 'px-new' && !('viaAccountId' in i)), '导入带所选代理，不再带借道账号')
  assert.ok(choiceState.buttons.includes('个人') && choiceState.buttons.includes('Acme'), '多工作区时给出可点选的候选')
  await capture('15-login-import-choices', 1440, 1100, 'light')
  await evaluate(`
    const acme = [...document.querySelectorAll('.import-preview-row button')].find(b => b.textContent.trim() === 'Acme');
    acme.click();
    await new Promise(r => setTimeout(r, 300));
  `)
  const retried = await evaluate(`return { last: window.__IMPORTS__.at(-1), text: document.body.textContent }`)
  assert.equal(retried.last.workspaceId, 'ws-acme', '点候选后带着工作区 ID 重试')
  assert.equal(retried.last.email, 'new-team@acme.io')

  const direct = await evaluate(`
    const select = document.querySelector('select[aria-label="新账号出口"]');
    select.value = '';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    const earlier = window.__IMPORTS__.length;
    [...document.querySelectorAll('button')].find(b => b.textContent.includes('导入 2 个')).click();
    await new Promise(r => setTimeout(r, 400));
    return { imports: window.__IMPORTS__.slice(earlier), egress: select.value };
  `)
  assert.equal(direct.imports.length, 2)
  assert.ok(direct.imports.every(i => i.proxyId === ''))
  assert.equal(direct.egress, '', '刷新代理列表后保留用户选的直连')

  assert.deepEqual(errors, [], `页面运行不应有未捕获异常: ${errors.join('; ')}`)
  console.log('UI check passed. Generated screenshots:')
  for (const p of screenshotPaths) {
    console.log(`  - ${p}`)
  }
}
finally {
  browserWs.close()
  chrome.kill()
  server.close()
}

function sleep(ms) {
  return new Promise(resolveSleep => setTimeout(resolveSleep, ms))
}
