<script setup lang="ts">
import type { Account } from '../api'
import type { AccountLoginStatus } from '../login/helpers'
import type { ReloginTarget } from '../login/ReloginDialog.vue'
import type { Models, Overview, OverviewAccount } from './api'
import type { DetailTab } from './components/ManagedAccount.vue'
import { ZButton } from '@codex-proxy/ui'
import { Activity, ArrowLeft, ChevronRight, CreditCard, KeyRound, RefreshCw, Settings, UserPlus } from '@lucide/vue'
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { api as panelApi } from '../api'
import { hasGracePeriod, needsCredentialRelogin } from '../format'
import { loginApi } from '../login/client'
import LoginPage from '../login/LoginPage.vue'
import ReloginDialog from '../login/ReloginDialog.vue'
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
type Batch = 'subscription' | 'billing' | 'usage'
const batchLabel: Record<Batch, string> = { subscription: '订阅查询', billing: '账单查询', usage: '用量查询' }

const view = ref<View>({ name: 'list' })
const settingsTab = ref<'groups' | 'notify'>('groups')
const overview = shallowRef<Overview | null>(null)
const panel = shallowRef<Account[] | null>(null)
const panelById = computed(() => new Map(panel.value?.map(a => [a.id, a])))
const models = shallowRef<Models | null>(null)
const loginInfo = shallowRef<AccountLoginStatus[]>([])
const loginById = computed(() => new Map(loginInfo.value.map(a => [a.accountId, a])))
const savedPasswords = computed(() => new Set(loginInfo.value.filter(a => a.hasPassword).map(a => a.accountId)))
const visibleAccountIds = ref<string[]>([])
const loading = ref(false)
const panelLoading = ref(false)
const error = ref('')
const panelError = ref('')
const notice = ref('')
const noticeTone = ref<'ok' | 'error'>('ok')
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
const reloginDialogTargets = shallowRef<ReloginTarget[]>([])
const reloginSkipped = ref(0)
const reloginOpen = ref(false)

const probeAccounts = computed(() => overview.value?.accounts.filter(a => a.supported) ?? [])
const currentProbe = computed(() => {
  const v = view.value
  return v.name === 'account' ? probeAccounts.value.find(a => a.id === v.id) : undefined
})
const attention = computed(() => probeAccounts.value.filter(needsAttention).length)
const grace = computed(() => panel.value?.filter(a => hasGracePeriod(a)).length ?? 0)
const canQuery = computed(() => panel.value?.some(a => !a.blocker) ?? false)
const reloginTargets = computed(() =>
  (panel.value ?? []).filter(a => a.enabled && needsCredentialRelogin(a) && savedPasswords.value.has(a.id)),
)
const accountNavIds = computed(() =>
  visibleAccountIds.value.length
    ? visibleAccountIds.value
    : (panel.value?.map(a => a.id) ?? overview.value?.accounts.map(a => a.id) ?? []),
)

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

function openRelogin(ids: string[]) {
  if (batch.value || reloginOpen.value)
    return
  const targets: ReloginTarget[] = []
  let skipped = 0
  for (const id of ids) {
    const a = panelById.value.get(id)
    if (a && a.enabled && savedPasswords.value.has(a.id)) {
      targets.push({
        id: a.id,
        name: a.name,
        email: a.email,
        planType: a.plan_type,
      })
    }
    else if (a) {
      skipped++
    }
  }
  if (!targets.length) {
    noticeTone.value = 'error'
    notice.value = '所选账号未保存密码或已停用，无法免输重登'
    return
  }
  notice.value = ''
  noticeTone.value = 'ok'
  reloginDialogTargets.value = targets
  reloginSkipped.value = skipped
  reloginOpen.value = true
}

function onReloginSucceeded(payload: { accountId: string, expiresAtMs: number }) {
  patch(payload.accountId, {
    credential_state: 'ready',
    access_token_expires_at_ms: payload.expiresAtMs,
    blocker: null,
  })
}

