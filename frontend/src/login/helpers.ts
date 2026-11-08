export interface IdentityInfo {
  profile: string
  deviceId: string
}

/** 登录时可选的工作区或组织；点一下就带着它重试 */
export interface LoginChoice {
  kind: 'workspace' | 'organization'
  id: string
  name: string
}

export interface StoredLoginError {
  stage: string
  code: string
  message: string
  atMs: number
  choices?: LoginChoice[]
}

export interface PendingOAuthView {
  authorizeUrl: string
  expiresAtMs: number
}

export interface AccountLoginStatus {
  accountId: string
  name: string
  email: string
  kind: 'personal' | 'team' | 'unknown'
  enabled: boolean
  credentialState: string
  hasPassword: boolean
  hasTotp: boolean
  identity: IdentityInfo
  lastLoginAtMs?: number
  lastError?: StoredLoginError
  pendingOauth?: PendingOAuthView
}

export interface PasteItem {
  line: number
  email: string
  kind: 'personal' | 'team' | 'unknown'
  hasPassword: boolean
  hasTotp: boolean
  action: 'create' | 'update' | 'skip'
  existingAccountId?: string
  inCpr: boolean
  code?: string
  message?: string
}

export interface PasteReport {
  applied: boolean
  lines: number
  created: number
  updated: number
  skipped: number
  existingInCpr: number
  items: PasteItem[]
}

export interface QueuedImportTask {
  line: number
  email: string
  password: string
  totp: string
  kind: 'personal' | 'team'
  action: 'create' | 'update'
  existingAccountId?: string
}

const ERROR_LABELS: Record<string, string> = {
  admin_error: 'CPR 管理接口调用失败',
  proxy_missing: '没选代理',
  proxy_unavailable: '代理不可用',
  no_carrier: '代理还没有账号在用',
  mfa_issue_failed: '发起 2FA 验证失败',
  source_proxy_unavailable: '账号绑定代理不可用',
  browser_challenge: '需浏览器验证（Cloudflare）',
  rate_limited: '请求过于频繁（429）',
  password_rejected: '密码被拒绝',
  totp_rejected: '2FA 验证码被拒绝',
  totp_secret_missing: '缺少 2FA 密钥',
  totp_invalid: '2FA 密钥格式无效',
  unsupported_mfa: '不支持的 MFA 类型',
  email_otp_required: '需邮箱验证码（Email OTP）',
  unsupported_challenge: '需附加验证',
  workspace_unavailable: '工作区不可用',
  workspace_ambiguous: '有多个工作区，请选一个',
  identity_mismatch: '登录账号邮箱不匹配',
  invalid_token_identity: '返回令牌身份无效',
  incomplete_tokens: '返回令牌不完整',
  token_exchange_failed: '授权码换取令牌失败',
  invalid_callback: '回调链接无效',
  oauth_session_expired: '手动授权会话已过期',
  already_in_source: '已在 CPR 中',
  duplicate_in_input: '输入重复',
  invalid_line_format: '行格式错误',
  invalid_email: '邮箱格式无效',
  email_missing: '缺少邮箱',
  password_missing: '缺少密码',
  invalid_totp: '2FA 格式无效',
  invalid_kind: '类型无效',
  account_unavailable: '账号已停用或不可用',
  credential_changed: '凭据版本冲突',
  network_timeout: '连接超时',
  network: '网络请求失败',
  login_rejected: '认证请求被拒绝',
  login_forbidden: '认证请求被禁止',
  upstream_rejected: '上游认证服务错误',
  invalid_redirect: '跳转地址无效',
  unsupported_redirect: '跳转目标不受信任',
  invalid_response: '响应数据无效',
  unexpected_callback: '未预期收到回调跳转',
  unsupported_login_step: '未知的登录步骤',
  too_many_steps: '登录跳转步骤过多',
  internal_error: '内部错误',
  ambiguous_existing: 'CPR 中存在多个同名账号',
  unsupported_account: '不支持的账号类型',
  not_found: '账号不存在',
}

export function hasKnownErrorCode(code?: string | null): boolean {
  if (!code)
    return false
  return Object.prototype.hasOwnProperty.call(ERROR_LABELS, code)
}

