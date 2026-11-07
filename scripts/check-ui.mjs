// Headless UI verification & screenshot generator for cpr-chatgpt-account-panel.
// Renders the built frontend bundle against a mocked CPR host bridge across
// light, dark, and 390px narrow viewports, verifies layout/DOM invariants,
// and saves persistent screenshots for List, Detail (Usage & Billing),
// Detail (Devices & Login), and Login & Import pages.

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
    id: 'acc-fresh',
    name: 'pro-unchecked',
    email: 'fresh@example.com',
    plan_type: 'pro',
    kind: 'personal',
    enabled: true,
    credential_state: 'ready',
    access_token_expires_at_ms: now + 4 * dayMs,
    quota: {
      windows: [
        { window_seconds: 18000, used_percent: 5 },
        { window_seconds: 604800, used_percent: 12 },
      ],
    },
    usage: null,
    payment: null,
    blocker: null,
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
      message: '访问令牌已过期，请先在 CPR 刷新账号',
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
        plan: 'ChatGPT Team (5 seats)',
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
        plan: 'ChatGPT Team (5 seats)',
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
      code: 'cloudflare_blocked',
      message: '遇到 Cloudflare 质询拦截，请使用手动 OAuth 授权链接兜底',
      atMs: now - 1800_000,
    },
    pendingOauth: null,
  },
]

