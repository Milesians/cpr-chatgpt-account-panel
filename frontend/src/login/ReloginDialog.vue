<script setup lang="ts">
import type { AccountLoginStatus, LoginChoice } from './helpers'
import { ZButton, ZDialog, ZTag } from '@codex-proxy/ui'
import { Link2, RotateCcw } from '@lucide/vue'
import { computed, onUnmounted, ref, watch } from 'vue'
import { ApiError } from '../api'
import { dateTime, formatPlanName } from '../format'
import { loginApi } from './client'
import {
  extractLoginStage,
  formatLoginError,
  LOGIN_STAGE_STEPS,
  loginChoices,
  loginStageLabel,
  needsManualOAuth,
} from './helpers'

export interface ReloginTarget {
  id: string
  name: string
  email?: string | null
  planType?: string | null
  workspaceId?: string
  organizationId?: string
}

type ItemState = 'queued' | 'running' | 'done' | 'failed'

interface ReloginItem {
  target: ReloginTarget
  state: ItemState
  stage: string
  stageLabel: string
  startedAtMs?: number
  finishedAtMs?: number
  planType?: string
  expiresAtMs?: number
  errorCode?: string
  errorStage?: string
  errorMessage?: string
  choices?: LoginChoice[]
}

const props = defineProps<{
  targets: ReloginTarget[]
  skippedCount?: number
  canOpenAccount?: boolean
}>()

const emit = defineEmits<{
  succeeded: [payload: { accountId: string, account: AccountLoginStatus, planType: string, expiresAtMs: number }]
  failed: [payload: { accountId: string, code: string, message: string, rawMessage: string, stage: string, choices: LoginChoice[] }]
  done: [summary: { total: number, succeeded: number, failed: number, stopped: boolean, firstError?: string }]
  openAccount: [accountId: string, manualOAuth: boolean]
}>()

const open = defineModel<boolean>({ default: false })

const items = ref<ReloginItem[]>([])
const running = ref(false)
const stopped = ref(false)
let pollTimer: ReturnType<typeof setInterval> | null = null
let sessionSeq = 0

