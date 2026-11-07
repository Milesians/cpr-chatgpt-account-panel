<script setup lang="ts">
import type { Account } from '../api'
import type { AccountFilterKey } from '../format'
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
  Info,
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
  hasHighWeeklyQuota,
  hasPaymentAnomaly,
  hasWork,
  isUnchecked,
  isUnqueryable,
  kindLabel,
  matchesAccountFilter,
  paymentText,
  percent,
  rowAccentTone,
  shareOf,
  title,
  weeklyQuotaPercent,
  windowName,
} from '../format'
import { slotColour, slotOf, surfaceLabel } from '../surfaces'
import WindowMeter from './WindowMeter.vue'

const props = defineProps<{ accounts: Account[] | null, loading: boolean }>()
const emit = defineEmits<{ open: [id: string], patch: [id: string, patch: Partial<Account>] }>()

const search = ref('')
const kind = ref<'all' | 'personal' | 'team'>('all')
const kinds = [
  { label: '全部类型', value: 'all' },
  { label: '个人', value: 'personal' },
  { label: 'Team', value: 'team' },
]

const filter = ref<AccountFilterKey>('all')
const sortBy = ref<'attention' | 'work_share' | 'weekly_quota' | 'name'>('attention')
const sortOptions = [
  { label: '按关注度排序', value: 'attention' },
  { label: '按 Work 占比排序', value: 'work_share' },
  { label: '按周额度排序', value: 'weekly_quota' },
  { label: '按账号名称排序', value: 'name' },
]

const selectedIds = ref<Set<string>>(new Set())

const filterChips = computed<Array<{
  key: AccountFilterKey
  label: string
  dot?: 'work' | 'bad' | 'warn' | 'neutral'
  count: number
}>>(() => {
  const list = props.accounts ?? []
  return [
    { key: 'all', label: '全部', count: list.length },
    { key: 'work', label: 'Work 有用量', dot: 'work', count: list.filter(a => hasWork(a.usage)).length },
    { key: 'payment', label: '扣款异常', dot: 'bad', count: list.filter(a => hasPaymentAnomaly(a)).length },
    { key: 'quota90', label: '周额度 >90%', dot: 'warn', count: list.filter(a => hasHighWeeklyQuota(a)).length },
    { key: 'unchecked', label: '未查询', dot: 'neutral', count: list.filter(a => isUnchecked(a)).length },
    { key: 'blocked', label: '查不了', dot: 'warn', count: list.filter(a => isUnqueryable(a)).length },
  ]
})

