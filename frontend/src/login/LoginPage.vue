<script setup lang="ts">
import type { ApiError } from '../api'
import type {
  AccountLoginStatus,
  PasteReport,
  QueuedImportTask,
} from './helpers'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { computed, onMounted, ref } from 'vue'
import { dateTime, kindLabel } from '../format'
import { loginApi } from './client'
import { buildImportQueue, errorCodeLabel, needsManualOAuth } from './helpers'

interface ImportExecutionState {
  status: 'idle' | 'running' | 'ok' | 'error'
  accountId?: string
  code?: string
  message?: string
}

const loadingOverview = ref(false)
const encryptionConfigured = ref(false)
const encryptionError = ref<string | null>(null)
const accounts = ref<AccountLoginStatus[]>([])
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

// 导入行手动 OAuth 兜底
const manualTarget = ref<{
  line?: number
  accountId?: string
  email: string
  kind: string
} | null>(null)
const manualAuthUrl = ref('')
const manualCallbackUrl = ref('')
const manualBusy = ref(false)
const manualCopied = ref(false)
const manualError = ref('')

// 已有账号单行展开登录
const activeAccountId = ref<string | null>(null)
const quickPassword = ref('')
const quickTotp = ref('')
const quickWorkspaceId = ref('')
const quickSave = ref(true)
const runningAccountId = ref<string | null>(null)
const accountFeedback = ref<Record<string, { ok: boolean, text: string, code?: string }>>({})

const enabledAccounts = computed(() => accounts.value.filter(a => a.enabled))
const importQueue = computed(() => {
  if (!previewReport.value)
    return []
  return buildImportQueue(pasteText.value, previewReport.value, includeExistingUpdates.value)
})

