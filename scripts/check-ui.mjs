// Headless UI verification & screenshot generator for cpr-chatgpt-account-panel.
// Renders the built frontend bundle against a mocked CPR host bridge across
// light, dark, and 390px narrow viewports, verifies layout/DOM invariants,
// and saves persistent screenshots for List, Batch Relogin, Detail (Usage & Billing),
// Detail (Devices & Login), and Import Account pages.

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
      grace_ends_at_ms: now + 4 * dayMs,
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
  grace_ends_at_ms: now + 4 * dayMs,
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
    if (fn === 'api/usage') return __reply(__MOCK__.usage);
    if (fn === 'api/subscription') return __reply(__MOCK__.subscription);
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
    if (fn === 'api/login/paste') return __reply(__MOCK__.pastePreview);
    if (fn === 'api/login/oauth/start') return __reply(__MOCK__.oauthStart);
    if (fn === 'api/login/run') {
      if (window.__LOGIN_DELAY_MS__ > 0) {
        await new Promise(r => setTimeout(r, window.__LOGIN_DELAY_MS__));
      }
      const acctId = payload?.accountId || 'acc-work';
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

  // ── 1. 列表页验收与截图 (Light / Dark / 390px) ──
  const listStats = await evaluate(`
    const chips = [...document.querySelectorAll('.filter-chip')].map(el => el.textContent.replace(/\\s+/g, ' ').trim());
    const rows = document.querySelectorAll('.monitor-row').length;
    const actionButtons = [...document.querySelectorAll('.row-col-actions button')].map(b => b.textContent.trim());
    const lightBg = getComputedStyle(document.querySelector('.monitor-list')).backgroundColor;
    return { chips, rows, actionButtons, lightBg };
  `)
  assert.equal(listStats.rows, 4, '列表应渲染 4 个账号监控行')
  assert.equal(listStats.chips.length, 5, '列表顶部应有 5 个分类计数筛选芯片')
  assert.ok(listStats.chips.some(c => c.includes('无法查询')), '筛选芯片应显示「无法查询」')
  assert.ok(listStats.actionButtons.includes('免输重登'), '令牌过期且有已存密码的账号行应显示「免输重登」')
  assert.ok(listStats.actionButtons.includes('去重新登录'), '令牌过期且未存密码的账号行应显示「去重新登录」')

  await capture('01-list-light', 1440, 900, 'light')
  await capture('02-list-dark', 1440, 900, 'dark')
  const darkBg = await evaluate(`return getComputedStyle(document.querySelector('.monitor-list')).backgroundColor;`)
  assert.notEqual(darkBg, listStats.lightBg, '深色模式下列表背景色应切换为深色容器色')
  await capture('03-list-390px', 390, 980, 'light')
  await assertNoClippedButtons('390px 列表页')

  // ── 1b. 列表行内「免输重登」与批量免输重登进度截图 ──
  await setViewport(1440, 900, 'light')
  await evaluate(`
    window.__LOGIN_DELAY_MS__ = 500;
    const allCheckbox = document.querySelector('.monitor-toolbar input[type="checkbox"]');
    allCheckbox.click();
    await new Promise(r => setTimeout(r, 120));
    const batchBtn = [...document.querySelectorAll('.selection-bar button')].find(b => b.textContent.includes('批量免输重登'));
    batchBtn.click();
    await new Promise(r => setTimeout(r, 150));
  `)
  const batchProgressStats = await evaluate(`
    const barText = document.querySelector('.selection-bar')?.textContent ?? '';
    return {
      hasReloginProgress: barText.includes('正在批量免输重登'),
      hasSkippedReason: barText.includes('跳过') && barText.includes('未存密码'),
    };
  `)
  assert.equal(batchProgressStats.hasReloginProgress, true, '批量重登时底部操作栏应显示进度')
  assert.equal(batchProgressStats.hasSkippedReason, true, '批量重登时应说明跳过未存密码账号的原因')
  await capture('13-list-relogin-batch-progress', 1440, 900, 'light')
  await sleep(1600)
  await evaluate(`
    window.__LOGIN_DELAY_MS__ = 0;
  `)

  // 验证筛选芯片点击与恢复
  await setViewport(1440, 900, 'light')
  const filteredRows = await evaluate(`
    const payChip = [...document.querySelectorAll('.filter-chip')].find(el => el.textContent.includes('扣款异常'));
    payChip.click();
    await new Promise(r => setTimeout(r, 80));
    const count = document.querySelectorAll('.monitor-row').length;
    const allChip = [...document.querySelectorAll('.filter-chip')].find(el => el.textContent.includes('全部'));
    allChip.click();
    await new Promise(r => setTimeout(r, 80));
    return count;
  `)
  assert.equal(filteredRows, 1, '点击「扣款异常」芯片后应只显示 1 行')
  const listUsage = await evaluate(`
    return {
      chips: [...document.querySelectorAll('.filter-chip')].map(el => el.textContent),
      bars: document.querySelectorAll('.monitor-row .mini-bars').length,
      donuts: document.querySelectorAll('.monitor-row .mini-donut').length,
      firstBars: document.querySelector('.monitor-row .mini-bars')?.querySelectorAll('.mini-bar').length ?? 0,
      tones: [...document.querySelectorAll('.monitor-row')].map(el => el.dataset.tone),
    };
  `)
  assert.ok(!listUsage.chips.some(t => t.includes('Work')), '列表不再有 Work 筛选')
  assert.equal(listUsage.bars, 2, '查过用量的两行都有每日 Token 柱状图')
  assert.equal(listUsage.donuts, 2, '查过用量的两行都有使用位置饼图')
  assert.equal(listUsage.firstBars, 7, '每天一根柱子')
  assert.ok(!listUsage.tones.includes('work'), '行左竖条不再有 Work 紫色')

  // 点柱子弹出当天详情
  const barTip = await evaluate(`
    const bars = document.querySelectorAll('.monitor-row .mini-bars')[0].querySelectorAll('.mini-bar');
    bars[5].click();
    await new Promise(r => setTimeout(r, 80));
    const tip = document.querySelector('.mini-bars-tip');
    return tip ? tip.textContent.replace(/\\s+/g, ' ').trim() : '';
  `)
  assert.match(barTip, /11\/5 周四/, '点柱子应显示日期和星期')
  assert.match(barTip, /4,700,000 Token/, '点柱子应显示当天 Token 原值')
  assert.match(barTip, /占 7 天的/, '点柱子应显示占这段时间的比例')
  await capture('14-list-bar-tip', 1440, 900, 'light')
  await evaluate(`document.querySelector('.mini-bar.is-active')?.click();`)

  // ── 2. 详情页（官方用量 + 订阅账单）验收与截图 ──
  await evaluate(`
    document.querySelector('.row-title-btn').click();
    await new Promise(r => setTimeout(r, 150));
    const buttons = [...document.querySelectorAll('button')];
    buttons.find(b => b.textContent.trim() === '查询用量')?.click();
    buttons.find(b => b.textContent.includes('查询订阅与账单'))?.click();
    await new Promise(r => setTimeout(r, 250));
  `)

  const detailStats = await evaluate(`
    const buttons = [...document.querySelectorAll('button')].map(b => b.textContent.trim());
    return {
      verdictVisible: Boolean(document.querySelector('.verdict-panel')),
      segmentedCount: document.querySelectorAll('.detail-topbar .z-segmented').length,
      rateTiles: document.querySelectorAll('.rate-tile').length,
      trendCharts: document.querySelectorAll('.trend-chart').length,
      billingRows: document.querySelectorAll('.billing-row').length,
      invoiceInputs: document.querySelectorAll('.invoice-url').length,
      hasRefreshBtn: buttons.includes('刷新'),
    };
  `)
  assert.equal(detailStats.verdictVisible, true, '详情页顶部应有身份头部')
  assert.equal(detailStats.segmentedCount, 0, '详情页应去掉分段切换器')
  assert.equal(detailStats.rateTiles, 4, '详情页顶部应有 4 个核心指标块')
  assert.equal(detailStats.trendCharts, 2, '查询用量后应渲染每日位置走势图和每日 Token 走势图')
  assert.equal(detailStats.billingRows, 2, '查询订阅与账单后应渲染 2 条账单记录')
  assert.equal(detailStats.invoiceInputs, 0, '账单行不应再包含只读网址输入框')
  assert.equal(detailStats.hasRefreshBtn, true, '查询后按钮应变为「刷新」')

  await capture('04-detail-usage-billing-light', 1440, 1380, 'light')
  await capture('05-detail-usage-billing-dark', 1440, 1380, 'dark')
  await capture('06-detail-usage-billing-390px', 390, 1650, 'light')
  await assertNoClippedButtons('390px 详情页（用量与账单）')

  // ── 3. 详情页（重新登录 + 登录设备）验收与截图 ──
  await setViewport(1440, 1100, 'light')
  await evaluate(`
    const buttons = [...document.querySelectorAll('button')];
    buttons.find(b => b.textContent.includes('查询设备'))?.click();
    buttons.find(b => b.textContent.includes('浏览器手动登录'))?.click();
    await new Promise(r => setTimeout(r, 200));
    const genBtn = [...document.querySelectorAll('button')].find(b => b.textContent.includes('生成授权链接'));
    genBtn?.click();
    await new Promise(r => setTimeout(r, 200));
    document.querySelector('#account-login-section')?.scrollIntoView({ behavior: 'instant', block: 'start' });
    await new Promise(r => setTimeout(r, 150));
  `)

  const deviceStats = await evaluate(`
    return {
      deviceRows: document.querySelectorAll('.device-row').length,
      loginSection: Boolean(document.querySelector('#account-login-section')),
    };
  `)
  assert.equal(deviceStats.deviceRows, 3, '点击查询设备后应渲染 3 台登录设备行')
  assert.equal(deviceStats.loginSection, true, '详情页应包含重新登录卡片')

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

  // ── 4. 导入账号页验收与截图 ──
  await evaluate(`
    const loginTab = [...document.querySelectorAll('[role="radio"], .z-segmented button, .z-segmented-item')].find(el => el.textContent.includes('导入账号'));
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

  await capture('10-login-import-light', 1440, 1100, 'light')
  await capture('11-login-import-dark', 1440, 1100, 'dark')
  await capture('12-login-import-390px', 390, 1380, 'light')
  await assertNoClippedButtons('390px 导入账号页')

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