const rows = computed(() => {
  const q = search.value.trim().toLowerCase()
  return (props.accounts ?? [])
    .filter(a => kind.value === 'all' || a.kind === kind.value)
    .filter(a => matchesAccountFilter(a, filter.value))
    .filter(a => !q || `${a.email ?? ''} ${a.name} ${a.plan_type ?? ''} ${a.id}`.toLowerCase().includes(q))
    .sort((a, b) => {
      if (sortBy.value === 'work_share') {
        const diff = (b.usage?.work?.share ?? -1) - (a.usage?.work?.share ?? -1)
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

function surfaceTotal(a: Account): number {
  return (a.usage?.surfaces ?? []).reduce((s, item) => s + item.value, 0)
}

// 「查询全部 / 批量查询」：一个接一个查，可以中途停；只查现在能查的账号
type Bulk = 'usage' | 'billing'
const running = ref<Bulk | null>(null)
const progress = ref({ done: 0, total: 0, failed: 0, currentName: '' })
let stop = false

async function runBatch(what: Bulk, onlySelected = false) {
  const pool = onlySelected && selectedIds.value.size > 0
    ? (props.accounts ?? []).filter(a => selectedIds.value.has(a.id))
    : (props.accounts ?? [])
  const targets = pool.filter(a => !a.blocker)
  if (!targets.length)
    return

  running.value = what
  stop = false
  progress.value = { done: 0, total: targets.length, failed: 0, currentName: '' }
  for (const a of targets) {
    if (stop)
      break
    progress.value.currentName = title(a)
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
    progress.value.done++
  }
  running.value = null
}

onBeforeUnmount(() => { stop = true })
</script>

<template>
  <section aria-label="账号监控列表">
    <!-- 顶部判定分类计数筛选芯片 (turnstate 风格) -->
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
            :disabled="!accounts?.length || !!running"
            title="逐个查每个可查账号最近 7 天的官方用量"
            @click="runBatch('usage', false)"
          >
            <Sparkles :size="13" aria-hidden="true" /> 查询全部用量
          </ZButton>
          <ZButton
            size="small"
            :disabled="!accounts?.length || !!running"
            title="逐个查每个可查账号的第一页账单，记下最近一笔"
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
          :description="accounts?.length ? '可以尝试切换上方筛选芯片或清空搜索词' : '可切换到「登录与导入」页面批量导入账号'"
        />
      </div>

      <template v-else>
        <div class="monitor-head" aria-hidden="true">
          <span />
          <span>账号与状态</span>
          <span>CPR 记下的额度</span>
          <span>官方用量（使用位置）</span>
          <span>最近支付</span>
          <span class="text-right">操作</span>
        </div>

        <div
          v-for="a in rows"
          :key="a.id"
          class="monitor-row"
          :class="{ 'is-selected': selectedIds.has(a.id) }"
          :data-tone="rowAccentTone(a)"
          :data-work="hasWork(a.usage)"
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
                <span class="account-name">{{ title(a) }}</span>
                <span v-if="hasWork(a.usage)" class="verdict-pill work">
                  <span class="state-dot work" aria-hidden="true" />
                  Work {{ percent(a.usage!.work!.share, a.usage!.work!.share < 0.1 ? 1 : 0) }}
                </span>
                <span v-else-if="a.blocker" class="verdict-pill warn" :title="a.blocker.message">
                  <AlertTriangle :size="11" aria-hidden="true" />
                  {{ a.blocker.code === 'token_expired' ? '令牌过期' : a.blocker.code === 'account_unavailable' ? '已停用' : '不可查' }}
                </span>
              </div>
              <span class="account-meta">
                <span v-if="a.plan_type" class="plan">{{ a.plan_type }}</span>
                <span>{{ kindLabel[a.kind] }}</span>
                <span v-if="a.email && a.name !== a.email">{{ a.name }}</span>
                <span v-if="!a.enabled" class="text-warn">CPR 已停用</span>
              </span>
            </button>
          </div>

          <!-- 2. CPR 记下的额度 -->
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

          <!-- 3. 官方用量（使用位置占比条 + 悬停卡片） -->
          <div class="usage-cell row-col-usage">
            <template v-if="a.usage?.error">
              <div class="flex items-center gap-1.5">
                <span class="verdict-pill bad" :title="a.usage.error.message">查询失败</span>
                <span class="muted text-xs">· {{ ago(a.usage.checked_at_ms) }}</span>
              </div>
            </template>
            <template v-else-if="a.usage">
              <ZPopover
                v-if="a.usage.surfaces.length"
                trigger="hover"
                placement="top"
                :width="270"
              >
                <template #reference>
                  <button
                    type="button"
                    class="surface-strip-trigger"
                    :aria-label="`${title(a)} 官方用量位置分布`"
                    @click="emit('open', a.id)"
                  >
                    <div class="mini-stack">
                      <i
                        v-for="s in a.usage.surfaces"
                        :key="s.key"
                        :style="{ flexGrow: s.value, background: slotColour(slotOf(s.key)) }"
                      />
                    </div>
                    <div class="flex flex-wrap items-center gap-1.5 text-xs">
                      <span v-if="hasWork(a.usage)" class="font-semibold" style="color: var(--series-6)">
                        Work 占 {{ percent(a.usage.work!.share, a.usage.work!.share < 0.1 ? 1 : 0) }}
                      </span>
                      <span v-else-if="a.usage.work" class="muted">无 Work</span>
                      <span v-else class="muted">明细未读到</span>
                      <span class="muted">· {{ a.usage.days }} 天 · {{ ago(a.usage.checked_at_ms) }}</span>
                      <Info :size="11" class="muted" aria-hidden="true" />
                    </div>
                  </button>
                </template>

                <div class="surface-popover">
                  <div class="surface-popover-head">
                    <strong>最近 {{ a.usage.days }} 天使用位置</strong>
                    <span v-if="a.usage.work" :class="hasWork(a.usage) ? 'verdict-pill work' : 'verdict-pill neutral'">
                      Work {{ percent(a.usage.work.share, a.usage.work.share < 0.1 ? 1 : 0) }}
                    </span>
                  </div>
                  <ul class="surface-popover-list">
                    <li v-for="s in a.usage.surfaces" :key="s.key" class="surface-popover-row">
                      <span class="legend-item truncate">
                        <i class="swatch" :style="{ background: slotColour(slotOf(s.key)) }" />
                        {{ surfaceLabel(s.key) }}
                      </span>
                      <span class="muted">{{ compact(s.value) }}</span>
                      <span class="font-semibold">{{ shareOf(s, surfaceTotal(a)) }}</span>
                    </li>
                  </ul>
                  <div v-if="a.usage.work && a.usage.work.total > 0" class="muted border-t border-cp-border-secondary pt-1.5 text-[11px]">
                    Work 共 {{ a.usage.work.days_with_use }} 天有用量，合计 {{ compact(a.usage.work.total) }}
                  </div>
                </div>
              </ZPopover>

              <div v-else class="flex flex-wrap items-center gap-1.5 text-xs">
                <span v-if="hasWork(a.usage)" class="verdict-pill work">
                  Work {{ percent(a.usage.work!.share, a.usage.work!.share < 0.1 ? 1 : 0) }}
                </span>
                <span v-else-if="a.usage.work" class="muted">最近 {{ a.usage.days }} 天无用量</span>
                <span v-else class="muted">明细没读到</span>
                <span class="muted">· {{ ago(a.usage.checked_at_ms) }}</span>
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
          <div class="flex justify-end gap-1.5">
            <ZButton size="small" @click="emit('open', a.id)">
              详情 <ArrowRight :size="13" aria-hidden="true" />
            </ZButton>
          </div>
        </div>
      </template>

      <!-- 底部色块图例 -->
      <p class="list-legend muted">
        <span
          v-for="s in (['cli', 'vscode', 'desktop_app', 'web', 'exec', 'work', 'other'] as const)"
          :key="s"
          class="legend-item"
        >
          <i class="swatch" :style="{ background: slotColour(s) }" />
          {{ s === 'work' ? 'Work' : s === 'other' ? '其他' : surfaceLabel(s) }}
        </span>
        <span class="legend-note">
          额度是 CPR 自己记下的观测，不访问上游；官方用量和账单点了才查。
        </span>
      </p>
    </div>

    <!-- 底部粘性条：批量查询进度 或 已选账号操作 (turnstate selection-bar 风格) -->
    <div
      v-if="running || selectedIds.size > 0"
      class="selection-bar"
      role="region"
      aria-label="批量操作与查询进度"
    >
      <template v-if="running">
        <div class="flex flex-wrap items-center gap-3 text-cp-xs">
          <span class="font-semibold text-cp-text">
            正在{{ running === 'usage' ? '查询官方用量' : '查询账单' }}
            {{ progress.done }} / {{ progress.total }}
          </span>
          <div class="progress-track" aria-hidden="true">
            <i :style="{ width: `${progress.total ? (progress.done / progress.total) * 100 : 0}%` }" />
          </div>
          <span v-if="progress.currentName" class="muted">
            当前：{{ progress.currentName }}
          </span>
          <span v-if="progress.failed" class="text-bad">
            {{ progress.failed }} 个失败
          </span>
        </div>
        <ZButton size="small" type="danger" @click="stop = true">
          <Square :size="12" aria-hidden="true" /> 停止查询
        </ZButton>
      </template>

      <template v-else>
        <span class="text-cp-xs text-cp-text-secondary">
          已选 <strong class="text-cp-text">{{ selectedIds.size }}</strong> 个账号
        </span>
        <div class="flex flex-wrap items-center gap-2">
          <ZButton size="small" type="primary" @click="runBatch('usage', true)">
            查询所选用量
          </ZButton>
          <ZButton size="small" @click="runBatch('billing', true)">
            查询所选账单
          </ZButton>
          <ZButton size="small" text @click="clearSelection">
            取消选择
          </ZButton>
        </div>
      </template>
    </div>
  </section>
</template>