function stopPolling() {
  if (pollTimer !== null) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

function makeItem(target: ReloginTarget): ReloginItem {
  return {
    target,
    state: 'queued',
    stage: '',
    stageLabel: '',
  }
}

const finishedCount = computed(() =>
  items.value.filter(i => i.state === 'done' || i.state === 'failed').length,
)
const succeededItems = computed(() =>
  items.value.filter(i => i.state === 'done'),
)
const failedItems = computed(() =>
  items.value.filter(i => i.state === 'failed'),
)
const queuedCount = computed(() =>
  items.value.filter(i => i.state === 'queued').length,
)

const progressPercent = computed(() => {
  const total = items.value.length
  if (!total)
    return 0
  let units = 0
  const stageCount = LOGIN_STAGE_STEPS.length
  for (const item of items.value) {
    if (item.state === 'done' || item.state === 'failed') {
      units += 1
    }
    else if (item.state === 'running') {
      const idx = LOGIN_STAGE_STEPS.findIndex(s => s.key === item.stage)
      const fraction = idx >= 0 ? (idx + 0.6) / stageCount : 0.1
      units += Math.min(0.95, Math.max(0.08, fraction))
    }
  }
  return Math.min(100, Math.round((units / total) * 100))
})

const dialogTitle = computed(() => {
  if (items.value.length === 1) {
    const t = items.value[0]!.target
    return `免输重登 · ${t.email || t.name}`
  }
  return `免输重登（${items.value.length} 个账号）`
})

const summaryText = computed(() => {
  const total = items.value.length
  if (running.value)
    return `正在执行重登（已完成 ${finishedCount.value} / ${total}）`
  if (stopped.value)
    return `重登已停止（已完成 ${finishedCount.value} / ${total}）`
  return `重登完成（共 ${total} 个）`
})

function stepState(item: ReloginItem, stepKey: string): 'done' | 'active' | 'pending' {
  const currentIdx = LOGIN_STAGE_STEPS.findIndex(s => s.key === item.stage)
  const stepIdx = LOGIN_STAGE_STEPS.findIndex(s => s.key === stepKey)
  if (currentIdx < 0)
    return stepIdx === 0 ? 'active' : 'pending'
  if (stepIdx < currentIdx)
    return 'done'
  if (stepIdx === currentIdx)
    return 'active'
  return 'pending'
}

function itemDotState(item: ReloginItem): string {
  if (item.state === 'running')
    return 'running'
  if (item.state === 'done')
    return 'ok'
  if (item.state === 'failed')
    return 'degraded'
  return 'empty'
}

function itemStatusText(item: ReloginItem): string {
  if (item.state === 'queued')
    return stopped.value && !running.value ? '已取消' : '等待中'
  if (item.state === 'running')
    return item.stageLabel || '正在登录…'
  if (item.state === 'done') {
    if (item.startedAtMs && item.finishedAtMs)
      return `成功 · ${formatElapsed(item.finishedAtMs - item.startedAtMs)}`
    return '成功'
  }
  return '失败'
}

function formatElapsed(ms: number): string {
  const sec = Math.max(0.1, ms / 1000)
  return sec >= 10 ? `${Math.round(sec)}s` : `${sec.toFixed(1)}s`
}

async function runOne(item: ReloginItem, seq: number, choice?: LoginChoice) {
  if (seq !== sessionSeq)
    return
  item.state = 'running'
  item.stage = 'prepare'
  item.stageLabel = '准备重登…'
  item.startedAtMs = Date.now()
  item.finishedAtMs = undefined
  item.errorCode = undefined
  item.errorStage = undefined
  item.errorMessage = undefined
  item.choices = undefined

  const workspaceId = choice?.kind === 'workspace' ? choice.id : item.target.workspaceId
  const organizationId = choice?.kind === 'organization' ? choice.id : item.target.organizationId
  if (choice?.kind === 'workspace')
    item.target.workspaceId = choice.id
  else if (choice?.kind === 'organization')
    item.target.organizationId = choice.id

  stopPolling()
  pollTimer = setInterval(async () => {
    if (seq !== sessionSeq || item.state !== 'running')
      return
    try {
      const res = await loginApi.progress(item.target.id)
      if (seq === sessionSeq && item.state === 'running' && res.progress) {
        item.stage = res.progress.stage
        item.stageLabel = res.progress.label
      }
    }
    catch {
      // 旧版 mock 或瞬态轮询失败不影响主登录请求
    }
  }, 450)

  try {
    const r = await loginApi.runLogin({
      accountId: item.target.id,
      ...(workspaceId ? { workspaceId } : {}),
      ...(organizationId ? { organizationId } : {}),
    })
    if (seq !== sessionSeq)
      return
    item.state = 'done'
    item.stage = ''
    item.stageLabel = ''
    item.finishedAtMs = Date.now()
    item.planType = r.planType
    item.expiresAtMs = r.expiresAtMs
    emit('succeeded', {
      accountId: item.target.id,
      account: r.account,
      planType: r.planType,
      expiresAtMs: r.expiresAtMs,
    })
  }
  catch (e) {
    if (seq !== sessionSeq)
      return
    item.state = 'failed'
    item.finishedAtMs = Date.now()
    let rawMessage = ''
    if (e instanceof ApiError) {
      item.errorCode = e.code
      item.errorStage = extractLoginStage(e.details) || item.stage
      rawMessage = e.message
      item.errorMessage = formatLoginError(e.code, e.message)
      item.choices = loginChoices(e.details)
    }
    else {
      item.errorCode = ''
      item.errorStage = item.stage
      rawMessage = (e as Error)?.message?.trim() || '重登失败，未返回具体错误原因'
      item.errorMessage = rawMessage
      item.choices = []
    }
    emit('failed', {
      accountId: item.target.id,
      code: item.errorCode ?? '',
      message: item.errorMessage,
      rawMessage,
      stage: item.errorStage ?? '',
      choices: item.choices ?? [],
    })
  }
  finally {
    stopPolling()
  }
}

async function runList(targetsToRun: ReloginItem[]) {
  if (running.value || !targetsToRun.length)
    return
  const seq = ++sessionSeq
  running.value = true
  stopped.value = false
  for (const item of targetsToRun) {
    item.state = 'queued'
    item.stage = ''
    item.stageLabel = ''
    item.errorCode = undefined
    item.errorStage = undefined
    item.errorMessage = undefined
    item.choices = undefined
  }
  for (const item of targetsToRun) {
    if (stopped.value || seq !== sessionSeq)
      break
    await runOne(item, seq)
  }
  if (seq !== sessionSeq)
    return
  running.value = false
  emit('done', {
    total: items.value.length,
    succeeded: succeededItems.value.length,
    failed: failedItems.value.length,
    stopped: stopped.value,
    firstError: failedItems.value[0]?.errorMessage,
  })
}

async function retryOne(item: ReloginItem, choice?: LoginChoice) {
  if (running.value)
    return
  const seq = ++sessionSeq
  running.value = true
  stopped.value = false
  await runOne(item, seq, choice)
  if (seq !== sessionSeq)
    return
  running.value = false
  emit('done', {
    total: items.value.length,
    succeeded: succeededItems.value.length,
    failed: failedItems.value.length,
    stopped: false,
    firstError: failedItems.value[0]?.errorMessage,
  })
}

function retryFailed() {
  if (running.value || !failedItems.value.length)
    return
  void runList(failedItems.value)
}

function stop() {
  stopped.value = true
}

function goToAccount(item: ReloginItem, manualOAuth: boolean) {
  if (running.value)
    return
  open.value = false
  emit('openAccount', item.target.id, manualOAuth)
}

watch(open, (isOpen) => {
  if (isOpen) {
    items.value = props.targets.map(makeItem)
    running.value = false
    stopped.value = false
    if (items.value.length)
      void runList(items.value)
  }
  else {
    sessionSeq++
    stopPolling()
    running.value = false
  }
}, { immediate: true })

onUnmounted(() => {
  sessionSeq++
  stopPolling()
})
</script>

<template>
  <ZDialog
    v-model="open"
    :title="dialogTitle"
    width="42rem"
    :close-on-click-modal="!running"
    :close-on-press-escape="!running"
    :before-close="() => !running"
  >
    <div class="flex flex-col gap-3.5 text-cp-sm">
      <div class="relogin-summary" role="status" aria-live="polite">
        <div class="flex flex-wrap items-center justify-between gap-2 text-cp-xs">
          <span class="font-medium text-cp-text">{{ summaryText }}</span>
          <div class="flex flex-wrap items-center gap-3 tabular-nums text-cp-text-secondary">
            <span v-if="succeededItems.length" class="text-cp-success-text font-medium">成功 {{ succeededItems.length }}</span>
            <span v-if="failedItems.length" class="text-cp-error font-medium">失败 {{ failedItems.length }}</span>
            <span v-if="skippedCount">跳过 {{ skippedCount }}</span>
          </div>
        </div>
        <div class="relogin-progress-track" aria-hidden="true">
          <div
            class="relogin-progress-fill"
            :class="{ 'has-error': failedItems.length > 0 && !running }"
            :style="{ width: `${progressPercent}%` }"
          />
        </div>
        <p v-if="skippedCount" class="m-0 text-cp-xs text-cp-text-tertiary">
          已跳过 {{ skippedCount }} 个未保存密码或已停用的账号
        </p>
      </div>

      <ul class="relogin-list" aria-label="免输重登进度列表">
        <li
          v-for="item in items"
          :key="item.target.id"
          class="relogin-item"
          :data-relogin-state="item.state"
        >
          <div class="relogin-item-head">
            <span
              class="probe-state-dot"
              :class="{ 'is-pulsing': item.state === 'running' }"
              :data-state="itemDotState(item)"
            />
            <span class="min-w-0 flex flex-1 items-center gap-2">
              <span class="truncate font-medium text-cp-text" :title="item.target.email || item.target.name">
                {{ item.target.email || item.target.name }}
              </span>
              <span v-if="item.planType || item.target.planType" class="account-plan">
                {{ formatPlanName(item.planType || item.target.planType) }}
              </span>
            </span>
            <ZTag
              v-if="item.state === 'done'"
              size="small"
              type="success"
            >
              {{ itemStatusText(item) }}
            </ZTag>
            <ZTag
              v-else-if="item.state === 'failed'"
              size="small"
              type="danger"
            >
              失败
            </ZTag>
            <span
              v-else
              class="shrink-0 text-cp-xs tabular-nums"
              :class="item.state === 'running' ? 'text-cp-primary font-medium' : 'text-cp-text-tertiary'"
            >
              {{ itemStatusText(item) }}
            </span>
          </div>

          <div v-if="item.state === 'running'" class="relogin-steps" aria-label="登录阶段进度">
            <span
              v-for="step in LOGIN_STAGE_STEPS"
              :key="step.key"
              class="relogin-step-chip"
              :data-step-state="stepState(item, step.key)"
            >
              {{ step.label }}
            </span>
          </div>

          <p v-else-if="item.state === 'done'" class="relogin-ok-note">
            新令牌已写回 CPR<template v-if="item.planType || item.expiresAtMs">
              （<template v-if="item.planType">套餐：{{ formatPlanName(item.planType) }}</template><template v-if="item.planType && item.expiresAtMs">，</template><template v-if="item.expiresAtMs">到期：{{ dateTime(item.expiresAtMs) }}</template>）
            </template>
          </p>

          <div v-else-if="item.state === 'failed'" class="relogin-error-box" role="alert">
            <div class="relogin-error-text">
              <span v-if="item.errorStage" class="relogin-stage-badge">
                {{ loginStageLabel(item.errorStage) }}阶段
              </span>
              <span>{{ item.errorMessage }}</span>
            </div>
            <div v-if="!running" class="relogin-error-actions">
              <template v-if="item.choices?.length">
                <span class="text-cp-xs text-cp-text-secondary">
                  选{{ item.choices[0]?.kind === 'organization' ? '组织' : '工作区' }}重试：
                </span>
                <ZButton
                  v-for="c in item.choices"
                  :key="c.kind + c.id"
                  size="small"
                  :title="c.id"
                  @click="retryOne(item, c)"
                >
                  {{ c.name || c.id }}
                </ZButton>
              </template>
              <ZButton size="small" @click="retryOne(item)">
                <RotateCcw :size="12" aria-hidden="true" /> 重试
              </ZButton>
              <ZButton
                v-if="canOpenAccount"
                size="small"
                @click="goToAccount(item, needsManualOAuth(item.errorCode))"
              >
                <Link2 :size="12" aria-hidden="true" />
                {{ needsManualOAuth(item.errorCode) ? '浏览器手动登录' : '去账号页处理' }}
              </ZButton>
            </div>
          </div>
        </li>
      </ul>
    </div>

    <template #footer>
      <ZButton
        v-if="running && queuedCount > 0"
        :disabled="stopped"
        @click="stop"
      >
        {{ stopped ? '正在停止…' : '停止排队' }}
      </ZButton>
      <ZButton
        v-if="!running && failedItems.length"
        @click="retryFailed"
      >
        <RotateCcw :size="13" aria-hidden="true" /> 重试失败项 ({{ failedItems.length }})
      </ZButton>
      <ZButton
        :type="running ? 'default' : 'primary'"
        :disabled="running"
        @click="open = false"
      >
        {{ running ? '正在重登…' : '关闭' }}
      </ZButton>
    </template>
  </ZDialog>
</template>
