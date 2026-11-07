<script setup lang="ts">
import type { ApiError } from '../api'
import type {
  AccountLoginStatus,
  PasteReport,
  QueuedImportTask,
} from './helpers'
import { ZButton, ZTag } from '@codex-proxy/ui'
import {
  Check,
  Copy,
  Link2,
  Play,
  Square,
} from '@lucide/vue'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { loginApi } from './client'
import {
  buildImportQueue,
  formatLoginError,
  needsManualOAuth,
  resolveImportAction,
} from './helpers'

const props = defineProps<{
  loginAccounts?: AccountLoginStatus[]
}>()
const emit = defineEmits<{ accountsChanged: [] }>()

interface ImportExecutionState {
  status: 'idle' | 'running' | 'ok' | 'error'
  accountId?: string
  code?: string
  message?: string
}

const loadingOverview = ref(false)
const localAccounts = ref<AccountLoginStatus[]>([])
const overviewError = ref('')

// 快速导入状态
const pasteText = ref('')
const viaAccountId = ref('')
const includeExistingUpdates = ref(false)
const previewing = ref(false)
const previewReport = ref<PasteReport | null>(null)
const importing = ref(false)
const stopRequested = ref(false)
const importProgress = ref({ done: 0, total: 0 })
const rowStates = ref<Record<number, ImportExecutionState>>({})
const importError = ref('')
const importNotice = ref('')

// 导入行手动登录（在触发行原地展开）
const manualTarget = ref<{
  line: number
  email: string
  kind: string
} | null>(null)
const manualAuthUrl = ref('')
const manualCallbackUrl = ref('')
const manualBusy = ref(false)
const manualCopied = ref(false)
const manualError = ref('')

const accounts = computed(() => props.loginAccounts?.length ? props.loginAccounts : localAccounts.value)
const enabledAccounts = computed(() => accounts.value.filter(a => a.enabled))

const importQueue = computed(() => {
  if (!previewReport.value)
    return []
  return buildImportQueue(pasteText.value, previewReport.value, includeExistingUpdates.value)
})

const importAction = computed(() => resolveImportAction({
  rawText: pasteText.value,
  report: previewReport.value,
  queueLength: importQueue.value.length,
  importing: importing.value,
  includeExistingUpdates: includeExistingUpdates.value,
}))

const hasImportProgress = computed(() => importing.value || Object.keys(rowStates.value).length > 0)

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let previewSeq = 0

watch(enabledAccounts, (list) => {
  if (!viaAccountId.value && list.length > 0) {
    viaAccountId.value = list[0]!.accountId
  }
}, { immediate: true })

watch(pasteText, (next) => {
  if (debounceTimer)
    clearTimeout(debounceTimer)
  const trimmed = next.trim()
  if (!trimmed) {
    previewSeq++
    previewReport.value = null
    rowStates.value = {}
    manualTarget.value = null
    importError.value = ''
    importNotice.value = ''
    return
  }
  debounceTimer = setTimeout(() => {
    runPreview(next)
  }, 400)
})

onBeforeUnmount(() => {
  if (debounceTimer)
    clearTimeout(debounceTimer)
})

async function loadOverview() {
  loadingOverview.value = true
  overviewError.value = ''
  try {
    const r = await loginApi.overview()
    localAccounts.value = r.accounts
    if (!viaAccountId.value) {
      const firstEnabled = r.accounts.find(a => a.enabled)
      if (firstEnabled)
        viaAccountId.value = firstEnabled.accountId
    }
  }
  catch (e) {
    overviewError.value = (e as ApiError).message
  }
  finally {
    loadingOverview.value = false
  }
}

async function runPreview(text: string) {
  const seq = ++previewSeq
  previewing.value = true
  importError.value = ''
  importNotice.value = ''
  rowStates.value = {}
  manualTarget.value = null
  try {
    const r = await loginApi.paste(text, false)
    if (seq === previewSeq) {
      previewReport.value = r
    }
  }
  catch (e) {
    if (seq === previewSeq) {
      importError.value = (e as ApiError).message
    }
  }
  finally {
    if (seq === previewSeq) {
      previewing.value = false
    }
  }
}