const mockPastePreview = {
  lines: 3,
  created: 2,
  updated: 1,
  skipped: 0,
  failed: 0,
  items: [
    {
      line: 1,
      email: 'new-plus@acme.io',
      kind: 'personal',
      hasTotp: true,
      action: 'create',
      status: 'pending',
      message: '新个人账号，待执行 OAuth 登录建入 CPR',
    },
    {
      line: 2,
      email: 'new-team@acme.io',
      kind: 'team',
      hasTotp: false,
      action: 'create',
      status: 'pending',
      message: '新 Team 账号，待执行 OAuth 登录建入 CPR',
    },
    {
      line: 3,
      email: 'alpha@acme.io',
      kind: 'team',
      hasTotp: true,
      action: 'update',
      status: 'pending',
      accountId: 'acc-work',
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
      return __reply({
        accountId: payload?.accountId || 'acc-work',
        email: 'alpha@acme.io',
        planType: 'team',
        expiresAtMs: Date.now() + 7 * 86400000,
        account: __MOCK__.loginOverview.accounts[0],
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
    const overflowX = document.documentElement.scrollWidth > window.innerWidth + 1;
    const lightBg = getComputedStyle(document.querySelector('.monitor-list')).backgroundColor;
    return { chips, rows, overflowX, lightBg };
  `)
  assert.equal(listStats.rows, 4, '列表应渲染 4 个账号监控行')
  assert.equal(listStats.chips.length, 6, '列表顶部应有 6 个分类计数筛选芯片')
  assert.equal(listStats.overflowX, false, '桌面宽度下列表不能出现横向溢出')

  await capture('01-list-light', 1440, 900, 'light')
  await capture('02-list-dark', 1440, 900, 'dark')
  const darkBg = await evaluate(`return getComputedStyle(document.querySelector('.monitor-list')).backgroundColor;`)
  assert.notEqual(darkBg, listStats.lightBg, '深色模式下列表背景色应切换为深色容器色')
  await capture('03-list-390px', 390, 920, 'light')

  const listMobileOverflow = await evaluate(`
    return document.documentElement.scrollWidth > window.innerWidth + 1;
  `)
  assert.equal(listMobileOverflow, false, '390px 手机宽度下列表不能出现横向溢出')

  // 验证筛选芯片点击与恢复
  await setViewport(1440, 900, 'light')
  const filteredRows = await evaluate(`
    const workChip = [...document.querySelectorAll('.filter-chip')].find(el => el.textContent.includes('Work 有用量'));
    workChip.click();
    await new Promise(r => setTimeout(r, 80));
    const count = document.querySelectorAll('.monitor-row').length;
    const allChip = [...document.querySelectorAll('.filter-chip')].find(el => el.textContent.includes('全部'));
    allChip.click();
    await new Promise(r => setTimeout(r, 80));
    return count;
  `)
  assert.equal(filteredRows, 1, '点击「Work 有用量」芯片后应只显示 1 行')

  // ── 2. 详情页（官方用量 + 订阅账单）验收与截图 ──
  await evaluate(`
    document.querySelector('.row-title-btn').click();
    await new Promise(r => setTimeout(r, 150));
    const buttons = [...document.querySelectorAll('button')];
    buttons.find(b => b.textContent.trim() === '查询')?.click();
    buttons.find(b => b.textContent.includes('查询账单'))?.click();
    await new Promise(r => setTimeout(r, 250));
  `)

  const detailStats = await evaluate(`
    return {
      verdictVisible: Boolean(document.querySelector('.verdict-panel')),
      rateTiles: document.querySelectorAll('.rate-tile').length,
      trendCharts: document.querySelectorAll('.trend-chart').length,
      billingRows: document.querySelectorAll('.billing-row').length,
    };
  `)
  assert.equal(detailStats.verdictVisible, true, '详情页顶部应有 verdict-panel')
  assert.equal(detailStats.rateTiles, 4, '详情页顶部应有 4 个核心指标块')
  assert.equal(detailStats.trendCharts, 2, '查询用量后应渲染每日位置走势图和每日 Token 走势图')
  assert.equal(detailStats.billingRows, 2, '查询账单后应渲染 2 条卡片化账单记录')

  await capture('04-detail-usage-billing-light', 1440, 1380, 'light')
  await capture('05-detail-usage-billing-dark', 1440, 1380, 'dark')
  await capture('06-detail-usage-billing-390px', 390, 1650, 'light')

  // ── 3. 详情页（登录设备 + 重新登录与凭据管理）验收与截图 ──
  await setViewport(1440, 1100, 'light')
  await evaluate(`
    const buttons = [...document.querySelectorAll('button')];
    buttons.find(b => b.textContent.includes('查看设备'))?.click();
    buttons.find(b => b.textContent.includes('手动 OAuth 链接兜底'))?.click();
    await new Promise(r => setTimeout(r, 200));
    const genBtn = [...document.querySelectorAll('button')].find(b => b.textContent.includes('生成授权链接'));
    genBtn?.click();
    await new Promise(r => setTimeout(r, 200));
    document.querySelector('#devices-title')?.scrollIntoView({ behavior: 'instant', block: 'start' });
    await new Promise(r => setTimeout(r, 150));
  `)

  const deviceStats = await evaluate(`
    return {
      deviceRows: document.querySelectorAll('.device-row').length,
      loginSection: Boolean(document.querySelector('#account-login-section')),
    };
  `)
  assert.equal(deviceStats.deviceRows, 3, '点击查看设备后应渲染 3 台登录设备卡片行')
  assert.equal(deviceStats.loginSection, true, '详情页应包含重新登录与凭据管理卡片')

  await capture('07-detail-devices-login-light', 1440, 1100, 'light')
  await capture('08-detail-devices-login-dark', 1440, 1100, 'dark')
  await capture('09-detail-devices-login-390px', 390, 1380, 'light')

  // 验证登出所有设备后显示「一键重新登录」与「前往快速登录区」入口
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
      hasQuickReloginBtn: text.includes('一键重新登录') && text.includes('前往快速登录区'),
    };
  `)
  assert.equal(voidedBannerCheck.hasVoidedNotice, true, '登出所有设备后应提示需要重新登录写回 CPR')
  assert.equal(voidedBannerCheck.hasQuickReloginBtn, true, '提示旁应直接提供一键重新登录与前往快速登录区入口')

  // ── 4. 登录与快速导入页验收与截图 ──
  await evaluate(`
    const loginTab = [...document.querySelectorAll('[role="radio"], .z-segmented button, .z-segmented-item')].find(el => el.textContent.includes('登录与导入'));
    loginTab.click();
    await new Promise(r => setTimeout(r, 200));
    const ta = document.querySelector('textarea');
    ta.value = 'new-plus@acme.io----pass123----JBSWY3DPEHPK3PXP----personal\\nnew-team@acme.io----pass456--------team\\nalpha@acme.io----pass789----JBSWY3DPEHPK3PXP----team';
    ta.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise(r => setTimeout(r, 100));
    const previewBtn = [...document.querySelectorAll('button')].find(b => b.textContent.includes('解析并预览'));
    previewBtn.click();
    await new Promise(r => setTimeout(r, 200));
    const expandBtn = [...document.querySelectorAll('button')].find(b => b.textContent.includes('输入凭据登录'));
    expandBtn?.click();
    await new Promise(r => setTimeout(r, 150));
  `)

  const loginPageStats = await evaluate(`
    return {
      previewRows: document.querySelectorAll('.import-preview-row').length,
      existingRows: document.querySelectorAll('.login-account-row').length,
    };
  `)
  assert.equal(loginPageStats.previewRows, 3, '快速导入预览应渲染 3 行卡片')
  assert.equal(loginPageStats.existingRows, 3, '已有账号快速重登列表应渲染 3 行卡片')

  await capture('10-login-import-light', 1440, 1280, 'light')
  await capture('11-login-import-dark', 1440, 1280, 'dark')
  await capture('12-login-import-390px', 390, 1580, 'light')

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
