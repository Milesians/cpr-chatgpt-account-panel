export interface IdentityInfo {
  profile: string
  deviceId: string
}

export interface StoredLoginError {
  stage: string
  code: string
  message: string
  atMs: number
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
  admin_config_invalid: 'CPR 管理接口未配置',
  admin_error: 'CPR 管理接口调用失败',
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
  workspace_ambiguous: '存在多个工作区需指定',
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
  missing_email: '缺少邮箱',
  missing_password: '缺少密码',
  invalid_totp: '2FA 格式无效',
  invalid_kind: '类型无效',
  account_unavailable: '账号已停用或不可用',
  credential_changed: '凭据版本冲突',
  network_timeout: '连接超时',
  network: '网络请求失败',
}

export function errorCodeLabel(code?: string | null): string {
  if (!code)
    return '未知错误'
  return ERROR_LABELS[code] ?? code
}

export function needsManualOAuth(code?: string | null): boolean {
  if (!code)
    return false
  return [
    'browser_challenge',
    'email_otp_required',
    'unsupported_challenge',
    'unsupported_mfa',
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
