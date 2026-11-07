<script setup lang="ts">
import type { Account, ApiError, Share } from '../api'
import type { Slice } from '../surfaces'
import type { AccountFilterKey } from '../format'
import type { AccountLoginStatus } from '../login/helpers'
import {
  ZButton,
  ZCheckbox,
  ZEmpty,
  ZInput,
  ZPopover,
  ZSegmented,
  ZSelect,
  ZSkeleton,
  ZTag,
} from '@codex-proxy/ui'
import {
  AlertTriangle,
  ArrowRight,
  CreditCard,
  KeyRound,
  Search,
  Sparkles,
  Square,
} from '@lucide/vue'
import { computed, onBeforeUnmount, ref } from 'vue'
import { api } from '../api'
import {
  ago,
  attention,
  billingTag,
  compact,
  formatPlanWithKind,
  hasHighWeeklyQuota,
  hasPaymentAnomaly,
  isUnchecked,
  isUnqueryable,
  matchesAccountFilter,
  needsCredentialRelogin,
  paymentText,
  rowAccentTone,
  shareOf,
  title,
  weeklyQuotaPercent,
  windowName,
} from '../format'
import { loginApi } from '../login/client'
import { formatLoginError } from '../login/helpers'
import { slices, SURFACE_ORDER, surfaceLabel } from '../surfaces'
import MiniDonut from './MiniDonut.vue'
import MiniTokenBars from './MiniTokenBars.vue'
import WindowMeter from './WindowMeter.vue'

const props = defineProps<{
  accounts: Account[] | null
  loginAccounts: AccountLoginStatus[]
  loading: boolean
}>()
const emit = defineEmits<{
  open: [id: string, focusLogin?: boolean]
  patch: [id: string, patch: Partial<Account>]
  relogged: [account: AccountLoginStatus]
}>()

const search = ref('')
const kind = ref<'all' | 'personal' | 'team'>('all')
const kinds = [
  { label: '全部类型', value: 'all' },
  { label: '个人', value: 'personal' },
  { label: 'Team', value: 'team' },
]

const filter = ref<AccountFilterKey>('all')
const sortBy = ref<'attention' | 'tokens' | 'weekly_quota' | 'name'>('attention')
const sortOptions = [
  { label: '按关注度排序', value: 'attention' },
  { label: '按 Token 用量排序', value: 'tokens' },
  { label: '按周额度排序', value: 'weekly_quota' },
  { label: '按账号名称排序', value: 'name' },
]

const selectedIds = ref<Set<string>>(new Set())

const loginStatusById = computed(() => {
  const map = new Map<string, AccountLoginStatus>()
  for (const l of props.loginAccounts)
    map.set(l.accountId, l)
  return map
})

function hasSavedPassword(id: string): boolean {
  return Boolean(loginStatusById.value.get(id)?.hasPassword)
}

const filterChips = computed<Array<{
  key: AccountFilterKey
  label: string
  dot?: 'bad' | 'warn' | 'neutral'
  count: number
}>>(() => {
  const list = props.accounts ?? []
  return [
    { key: 'all', label: '全部', count: list.length },
    { key: 'payment', label: '扣款异常', dot: 'bad', count: list.filter(a => hasPaymentAnomaly(a)).length },
    { key: 'quota90', label: '周额度 >90%', dot: 'warn', count: list.filter(a => hasHighWeeklyQuota(a)).length },
    { key: 'unchecked', label: '未查询', dot: 'neutral', count: list.filter(a => isUnchecked(a)).length },
    { key: 'blocked', label: '无法查询', dot: 'warn', count: list.filter(a => isUnqueryable(a)).length },
  ]
})

const rows = computed(() => {
  const q = search.value.trim().toLowerCase()
  return (props.accounts ?? [])
    .filter(a => kind.value === 'all' || a.kind === kind.value)
    .filter(a => matchesAccountFilter(a, filter.value))
    .filter(a => !q || `${a.email ?? ''} ${a.name} ${a.plan_type ?? ''} ${a.id}`.toLowerCase().includes(q))
    .sort((a, b) => {
      if (sortBy.value === 'tokens') {
        const diff = tokenSum(b) - tokenSum(a)
        if (diff !== 0)
          return diff
      }
      else if (sortBy.value === 'weekly_quota') {
        const diff = weeklyQuotaPercent(b) - weeklyQuotaPercent(a)
        if (diff !== 0)
          return diff
      }
      else if (sortBy.value === 'name') {
        return title(a).localeCompare(title(b))
      }
      return attention(a) - attention(b) || title(a).localeCompare(title(b))
    })
})

