// 页面只经宿主桥访问插件自己注册的管理路由，不碰 CPR 的管理接口、Cookie 或 Key 原文

export interface ManagementReply {
  status: number
  contentType: string
  body: ArrayBuffer
}

export interface PluginHost {
  readonly version: 2
  readonly theme: 'light' | 'dark'
  request: (input: {
    method: 'GET' | 'POST'
    path: string
    query?: string
    contentType?: string
    body?: string
  }) => Promise<ManagementReply>
}

export interface Problem {
  code: string
  message: string
}

export interface Share {
  key: string
  value: number
}

export interface Work {
  total: number
  /** 占这段时间全部用量的比例，0～1 */
  share: number
  last_day?: string
  days_with_use: number
  surfaces: Share[]
}

/** 列表上记下的最近一次官方用量查询 */
export interface UsageSummary {
  checked_at_ms: number
  days: number
  unit?: 'relative' | 'credits'
  work?: Work
  surfaces: Share[]
  total?: number
  /** 每天的 Token；旧记录和 Token 没读到时没有 */
  daily_tokens?: { date: string, tokens: number }[]
  /** [窗口分钟数, 已用百分比] */
  windows: [number, number][]
  problems: string[]
  error?: Problem
}

export interface LastPayment {
  created_at_ms: number
  amount: number
  currency: string
  status: string
  plan: string
  checked_at_ms: number
  grace_ends_at_ms?: number
}

export interface QuotaWindow {
  window_seconds: number | null
  used_percent: number | null
  reset_at_ms: number | null
}

export interface Account {
  id: string
  name: string
  email: string | null
  plan_type: string | null
  kind: 'personal' | 'team' | 'unknown'
  enabled: boolean
  credential_state: string
  access_token_expires_at_ms: number | null
  /** 现在查不了的原因 */
  blocker: Problem | null
  /** CPR 已记下的额度（不访问上游） */
  quota: { observed_at_ms: number | null, windows: QuotaWindow[] } | null
  usage: UsageSummary | null
  payment: LastPayment | null
}

export interface Window {
  used_percent: number
  window_minutes: number
  resets_at_ms?: number
}

export interface BreakdownDay {
  date: string
  total: number
  surfaces: Record<string, number>
}

export interface CodexUsage {
  checked_at_ms: number
  days: number
  limits: { plan_type?: string, allowed?: boolean, limit_reached?: boolean, windows: Window[] } | null
  tokens: { daily: { date: string, tokens: number }[], lifetime?: number, peak_daily?: number, threads?: number, stats_as_of_ms?: number } | null
  breakdown: {
    unit: 'relative' | 'credits'
    updated_at_ms?: number
    total: number
    days: BreakdownDay[]
    surfaces: Share[]
    features: Share[]
    models: Share[]
  } | null
  work: Work | null
  problems?: Record<string, string>
}

export interface Subscription {
  plan_type: string
  status: 'active' | 'past_due' | 'inactive' | 'expired' | 'free' | 'unknown'
  expires_at_ms?: number
  will_renew?: boolean
  delinquent_since_ms?: number
  grace_ends_at_ms?: number
  checked_at_ms: number
}

export interface Transaction {
  id: string
  created_at_ms: number
  amount: number
  currency: string
  status: string
  kind: string
  plan: string
  document_url?: string
}

export interface History {
  transactions: Transaction[]
  next_cursor?: string
  checked_at_ms: number
  subscription?: Subscription
}

export interface Device {
  /** 登出时用来指认设备；不是会话 ID */
  id: string
  name: string
  platform: string
  apps: string[]
  current: boolean
  /** CPR 令牌所在的会话：不能单独登出 */
  in_use: boolean
  trusted: boolean
  signed_out: boolean
  last_signed_in_ms?: number
  city: string
  region: string
  country: string
}

export interface Devices {
  devices: Device[]
  /** 恰好一台是 CPR 的会话：只有这时才能一起登出其他设备 */
  in_use_known: boolean
  checked_at_ms: number
}

export type SignOutTarget = { device: string } | { others: true } | { all: true }

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly details?: unknown
  constructor(message: string, status: number, code = '', details?: unknown) {
    super(message)
    this.status = status
    this.code = code
    this.details = details
  }
}

function host(): PluginHost {
  const bridge = window.codexProxyPlugin
  if (!bridge || bridge.version !== 2)
    throw new ApiError('请在 CPR 的插件页面里打开', 0)
  return bridge
}

export async function call<T>(method: 'GET' | 'POST', path: string, query?: Record<string, string>, data?: unknown): Promise<T> {
  const reply = await host().request({
    method,
    path,
    ...(query ? { query: new URLSearchParams(query).toString() } : {}),
    ...(data === undefined ? {} : { contentType: 'application/json', body: JSON.stringify(data) }),
  })
  const text = new TextDecoder().decode(reply.body)
  let value: any
  try {
    value = JSON.parse(text)
  }
  catch {
    if (reply.status < 200 || reply.status >= 300)
      throw new ApiError(describeFailure(reply.status, '', snippet(text)), reply.status)
    throw new ApiError(`插件返回了无效响应（HTTP ${reply.status}）`, reply.status)
  }
  if (reply.status < 200 || reply.status >= 300)
    throw errorFromBody(reply.status, value, text)
  return value as T
}

function snippet(text: string): string {
  return text.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 200)
}

/**
 * 插件自己的错误是 `{error:{code,message,details}}`；CPR 宿主的错误信封是顶层
 * `{code:50201,message}`，也见过 `{error:"…"}`。都认出来，别把真实原因吞成一句「HTTP 502」
 */
export function errorFromBody(status: number, value: any, text = ''): ApiError {
  const e = value?.error
  if (e && typeof e === 'object') {
    const message = typeof e.message === 'string' ? e.message.trim() : ''
    const code = typeof e.code === 'string' ? e.code : ''
    return new ApiError(message || describeFailure(status, code, ''), status, code, e.details)
  }
  if (typeof e === 'string' && e.trim())
    return new ApiError(e.trim(), status)
  const message = typeof value?.message === 'string' ? value.message.trim() : ''
  const hostCode = typeof value?.code === 'number' ? ` · ${value.code}` : ''
  if (message)
    return new ApiError(`CPR：${message}（HTTP ${status}${hostCode}）`, status)
  return new ApiError(describeFailure(status, '', snippet(text)), status)
}

function describeFailure(status: number, code: string, body: string): string {
  const hint = status === 502 || status === 503 || status === 504
    ? 'CPR 没拿到插件的正常响应（插件调用超时、崩溃或被重启），请看 CPR 日志里这一时刻的插件记录'
    : '请求失败'
  const tail = [code, body].filter(Boolean).join('：')
  return `${hint}（HTTP ${status}）${tail ? `：${tail}` : ''}`
}

export const api = {
  accounts: () => call<{ accounts: Account[], checked_at_ms: number }>('GET', 'api/accounts'),
  usage: (id: string, days: number) => call<{ usage: CodexUsage, summary: UsageSummary }>('GET', 'api/usage', { id, days: String(days) }),
  subscription: (id: string) => call<{ subscription: Subscription }>('GET', 'api/subscription', { id }),
  devices: (id: string) => call<{ devices: Devices }>('GET', 'api/devices', { id }),
  signOut: (id: string, target: SignOutTarget) => call<{ signed_out: number, cpr_voided?: boolean }>('POST', 'api/devices/sign-out', undefined, { id, ...target }),
  billing: (id: string, cursor = '') => call<{ history: History, payment: LastPayment | null }>('GET', 'api/billing', cursor ? { id, cursor } : { id }),
}