function onReloginDone(summary: { total: number, succeeded: number, failed: number, stopped: boolean, firstError?: string }) {
  const skipped = reloginSkipped.value ? `，跳过 ${reloginSkipped.value} 个` : ''
  const detail = summary.failed && summary.firstError ? `：${summary.firstError}` : ''
  noticeTone.value = summary.failed > 0 ? 'error' : 'ok'
  notice.value = `免输重登${summary.stopped ? '已停止' : '完成'}：成功 ${summary.succeeded} 个，失败 ${summary.failed} 个${skipped}${detail}`
  refresh()
}

function handleBatchManage(kind: Batch | 'relogin', ids: string[]) {
  if (kind === 'relogin') {
    openRelogin(ids)
    return
  }
  void runBatch(kind, ids)
}

async function runBatch(kind: Batch, ids?: string[]) {
  if (batch.value || reloginOpen.value)
    return
  const pool = ids ? ids.map(id => panelById.value.get(id)) : (panel.value ?? [])
  const targets = pool.filter((a): a is Account => !!a && !a.blocker)
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
        else {
          patch(a.id, { usage: (await panelApi.usage(a.id, 7)).summary })
        }
      }
      catch (e) {
        state.failed++
        const message = (e as Error).message
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
  await Promise.all(Array.from({ length: Math.min(6, targets.length) }, worker))
  batch.value = null
  const skipped = state.skipped ? `，跳过 ${state.skipped} 个` : ''
  const detail = notice.value ? `：${notice.value}` : ''
  noticeTone.value = state.failed > 0 ? 'error' : 'ok'
  notice.value = `${batchLabel[kind]}${state.stopping ? '已停止' : '完成'}：成功 ${state.done - state.failed} 个，失败 ${state.failed} 个${skipped}${detail}`
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
function switchAccount(id: string) {
  if (view.value.name !== 'account')
    return
  view.value = { name: 'account', id, tab: view.value.tab }
  nextTick(() => app()?.scrollTo({ top: 0 }))
}
function openSettings(tab: 'groups' | 'notify' = 'groups') {
  settingsTab.value = tab
  leaveList({ name: 'settings' })
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
  noticeTone.value = 'ok'
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
      :initial-tab="view.tab" :run="view.run" :models="models" :account-ids="accountNavIds"
      @back="back" @patch="view.name === 'account' && patch(view.id, $event)" @login-updated="refresh"
      @select="switchAccount" @update:tab="t => { if (view.name === 'account') view = { ...view, tab: t } }"
    />
    <SettingsPage v-else-if="view.name === 'settings'" v-model:tab="settingsTab" :overview="overview" @back="back" @changed="() => { refresh(); loadModels() }" />
    <div v-else-if="view.name === 'import'" class="probe-detail-page">
      <header class="detail-header detail-header-bar">
        <ZButton variant="text" aria-label="返回列表" @click="back">
          <ArrowLeft :size="16" aria-hidden="true" /> 返回列表
        </ZButton>
        <div class="min-w-0 flex-1">
          <h1 class="m-0 text-lg font-semibold text-cp-text">导入账号</h1>
        </div>
      </header>
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
          <ZButton
            v-if="reloginTargets.length"
            size="small"
            type="primary"
            :loading="reloginOpen"
            :disabled="!!batch || reloginOpen"
            title="一键批量重登所有已过期且保存了密码的启用账号"
            @click="openRelogin(reloginTargets.map(a => a.id))"
          >
            <KeyRound :size="14" aria-hidden="true" /> 免输重登 ({{ reloginTargets.length }})
          </ZButton>
          <ZButton size="small" :loading="batch?.kind === 'subscription'" :disabled="!canQuery || !!batch || reloginOpen" title="查询所有可查账号的订阅状态与到期日；如需同步账单日，可勾选账号后点「查账单」" @click="runBatch('subscription')"><CreditCard :size="14" aria-hidden="true" /> 查询全部订阅</ZButton>
          <ZButton size="small" :loading="loading || panelLoading" aria-label="刷新列表" @click="refresh"><RefreshCw :size="14" aria-hidden="true" /> 刷新</ZButton>
          <ZButton size="small" @click="leaveList({ name: 'import' })"><UserPlus :size="14" aria-hidden="true" /> 导入账号</ZButton>
          <ZButton size="small" aria-label="探测设置" @click="openSettings('groups')"><Settings :size="14" aria-hidden="true" /> 设置</ZButton>
        </div>
      </header>
      <div v-if="overview && (!overview.config.probe_maintenance_enabled || !overview.config.probe_enabled || (overview.config.telegram_enabled && !overview.config.telegram_configured))" class="mb-3 flex flex-wrap gap-2">
        <button v-if="!overview.config.probe_maintenance_enabled" type="button" class="config-hint-pill" @click="openSettings('notify')">
          <span>后台任务未开启</span>
          <ChevronRight :size="12" aria-hidden="true" />
        </button>
        <button v-if="!overview.config.probe_enabled" type="button" class="config-hint-pill is-warn" @click="openSettings('notify')">
          <span>真实探测未开启</span>
          <ChevronRight :size="12" aria-hidden="true" />
        </button>
        <button v-if="overview.config.telegram_enabled && !overview.config.telegram_configured" type="button" class="config-hint-pill is-warn" @click="openSettings('notify')">
          <span>TG 配置不完整</span>
          <ChevronRight :size="12" aria-hidden="true" />
        </button>
      </div>
      <p v-if="error" class="probe-banner probe-banner-error" role="alert">探测记录：{{ error }}</p>
      <p v-if="panelError" class="probe-banner probe-banner-error" role="alert">订阅摘要：{{ panelError }}</p>
      <p v-if="notice" class="probe-banner" :class="noticeTone === 'error' ? 'probe-banner-error' : 'probe-banner-ok'" :role="noticeTone === 'error' ? 'alert' : 'status'">{{ notice }}</p>
      <AccountList
        v-model:hours="hours" :overview="overview" :loading="loading || panelLoading" :panel-loading="panelLoading"
        :panel="panelById" :login-info="loginById" :subscription-errors="subscriptionErrors" :checking="checking" :saved-passwords="savedPasswords" :batch-busy="!!batch || reloginOpen"
        @open="(id, run) => openAccount(id, 'probe', run)" @billing="id => openAccount(id, 'billing')" @manage="id => openAccount(id, 'access')"
        @relogin="id => openRelogin([id])" @probe="probe" @batch-probe="batchProbe" @batch-config="batchConfig" @batch-manage="handleBatchManage"
        @visible-ids="ids => { visibleAccountIds = ids }"
      />
    </div>

    <div v-if="batch" class="probe-selection-bar bulk-status" role="status" aria-live="polite">
      <div class="bulk-status-main">
        <span class="text-cp-sm tabular-nums">{{ batchLabel[batch.kind] }}：{{ batch.done }} / {{ batch.total }}<template v-if="batch.failed">，失败 {{ batch.failed }} 个</template></span>
        <div class="bulk-progress-track" aria-hidden="true">
          <div class="bulk-progress-fill" :style="{ width: `${batch.total ? Math.round((batch.done / batch.total) * 100) : 0}%` }" />
        </div>
      </div>
      <ZButton size="small" :disabled="batch.stopping" @click="batch.stopping = true">{{ batch.stopping ? '正在停止' : '停止' }}</ZButton>
    </div>
    <ProbeDialog
      v-if="probeAccount" v-model="probeOpen" :account-id="probeAccount.id" :account-name="probeAccount.email || probeAccount.name" method="turnstate"
      :saved-models="{ turnstate: probeAccount.schedule.turnstate.model, modeltrace: probeAccount.schedule.modeltrace.model }" :models="models" @done="load"
    />
    <BatchProbeDialog v-model="batchProbeOpen" :accounts="batchProbeAccounts" :models="models" @done="load" />
    <BatchConfigDialog v-model="batchConfigOpen" :account-ids="batchConfigIds" :models="models" @done="batchConfigDone" />
    <ReloginDialog
      v-model="reloginOpen"
      :targets="reloginDialogTargets"
      :skipped-count="reloginSkipped"
      can-open-account
      @succeeded="onReloginSucceeded"
      @done="onReloginDone"
      @open-account="id => openAccount(id, 'access')"
    />
  </div>
</template>