const visibleSelectedCount = computed(() => rows.value.filter(r => selectedIds.value.has(r.id)).length)
const allVisibleSelected = computed(() => rows.value.length > 0 && visibleSelectedCount.value === rows.value.length)
const someVisibleSelected = computed(() => visibleSelectedCount.value > 0 && !allVisibleSelected.value)

const queryableAllCount = computed(() => (props.accounts ?? []).filter(a => !a.blocker).length)
const selectedPool = computed(() => (props.accounts ?? []).filter(a => selectedIds.value.has(a.id)))
const selectedQueryableCount = computed(() => selectedPool.value.filter(a => !a.blocker).length)
const selectedReloginableCount = computed(() => selectedPool.value.filter(a => a.enabled && hasSavedPassword(a.id)).length)

function allQueryBtnTitle(what: 'usage' | 'billing'): string {
  if (running.value)
    return '正在执行批量任务，请稍候'
  if (!props.accounts?.length)
    return '当前没有可查询的账号'
  if (!queryableAllCount.value)
    return '当前所有账号均无法查询（已停用或凭据不可用）'
  return what === 'usage'
    ? '查每个可查账号最近 7 天的官方用量，同时查 6 个'
    : '查每个可查账号的第一页账单、记下最近一笔，同时查 6 个'
}

function selectedQueryBtnTitle(what: 'usage' | 'billing'): string | undefined {
  if (running.value)
    return '正在执行批量任务，请稍候'
  if (!selectedQueryableCount.value)
    return '所选账号当前均无法查询（已停用或凭据不可用）'
  return what === 'usage' ? '查询所选可查账号最近 7 天的官方用量' : '查询所选可查账号的第一页账单'
}

function selectedReloginBtnTitle(): string | undefined {
  if (running.value)
    return '正在执行批量任务，请稍候'
  if (!selectedReloginableCount.value) {
    const anySaved = selectedPool.value.some(a => hasSavedPassword(a.id))
    return anySaved ? '所选有已存密码的账号均已停用，无法发起重登' : '所选账号均未保存密码，无法免输重登'
  }
  return '对所选已保存密码的启用账号逐个免输重登'
}

function toggleSelectAll(val: unknown) {
  const next = new Set(selectedIds.value)
  if (val) {
    for (const r of rows.value)
      next.add(r.id)
  }
  else {
    for (const r of rows.value)
      next.delete(r.id)
  }
  selectedIds.value = next
}

function toggleSelectOne(id: string, val: unknown) {
  const next = new Set(selectedIds.value)
  if (val)
    next.add(id)
  else
    next.delete(id)
  selectedIds.value = next
}

function clearSelection() {
  selectedIds.value = new Set()
}

/** CPR 记下的额度：按窗口长度从短到长 */
function quota(a: Account) {
  return [...(a.quota?.windows ?? [])]
    .filter(w => w.used_percent !== null)
    .sort((x, y) => (x.window_seconds ?? Infinity) - (y.window_seconds ?? Infinity))
}

/** 摘要只记了用量最多的几个位置，剩下的算进“其他” */
function surfaceItems(a: Account): Share[] {
  const items = a.usage?.surfaces ?? []
  const listed = items.reduce((s, i) => s + i.value, 0)
  const rest = (a.usage?.total ?? listed) - listed
  return rest > 1e-9 ? [...items, { key: 'other', value: rest }] : items
}

function surfaceTotal(a: Account): number {
  return surfaceItems(a).reduce((s, i) => s + i.value, 0)
}

function surfaceSlices(a: Account): Slice[] {
  return slices(surfaceItems(a), SURFACE_ORDER)
}

function tokenSum(a: Account): number {
  return (a.usage?.daily_tokens ?? []).reduce((s, d) => s + d.tokens, 0)
}

