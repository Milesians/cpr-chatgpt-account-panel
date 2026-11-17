<script setup lang="ts">
import type { Account } from '../api'
import type { AccountLoginStatus } from '../login/helpers'
import type { Models, Overview, OverviewAccount } from './api'
import type { DetailTab } from './components/ManagedAccount.vue'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { Activity, CreditCard, RefreshCw, Settings, UserPlus } from '@lucide/vue'
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { ApiError, api as panelApi } from '../api'
import { hasGracePeriod } from '../format'
import { loginApi } from '../login/client'
import { formatLoginError } from '../login/helpers'
import LoginPage from '../login/LoginPage.vue'
import { api } from './api'
import AccountList from './components/AccountList.vue'
import BatchConfigDialog from './components/BatchConfigDialog.vue'
import BatchProbeDialog from './components/BatchProbeDialog.vue'
import ManagedAccount from './components/ManagedAccount.vue'
import ProbeDialog from './components/ProbeDialog.vue'
import SettingsPage from './components/SettingsPage.vue'
import { dateTime } from './format'
import { needsAttention } from './timeline'

type View = { name: 'list' } | { name: 'account', id: string, tab: DetailTab, run?: string } | { name: 'settings' } | { name: 'import' }
type Batch = 'subscription' | 'billing' | 'usage' | 'relogin'
const batchLabel: Record<Batch, string> = { subscription: '订阅查询', billing: '账单查询', usage: '用量查询', relogin: '免输重登' }

const view = ref<View>({ name: 'list' })
const overview = shallowRef<Overview | null>(null)
const panel = shallowRef<Account[] | null>(null)
const panelById = computed(() => new Map(panel.value?.map(a => [a.id, a])))
const models = shallowRef<Models | null>(null)
const loginInfo = shallowRef<AccountLoginStatus[]>([])
const savedPasswords = computed(() => new Set(loginInfo.value.filter(a => a.hasPassword).map(a => a.accountId)))
const loading = ref(false)
const panelLoading = ref(false)
const error = ref('')
const panelError = ref('')
const notice = ref('')
const hours = ref('24')
const batch = ref<{ kind: Batch, done: number, total: number, failed: number, skipped: number, stopping: boolean } | null>(null)
const subscriptionErrors = ref<Record<string, string>>({})
const checking = ref(new Set<string>())

const probeAccount = shallowRef<OverviewAccount | null>(null)
const probeOpen = ref(false)
const batchProbeAccounts = shallowRef<OverviewAccount[]>([])
const batchProbeOpen = ref(false)
const batchConfigIds = ref<string[]>([])
const batchConfigOpen = ref(false)

const probeAccounts = computed(() => overview.value?.accounts.filter(a => a.supported) ?? [])
const currentProbe = computed(() => {
  const v = view.value
  return v.name === 'account' ? probeAccounts.value.find(a => a.id === v.id) : undefined
})
const attention = computed(() => probeAccounts.value.filter(needsAttention).length)
const grace = computed(() => panel.value?.filter(a => hasGracePeriod(a)).length ?? 0)
const canQuery = computed(() => panel.value?.some(a => !a.blocker) ?? false)

let loadId = 0
async function load() {
  const id = ++loadId
  loading.value = true
  error.value = ''
  try {
    const result = await api.overview(Number(hours.value))
    if (id === loadId)
      overview.value = result
  }
  catch (e) {
    if (id === loadId)
      error.value = (e as Error).message
  }
  finally {
    if (id === loadId)
      loading.value = false
  }
}

let panelLoadId = 0
async function loadPanel() {
  const id = ++panelLoadId
  panelLoading.value = true
  panelError.value = ''
  try {
    const result = await panelApi.accounts()
    if (id === panelLoadId)
      panel.value = result.accounts
  }
  catch (e) {
    if (id === panelLoadId)
      panelError.value = (e as Error).message
  }
  finally {
    if (id === panelLoadId)
      panelLoading.value = false
  }
}
async function loadModels() {
  try { models.value = await api.models() }
  catch { models.value = null }
}
async function loadLogin() {
  try { loginInfo.value = (await loginApi.overview()).accounts }
  catch { /* 登录状态不阻塞订阅和探测 */ }
}
function refresh() {
  void load()
  void loadPanel()
  void loadLogin()
}

function patch(id: string, update: Partial<Account>) {
  panel.value = panel.value?.map(a => a.id === id ? { ...a, ...update } : a) ?? null
  if ('payment' in update)
    delete subscriptionErrors.value[id]
}

