import type { Account, LastPayment, Share, UsageSummary } from './api'

/** 一律按北京时间显示，和 CPR 其它插件一致 */
const TZ = 'Asia/Shanghai'

export function dateTime(ms: number | null | undefined): string {
  if (!ms)
    return '—'
  return new Date(ms).toLocaleString('zh-CN', { timeZone: TZ, hour12: false, month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
}

export function day(ms: number | null | undefined): string {
  if (!ms)
    return '—'
  return new Date(ms).toLocaleDateString('zh-CN', { timeZone: TZ, year: 'numeric', month: '2-digit', day: '2-digit' })
}

/** 2026-11-06 → 11/6 */
export function shortDate(date: string): string {
  const [, m, d] = date.split('-')
  return m && d ? `${Number(m)}/${Number(d)}` : date
}

/** 多久以前：刚刚 / 5 分钟前 / 3 小时前 / 2 天前 */
export function ago(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000))
  if (s < 60)
    return '刚刚'
  if (s < 3600)
    return `${Math.floor(s / 60)} 分钟前`
  if (s < 86400)
    return `${Math.floor(s / 3600)} 小时前`
  return `${Math.floor(s / 86400)} 天前`
}

/** 剩几天（向上取整，过了是 0） */
export function daysLeft(ms: number, now = Date.now()): number {
  return Math.max(0, Math.ceil((ms - now) / 86400000))
}

export function percent(v: number, digits = 0): string {
  return `${(v * 100).toFixed(digits)}%`
}

/** 300 → 5 小时，10080 → 每周 */
export function windowName(minutes: number): string {
  if (minutes === 300)
    return '5 小时'
  if (minutes === 10080)
    return '每周'
  if (!minutes)
    return '额度'
  if (minutes % 1440 === 0)
    return `${minutes / 1440} 天`
  if (minutes % 60 === 0)
    return `${minutes / 60} 小时`
  return `${minutes} 分钟`
}

export const kindLabel: Record<Account['kind'], string> = { personal: '个人', team: 'Team', unknown: '未知' }

const PLAN_LABELS: Record<string, string> = {
  plus: 'Plus',
  pro: 'Pro',
  team: 'Team',
  business: 'Business',
  enterprise: 'Enterprise',
  edu: 'Edu',
  free: 'Free',
  free_workspace: 'Free Workspace',
}

const TEAM_PLANS = new Set(['team', 'business', 'enterprise', 'edu', 'free_workspace'])