async function handleStartImport() {
  const queue = importQueue.value
  if (!queue.length) {
    importError.value = importAction.value.title ?? '没有可导入的行'
    return
  }
  if (!viaAccountId.value && enabledAccounts.value.length === 0) {
    importError.value = 'CPR 中需要至少有一个启用状态的 OpenAI 账号来提供代理出站连接'
    return
  }

  importing.value = true
  stopRequested.value = false
  importError.value = ''
  importNotice.value = ''
  importProgress.value = { done: 0, total: queue.length }

  let succeeded = 0
  let failed = 0

  for (const task of queue) {
    if (stopRequested.value)
      break
    rowStates.value[task.line] = { status: 'running' }
    try {
      await executeImportTask(task)
      succeeded += 1
    }
    catch {
      failed += 1
    }
    finally {
      importProgress.value.done += 1
    }
  }

  importing.value = false
  const stoppedText = stopRequested.value ? '（已手动停止）' : ''
  importNotice.value = `导入执行结束${stoppedText}：成功 ${succeeded} 个，失败 ${failed} 个`
  await loadOverview()
  emit('accountsChanged')
}

async function executeImportTask(task: QueuedImportTask) {
  try {
    const res = await loginApi.importAccount({
      email: task.email,
      password: task.password,
      ...(task.totp ? { totp: task.totp } : {}),
      kind: task.kind,
      ...(viaAccountId.value ? { viaAccountId: viaAccountId.value } : {}),
      replaceExisting: task.action === 'update',
      saveCredentials: true,
    })
    rowStates.value[task.line] = {
      status: 'ok',
      accountId: res.accountId,
      message: task.action === 'update' ? '已更新令牌与已保存密码/2FA' : `已建入 CPR (${res.accountId})`,
    }
  }
  catch (e) {
    const err = e as ApiError
    rowStates.value[task.line] = {
      status: 'error',
      code: err.code,
      message: err.message,
    }
    throw err
  }
}

function handleStopImport() {
  stopRequested.value = true
}

async function toggleManualForImport(line: number, email: string, kind: string) {
  if (manualTarget.value?.line === line) {
    manualTarget.value = null
    return
  }
  manualTarget.value = { line, email, kind }
  manualAuthUrl.value = ''
  manualCallbackUrl.value = ''
  manualError.value = ''
  manualCopied.value = false
  manualBusy.value = true
  try {
    const r = await loginApi.oauthStart({
      email,
      kind,
      ...(viaAccountId.value ? { viaAccountId: viaAccountId.value } : {}),
    })
    manualAuthUrl.value = r.authorizeUrl
  }
  catch (e) {
    manualError.value = (e as ApiError).message
  }
  finally {
    manualBusy.value = false
  }
}

async function submitManualCallback() {
  if (!manualTarget.value || !manualCallbackUrl.value.trim())
    return
  manualBusy.value = true
  manualError.value = ''
  try {
    const target = manualTarget.value
    const r = await loginApi.oauthComplete({
      email: target.email,
      kind: target.kind,
      ...(viaAccountId.value ? { viaAccountId: viaAccountId.value } : {}),
      callbackUrl: manualCallbackUrl.value.trim(),
    })
    rowStates.value[target.line] = {
      status: 'ok',
      accountId: r.accountId,
      message: '手动登录成功，已写回 CPR',
    }
    manualTarget.value = null
    manualAuthUrl.value = ''
    manualCallbackUrl.value = ''
    await loadOverview()
    emit('accountsChanged')
  }
  catch (e) {
    manualError.value = (e as ApiError).message
  }
  finally {
    manualBusy.value = false
  }
}

async function copyManualUrl() {
  if (!manualAuthUrl.value)
    return
  try {
    await navigator.clipboard.writeText(manualAuthUrl.value)
    manualCopied.value = true
  }
  catch {
    manualCopied.value = false
  }
}

onMounted(loadOverview)
</script>

