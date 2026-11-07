import assert from 'node:assert/strict'
import { test } from 'node:test'
import { attention, billingTag, daysLeft, money, paymentIssue, shortDate, windowName } from '../src/format.ts'

test('payment issues', () => {
  assert.equal(paymentIssue('Past_Due'), 'failed')
  assert.equal(paymentIssue('open'), 'due')
  assert.equal(paymentIssue('paid'), null)
  assert.equal(billingTag('succeeded'), 'success')
  assert.equal(billingTag('failed'), 'danger')
  assert.equal(billingTag('pending'), 'warning')
  assert.equal(billingTag('refunded'), 'default')
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