export function errorCodeLabel(code?: string | null, fallbackMessage?: string | null): string {
  if (!code)
    return fallbackMessage?.trim() || '未知错误'
  return ERROR_LABELS[code] ?? (fallbackMessage?.trim() || code)
}

export function formatLoginError(code?: string | null, message?: string | null): string {
  const msg = message?.trim() ?? ''
  if (code && hasKnownErrorCode(code)) {
    const label = ERROR_LABELS[code]!
    return msg ? `[${label}] ${msg}` : label
  }
  return msg || errorCodeLabel(code)
}

/** 详情页重登主按钮文字与禁用原因 */
export function resolveLoginMainAction(state: {
  enabled: boolean
  hasPassword: boolean
  inputPassword: string
}): { label: string, disabled: boolean, title?: string } {
  const hasInput = state.inputPassword.trim().length > 0
  const label = !hasInput && state.hasPassword ? '免输重登' : '登录并写回 CPR'
  if (!state.enabled) {
    return { label, disabled: true, title: '账号在 CPR 里已停用，无法发起登录' }
  }
  if (!hasInput && !state.hasPassword) {
    return { label, disabled: true, title: '请先输入登录密码' }
  }
  return { label, disabled: false }
}

/** 导入按钮文字与禁用原因 */
export function resolveImportAction(state: {
  rawText: string
  report: PasteReport | null
  queueLength: number
  importing: boolean
  includeExistingUpdates: boolean
}): { label: string, disabled: boolean, title?: string } {
  const label = `导入 ${state.queueLength} 个`
  if (state.importing) {
    return { label, disabled: true, title: '正在逐个导入账号' }
  }
  if (!state.rawText.trim()) {
    return { label, disabled: true, title: '没有内容：请先粘贴要导入的账号文本' }
  }
  if (!state.report || state.queueLength === 0) {
    if (state.report && state.report.created === 0 && state.report.updated > 0 && !state.includeExistingUpdates) {
      return { label, disabled: true, title: '都已在 CPR，勾选更新才会重登' }
    }
    return { label, disabled: true, title: '没有可导入的行' }
  }
  return { label, disabled: false }
}

export function needsManualOAuth(code?: string | null): boolean {
  if (!code)
    return false
  return [
    'browser_challenge',
    'email_otp_required',
    'unsupported_challenge',
    'unsupported_mfa',
    'no_carrier',
  ].includes(code)
}

/**
 * 将粘贴原文与后端预览报告对齐，生成可逐条执行的导入任务队列（支持随时中断）
 */
export function buildImportQueue(
  rawText: string,
  report: PasteReport,
  includeExistingUpdates = false,
): QueuedImportTask[] {
  const lines = rawText.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n')
  const tasks: QueuedImportTask[] = []

  for (const item of report.items) {
    if (item.action !== 'create' && !(includeExistingUpdates && item.action === 'update'))
      continue
    const rawLine = (lines[item.line - 1] ?? '').trim()
    const parts = rawLine.split('----')
    if (parts.length < 2)
      continue
    const email = (parts[0] ?? '').trim().toLowerCase()
    const password = (parts[1] ?? '').trim()
    const totp = parts.length >= 3 ? (parts[2] ?? '').trim() : ''
    const kind: 'personal' | 'team' = item.kind === 'team' ? 'team' : 'personal'
    if (!email || !password)
      continue
    tasks.push({
      line: item.line,
      email,
      password,
      totp,
      kind,
      action: item.action,
      ...(item.existingAccountId ? { existingAccountId: item.existingAccountId } : {}),
    })
  }
  return tasks
}

/** 从错误的 `details.choices` 取出可点选的工作区或组织 */
export function loginChoices(details: unknown): LoginChoice[] {
  const list = (details as { choices?: unknown } | undefined)?.choices
  if (!Array.isArray(list))
    return []
  return list.filter((c): c is LoginChoice =>
    !!c && typeof c === 'object'
    && (c.kind === 'workspace' || c.kind === 'organization')
    && typeof c.id === 'string' && c.id !== ''
    && typeof c.name === 'string')
}