<template>
  <div class="detail-page">
    <p v-if="overviewError" class="banner banner-error" role="alert">
      {{ overviewError }}
    </p>

    <section class="detail-card" aria-labelledby="quick-import-title">
      <div class="detail-card-head">
        <div class="card-title-group">
          <h2 id="quick-import-title" class="card-title">
            导入账号
          </h2>
          <span class="muted">
            一行一个 <code>邮箱----密码----2FA</code>，行尾可加 <code>----team</code> 或 <code>----personal</code>（2FA 为空写 <code>邮箱----密码--------team</code>）
          </span>
        </div>
      </div>

      <textarea
        v-model="pasteText"
        rows="6"
        class="cp-textarea font-mono"
        placeholder="user1@example.com----password123----JBSWY3DPEHPK3PXP----personal&#10;user2@example.com----password456--------team&#10;user3@example.com----password789----otpauth://totp/OpenAI:user3@example.com?secret=JBSWY3DPEHPK3PXP----个人"
        aria-label="导入账号文本"
      />

      <div class="flex flex-wrap items-center justify-between gap-3 text-xs">
        <div class="flex flex-wrap items-center gap-4">
          <label class="inline-flex items-center gap-2 text-cp-text-secondary">
            <span>借哪个账号的代理登录：</span>
            <select v-model="viaAccountId" class="cp-select w-64" aria-label="借哪个账号的代理登录">
              <option v-if="!enabledAccounts.length" value="">无启用中的 CPR 账号</option>
              <option v-for="a in enabledAccounts" :key="a.accountId" :value="a.accountId">
                {{ a.email || a.name }} ({{ a.accountId }})
              </option>
            </select>
          </label>

          <label class="inline-flex cursor-pointer items-center gap-1.5 text-cp-text-secondary">
            <input v-model="includeExistingUpdates" type="checkbox">
            <span>同时对 CPR 中已存在的同邮箱同类型账号重新登录并更新</span>
          </label>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <span v-if="previewing" class="muted">正在解析预览…</span>
          <ZButton
            type="primary"
            size="small"
            :loading="importing"
            :disabled="importAction.disabled"
            :title="importAction.title"
            @click="handleStartImport"
          >
            <Play :size="13" aria-hidden="true" /> {{ importAction.label }}
          </ZButton>
          <ZButton
            v-if="importing"
            size="small"
            @click="handleStopImport"
          >
            <Square :size="12" aria-hidden="true" /> 停止导入 ({{ importProgress.done }}/{{ importProgress.total }})
          </ZButton>
        </div>
      </div>

      <p v-if="importError" class="banner banner-error" role="alert">
        {{ importError }}
      </p>
      <p v-if="importNotice" class="banner banner-ok" role="status">
        {{ importNotice }}
      </p>

      <template v-if="previewReport">
        <div class="preview-stats" aria-label="预览统计">
          <span class="preview-stat">
            <span>总行数</span>
            <strong>{{ previewReport.lines }}</strong>
          </span>
          <span class="preview-stat">
            <span class="state-dot ok" aria-hidden="true" />
            <span>待新建</span>
            <strong>{{ previewReport.created }}</strong>
          </span>
          <span class="preview-stat">
            <span class="state-dot work" aria-hidden="true" />
            <span>已在 CPR</span>
            <strong>{{ previewReport.updated }}</strong>
          </span>
          <span v-if="previewReport.skipped" class="preview-stat">
            <span class="state-dot warn" aria-hidden="true" />
            <span>跳过</span>
            <strong>{{ previewReport.skipped }}</strong>
          </span>
        </div>

        <div v-if="previewReport.items.length" class="card-list" role="list" aria-label="导入预览列表">
          <div
            class="card-head import-preview-head"
            :class="{ 'has-progress': hasImportProgress }"
            aria-hidden="true"
          >
            <span>行号</span>
            <span>邮箱</span>
            <span>类型</span>
            <span>2FA</span>
            <span>状态</span>
            <span v-if="hasImportProgress">导入进度</span>
          </div>

          <template v-for="item in previewReport.items" :key="item.line">
            <div
              role="listitem"
              class="card-row import-preview-row"
              :class="{ 'has-progress': hasImportProgress }"
            >
              <div class="font-mono font-semibold text-cp-text-secondary cell-item" data-label="行号">
                #{{ item.line }}
              </div>

              <div class="min-w-0 cell-item" data-label="邮箱">
                <div class="font-semibold truncate text-cp-text">{{ item.email || '—' }}</div>
              </div>

              <div class="cell-item" data-label="类型">
                <ZTag size="small" :type="item.kind === 'team' ? 'info' : 'default'">
                  {{ item.kind === 'team' ? 'Team' : '个人' }}
                </ZTag>
              </div>

              <div class="cell-item" data-label="2FA">
                <span class="text-xs" :class="item.hasTotp ? 'text-cp-text font-medium' : 'muted'">
                  {{ item.hasTotp ? '含 2FA' : '未存 2FA' }}
                </span>
              </div>

              <div class="cell-item" data-label="状态">
                <ZTag
                  size="small"
                  :type="item.action === 'create' ? 'success' : item.action === 'update' ? 'info' : 'warning'"
                >
                  {{ item.action === 'create' ? '待新建' : item.action === 'update' ? '已在 CPR' : '跳过' }}
                </ZTag>
                <div v-if="item.message" class="muted text-xs mt-0.5">
                  {{ item.message }}
                </div>
              </div>

              <div v-if="hasImportProgress" class="flex flex-wrap items-center gap-1.5 cell-item" data-label="导入进度">
                <template v-if="rowStates[item.line]">
                  <ZTag
                    size="small"
                    :type="rowStates[item.line]?.status === 'ok' ? 'success' : rowStates[item.line]?.status === 'error' ? 'danger' : 'default'"
                  >
                    {{
                      rowStates[item.line]?.status === 'running'
                        ? '正在登录导入…'
                        : rowStates[item.line]?.status === 'ok'
                          ? '成功'
                          : '失败'
                    }}
                  </ZTag>
                  <span class="muted text-xs">
                    {{
                      rowStates[item.line]?.status === 'error'
                        ? formatLoginError(rowStates[item.line]?.code, rowStates[item.line]?.message)
                        : rowStates[item.line]?.message
                    }}
                  </span>
                  <ZButton
                    v-if="rowStates[item.line]?.status === 'error' && needsManualOAuth(rowStates[item.line]?.code)"
                    size="small"
                    @click="toggleManualForImport(item.line, item.email, item.kind)"
                  >
                    <Link2 :size="12" aria-hidden="true" />
                    {{ manualTarget?.line === item.line ? '收起手动登录' : '浏览器手动登录' }}
                  </ZButton>
                </template>
                <span v-else class="muted text-xs">待执行</span>
              </div>
            </div>

            <!-- 触发行原地展开浏览器手动登录 -->
            <div
              v-if="manualTarget?.line === item.line"
              class="sub-panel"
              aria-label="浏览器手动登录"
            >
              <div class="detail-card-head">
                <strong class="text-xs text-cp-text">
                  浏览器手动登录：{{ manualTarget.email }}（{{ manualTarget.kind === 'team' ? 'Team' : '个人' }}）
                </strong>
                <ZButton size="small" @click="manualTarget = null">
                  收起手动登录
                </ZButton>
              </div>
              <p class="muted note">
                请复制下方授权链接在浏览器中打开并完成验证，然后将跳转后的 <code>http://localhost:1455/auth/callback?code=...&amp;state=...</code> 完整地址粘贴到下方提交。
              </p>
              <p v-if="manualError" class="banner banner-error" role="alert">
                {{ manualError }}
              </p>
              <div v-if="manualAuthUrl" class="inline-copy-row">
                <input
                  readonly
                  :value="manualAuthUrl"
                  class="cp-input font-mono text-xs"
                  aria-label="OAuth 授权链接"
                >
                <ZButton size="small" @click="copyManualUrl">
                  <Check v-if="manualCopied" :size="13" aria-hidden="true" />
                  <Copy v-else :size="13" aria-hidden="true" />
                  {{ manualCopied ? '已复制' : '复制授权链接' }}
                </ZButton>
              </div>
              <div class="inline-copy-row">
                <input
                  v-model="manualCallbackUrl"
                  type="text"
                  class="cp-input font-mono text-xs"
                  placeholder="粘贴 http://localhost:1455/auth/callback?code=...&state=..."
                  aria-label="OAuth 回调链接"
                >
                <ZButton
                  type="primary"
                  size="small"
                  :loading="manualBusy"
                  :disabled="!manualCallbackUrl.trim()"
                  :title="!manualCallbackUrl.trim() ? '请先粘贴浏览器跳转后的回调链接' : undefined"
                  @click="submitManualCallback"
                >
                  提交回调链接
                </ZButton>
              </div>
            </div>
          </template>
        </div>
      </template>
    </section>
  </div>
</template>
