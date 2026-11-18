<script setup lang="ts">
import type { Account } from '../../api'
import type { AccountLoginStatus } from '../../login/helpers'
import type { Overview, OverviewAccount } from '../api'
import type { CellState } from '../timeline'
import { ZButton, ZCheckbox, ZEmpty, ZInput, ZSegmented, ZSelect } from '@codex-proxy/ui'
import { ChevronRight, Search } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { credentialStateLabel, hasGracePeriod, hasPaymentAnomaly, needsCredentialRelogin } from '../../format'
import { ago, rate, span } from '../format'
import { defaultModels } from '../probe-models'
import { accountState, blockedLabel, isBlocked, needsAttention, pendingChange, pendingLabel, probeInterval, rank, stateLabel } from '../timeline'
import ProbeStrip from './ProbeStrip.vue'
import SubscriptionCell from './SubscriptionCell.vue'

const props = defineProps<{
  overview: Overview | null
  loading: boolean
  panelLoading: boolean
  panel: Map<string, Account>
  loginInfo?: Map<string, AccountLoginStatus>
  subscriptionErrors: Record<string, string>
  checking: Set<string>
  savedPasswords: Set<string>
  batchBusy: boolean
}>()
const emit = defineEmits<{
  open: [id: string, run?: string]
  billing: [id: string]
  manage: [id: string]
  relogin: [id: string]
  probe: [account: OverviewAccount]
  batchProbe: [accounts: OverviewAccount[]]
  batchConfig: [ids: string[]]
  batchManage: [kind: 'subscription' | 'billing' | 'usage' | 'relogin', ids: string[]]
  visibleIds: [ids: string[]]
}>()
const hours = defineModel<string>('hours', { required: true })

const MAX_BATCH = 200
const search = ref('')
const filter = ref<'all' | 'attention' | 'blocked' | 'grace' | 'payment' | CellState>('all')
const plan = ref('')
const sort = ref<'state' | 'name' | 'rate' | 'grace'>('state')
const selectedIds = ref<string[]>([])
const expandedStrip = ref(false)

const ranges = [{ label: '24 小时', value: '24' }, { label: '7 天', value: '168' }, { label: '30 天', value: '720' }]
const rangeLabel = computed(() => ranges.find(r => r.value === hours.value)?.label ?? '')
const sorts = [{ label: '问题优先', value: 'state' }, { label: '宽限期先到期', value: 'grace' }, { label: '按名称', value: 'name' }, { label: '正常率低在前', value: 'rate' }]

const overviewById = computed(() => new Map(props.overview?.accounts.map(a => [a.id, a])))
const probeById = computed(() => new Map(props.overview?.accounts.filter(a => a.supported).map(a => [a.id, a])))
// 先全量保留 overview 账号（含 supported: false），再用 panel 补全尚未进入 overview 的账号。
const all = computed<OverviewAccount[]>(() => {
  const merged = [...(props.overview?.accounts ?? [])]
  const noRate = { ok: 0, judged: 0 }
  for (const a of props.panel.values()) {
    if (!overviewById.value.has(a.id)) {
      merged.push({
        ...a,
        supported: true,
        token_expires_ms: a.access_token_expires_at_ms,
        running: null,
        health: null,
        blocked: null,
        latest: { turnstate: null, modeltrace: null },
        strip: [],
        last_attempt: null,
        schedule: {
          turnstate: { enabled: false, model: defaultModels.turnstate, interval_minutes: 60 },
          modeltrace: { enabled: false, model: defaultModels.modeltrace, interval_minutes: 60 },
        },
        rate: noRate,
        rate_24h: noRate,
        rate_7d: noRate,
      })
    }
  }
  return merged
})
const states = computed(() => new Map(all.value.map(a => [a.id, accountState(a)])))
const stateOf = (a: OverviewAccount) => states.value.get(a.id) ?? 'empty'
const planOf = (a: OverviewAccount) => (a.plan_type || 'unknown').toLowerCase()
const plans = computed(() => [...new Set(all.value.map(planOf))].sort())
const planOptions = computed(() => [{ label: '全部类型', value: '' }, ...plans.value.map(p => ({ label: p === 'unknown' ? '未知类型' : p.toUpperCase(), value: p }))])
const paymentProblem = (a: OverviewAccount) => {
  const panel = props.panel.get(a.id)
  return !!panel && (hasPaymentAnomaly(panel) || ['expired', 'inactive'].includes(panel.payment?.subscription_status ?? ''))
}
const gracePeriod = (a: OverviewAccount) => hasGracePeriod(props.panel.get(a.id))
const attention = (a: OverviewAccount) => needsAttention(a) || paymentProblem(a)
const reloginable = (id: string) => !!props.panel.get(id)?.enabled && props.savedPasswords.has(id)
const reloginNeeded = (id: string) => {
  const account = props.panel.get(id)
  return !!account?.enabled && needsCredentialRelogin(account)
}
const loginStatus = (id: string) => props.loginInfo?.get(id)