async function loadOverview() {
  loadingOverview.value = true
  overviewError.value = ''
  try {
    const r = await loginApi.overview()
    encryptionConfigured.value = r.encryptionConfigured
    encryptionError.value = r.encryptionError
    accounts.value = r.accounts
    quickSave.value = r.encryptionConfigured
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

async function handlePreview() {
  if (!pasteText.value.trim()) {
    importError.value = '请先粘贴要导入的账号文本'
    return
  }
  previewing.value = true
  importError.value = ''
  importNotice.value = ''
  rowStates.value = {}
  try {
    previewReport.value = await loginApi.paste(pasteText.value, false)
  }
  catch (e) {
    importError.value = (e as ApiError).message
  }
  finally {
    previewing.value = false
  }
}

async function handleStartImport() {
  const queue = importQueue.value
  if (!queue.length) {
    importError.value = '没有可执行的导入项，请先预览或检查行格式'
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
      saveCredentials: encryptionConfigured.value,
    })
    rowStates.value[task.line] = {
      status: 'ok',
      accountId: res.accountId,
      message: task.action === 'update' ? '已更新令牌与凭据' : `已建入 CPR (${res.accountId})`,
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

async function openManualForImport(line: number, email: string, kind: string) {
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

async function openManualForAccount(acct: AccountLoginStatus) {
  manualTarget.value = {
    accountId: acct.accountId,
    email: acct.email,
    kind: acct.kind,
  }
  manualAuthUrl.value = ''
  manualCallbackUrl.value = ''
  manualError.value = ''
  manualCopied.value = false
  manualBusy.value = true
  try {
    const r = await loginApi.oauthStart({ accountId: acct.accountId })
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
      ...(target.accountId ? { accountId: target.accountId } : {}),
      email: target.email,
      kind: target.kind,
      ...(viaAccountId.value ? { viaAccountId: viaAccountId.value } : {}),
      callbackUrl: manualCallbackUrl.value.trim(),
    })
    if (target.line !== undefined) {
      rowStates.value[target.line] = {
        status: 'ok',
        accountId: r.accountId,
        message: `手动授权已建入 CPR (${r.accountId})`,
      }
    }
    if (target.accountId) {
      accountFeedback.value[target.accountId] = {
        ok: true,
        text: '手动 OAuth 换码成功，已写回 CPR',
      }
    }
    manualTarget.value = null
    manualAuthUrl.value = ''
    manualCallbackUrl.value = ''
    await loadOverview()
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

function toggleAccountRow(acct: AccountLoginStatus) {
  if (activeAccountId.value === acct.accountId) {
    activeAccountId.value = null
    return
  }
  activeAccountId.value = acct.accountId
  quickPassword.value = ''
  quickTotp.value = ''
  quickWorkspaceId.value = ''
}

async function runQuickRelogin(acct: AccountLoginStatus) {
  runningAccountId.value = acct.accountId
  delete accountFeedback.value[acct.accountId]
  try {
    const isExpanded = activeAccountId.value === acct.accountId
    const r = await loginApi.runLogin({
      accountId: acct.accountId,
      ...(isExpanded && quickPassword.value ? { password: quickPassword.value } : {}),
      ...(isExpanded && quickTotp.value.trim() ? { totp: quickTotp.value.trim() } : {}),
      ...(isExpanded && quickWorkspaceId.value.trim() ? { workspaceId: quickWorkspaceId.value.trim() } : {}),
      saveCredentials: isExpanded && quickSave.value && encryptionConfigured.value,
    })
    accounts.value = accounts.value.map(a => a.accountId === acct.accountId ? r.account : a)
    accountFeedback.value[acct.accountId] = {
      ok: true,
      text: `登录成功（套餐：${r.planType || 'plus'}）`,
    }
    if (isExpanded) {
      quickPassword.value = ''
      quickTotp.value = ''
    }
  }
  catch (e) {
    const err = e as ApiError
    accountFeedback.value[acct.accountId] = {
      ok: false,
      code: err.code,
      text: err.message,
    }
    await loadOverview().catch(() => {})
  }
  finally {
    runningAccountId.value = null
  }
}

onMounted(loadOverview)
</script>

<template>
  <div class="detail-page">
    <p v-if="overviewError" class="banner banner-error" role="alert">
      {{ overviewError }}
    </p>
    <p v-if="encryptionError" class="banner banner-error" role="alert">
      加密配置错误：{{ encryptionError }}
    </p>
    <p v-else-if="!encryptionConfigured && !loadingOverview" class="banner banner-warn">
      提示：当前尚未在插件敏感设置中配置 <code>encryptionKey</code>。单次输入密码登录或导入仍可运行，但若需持久化保存密码与 2FA 密钥（免输重登），请先在插件设置中填写 32 字节密钥。
    </p>

    <!-- 1. 快速导入模块 -->
    <section class="detail-card" aria-labelledby="quick-import-title">
      <div class="detail-card-head">
        <h2 id="quick-import-title" class="card-title">快速导入账号</h2>
        <span class="muted">
          一行一个 <code>邮箱----密码----2FA</code>，行尾可加 <code>----team</code> 或 <code>----personal</code>（2FA 为空写 <code>邮箱----密码--------team</code>）
        </span>
      </div>

      <textarea
        v-model="pasteText"
        rows="6"
        class="import-textarea font-mono"
        placeholder="user1@example.com----password123----JBSWY3DPEHPK3PXP----personal&#10;user2@example.com----password456--------team&#10;user3@example.com----password789----otpauth://totp/OpenAI:user3@example.com?secret=JBSWY3DPEHPK3PXP----个人"
        aria-label="快速导入账号文本"
      />

      <div class="import-controls">
        <label class="import-select-label">
          <span>借用代理账号（经 host.http.account 出站）：</span>
          <select v-model="viaAccountId" class="import-select" aria-label="借用代理账号">
            <option v-if="!enabledAccounts.length" value="">无启用中的 CPR 账号</option>
            <option v-for="a in enabledAccounts" :key="a.accountId" :value="a.accountId">
              {{ a.email || a.name }} ({{ a.accountId }})
            </option>
          </select>
        </label>

        <label class="import-checkbox">
          <input v-model="includeExistingUpdates" type="checkbox">
          <span>同时对 CPR 中已存在的同邮箱同类型账号重新登录并更新</span>
        </label>

        <div class="import-buttons">
          <ZButton
            size="small"
            :loading="previewing"
            :disabled="importing"
            @click="handlePreview"
          >
            1. 解析并预览
          </ZButton>
          <ZButton
            type="primary"
            size="small"
            :loading="importing"
            :disabled="!importQueue.length || importing"
            @click="handleStartImport"
          >
            2. 确认逐个导入 ({{ importQueue.length }})
          </ZButton>
          <ZButton
            v-if="importing"
            size="small"
            @click="handleStopImport"
          >
            停止导入 ({{ importProgress.done }}/{{ importProgress.total }})
          </ZButton>
        </div>
      </div>

      <p v-if="importError" class="banner banner-error" role="alert">
        {{ importError }}
      </p>
      <p v-if="importNotice" class="banner import-banner-ok" role="status">
        {{ importNotice }}
      </p>

      <template v-if="previewReport">
        <div class="detail-card-head">
          <ZTag size="small">共 {{ previewReport.lines }} 行</ZTag>
          <ZTag size="small" type="success">待新建 {{ previewReport.created }}</ZTag>
          <ZTag size="small" type="info">已在 CPR {{ previewReport.updated }}</ZTag>
          <ZTag v-if="previewReport.skipped" size="small" type="warning">
            跳过 {{ previewReport.skipped }}
          </ZTag>
        </div>

        <table v-if="previewReport.items.length" class="data-table">
          <thead>
            <tr>
              <th>行号</th>
              <th>邮箱</th>
              <th>类型</th>
              <th>2FA</th>
              <th>预览查重</th>
              <th>导入结果</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in previewReport.items" :key="item.line">
              <td class="num">#{{ item.line }}</td>
              <td>{{ item.email || '—' }}</td>
              <td>
                <ZTag size="small" :type="item.kind === 'team' ? 'info' : 'default'">
                  {{ item.kind === 'team' ? 'Team' : '个人' }}
                </ZTag>
              </td>
              <td>{{ item.hasTotp ? '已提供' : '无' }}</td>
              <td>
                <ZTag
                  size="small"
                  :type="item.action === 'create' ? 'success' : item.action === 'update' ? 'info' : 'warning'"
                >
                  {{ item.action === 'create' ? '待新建' : item.action === 'update' ? '已在 CPR' : '跳过' }}
                </ZTag>
                <span v-if="item.message" class="muted ml-2 text-xs">
                  {{ item.message }}
                </span>
              </td>
              <td>
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
                          : errorCodeLabel(rowStates[item.line]?.code)
                    }}
                  </ZTag>
                  <span class="muted ml-2 text-xs">{{ rowStates[item.line]?.message }}</span>
                  <ZButton
                    v-if="rowStates[item.line]?.status === 'error' && needsManualOAuth(rowStates[item.line]?.code)"
                    size="small"
                    class="ml-2"
                    @click="openManualForImport(item.line, item.email, item.kind)"
                  >
                    手动 OAuth 兜底
                  </ZButton>
                </template>
                <span v-else class="muted">—</span>
              </td>
            </tr>
          </tbody>
        </table>
      </template>
    </section>

    <!-- 手动 OAuth 授权兜底面板 -->
    <section v-if="manualTarget" class="detail-card" aria-labelledby="manual-oauth-title">
      <div class="detail-card-head">
        <h3 id="manual-oauth-title" class="card-title">
          手动 OAuth 授权链接兜底：{{ manualTarget.email }} ({{ manualTarget.kind === 'team' ? 'Team' : '个人' }})
        </h3>
        <ZButton size="small" @click="manualTarget = null">关闭</ZButton>
      </div>
      <p class="muted note">
        请复制下方授权链接在浏览器中打开并完成验证，然后将跳转后的 <code>http://localhost:1455/auth/callback?code=...&amp;state=...</code> 完整地址粘贴到下方完成令牌写回。
      </p>
      <p v-if="manualError" class="banner banner-error" role="alert">
        {{ manualError }}
      </p>
      <div v-if="manualAuthUrl" class="import-inline-row">
        <input
          readonly
          :value="manualAuthUrl"
          class="import-input font-mono text-xs"
          aria-label="手动 OAuth 授权链接"
        >
        <ZButton size="small" @click="copyManualUrl">
          {{ manualCopied ? '已复制' : '复制授权链接' }}
        </ZButton>
      </div>
      <div class="import-inline-row">
        <input
          v-model="manualCallbackUrl"
          type="text"
          class="import-input font-mono text-xs"
          placeholder="粘贴 http://localhost:1455/auth/callback?code=...&state=..."
          aria-label="手动 OAuth 回调链接"
        >
        <ZButton
          type="primary"
          size="small"
          :loading="manualBusy"
          :disabled="!manualCallbackUrl.trim()"
          @click="submitManualCallback"
        >
          换取令牌并写回 CPR
        </ZButton>
      </div>
    </section>

    <!-- 2. 已有账号快速登录列表 -->
    <section class="detail-card" aria-labelledby="existing-login-title">
      <div class="detail-card-head">
        <h2 id="existing-login-title" class="card-title">CPR 已有账号快速重新登录</h2>
        <span class="muted">
          支持直接使用已加密保存的凭据免输重登，或输入新凭据登录并写回 CPR
        </span>
      </div>

      <p v-if="!accounts.length && !loadingOverview" class="muted">
        当前 CPR 中还没有 OpenAI OAuth 账号。
      </p>

      <table v-else class="data-table">
        <thead>
          <tr>
            <th>账号</th>
            <th>类型</th>
            <th>CPR 状态</th>
            <th>已存加密凭据</th>
            <th>最近登录 / 状态</th>
            <th class="actions">操作</th>
          </tr>
        </thead>
        <tbody>
          <template v-for="acct in accounts" :key="acct.accountId">
            <tr>
              <td>
                <div class="font-semibold">{{ acct.email || acct.name }}</div>
                <div class="muted text-xs">{{ acct.accountId }}</div>
              </td>
              <td>
                <ZTag size="small" :type="acct.kind === 'team' ? 'info' : 'default'">
                  {{ kindLabel[acct.kind] ?? acct.kind }}
                </ZTag>
              </td>
              <td>
                <ZTag v-if="!acct.enabled" size="small" type="warning">已停用</ZTag>
                <ZTag
                  v-else
                  size="small"
                  :type="acct.credentialState === 'ready' ? 'success' : 'danger'"
                >
                  {{ acct.credentialState }}
                </ZTag>
              </td>
              <td>
                <div class="flex flex-wrap gap-1">
                  <ZTag size="small" :type="acct.hasPassword ? 'success' : 'default'">
                    {{ acct.hasPassword ? '密码已存' : '无密码' }}
                  </ZTag>
                  <ZTag size="small" :type="acct.hasTotp ? 'success' : 'default'">
                    {{ acct.hasTotp ? '2FA 已存' : '无 2FA' }}
                  </ZTag>
                </div>
              </td>
              <td>
                <div v-if="accountFeedback[acct.accountId]" :class="accountFeedback[acct.accountId]?.ok ? 'text-xs' : 'text-bad text-xs'">
                  <strong v-if="accountFeedback[acct.accountId]?.code">
                    [{{ errorCodeLabel(accountFeedback[acct.accountId]?.code) }}]
                  </strong>
                  {{ accountFeedback[acct.accountId]?.text }}
                </div>
                <div v-else-if="acct.lastError" class="text-bad text-xs">
                  [{{ errorCodeLabel(acct.lastError.code) }}] {{ acct.lastError.message }}
                </div>
                <div v-else-if="acct.lastLoginAtMs" class="muted text-xs">
                  上次登录 {{ dateTime(acct.lastLoginAtMs) }}
                </div>
                <span v-else class="muted text-xs">—</span>
              </td>
              <td class="actions">
                <div class="inline-flex flex-wrap justify-end gap-2">
                  <ZButton
                    v-if="acct.hasPassword"
                    type="primary"
                    size="small"
                    :loading="runningAccountId === acct.accountId"
                    :disabled="!acct.enabled"
                    @click="runQuickRelogin(acct)"
                  >
                    免输重登
                  </ZButton>
                  <ZButton
                    size="small"
                    :disabled="!acct.enabled"
                    @click="toggleAccountRow(acct)"
                  >
                    {{ activeAccountId === acct.accountId ? '收起' : '输入凭据登录' }}
                  </ZButton>
                  <ZButton
                    size="small"
                    :disabled="!acct.enabled"
                    @click="openManualForAccount(acct)"
                  >
                    手动 OAuth
                  </ZButton>
                </div>
              </td>
            </tr>
            <tr v-if="activeAccountId === acct.accountId">
              <td colspan="6" class="bg-cp-fill-quaternary p-4">
                <div class="import-controls">
                  <input
                    v-model="quickPassword"
                    type="password"
                    class="import-input"
                    :placeholder="acct.hasPassword ? '留空则使用已保存密码' : '输入登录密码'"
                    aria-label="登录密码"
                  >
                  <input
                    v-model="quickTotp"
                    type="password"
                    class="import-input"
                    :placeholder="acct.hasTotp ? '留空则使用已保存 2FA' : '2FA 密钥（可选）'"
                    aria-label="2FA 密钥"
                  >
                  <input
                    v-model="quickWorkspaceId"
                    type="text"
                    class="import-input"
                    placeholder="Workspace ID（可选）"
                    aria-label="Workspace ID"
                  >
                  <label class="import-checkbox">
                    <input v-model="quickSave" type="checkbox" :disabled="!encryptionConfigured">
                    <span>加密保存凭据</span>
                  </label>
                  <ZButton
                    type="primary"
                    size="small"
                    :loading="runningAccountId === acct.accountId"
                    :disabled="!quickPassword && !acct.hasPassword"
                    @click="runQuickRelogin(acct)"
                  >
                    立即登录并写回
                  </ZButton>
                </div>
              </td>
            </tr>
          </template>
        </tbody>
      </table>
    </section>
  </div>
</template>

<style scoped>
.import-textarea {
  width: 100%;
  box-sizing: border-box;
  padding: 10px 12px;
  border: 1px solid var(--cp-color-border-secondary);
  border-radius: 10px;
  background: var(--cp-color-bg-container);
  color: var(--cp-color-text);
  font-size: 12px;
  line-height: 1.5;
  resize: vertical;
}
.import-controls {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 12px;
}
.import-select-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--cp-color-text-secondary);
}
.import-select,
.import-input {
  padding: 6px 10px;
  border: 1px solid var(--cp-color-border-secondary);
  border-radius: 8px;
  background: var(--cp-color-bg-container);
  color: var(--cp-color-text);
  font-size: 12px;
}
.import-inline-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.import-inline-row .import-input {
  flex: 1;
}
.import-checkbox {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--cp-color-text-secondary);
  cursor: pointer;
}
.import-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-left: auto;
}
.import-banner-ok {
  background: color-mix(in srgb, var(--series-3) 14%, var(--cp-color-bg-container));
  color: var(--cp-color-text);
}
</style>