/** 套餐名格式化：首字母大写（Plus / Pro / Team / Business / Enterprise / Edu / Free，未知原样但不全大写） */
export function formatPlanName(plan: string | null | undefined): string {
  const trimmed = plan?.trim() ?? ''
  if (!trimmed)
    return ''
  const lower = trimmed.toLowerCase()
  if (PLAN_LABELS[lower])
    return PLAN_LABELS[lower]
  if (trimmed === trimmed.toUpperCase() && /[A-Z]/.test(trimmed)) {
    return trimmed
      .toLowerCase()
      .split(/[\s_-]+/)
      .filter(Boolean)
      .map(w => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ')
  }
  return trimmed
}

export function isTeamPlan(plan: string | null | undefined): boolean {
  const lower = plan?.trim().toLowerCase() ?? ''
  return TEAM_PLANS.has(lower)
}

/** 套餐与类型展示：Team 类套餐不再另加 Team 类型标签，个人套餐显示「Plus · 个人」 */
export function formatPlanWithKind(plan: string | null | undefined, kind: Account['kind']): string {
  const p = formatPlanName(plan)
  if (p) {
    if (isTeamPlan(plan) || kind === 'team')
      return p
    if (kind === 'personal')
      return `${p} · 个人`
    return p
  }
  return kindLabel[kind] ?? '未知'
}

export const credentialStateMap: Record<string, string> = {
  ready: '凭据有效',
  expired: '已过期',
  banned: '已封禁',
  invalid: '凭据无效',
  missing: '缺少凭据',
  unconfigured: '未配置凭据',
  revoked: '已吊销',
  unknown: '状态未知',
}

export function credentialStateLabel(state: string | null | undefined): string {
  const s = state?.trim().toLowerCase() ?? ''
  if (!s)
    return '状态未知'
  return credentialStateMap[s] ?? '状态异常'
}

export const billingStatusLabel: Record<string, string> = {
  paid: '已支付', succeeded: '已支付', refunded: '已退款', void: '已作废',
  due: '待续费', open: '待支付', pending: '待支付',
  failed: '扣款失败', past_due: '逾期未付', unpaid: '未支付', uncollectible: '无法收款',
  requires_payment_method: '需要换卡', requires_action: '需要验证', incomplete: '未完成',
}

const FAILED = ['failed', 'past_due', 'unpaid', 'uncollectible', 'requires_payment_method', 'requires_action', 'incomplete']
const DUE = ['due', 'open', 'pending']

/** 最近一笔账单的问题：扣款失败、待续费 */
export function paymentIssue(status: string | undefined): 'failed' | 'due' | null {
  const s = status?.trim().toLowerCase() ?? ''
  if (FAILED.includes(s))
    return 'failed'
  if (DUE.includes(s))
    return 'due'
  return null
}

export type TagType = 'default' | 'success' | 'warning' | 'danger' | 'info'

export function billingTag(status: string | undefined): TagType {
  const s = status?.trim().toLowerCase()
  if (s === 'paid' || s === 'succeeded')
    return 'success'
  const issue = paymentIssue(s)
  return issue === 'failed' ? 'danger' : issue === 'due' ? 'warning' : 'default'
}

export const subscriptionStatusLabel: Record<string, string> = {
  active: '有效', past_due: '付款逾期', inactive: '未生效', expired: '已到期', free: '免费', unknown: '未知',
}

/** 最小货币单位的金额 */
export function money(amount: number | undefined, currency: string | undefined): string {
  if (amount === undefined || !currency)
    return '—'
  try {
    const f = new Intl.NumberFormat('zh-CN', { style: 'currency', currency })
    const digits = f.resolvedOptions().maximumFractionDigits ?? 2
    return f.format(amount / 10 ** digits)
  }
  catch {
    return `${amount} ${currency}（最小货币单位）`
  }
}

export function paymentText(p: LastPayment): string {
  return `${billingStatusLabel[p.status.toLowerCase()] ?? p.status} · ${day(p.created_at_ms)} · ${ago(p.checked_at_ms)}查`
}

/** 用量的单位说明 */
export function unitNote(unit: 'relative' | 'credits' | undefined): string {
  return unit === 'credits' ? 'credits' : '相对用量（占套餐额度的比例，不是 Token）'
}

export function compact(v: number): string {
  if (v >= 1e9)
    return `${(v / 1e9).toFixed(1)}B`
  if (v >= 1e6)
    return `${(v / 1e6).toFixed(1)}M`
  if (v >= 1e3)
    return `${(v / 1e3).toFixed(1)}K`
  return Number.isInteger(v) ? String(v) : v.toFixed(2)
}

export function shareOf(s: Share, total: number): string {
  return total > 0 ? percent(s.value / total, s.value / total < 0.1 ? 1 : 0) : '—'
}

/**
 * 列表排序：Work 有用量的最前，其次周额度超过 90% 的、没查过或查失败的，最后按名字。
 * 返回越小越靠前
 */
export function attention(a: Account): number {
  const u = a.usage
  if (u?.work && u.work.total > 0)
    return 0
  if (u?.windows.some(([m, p]) => m === 10080 && p >= 90))
    return 1
  if (!u || u.error)
    return 2
  return 3
}

export function hasWork(u: UsageSummary | null | undefined): boolean {
  return !!u?.work && u.work.total > 0
}

/** 周额度（10080 分钟 = 7 天）是否达到或超过 90%，同时参考 CPR 记下的额度和官方用量窗口 */
export function hasHighWeeklyQuota(a: Account): boolean {
  const fromUsage = a.usage?.windows.some(([m, p]) => m === 10080 && p >= 90) ?? false
  const fromQuota = a.quota?.windows.some(w => w.window_seconds === 604800 && (w.used_percent ?? 0) >= 90) ?? false
  return fromUsage || fromQuota
}

/** 最近一笔账单是否异常（扣款失败或待续费） */
export function hasPaymentAnomaly(a: Account): boolean {
  if (!a.payment)
    return false
  return paymentIssue(a.payment.status) !== null
}

/** 账号是否查不了（有 blocker 或官方用量查询报错） */
export function isUnqueryable(a: Account): boolean {
  return Boolean(a.blocker || a.usage?.error)
}

/** 账号是否未查询（既可查、又还没查过官方用量） */
export function isUnchecked(a: Account): boolean {
  return !a.blocker && !a.usage
}

/** 账号是否令牌过期或凭据异常（需要在列表操作区显示免输重登或去重新登录） */
export function needsCredentialRelogin(a: Account): boolean {
  if (a.credential_state !== 'ready')
    return true
  if (!a.blocker)
    return false
  return a.blocker.code === 'token_expired'
    || a.blocker.code === 'credential_state'
    || a.blocker.code === 'no_credentials'
}

export type AccountFilterKey = 'all' | 'work' | 'payment' | 'quota90' | 'unchecked' | 'blocked'

export function matchesAccountFilter(a: Account, filter: AccountFilterKey): boolean {
  switch (filter) {
    case 'all':
      return true
    case 'work':
      return hasWork(a.usage)
    case 'payment':
      return hasPaymentAnomaly(a)
    case 'quota90':
      return hasHighWeeklyQuota(a)
    case 'unchecked':
      return isUnchecked(a)
    case 'blocked':
      return isUnqueryable(a)
  }
}

export function weeklyQuotaPercent(a: Account): number {
  const fromUsage = a.usage?.windows.find(([m]) => m === 10080)?.[1]
  if (fromUsage !== undefined)
    return fromUsage
  const fromQuota = a.quota?.windows.find(w => w.window_seconds === 604800)?.used_percent
  return fromQuota ?? -1
}

/** 行左竖条优先级：红（扣款失败/逾期/查询报错）> 紫（Work 有用量）> 黄（待续费/周额度 >90%/令牌过期或凭据不可查） */
export function rowAccentTone(a: Account): 'work' | 'bad' | 'warn' | 'none' {
  if ((hasPaymentAnomaly(a) && paymentIssue(a.payment?.status) === 'failed') || Boolean(a.usage?.error))
    return 'bad'
  if (hasWork(a.usage))
    return 'work'
  if (hasPaymentAnomaly(a) || hasHighWeeklyQuota(a) || Boolean(a.blocker))
    return 'warn'
  return 'none'
}

export function title(a: Account): string {
  return a.email || a.name
}