const filters = computed(() => {
  const count = (pred: (s: CellState) => boolean) => all.value.filter(a => probeById.value.has(a.id) && pred(stateOf(a))).length
  return [
    { value: 'all', label: '全部', state: null, count: all.value.length },
    { value: 'attention', label: '需要关注', state: 'degraded', count: all.value.filter(attention).length },
    { value: 'degraded', label: '降智', state: 'degraded', count: count(s => s === 'degraded') },
    { value: 'suspect', label: '疑似', state: 'suspect', count: count(s => s === 'suspect') },
    { value: 'blocked', label: '探测受阻', state: 'blocked', count: all.value.filter(isBlocked).length },
    { value: 'grace', label: '订阅宽限期', state: 'degraded', count: all.value.filter(gracePeriod).length },
    { value: 'payment', label: '订阅异常', state: 'suspect', count: all.value.filter(paymentProblem).length },
    { value: 'ok', label: '正常', state: 'ok', count: count(s => s === 'ok') },
    { value: 'unknown', label: '未判定', state: 'unknown', count: count(s => s === 'unknown' || s === 'running') },
    { value: 'empty', label: '没有探测', state: 'empty', count: count(s => s === 'empty') },
  ] as const
})

function matches(a: OverviewAccount) {
  const s = stateOf(a)
  if (filter.value === 'attention' && !attention(a))
    return false
  if (filter.value === 'blocked' && !isBlocked(a))
    return false
  if (filter.value === 'unknown' && s !== 'unknown' && s !== 'running')
    return false
  if (filter.value === 'grace' && !gracePeriod(a))
    return false
  if (filter.value === 'payment' && !paymentProblem(a))
    return false
  if (!['all', 'attention', 'blocked', 'unknown', 'grace', 'payment'].includes(filter.value) && (!probeById.value.has(a.id) || s !== filter.value))
    return false
  if (plan.value && planOf(a) !== plan.value)
    return false
  const q = search.value.trim().toLowerCase()
  return !q || `${a.email ?? ''} ${a.name}`.toLowerCase().includes(q)
}
const rateValue = (a: OverviewAccount) => a.rate.judged ? a.rate.ok / a.rate.judged : 2
const name = (a: OverviewAccount) => a.email || a.name
const graceAt = (a: OverviewAccount) => props.panel.get(a.id)?.payment?.grace_ends_at_ms ?? Number.MAX_SAFE_INTEGER
const accounts = computed(() => all.value.filter(matches).sort((x, y) => {
  if (sort.value === 'rate')
    return rateValue(x) - rateValue(y) || name(x).localeCompare(name(y))
  if (sort.value === 'grace')
    return graceAt(x) - graceAt(y) || name(x).localeCompare(name(y))
  if (sort.value === 'state')
    return Number(attention(y)) - Number(attention(x)) || rank(x) - rank(y)
      || Number(paymentProblem(y)) - Number(paymentProblem(x)) || graceAt(x) - graceAt(y) || name(x).localeCompare(name(y))
  return name(x).localeCompare(name(y))
}))

watch(accounts, (list) => {
  emit('visibleIds', list.map(a => a.id))
}, { immediate: true })

