<script setup lang="ts">
import type { Account } from '../api'
import { ZButton, ZEmpty, ZInput, ZSegmented, ZTag } from '@codex-proxy/ui'
import { computed, onBeforeUnmount, ref } from 'vue'
import { api } from '../api'
import { ago, attention, billingTag, compact, hasWork, kindLabel, paymentText, percent, title, windowName } from '../format'
import { slotColour, slotOf, surfaceLabel } from '../surfaces'
import WindowMeter from './WindowMeter.vue'

const props = defineProps<{ accounts: Account[] | null, loading: boolean }>()
const emit = defineEmits<{ open: [id: string], patch: [id: string, patch: Partial<Account>] }>()

const search = ref('')
const kind = ref('all')
const kinds = [{ label: '全部', value: 'all' }, { label: '个人', value: 'personal' }, { label: 'Team', value: 'team' }]
const filter = ref('all')
const filters = computed(() => [
  { label: '全部', value: 'all' },
  { label: `Work 有用量 ${props.accounts?.filter(a => hasWork(a.usage)).length ?? 0}`, value: 'work' },
  { label: `扣款异常 ${props.accounts?.filter(a => a.payment && billingTag(a.payment.status) !== 'success' && billingTag(a.payment.status) !== 'default').length ?? 0}`, value: 'payment' },
])

const rows = computed(() => {
  const q = search.value.trim().toLowerCase()
  return (props.accounts ?? [])
    .filter(a => kind.value === 'all' || a.kind === kind.value)
    .filter(a => filter.value !== 'work' || hasWork(a.usage))
    .filter(a => filter.value !== 'payment' || (a.payment && ['danger', 'warning'].includes(billingTag(a.payment.status))))
    .filter(a => !q || `${a.email ?? ''} ${a.name} ${a.plan_type ?? ''}`.toLowerCase().includes(q))
    .sort((a, b) => attention(a) - attention(b) || title(a).localeCompare(title(b)))
})

/** CPR 记下的额度：按窗口长度从短到长 */
function quota(a: Account) {
  return [...(a.quota?.windows ?? [])]
    .filter(w => w.used_percent !== null)
    .sort((x, y) => (x.window_seconds ?? Infinity) - (y.window_seconds ?? Infinity))
}

// 「查询全部」：一个接一个查，可以中途停；只查现在能查的账号
type Bulk = 'usage' | 'billing'
const running = ref<Bulk | null>(null)
const progress = ref({ done: 0, total: 0, failed: 0 })
let stop = false

async function runAll(what: Bulk) {
  const targets = (props.accounts ?? []).filter(a => !a.blocker)
  running.value = what
  stop = false
  progress.value = { done: 0, total: targets.length, failed: 0 }
  for (const a of targets) {
    if (stop)
      break
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
  <section class="account-list">
    <div class="list-toolbar">
      <ZInput v-model="search" placeholder="搜索邮箱、名称、套餐" clearable class="list-search" aria-label="搜索账号" />
      <ZSegmented v-model="kind" :options="kinds" size="small" aria-label="账号类型" />
      <ZSegmented v-model="filter" :options="filters" size="small" aria-label="筛选" />
      <span class="grow" />
      <template v-if="running">
        <span class="muted">{{ running === 'usage' ? '查询用量' : '查询账单' }} {{ progress.done }}/{{ progress.total }}<template v-if="progress.failed">，{{ progress.failed }} 个失败</template></span>
        <ZButton size="small" @click="stop = true">停止</ZButton>
      </template>
      <template v-else>
        <ZButton size="small" :disabled="!accounts?.length" title="逐个查每个账号最近 7 天的官方用量" @click="runAll('usage')">查询全部用量</ZButton>
        <ZButton size="small" :disabled="!accounts?.length" title="逐个查每个账号的第一页账单，记下最近一笔" @click="runAll('billing')">查询全部账单</ZButton>
      </template>
    </div>

    <div v-if="loading && !accounts" class="list-empty">加载中…</div>
    <ZEmpty v-else-if="!rows.length" size="small" :title="accounts?.length ? '没有符合筛选的账号' : 'CPR 里没有 OpenAI OAuth 账号'" />
    <table v-else class="data-table accounts-table">
      <thead>
        <tr><th>账号</th><th>CPR 记下的额度</th><th>官方用量（使用位置）</th><th>最近支付</th><th /></tr>
      </thead>
      <tbody>
        <tr v-for="a in rows" :key="a.id" :data-work="hasWork(a.usage)">
          <td>
            <button type="button" class="account-cell" @click="emit('open', a.id)">
              <span class="account-name">{{ title(a) }}</span>
              <span class="account-meta">
                <span v-if="a.plan_type" class="plan">{{ a.plan_type }}</span>
                <span>{{ kindLabel[a.kind] }}</span>
                <span v-if="a.email && a.name !== a.email">{{ a.name }}</span>
                <ZTag v-if="a.blocker" size="small" type="warning" :title="a.blocker.message">{{ a.blocker.code === 'token_expired' ? '令牌过期' : a.blocker.code === 'account_unavailable' ? '已停用' : '不可查' }}</ZTag>
              </span>
            </button>
          </td>
          <td class="quota-cell">
            <WindowMeter v-for="w in quota(a)" :key="w.window_seconds ?? 0" :label="windowName((w.window_seconds ?? 0) / 60)" :used="w.used_percent" :resets-at="w.reset_at_ms" />
            <span v-if="!quota(a).length" class="muted">—</span>
          </td>
          <td class="usage-cell">
            <template v-if="a.usage?.error">
              <span class="text-bad" :title="a.usage.error.message">查询失败</span>
              <span class="muted"> · {{ ago(a.usage.checked_at_ms) }}</span>
            </template>
            <template v-else-if="a.usage">
              <div v-if="a.usage.surfaces.length" class="mini-stack" :title="a.usage.surfaces.map(s => `${surfaceLabel(s.key)} ${compact(s.value)}`).join('，')">
                <i v-for="s in a.usage.surfaces" :key="s.key" :style="{ flexGrow: s.value, background: slotColour(slotOf(s.key)) }" />
              </div>
              <span v-if="hasWork(a.usage)" class="work-badge">Work {{ percent(a.usage.work!.share, a.usage.work!.share < 0.1 ? 1 : 0) }}</span>
              <span v-else-if="a.usage.work" class="muted">无 Work</span>
              <span v-else class="muted">明细没读到</span>
              <span class="muted"> · {{ a.usage.days }} 天 · {{ ago(a.usage.checked_at_ms) }}</span>
            </template>
            <span v-else class="muted">用量未查询</span>
          </td>
          <td>
            <ZTag v-if="a.payment" size="small" :type="billingTag(a.payment.status)" :title="a.payment.grace_ends_at_ms ? '付款逾期，宽限期内' : undefined">{{ paymentText(a.payment) }}</ZTag>
            <span v-else class="muted">账单未查询</span>
          </td>
          <td class="actions"><ZButton size="small" @click="emit('open', a.id)">详情</ZButton></td>
        </tr>
      </tbody>
    </table>
    <p class="list-legend muted">
      <span v-for="s in (['cli', 'vscode', 'desktop_app', 'web', 'exec', 'work', 'other'] as const)" :key="s" class="legend-item"><i class="swatch" :style="{ background: slotColour(s) }" />{{ s === 'work' ? 'Work' : s === 'other' ? '其他' : surfaceLabel(s) }}</span>
      <span class="legend-note">额度是 CPR 自己记下的观测，不访问上游；官方用量和账单点了才查。</span>
    </p>
  </section>
</template>