async function runBatch(kind: Batch, ids?: string[]) {
  if (batch.value)
    return
  const pool = ids ? ids.map(id => panelById.value.get(id)) : (panel.value ?? [])
  const targets = pool.filter((a): a is Account => !!a && (kind === 'relogin' ? a.enabled && savedPasswords.value.has(a.id) : !a.blocker))
  if (!targets.length)
    return
  notice.value = ''
  batch.value = { kind, done: 0, total: targets.length, failed: 0, skipped: pool.length - targets.length, stopping: false }
  const state = batch.value
  let next = 0
  async function worker() {
    while (!state.stopping && next < targets.length) {
      const a = targets[next++]!
      if (kind === 'subscription' || kind === 'billing') {
        checking.value.add(a.id)
        delete subscriptionErrors.value[a.id]
      }
      try {
        if (kind === 'subscription') {
          const result = await panelApi.subscription(a.id)
          if (result.payment)
            patch(a.id, { payment: result.payment })
        }
        else if (kind === 'billing') {
          const result = await panelApi.billing(a.id)
          patch(a.id, { payment: result.payment })
          if (result.history.subscription_problem)
            subscriptionErrors.value[a.id] = result.history.subscription_problem
          if (result.history.problem || result.history.subscription_problem)
            state.failed++
        }
        else if (kind === 'usage') {
          patch(a.id, { usage: (await panelApi.usage(a.id, 7)).summary })
        }
        else {
          const r = await loginApi.runLogin({ accountId: a.id })
          patch(a.id, { credential_state: 'ready', access_token_expires_at_ms: r.expiresAtMs, blocker: null })
        }
      }
      catch (e) {
        state.failed++
        const message = e instanceof ApiError && kind === 'relogin' ? formatLoginError(e.code, e.message) : (e as Error).message
        if (kind === 'subscription' || kind === 'billing')
          subscriptionErrors.value[a.id] = message
        else if (targets.length === 1)
          notice.value = message
      }
      finally {
        checking.value.delete(a.id)
        state.done++
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(kind === 'relogin' ? 1 : 6, targets.length) }, worker))
  batch.value = null
  const skipped = state.skipped ? `，跳过 ${state.skipped} 个` : ''
  const detail = notice.value ? `：${notice.value}` : ''
  notice.value = `${batchLabel[kind]}${state.stopping ? '已停止' : '完成'}：成功 ${state.done - state.failed} 个，失败 ${state.failed} 个${skipped}${detail}`
  if (kind === 'relogin')
    refresh()
}

let listScroll = 0
const app = () => document.querySelector<HTMLElement>('#app')
function leaveList(next: View) {
  if (view.value.name === 'list')
    listScroll = app()?.scrollTop ?? 0
  view.value = next
  nextTick(() => app()?.scrollTo({ top: 0 }))
}
function openAccount(id: string, tab: DetailTab = 'probe', run?: string) {
  leaveList({ name: 'account', id, tab, run })
}
function back() {
  view.value = { name: 'list' }
  nextTick(() => app()?.scrollTo({ top: listScroll }))
  refresh()
}
function probe(account: OverviewAccount) {
  probeAccount.value = account
  probeOpen.value = true
}
function batchProbe(accounts: OverviewAccount[]) {
  batchProbeAccounts.value = accounts
  batchProbeOpen.value = true
}
function batchConfig(ids: string[]) {
  batchConfigIds.value = ids
  batchConfigOpen.value = true
}
function batchConfigDone(count: number) {
  notice.value = `已更新 ${count} 个账号的测票配置`
  void load()
}

watch(hours, load)
let noticeTimer: ReturnType<typeof setTimeout> | undefined
watch(notice, (value) => {
  clearTimeout(noticeTimer)
  if (value)
    noticeTimer = setTimeout(() => { notice.value = '' }, 7000)
})
onMounted(() => { refresh(); void loadModels() })
onUnmounted(() => { clearTimeout(noticeTimer); if (batch.value) batch.value.stopping = true })
</script>

