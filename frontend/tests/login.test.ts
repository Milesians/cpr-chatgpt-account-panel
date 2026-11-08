import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  buildImportQueue,
  errorCodeLabel,
  formatLoginError,
  hasKnownErrorCode,
  loginChoices,
  needsManualOAuth,
  resolveImportAction,
  resolveLoginMainAction,
} from '../src/login/helpers.ts'

test('maps login error codes and detects manual OAuth fallback conditions', () => {
  assert.equal(errorCodeLabel('browser_challenge'), '需浏览器验证（Cloudflare）')
  assert.equal(errorCodeLabel('source_proxy_unavailable'), '账号绑定代理不可用')
  assert.equal(errorCodeLabel('no_carrier'), '代理还没有账号在用')
  assert.equal(errorCodeLabel('mfa_issue_failed'), '发起 2FA 验证失败')
  assert.equal(errorCodeLabel('email_missing'), '缺少邮箱')
  assert.equal(errorCodeLabel('password_missing'), '缺少密码')
  assert.equal(errorCodeLabel('login_rejected'), '认证请求被拒绝')
  assert.equal(errorCodeLabel('login_forbidden'), '认证请求被禁止')
  assert.equal(errorCodeLabel('upstream_rejected'), '上游认证服务错误')
  assert.equal(errorCodeLabel('invalid_redirect'), '跳转地址无效')
  assert.equal(errorCodeLabel('unsupported_redirect'), '跳转目标不受信任')
  assert.equal(errorCodeLabel('invalid_response'), '响应数据无效')
  assert.equal(errorCodeLabel('unexpected_callback'), '未预期收到回调跳转')
  assert.equal(errorCodeLabel('unsupported_login_step'), '未知的登录步骤')
  assert.equal(errorCodeLabel('too_many_steps'), '登录跳转步骤过多')
  assert.equal(errorCodeLabel('internal_error'), '内部错误')
  assert.equal(errorCodeLabel('ambiguous_existing'), 'CPR 中存在多个同名账号')
  assert.equal(errorCodeLabel('unsupported_account'), '不支持的账号类型')
  assert.equal(errorCodeLabel('not_found'), '账号不存在')
  assert.equal(errorCodeLabel(null), '未知错误')

  assert.equal(hasKnownErrorCode('email_missing'), true)
  assert.equal(hasKnownErrorCode('unknown_raw_code'), false)

  assert.equal(formatLoginError('password_rejected', '密码错误'), '[密码被拒绝] 密码错误')
  assert.equal(formatLoginError('unknown_raw_code', '后端直接返回的中文原因'), '后端直接返回的中文原因')

  assert.equal(needsManualOAuth('browser_challenge'), true)
  assert.equal(needsManualOAuth('email_otp_required'), true)
  assert.equal(needsManualOAuth('password_rejected'), false)
  assert.equal(needsManualOAuth('no_carrier'), true)
})

test('button labels and disabled reasons follow state changes', () => {
  // 1. 详情重登主按钮：有已存密码且未输入新密码 → 免输重登
  assert.deepEqual(
    resolveLoginMainAction({ enabled: true, hasPassword: true, inputPassword: '' }),
    { label: '免输重登', disabled: false },
  )
  // 输入新密码 → 登录并写回 CPR
  assert.deepEqual(
    resolveLoginMainAction({ enabled: true, hasPassword: true, inputPassword: 'new-pw' }),
    { label: '登录并写回 CPR', disabled: false },
  )
  // 无已存密码且未输入 → 禁用并带中文原因
  assert.deepEqual(
    resolveLoginMainAction({ enabled: true, hasPassword: false, inputPassword: '' }),
    { label: '登录并写回 CPR', disabled: true, title: '请先输入登录密码' },
  )
  // 停用账号 → 禁用并说明原因
  assert.deepEqual(
    resolveLoginMainAction({ enabled: false, hasPassword: true, inputPassword: '' }),
    { label: '免输重登', disabled: true, title: '账号在 CPR 里已停用，无法发起登录' },
  )

  // 2. 导入主按钮：随队列长度和状态变化
  assert.deepEqual(
    resolveImportAction({ rawText: '', report: null, queueLength: 0, importing: false, includeExistingUpdates: false }),
    { label: '导入 0 个', disabled: true, title: '没有内容：请先粘贴要导入的账号文本' },
  )
  assert.deepEqual(
    resolveImportAction({
      rawText: 'a@b.com----pw',
      report: { applied: false, lines: 1, created: 0, updated: 1, skipped: 0, existingInCpr: 1, items: [] },
      queueLength: 0,
      importing: false,
      includeExistingUpdates: false,
    }),
    { label: '导入 0 个', disabled: true, title: '都已在 CPR，勾选更新才会重登' },
  )
  assert.deepEqual(
    resolveImportAction({
      rawText: 'a@b.com----pw',
      report: { applied: false, lines: 2, created: 2, updated: 0, skipped: 0, existingInCpr: 0, items: [] },
      queueLength: 2,
      importing: false,
      includeExistingUpdates: false,
    }),
    { label: '导入 2 个', disabled: false },
  )
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

test('reads workspace choices from error details and drops odd entries', () => {
  assert.deepEqual(loginChoices(undefined), [])
  assert.deepEqual(loginChoices({ choices: 'x' }), [])
  assert.deepEqual(loginChoices({ choices: [
    { kind: 'workspace', id: 'ws-a', name: '个人' },
    { kind: 'workspace', id: '', name: 'x' },
    { kind: 'project', id: 'p', name: 'x' },
    null,
    { kind: 'organization', id: 'org-1', name: '' },
  ] }), [
    { kind: 'workspace', id: 'ws-a', name: '个人' },
    { kind: 'organization', id: 'org-1', name: '' },
  ])
})
