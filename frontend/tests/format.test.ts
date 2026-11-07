import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  attention,
  billingTag,
  credentialStateLabel,
  daysLeft,
  formatPlanName,
  formatPlanWithKind,
  hasHighWeeklyQuota,
  hasPaymentAnomaly,
  isTeamPlan,
  isUnchecked,
  isUnqueryable,
  matchesAccountFilter,
  money,
  needsCredentialRelogin,
  paymentIssue,
  paymentText,
  rowAccentTone,
  shortDate,
  windowName,
} from '../src/format.ts'

test('payment issues and text', () => {
  assert.equal(paymentIssue('Past_Due'), 'failed')
  assert.equal(paymentIssue('open'), 'due')
  assert.equal(paymentIssue('paid'), null)
  assert.equal(billingTag('succeeded'), 'success')
  assert.equal(billingTag('failed'), 'danger')
  assert.equal(billingTag('pending'), 'warning')
  assert.equal(billingTag('refunded'), 'default')
  assert.match(paymentText({
    status: 'paid',
    created_at_ms: Date.now() - 86400_000,
    checked_at_ms: Date.now() - 3600_000,
    amount: 2000,
    currency: 'USD',
    plan: 'plus',
  }), /1 小时前查/)
})

test('plan formatting and kind labels', () => {
  assert.equal(formatPlanName('plus'), 'Plus')
  assert.equal(formatPlanName('PRO'), 'Pro')
  assert.equal(formatPlanName('team'), 'Team')
  assert.equal(formatPlanName('business'), 'Business')
  assert.equal(formatPlanName('enterprise'), 'Enterprise')
  assert.equal(formatPlanName('edu'), 'Edu')
  assert.equal(formatPlanName('free'), 'Free')
  assert.equal(formatPlanName('free_workspace'), 'Free Workspace')
  assert.equal(formatPlanName('CUSTOM_PLAN'), 'Custom Plan')
  assert.equal(formatPlanName('CustomTier'), 'CustomTier')

  assert.equal(isTeamPlan('team'), true)
  assert.equal(isTeamPlan('business'), true)
  assert.equal(isTeamPlan('plus'), false)

  assert.equal(formatPlanWithKind('team', 'team'), 'Team')
  assert.equal(formatPlanWithKind('business', 'team'), 'Business')
  assert.equal(formatPlanWithKind('plus', 'personal'), 'Plus · 个人')
  assert.equal(formatPlanWithKind('pro', 'personal'), 'Pro · 个人')
  assert.equal(formatPlanWithKind(null, 'personal'), '个人')
})

test('credential_state mapping', () => {
  assert.equal(credentialStateLabel('ready'), '凭据有效')
  assert.equal(credentialStateLabel('expired'), '已过期')
  assert.equal(credentialStateLabel('banned'), '已封禁')
  assert.equal(credentialStateLabel('invalid'), '凭据无效')
  assert.equal(credentialStateLabel('missing'), '缺少凭据')
  assert.equal(credentialStateLabel('unconfigured'), '未配置凭据')
  assert.equal(credentialStateLabel('revoked'), '已吊销')
  assert.equal(credentialStateLabel(''), '状态未知')
  assert.equal(credentialStateLabel('other_state'), '状态异常')
})

test('needsCredentialRelogin check', () => {
  assert.equal(needsCredentialRelogin({ credential_state: 'ready', blocker: null } as any), false)
  assert.equal(needsCredentialRelogin({ credential_state: 'expired', blocker: null } as any), true)
  assert.equal(needsCredentialRelogin({ credential_state: 'ready', blocker: { code: 'token_expired', message: 'x' } } as any), true)
  assert.equal(needsCredentialRelogin({ credential_state: 'ready', blocker: { code: 'account_unavailable', message: 'x' } } as any), false)
})

test('money in the smallest unit', () => {
  assert.match(money(2000, 'USD'), /20\.00/)
  assert.match(money(3000, 'JPY'), /3,000/)
  assert.equal(money(undefined, 'USD'), '—')
})

test('window names follow length, not position', () => {
  assert.equal(windowName(300), '5 小时')
  assert.equal(windowName(10080), '每周')
  assert.equal(windowName(1440), '1 天')
  assert.equal(windowName(0), '额度')
})

test('dates', () => {
  assert.equal(shortDate('2026-11-06'), '11/6')
  assert.equal(daysLeft(Date.now() + 1.5 * 86400000), 2)
  assert.equal(daysLeft(Date.now() - 1000), 0)
})

test('work usage sorts first, then a nearly full week, then unchecked', () => {
  const base = { checked_at_ms: 1, days: 7, surfaces: [], windows: [] as [number, number][], problems: [] }
  const acct = (usage: any) => ({ usage }) as any
  assert.equal(attention(acct({ ...base, work: { total: 1, share: 0.1, days_with_use: 1, surfaces: [] } })), 0)
  assert.equal(attention(acct({ ...base, windows: [[10080, 95]] })), 1)
  assert.equal(attention(acct(null)), 2)
  assert.equal(attention(acct({ ...base, error: { code: 'x', message: 'y' } })), 2)
  assert.equal(attention(acct({ ...base, work: { total: 0, share: 0, days_with_use: 0, surfaces: [] } })), 3)
})

test('account filter classification and row accent tones', () => {
  const baseUsage = { checked_at_ms: 1, days: 7, surfaces: [], windows: [] as [number, number][], problems: [] }
  const workAcct = {
    id: 'a1',
    usage: { ...baseUsage, work: { total: 12, share: 0.35, days_with_use: 3, surfaces: [] } },
    payment: { status: 'paid', created_at_ms: 1, checked_at_ms: 1 },
  } as any
  const failedAcct = {
    id: 'a2',
    usage: { ...baseUsage, work: { total: 0, share: 0, days_with_use: 0, surfaces: [] } },
    payment: { status: 'past_due', created_at_ms: 1, checked_at_ms: 1 },
  } as any
  const usageErrorAcct = {
    id: 'a_err',
    usage: { ...baseUsage, error: { code: 'oauth_unauthorized', message: '查询失败' } },
    payment: null,
  } as any
  const quota90Acct = {
    id: 'a3',
    quota: { windows: [{ window_seconds: 604800, used_percent: 92 }] },
    usage: null,
    payment: null,
  } as any
  const blockedAcct = {
    id: 'a4',
    blocker: { code: 'token_expired', message: '令牌已过期' },
    usage: null,
    payment: null,
  } as any

  assert.equal(matchesAccountFilter(workAcct, 'work'), true)
  assert.equal(matchesAccountFilter(workAcct, 'payment'), false)
  assert.equal(rowAccentTone(workAcct), 'work')

  assert.equal(hasPaymentAnomaly(failedAcct), true)
  assert.equal(matchesAccountFilter(failedAcct, 'payment'), true)
  assert.equal(rowAccentTone(failedAcct), 'bad')

  assert.equal(rowAccentTone(usageErrorAcct), 'bad')

  assert.equal(hasHighWeeklyQuota(quota90Acct), true)
  assert.equal(isUnchecked(quota90Acct), true)
  assert.equal(matchesAccountFilter(quota90Acct, 'quota90'), true)
  assert.equal(matchesAccountFilter(quota90Acct, 'unchecked'), true)
  assert.equal(rowAccentTone(quota90Acct), 'warn')

  assert.equal(isUnqueryable(blockedAcct), true)
  assert.equal(isUnchecked(blockedAcct), false)
  assert.equal(matchesAccountFilter(blockedAcct, 'blocked'), true)
  assert.equal(rowAccentTone(blockedAcct), 'warn')
})