// 单行「免输重登」
const rowReloggingId = ref<string | null>(null)
const rowReloginNotice = ref<{ id: string, ok: boolean, text: string } | null>(null)

async function runSingleRelogin(a: Account) {
  if (!a.enabled || !hasSavedPassword(a.id))
    return
  rowReloggingId.value = a.id
  rowReloginNotice.value = null
  try {
    const r = await loginApi.runLogin({ accountId: a.id })
    rowReloginNotice.value = {
      id: a.id,
      ok: true,
      text: `${title(a)} 免输重登成功，新令牌已写回 CPR`,
    }
    emit('patch', a.id, {
      credential_state: 'ready',
      access_token_expires_at_ms: r.expiresAtMs,
      blocker: null,
    })
    emit('relogged', r.account)
  }
  catch (e) {
    const err = e as ApiError
    rowReloginNotice.value = {
      id: a.id,
      ok: false,
      text: `${title(a)} 重登失败：${formatLoginError(err.code, err.message)}`,
    }
  }
  finally {
    rowReloggingId.value = null
  }
}

// 「查询全部 / 批量查询」几个账号同时跑；「批量免输重登」一个接一个跑；都可以中途停
type Bulk = 'usage' | 'billing' | 'relogin'
const running = ref<Bulk | null>(null)
const progress = ref({
  done: 0,
  total: 0,
  failed: 0,
  skipped: 0,
  skippedReasons: [] as string[],
  currentName: '',
  /** 正在进行的账号数 */
  active: 0,
})
const lastBatchNote = ref('')
let stop = false
/** 批量查询用量、账单时同时进行的账号数 */
const BATCH_CONCURRENCY = 6

async function runBatch(what: 'usage' | 'billing', onlySelected = false) {
  const pool = onlySelected && selectedIds.value.size > 0
    ? selectedPool.value
    : (props.accounts ?? [])
  const targets = pool.filter(a => !a.blocker)
  if (!targets.length)
    return

  lastBatchNote.value = ''
  running.value = what
  stop = false
  progress.value = { done: 0, total: targets.length, failed: 0, skipped: 0, skippedReasons: [], currentName: '', active: 0 }
  // 几个账号同时查：每个账号走自己绑定的代理，互不影响；停止后不再发起新的，已发出的等它回来
  let next = 0
  async function worker() {
    while (!stop && next < targets.length) {
      const a = targets[next++]!
      progress.value.active++
      try {
        if (what === 'usage') {
          const r = await api.usage(a.id, 7)
          emit('patch', a.id, { usage: r.summary })
        }
        else {
          const r = await api.billing(a.id)
          emit('patch', a.id, { payment: r.payment })
        }
      }
      catch {
        progress.value.failed++
      }
      progress.value.active--
      progress.value.done++
    }
  }
  await Promise.all(Array.from({ length: Math.min(BATCH_CONCURRENCY, targets.length) }, worker))
  running.value = null
}

async function runBatchRelogin() {
  const pool = selectedPool.value
  if (!pool.length)
    return

  const skippedReasons: string[] = []
  const targets: Account[] = []
  for (const a of pool) {
    if (!a.enabled) {
      skippedReasons.push(`${title(a)}（CPR 已停用）`)
      continue
    }
    if (!hasSavedPassword(a.id)) {
      skippedReasons.push(`${title(a)}（未存密码）`)
      continue
    }
    targets.push(a)
  }

  if (!targets.length)
    return

  lastBatchNote.value = ''
  running.value = 'relogin'
  stop = false
  progress.value = {
    done: 0,
    total: targets.length,
    failed: 0,
    skipped: skippedReasons.length,
    skippedReasons,
    currentName: '',
    active: 0,
  }

  let succeeded = 0
  for (const a of targets) {
    if (stop)
      break
    progress.value.currentName = title(a)
    try {
      const r = await loginApi.runLogin({ accountId: a.id })
      succeeded++
      emit('patch', a.id, {
        credential_state: 'ready',
        access_token_expires_at_ms: r.expiresAtMs,
        blocker: null,
      })
      emit('relogged', r.account)
    }
    catch {
      progress.value.failed++
    }
    progress.value.done++
  }

  const skippedDetail = skippedReasons.length ? `，跳过 ${skippedReasons.length} 个：${skippedReasons.join('、')}` : ''
  const stopText = stop ? '（已停止）' : ''
  lastBatchNote.value = `批量免输重登完成${stopText}：成功 ${succeeded} 个，失败 ${progress.value.failed} 个${skippedDetail}`
  running.value = null
}