const selectable = computed(() => accounts.value.filter(a => a.supported).map(a => a.id))
const allSelected = computed({
  get: () => !!selectable.value.length && selectable.value.every(id => selectedIds.value.includes(id)),
  set: (value) => {
    const visible = new Set(selectable.value)
    selectedIds.value = value
      ? [...new Set([...selectedIds.value, ...selectable.value])].slice(0, MAX_BATCH)
      : selectedIds.value.filter(id => !visible.has(id))
  },
})
function select(id: string, value: boolean) {
  selectedIds.value = value ? [...new Set([...selectedIds.value, id])].slice(0, MAX_BATCH) : selectedIds.value.filter(s => s !== id)
}
const selectedAccounts = computed(() => [...probeById.value.values()].filter(a => selectedIds.value.includes(a.id)))
const queryableSelection = computed(() => selectedIds.value.some(id => props.panel.has(id) && !props.panel.get(id)?.blocker))
/** 选中了但被当前筛选隐藏的账号：批量操作也会作用到它们，要说清楚 */
const hiddenSelected = computed(() => {
  const visible = new Set(accounts.value.map(a => a.id))
  return selectedIds.value.filter(id => !visible.has(id)).length
})
function keepVisible() {
  const visible = new Set(selectable.value)
  selectedIds.value = selectedIds.value.filter(id => visible.has(id))
}
// 每次拿到新列表后去掉已删除或不再支持探测的账号，否则批量配置会整批被拒
watch(all, (list) => { selectedIds.value = selectedIds.value.filter(id => list.some(a => a.id === id && a.supported)) })

function verdictText(a: OverviewAccount): string {
  if (!a.supported)
    return '不支持探测'
  if (!props.overview && props.loading)
    return '读取中…'
  return stateLabel[stateOf(a)]
}
function lastProbe(a: OverviewAccount) {
  const l = a.latest.turnstate
  return l ? `${ago(l.finished_ms || l.started_ms, props.overview?.now_ms)}` : ''
}
function pendingText(a: OverviewAccount) {
  const p = pendingChange(a)
  return p && a.health ? pendingLabel(a.health.level, p.to, p.streak) : ''
}
/** 受阻：确认的醒目显示；还没满 30 分钟的作为提示 */
function blockedText(a: OverviewAccount) {
  const b = a.blocked
  if (!b)
    return ''
  return `${blockedLabel(b.code)} · ${span((props.overview?.now_ms ?? Date.now()) - b.since_ms)}`
}
/** 还未被后台记成受阻的账号，也显示 CPR 已知的凭据异常。 */
function credentialNote(a: OverviewAccount) {
  return !a.blocked && ['expired', 'banned', 'invalid'].includes(a.credential_state) ? credentialStateLabel(a.credential_state) : ''
}
function warning(a: OverviewAccount): { text: string, detail: string } | null {
  if (a.blocked)
    return null
  const blockers = a.last_attempt?.blockers
  if (blockers?.length)
    return { text: '自动探测被拦截', detail: blockers.map(b => b.message).join('；') }
  if (a.health?.notification_error)
    return { text: 'TG 通知未送达', detail: a.health.notification_error }
  return null
}
function schedule(a: OverviewAccount) {
  const s = a.schedule.turnstate
  return s.enabled ? `每 ${s.interval_minutes} 分钟` : '自动探测关'
}
</script>

