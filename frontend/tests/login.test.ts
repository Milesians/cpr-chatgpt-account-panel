import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildImportQueue, errorCodeLabel, needsManualOAuth } from '../src/login/helpers.ts'

test('maps login error codes and detects manual OAuth fallback conditions', () => {
  assert.equal(errorCodeLabel('browser_challenge'), '需浏览器验证（Cloudflare）')
  assert.equal(errorCodeLabel('source_proxy_unavailable'), '账号绑定代理不可用')
  assert.equal(errorCodeLabel('encryption_key_missing'), '未配置加密密钥')
  assert.equal(errorCodeLabel(null), '未知错误')

  assert.equal(needsManualOAuth('browser_challenge'), true)
  assert.equal(needsManualOAuth('email_otp_required'), true)
  assert.equal(needsManualOAuth('password_rejected'), false)
})

test('builds one-by-one import queue from raw text and preview report', () => {
  const raw = [
    '# comment',
    'u1@example.com----pass1----JBSWY3DPEHPK3PXP----team',
    'u2@example.com----pass2--------个人',
    'bad-line',
  ].join('\n')

  const report = {
    applied: false,
    lines: 3,
    created: 1,
    updated: 1,
    skipped: 1,
    existingInCpr: 1,
    items: [
      {
        line: 2,
        email: 'u1@example.com',
        kind: 'team' as const,
        hasPassword: true,
        hasTotp: true,
        action: 'create' as const,
        inCpr: false,
      },
      {
        line: 3,
        email: 'u2@example.com',
        kind: 'personal' as const,
        hasPassword: true,
        hasTotp: false,
        action: 'update' as const,
        existingAccountId: 'acct_a',
        inCpr: true,
      },
      {
        line: 4,
        email: '',
        kind: 'personal' as const,
        hasPassword: false,
        hasTotp: false,
        action: 'skip' as const,
        inCpr: false,
        code: 'invalid_line_format',
      },
    ],
  }

  const createOnly = buildImportQueue(raw, report, false)
  assert.equal(createOnly.length, 1)
  assert.deepEqual(createOnly[0], {
    line: 2,
    email: 'u1@example.com',
    password: 'pass1',
    totp: 'JBSWY3DPEHPK3PXP',
    kind: 'team',
    action: 'create',
  })

  const withUpdates = buildImportQueue(raw, report, true)
  assert.equal(withUpdates.length, 2)
  assert.equal(withUpdates[1]?.existingAccountId, 'acct_a')
  assert.equal(withUpdates[1]?.totp, '')
})