<template>
  <div class="probe-workspace">
    <ManagedAccount
      v-if="view.name === 'account'" :key="view.id" :id="view.id"
      :account="panelById.get(view.id)" :probe="currentProbe"
      :initial-tab="view.tab" :run="view.run" :models="models"
      @back="back" @patch="view.name === 'account' && patch(view.id, $event)" @login-updated="refresh"
    />
    <SettingsPage v-else-if="view.name === 'settings'" :overview="overview" @back="back" @changed="() => { refresh(); loadModels() }" />
    <div v-else-if="view.name === 'import'" class="probe-detail-page">
      <header class="detail-header"><ZButton variant="text" @click="back">返回列表</ZButton><h1 class="m-0 text-lg font-semibold">导入账号</h1></header>
      <LoginPage @accounts-changed="refresh" />
    </div>
    <div v-show="view.name === 'list'">
      <header class="monitor-header">
        <div class="flex min-w-0 items-center gap-3">
          <span class="monitor-brand"><Activity :size="22" aria-hidden="true" /></span>
          <div class="min-w-0">
            <h1 class="m-0 text-xl font-semibold tracking-tight">ChatGPT 账号</h1>
            <p class="m-0 mt-1 text-cp-xs text-cp-text-tertiary">
              <template v-if="overview || panel">
                {{ panel?.length ?? probeAccounts.length }} 个账号
                <template v-if="attention"> · {{ attention }} 个探测需关注</template>
                <template v-if="grace"> · {{ grace }} 个订阅宽限期</template>
                <template v-if="overview"> · 更新于 {{ dateTime(overview.now_ms) }}</template>
              </template>
              <template v-else>探测判定、测票记录和订阅状态</template>
            </p>
          </div>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <ZButton size="small" :loading="batch?.kind === 'subscription'" :disabled="!canQuery || !!batch" @click="runBatch('subscription')"><CreditCard :size="14" aria-hidden="true" /> 查询全部订阅</ZButton>
          <ZButton size="small" :loading="loading || panelLoading" aria-label="刷新列表" @click="refresh"><RefreshCw :size="14" aria-hidden="true" /> 刷新</ZButton>
          <ZButton size="small" @click="leaveList({ name: 'import' })"><UserPlus :size="14" aria-hidden="true" /> 导入账号</ZButton>
          <ZButton size="small" aria-label="探测设置" @click="leaveList({ name: 'settings' })"><Settings :size="14" aria-hidden="true" /> 设置</ZButton>
        </div>
      </header>
      <div v-if="overview && (!overview.config.probe_maintenance_enabled || !overview.config.probe_enabled || (overview.config.telegram_enabled && !overview.config.telegram_configured))" class="mb-3 flex flex-wrap gap-2">
        <ZTag v-if="!overview.config.probe_maintenance_enabled" size="small">后台任务未开启</ZTag>
        <ZTag v-if="!overview.config.probe_enabled" type="warning" size="small">真实探测未开启</ZTag>
        <ZTag v-if="overview.config.telegram_enabled && !overview.config.telegram_configured" type="warning" size="small">TG 配置不完整</ZTag>
      </div>
      <p v-if="error" class="probe-banner probe-banner-error" role="alert">探测记录：{{ error }}</p>
      <p v-if="panelError" class="probe-banner probe-banner-error" role="alert">订阅摘要：{{ panelError }}</p>
      <p v-if="notice" class="probe-banner" role="status">{{ notice }}</p>
      <AccountList
        v-model:hours="hours" :overview="overview" :loading="loading || panelLoading" :panel-loading="panelLoading"
        :panel="panelById" :subscription-errors="subscriptionErrors" :checking="checking" :saved-passwords="savedPasswords" :batch-busy="!!batch"
        @open="(id, run) => openAccount(id, 'probe', run)" @billing="id => openAccount(id, 'billing')" @manage="id => openAccount(id, 'access')"
        @relogin="id => runBatch('relogin', [id])" @probe="probe" @batch-probe="batchProbe" @batch-config="batchConfig" @batch-manage="runBatch"
      />
    </div>

    <div v-if="batch" class="probe-selection-bar bulk-status" role="status" aria-live="polite">
      <span>{{ batchLabel[batch.kind] }}：{{ batch.done }} / {{ batch.total }}<template v-if="batch.failed">，失败 {{ batch.failed }} 个</template></span>
      <ZButton size="small" :disabled="batch.stopping" @click="batch.stopping = true">{{ batch.stopping ? '正在停止' : '停止' }}</ZButton>
    </div>
    <ProbeDialog
      v-if="probeAccount" v-model="probeOpen" :account-id="probeAccount.id" :account-name="probeAccount.email || probeAccount.name" method="turnstate"
      :saved-models="{ turnstate: probeAccount.schedule.turnstate.model, modeltrace: probeAccount.schedule.modeltrace.model }" :models="models" @done="load"
    />
    <BatchProbeDialog v-model="batchProbeOpen" :accounts="batchProbeAccounts" :models="models" @done="load" />
    <BatchConfigDialog v-model="batchConfigOpen" :account-ids="batchConfigIds" :models="models" @done="batchConfigDone" />
  </div>
</template>