<template>
  <section class="probe-filter-bar" aria-label="按当前判定筛选">
    <button
      v-for="f in filters" :key="f.value" type="button" class="probe-filter-chip"
      :class="{
        active: filter === f.value,
        emphasis: f.value === 'attention' && f.count > 0,
        'is-zero': f.value !== 'all' && f.count === 0 && filter !== f.value,
      }"
      :aria-pressed="filter === f.value" @click="filter = f.value"
    >
      <i v-if="f.state" class="probe-state-dot" :data-state="f.state" />
      <span>{{ f.label }}</span>
      <strong>{{ overview || panel.size ? f.count : '—' }}</strong>
    </button>
  </section>

  <section class="probe-monitor-list" :aria-busy="loading" :data-strip-expanded="expandedStrip">
    <div class="probe-monitor-toolbar">
      <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
        <ZInput v-model="search" placeholder="搜索名称或邮箱" clearable class="monitor-search" aria-label="搜索账号"><template #prefix><Search :size="15" aria-hidden="true" /></template></ZInput>
        <ZSelect v-if="plans.length > 1" v-model="plan" :options="planOptions" class="w-32" aria-label="账号类型" />
        <ZSelect v-model="sort" :options="sorts" class="w-36" aria-label="排序" />
      </div>
      <div class="probe-range-controls">
        <ZSegmented v-model="hours" :options="ranges" size="small" aria-label="时间条范围" />
        <ZButton size="small" :aria-expanded="expandedStrip" @click="expandedStrip = !expandedStrip">{{ expandedStrip ? '收起探针' : '展开探针' }}</ZButton>
      </div>
    </div>
    <div class="probe-monitor-head">
      <ZCheckbox v-model="allSelected" :indeterminate="!allSelected && selectable.some(id => selectedIds.includes(id))" :disabled="!selectable.length" :aria-label="`全选筛选出的 ${selectable.length} 个账号`" />
      <span class="head-account">{{ accounts.length }} 个账号<template v-if="accounts.length !== all.length"> / 共 {{ all.length }}</template></span>
      <span class="head-strip">测票探针 · 最近 {{ rangeLabel }}</span>
      <span class="head-rate">正常率</span>
      <span class="head-subscription">订阅与账单</span>
      <span class="head-actions" />
    </div>
    <div v-if="loading && !overview && !panel.size" class="monitor-loading">正在读取账号…</div>
    <ZEmpty v-else-if="!accounts.length" class="py-12" :title="all.length ? '没有符合筛选的账号' : 'CPR 里还没有 OpenAI 账号'" />
    <article v-for="a in accounts" :key="a.id" class="probe-monitor-row" :data-account="a.id" :data-state="stateOf(a)" :data-blocked="isBlocked(a) || undefined">
      <ZCheckbox :model-value="selectedIds.includes(a.id)" :disabled="!a.supported || (selectedIds.length >= MAX_BATCH && !selectedIds.includes(a.id))" :aria-label="`选择 ${name(a)}`" @update:model-value="(v: boolean) => select(a.id, v)" />
      <button type="button" class="monitor-account" @click="emit('open', a.id)">
        <span class="probe-account-name">
          <span class="truncate" :title="name(a)">{{ name(a) }}</span>
          <span v-if="a.plan_type" class="account-plan">{{ a.plan_type }}</span>
        </span>
        <span class="probe-account-meta">
          <span class="probe-verdict-pill" :data-state="stateOf(a)"><i class="probe-state-dot" :data-state="stateOf(a)" />{{ verdictText(a) }}</span>
          <span v-if="pendingText(a)" class="pending-change" :title="`连续 3 次同类结果才改变当前判定，已 ${pendingChange(a)!.streak} 次`">{{ pendingText(a) }}</span>
          <span v-if="a.supported && !a.enabled">已停用</span>
          <span v-else-if="a.supported && probeById.has(a.id)" class="text-cp-text-tertiary">{{ lastProbe(a) ? `${lastProbe(a)}探测` : '还没探测' }} · {{ schedule(a) }}</span>
          <span v-if="loginStatus(a.id)?.autoLoginEnabled" class="meta-badge" title="已开启 401 / 令牌过期时自动重登一次">自动重登</span>
          <span v-if="a.blocked" class="blocked-pill" :data-confirmed="isBlocked(a)" :title="`${a.blocked.message}${isBlocked(a) ? '' : '（持续 30 分钟会发 TG 通知）'}`">{{ isBlocked(a) ? '探测受阻' : '受阻' }} · {{ blockedText(a) }}</span>
          <span v-else-if="credentialNote(a)" class="row-warning">{{ credentialNote(a) }}<template v-if="reloginNeeded(a.id) && !reloginable(a.id)"> · 未存密码</template></span>
          <span v-else-if="reloginNeeded(a.id) && !reloginable(a.id)" class="row-warning">未存密码</span>
          <span v-if="warning(a)" class="row-warning" :title="warning(a)!.detail">{{ warning(a)!.text }}</span>
        </span>
      </button>
      <div class="monitor-strip">
        <ProbeStrip v-if="overview && probeById.has(a.id)" :range="overview.range" :cells="a.strip" :interval-ms="probeInterval(a, overview.config.min_interval_minutes)" :account-id="a.id" method="turnstate" @open="run => emit('open', a.id, run)" />
      </div>
      <span class="track-rate" :class="{ 'rate-empty': !a.rate.judged }" :title="`${rangeLabel}内 ${a.rate.ok} / ${a.rate.judged} 次有效判定为正常；失败、未判定和无探测不计入`">{{ rate(a.rate) }}</span>
      <SubscriptionCell :account="panel.get(a.id)" :reading="panelLoading || checking.has(a.id)" :error="subscriptionErrors[a.id]" @open="emit('billing', a.id)" />
      <div class="monitor-actions">
        <ZButton v-if="reloginNeeded(a.id)" size="small" :disabled="reloginable(a.id) && batchBusy" @click="reloginable(a.id) ? emit('relogin', a.id) : emit('manage', a.id)">{{ reloginable(a.id) ? '免输重登' : '去重新登录' }}</ZButton>
        <ZButton v-else size="small" :disabled="!probeById.has(a.id)" @click="emit('probe', a)">探测</ZButton>
        <button type="button" class="account-details" :aria-label="`查看 ${name(a)}`" title="详情与配置" @click="emit('open', a.id)"><ChevronRight :size="18" aria-hidden="true" /></button>
      </div>
    </article>
    <footer class="monitor-legend">
      <span v-for="s in (['ok', 'suspect', 'degraded', 'unknown', 'empty', 'running'] as const)" :key="s" class="inline-flex items-center gap-1.5"><i class="legend-cell" :data-state="s" />{{ stateLabel[s] }}</span>
      <span class="probe-legend-note">账号状态是测票的当前判定（连续 3 次确认）· 时间条和正常率是每次的原始结果</span>
    </footer>
  </section>

  <Transition name="selection">
    <div v-if="selectedIds.length && !batchBusy" class="probe-selection-bar" role="region" aria-label="已选账号的操作">
      <span class="text-cp-sm">已选 <strong>{{ selectedIds.length }}</strong> 个<span v-if="selectedIds.length >= MAX_BATCH" class="text-cp-xs text-cp-text-tertiary">（单批上限 {{ MAX_BATCH }}）</span></span>
      <span v-if="hiddenSelected" class="text-cp-xs text-cp-warning-text">其中 {{ hiddenSelected }} 个不在当前筛选里，也会被操作</span>
      <button v-if="hiddenSelected" type="button" class="inline-action-btn" @click="keepVisible">只保留筛选出的</button>
      <span class="flex-1" />
      <ZButton size="small" type="primary" :disabled="!selectedAccounts.length" @click="emit('batchProbe', selectedAccounts)">批量探测</ZButton>
      <ZButton size="small" :disabled="!selectedAccounts.length" @click="emit('batchConfig', selectedAccounts.map(a => a.id))">批量配置</ZButton>
      <ZButton size="small" :disabled="batchBusy || !queryableSelection" @click="emit('batchManage', 'subscription', selectedIds)">查订阅</ZButton>
      <ZButton size="small" :disabled="batchBusy || !queryableSelection" @click="emit('batchManage', 'billing', selectedIds)">查账单</ZButton>
      <ZButton size="small" :disabled="batchBusy || !queryableSelection" @click="emit('batchManage', 'usage', selectedIds)">查用量</ZButton>
      <ZButton v-if="selectedIds.some(reloginable)" size="small" :disabled="batchBusy" @click="emit('batchManage', 'relogin', selectedIds)">批量免输重登</ZButton>
      <button type="button" class="inline-action-btn" @click="selectedIds = []">清空</button>
    </div>
  </Transition>
</template>