onBeforeUnmount(() => { stop = true })
</script>

<template>
  <section aria-label="账号监控列表">
    <!-- 顶部判定分类计数筛选芯片 -->
    <div class="filter-bar" role="tablist" aria-label="按账号状态筛选">
      <button
        v-for="chip in filterChips"
        :key="chip.key"
        type="button"
        role="tab"
        class="filter-chip"
        :class="{ 'is-active': filter === chip.key }"
        :aria-selected="filter === chip.key"
        @click="filter = chip.key"
      >
        <span v-if="chip.dot" class="state-dot" :class="chip.dot" aria-hidden="true" />
        <span>{{ chip.label }}</span>
        <span class="filter-count">{{ chip.count }}</span>
      </button>
    </div>

    <p
      v-if="rowReloginNotice"
      class="banner mb-3"
      :class="rowReloginNotice.ok ? 'banner-ok' : 'banner-error'"
      role="status"
    >
      {{ rowReloginNotice.text }}
    </p>

    <p
      v-if="lastBatchNote"
      class="banner banner-ok mb-3"
      role="status"
    >
      {{ lastBatchNote }}
    </p>

    <div class="monitor-list">
      <!-- 工具栏：搜索、类型、排序、批量查询 -->
      <div class="monitor-toolbar">
        <div class="flex flex-wrap items-center gap-2">
          <ZCheckbox
            :model-value="allVisibleSelected"
            :indeterminate="someVisibleSelected"
            aria-label="全选当前列表账号"
            @update:model-value="toggleSelectAll"
          />
          <ZInput
            v-model="search"
            placeholder="搜索邮箱、名称、套餐"
            clearable
            size="small"
            class="w-56"
            aria-label="搜索账号"
          >
            <template #prefix>
              <Search :size="14" class="text-cp-text-tertiary" aria-hidden="true" />
            </template>
          </ZInput>
          <ZSegmented v-model="kind" :options="kinds" size="small" aria-label="账号类型" />
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <ZSelect
            v-model="sortBy"
            :options="sortOptions"
            size="small"
            class="w-40"
            aria-label="排序方式"
          />
          <ZButton
            size="small"
            :disabled="!queryableAllCount || !!running"
            :title="allQueryBtnTitle('usage')"
            @click="runBatch('usage', false)"
          >
            <Sparkles :size="13" aria-hidden="true" /> 查询全部用量
          </ZButton>
          <ZButton
            size="small"
            :disabled="!queryableAllCount || !!running"
            :title="allQueryBtnTitle('billing')"
            @click="runBatch('billing', false)"
          >
            <CreditCard :size="13" aria-hidden="true" /> 查询全部账单
          </ZButton>
        </div>
      </div>

      <!-- 列表骨架屏 / 空状态 / 监控行 -->
      <div v-if="loading && !accounts" class="space-y-2 p-4" aria-label="加载中">
        <ZSkeleton v-for="n in 4" :key="n" variant="text" style="height: 58px" />
      </div>

      <div v-else-if="!rows.length" class="p-8">
        <ZEmpty
          size="small"
          :title="accounts?.length ? '没有符合当前筛选条件的账号' : 'CPR 里还没有 OpenAI OAuth 账号'"
          :description="accounts?.length ? '可以尝试切换上方筛选芯片或清空搜索词' : '可切换到「导入账号」页面批量导入账号'"
        />
      </div>

      <template v-else>
        <div class="monitor-head" aria-hidden="true">
          <span />
          <span>账号与状态</span>
          <span>CPR 额度</span>
          <span>官方用量（每日 Token · 使用位置）</span>
          <span>最近支付</span>
          <span class="text-right">操作</span>
        </div>

        <div
          v-for="a in rows"
          :key="a.id"
          class="monitor-row"
          :class="{ 'is-selected': selectedIds.has(a.id) }"
          :data-tone="rowAccentTone(a)"
        >
          <div>
            <ZCheckbox
              :model-value="selectedIds.has(a.id)"
              :aria-label="`选择 ${title(a)}`"
              @update:model-value="v => toggleSelectOne(a.id, v)"
            />
          </div>

          <!-- 1. 账号信息 + 状态胶囊 -->
          <div class="min-w-0">
            <button type="button" class="row-title-btn" @click="emit('open', a.id)">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="account-name" :title="title(a)">{{ title(a) }}</span>
                <span v-if="a.blocker" class="verdict-pill warn" :title="a.blocker.message">
                  <AlertTriangle :size="11" aria-hidden="true" />
                  {{ a.blocker.code === 'token_expired' ? '令牌过期' : a.blocker.code === 'account_unavailable' ? '已停用' : '凭据异常' }}
                </span>
              </div>
              <span class="account-meta">
                <span class="plan">{{ formatPlanWithKind(a.plan_type, a.kind) }}</span>
                <span v-if="a.email && a.name !== a.email">{{ a.name }}</span>
                <span v-if="!a.enabled" class="text-warn">CPR 已停用</span>
              </span>
            </button>
          </div>

          <!-- 2. CPR 额度 -->
          <div class="quota-cell row-col-quota">
            <WindowMeter
              v-for="w in quota(a)"
              :key="w.window_seconds ?? 0"
              :label="windowName((w.window_seconds ?? 0) / 60)"
              :used="w.used_percent"
              :resets-at="w.reset_at_ms"
            />
            <span v-if="!quota(a).length" class="muted text-xs">—</span>
          </div>

          <!-- 3. 官方用量（每日 Token 柱状图 + 使用位置占比饼图） -->
          <div class="usage-cell row-col-usage">
            <template v-if="a.usage?.error">
              <div class="flex items-center gap-1.5">
                <span class="verdict-pill bad" :title="a.usage.error.message">查询失败</span>
                <span class="muted text-xs">· {{ ago(a.usage.checked_at_ms) }}查</span>
              </div>
            </template>
            <template v-else-if="a.usage">
              <div class="usage-mini">
                <div class="usage-mini-tokens">
                  <MiniTokenBars v-if="a.usage.daily_tokens?.length" :days="a.usage.daily_tokens" />
                  <span v-else class="muted text-xs">每日 Token 未读到</span>
                  <span class="usage-mini-caption">
                    <strong v-if="a.usage.daily_tokens?.length">{{ compact(tokenSum(a)) }} Token</strong>
                    <span class="muted">近 {{ a.usage.days }} 天 · {{ ago(a.usage.checked_at_ms) }}查</span>
                  </span>
                </div>
                <ZPopover
                  v-if="surfaceSlices(a).length"
                  trigger="hover"
                  placement="top"
                  :width="250"
                >
                  <template #reference>
                    <button
                      type="button"
                      class="usage-mini-pie"
                      :aria-label="`${title(a)} 使用位置占比`"
                      @click="emit('open', a.id)"
                    >
                      <MiniDonut :parts="surfaceSlices(a)" :label="surfaceSlices(a).map(s => `${surfaceLabel(s.key)} ${shareOf(s, surfaceTotal(a))}`).join('，')" />
                    </button>
                  </template>
                  <div class="surface-popover">
                    <div class="surface-popover-head">
                      <strong>近 {{ a.usage.days }} 天使用位置</strong>
                    </div>
                    <ul class="surface-popover-list">
                      <li v-for="s in surfaceSlices(a)" :key="s.key" class="surface-popover-row">
                        <span class="legend-item truncate">
                          <i class="swatch" :style="{ background: s.colour }" />
                          {{ surfaceLabel(s.key) }}
                        </span>
                        <span class="font-semibold">{{ shareOf(s, surfaceTotal(a)) }}</span>
                      </li>
                    </ul>
                  </div>
                </ZPopover>
                <span v-else class="muted text-xs">占比未读到</span>
              </div>
            </template>
            <span v-else class="muted text-xs">用量未查询</span>
          </div>

          <!-- 4. 最近支付 -->
          <div class="row-col-payment">
            <ZTag
              v-if="a.payment"
              size="small"
              :type="billingTag(a.payment.status)"
              :title="a.payment.grace_ends_at_ms ? '付款逾期，宽限期内' : undefined"
            >
              {{ paymentText(a.payment) }}
            </ZTag>
            <span v-else class="muted text-xs">账单未查询</span>
          </div>

          <!-- 5. 操作 -->
          <div class="row-col-actions flex justify-end gap-1.5">
            <template v-if="needsCredentialRelogin(a)">
              <ZButton
                v-if="hasSavedPassword(a.id)"
                size="small"
                type="primary"
                :loading="rowReloggingId === a.id"
                :disabled="!a.enabled || !!running"
                :title="!a.enabled ? '账号在 CPR 里已停用，无法发起登录' : running ? '正在执行批量任务，请稍候' : '用已保存的密码免输重新登录并写回 CPR'"
                @click="runSingleRelogin(a)"
              >
                <KeyRound :size="12" aria-hidden="true" /> 免输重登
              </ZButton>
              <ZButton
                v-else
                size="small"
                @click="emit('open', a.id, true)"
              >
                去重新登录
              </ZButton>
            </template>
            <ZButton size="small" @click="emit('open', a.id)">
              详情 <ArrowRight :size="13" aria-hidden="true" />
            </ZButton>
          </div>
        </div>
      </template>

      <p class="list-legend muted">
        <span class="legend-note">
          行左竖条：红=扣款失败/查询报错，黄=待续费/周额度 &gt;90%/令牌过期。CPR 额度是 CPR 自己记下的观测值；官方用量和账单点了才查。悬停饼图看各使用位置的占比。
        </span>
      </p>
    </div>

    <!-- 底部粘性条：批量查询/重登进度 或 已选账号操作 -->
    <div
      v-if="running || selectedIds.size > 0"
      class="selection-bar"
      role="region"
      aria-label="批量操作与查询进度"
    >
      <template v-if="running">
        <div class="flex flex-wrap items-center gap-3 text-cp-xs">
          <span class="font-semibold text-cp-text">
            正在{{ running === 'usage' ? '查询官方用量' : running === 'billing' ? '查询账单' : '批量免输重登' }}
            {{ progress.done }} / {{ progress.total }}
          </span>
          <div class="progress-track" aria-hidden="true">
            <i :style="{ width: `${progress.total ? (progress.done / progress.total) * 100 : 0}%` }" />
          </div>
          <span v-if="progress.currentName" class="muted">
            当前：{{ progress.currentName }}
          </span>
          <span v-else-if="progress.active" class="muted">
            同时查询 {{ progress.active }} 个
          </span>
          <span v-if="progress.failed" class="text-bad">
            {{ progress.failed }} 个失败
          </span>
          <span v-if="progress.skipped" class="text-warn" :title="progress.skippedReasons.join('；')">
            跳过 {{ progress.skipped }} 个（{{ progress.skippedReasons.join('、') }}）
          </span>
        </div>
        <ZButton size="small" @click="stop = true">
          <Square :size="12" aria-hidden="true" /> {{ running === 'relogin' ? '停止重登' : '停止查询' }}
        </ZButton>
      </template>

      <template v-else>
        <span class="text-cp-xs text-cp-text-secondary">
          已选 <strong class="text-cp-text">{{ selectedIds.size }}</strong> 个账号
        </span>
        <div class="flex flex-wrap items-center gap-2">
          <ZButton
            size="small"
            type="primary"
            :disabled="!selectedQueryableCount"
            :title="selectedQueryBtnTitle('usage')"
            @click="runBatch('usage', true)"
          >
            查询所选用量
          </ZButton>
          <ZButton
            size="small"
            :disabled="!selectedQueryableCount"
            :title="selectedQueryBtnTitle('billing')"
            @click="runBatch('billing', true)"
          >
            查询所选账单
          </ZButton>
          <ZButton
            size="small"
            :disabled="!selectedReloginableCount"
            :title="selectedReloginBtnTitle()"
            @click="runBatchRelogin"
          >
            <KeyRound :size="12" aria-hidden="true" /> 批量免输重登
          </ZButton>
          <ZButton size="small" variant="text" @click="clearSelection">
            取消选择
          </ZButton>
        </div>
      </template>
    </div>
  </section>
</template>
