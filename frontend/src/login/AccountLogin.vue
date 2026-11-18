<script setup lang="ts">
import type { ApiError } from '../api'
import type { AccountLoginStatus, LoginChoice } from './helpers'
import type { ReloginTarget } from './ReloginDialog.vue'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { Check, Copy, ExternalLink, Eye, EyeOff, KeyRound, Link2, Lock, Trash2 } from '@lucide/vue'
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { canOpenExternal, openExternal } from '../api'
import { copyText, selectField } from '../clipboard'
import { credentialStateLabel, dateTime, formatPlanName } from '../format'
import { loginApi } from './client'
import { formatLoginError, loginChoices, needsManualOAuth, resolveLoginMainAction } from './helpers'
import ReloginDialog from './ReloginDialog.vue'

const props = defineProps<{ accountId: string, refreshSeq?: number }>()
const emit = defineEmits<{ updated: [account: AccountLoginStatus] }>()

const status = ref<AccountLoginStatus | null>(null)
const loadingStatus = ref(false)
const runningLogin = ref(false)
const reloginOpen = ref(false)
const savingCreds = ref(false)
const savingAutoLogin = ref(false)
const startingOAuth = ref(false)
const completingOAuth = ref(false)

const password = ref('')
const totp = ref('')
const showPassword = ref(false)
const showTotp = ref(false)
const workspaceId = ref('')
const organizationId = ref('')
const saveCredentials = ref(true)
const autoLoginEnabled = ref(false)
const showAdvanced = ref(false)
const showManualOAuth = ref(false)

const authorizeUrl = ref('')
const callbackUrl = ref('')
const copiedUrl = ref(false)
const copyFailed = ref(false)
const authUrlInput = ref<HTMLInputElement | null>(null)
const hostOpens = canOpenExternal()

const errorMsg = ref('')
const errorCode = ref('')
const errorChoices = ref<LoginChoice[]>([])
const successMsg = ref('')

/** 多个工作区或组织时可点选的候选：本次的错误优先，其次上次登录失败时记下的 */
const choices = computed<LoginChoice[]>(() => {
  if (errorMsg.value)
    return errorChoices.value
  return status.value?.lastError?.choices ?? []
})

/** 选了工作区或组织后直接重登 */
async function loginWithChoice(choice: LoginChoice) {
  if (choice.kind === 'workspace')
    workspaceId.value = choice.id
  else
    organizationId.value = choice.id
  await handleRunLogin()
}

const blockedByDisabled = computed(() => status.value !== null && !status.value.enabled)
const mainAction = computed(() => resolveLoginMainAction({
  enabled: status.value === null ? true : status.value.enabled,
  hasPassword: Boolean(status.value?.hasPassword),
  inputPassword: password.value,
}))

const saveOnlyTitle = computed(() => {
  if (!password.value && !totp.value.trim())
    return '请先输入要保存的密码或 2FA 密钥'
  return undefined
})

const autoLoginTitle = computed(() => {
  if (blockedByDisabled.value)
    return '账号在 CPR 里已停用，无法自动重登'
  if (!status.value?.hasPassword)
    return '请先保存登录密码'
  return 'CPR 凭据过期/失效或最近一次探测返回 401 时，自动尝试一次登录'
})

const manualToggleTitle = computed(() => {
  if (blockedByDisabled.value)
    return '账号在 CPR 里已停用，无法发起登录'
  return undefined
})

const submitCallbackTitle = computed(() => {
  if (blockedByDisabled.value)
    return '账号在 CPR 里已停用，无法发起登录'
  if (!callbackUrl.value.trim())
    return '请先粘贴浏览器跳转后的回调链接'
  return undefined
})

const reloginTargets = computed<ReloginTarget[]>(() => [{
  id: props.accountId,
  name: status.value?.name || status.value?.email || props.accountId,
  email: status.value?.email,
  ...(workspaceId.value.trim() ? { workspaceId: workspaceId.value.trim() } : {}),
  ...(organizationId.value.trim() ? { organizationId: organizationId.value.trim() } : {}),
}])

async function loadStatus(preserveError = false) {
  loadingStatus.value = true
  if (!preserveError) {
    errorMsg.value = ''
    errorCode.value = ''
  }
  try {
    const r = await loginApi.accountStatus(props.accountId)
    status.value = r.account
    autoLoginEnabled.value = r.account.autoLoginEnabled
    if (r.account.pendingOauth) {
      authorizeUrl.value = r.account.pendingOauth.authorizeUrl
      showManualOAuth.value = true
    }
    if (r.account.lastError?.code && needsManualOAuth(r.account.lastError.code)) {
      showManualOAuth.value = true
      if (!authorizeUrl.value && r.account.enabled && !startingOAuth.value)
        void handleStartOAuth()
    }
    if (r.account.lastError?.code === 'workspace_ambiguous') {
      showAdvanced.value = true
    }
  }
  catch (e) {
    const err = e as ApiError
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
  }
  finally {
    loadingStatus.value = false
  }
}

function onReloginSucceeded(payload: { account: AccountLoginStatus, planType: string, expiresAtMs: number }) {
  status.value = payload.account
  errorMsg.value = ''
  errorCode.value = ''
  errorChoices.value = []
  const planLabel = formatPlanName(payload.planType) || 'Plus'
  successMsg.value = `登录成功，新令牌已写回 CPR（套餐：${planLabel}，到期：${dateTime(payload.expiresAtMs)}）`
  emit('updated', payload.account)
}

function onReloginFailed(payload: { code: string, rawMessage: string, choices: LoginChoice[] }) {
  errorCode.value = payload.code
  errorMsg.value = payload.rawMessage
  errorChoices.value = payload.choices
  if (needsManualOAuth(payload.code)) {
    showManualOAuth.value = true
    if (!authorizeUrl.value && !blockedByDisabled.value && !startingOAuth.value)
      void handleStartOAuth()
  }
  if (payload.code === 'workspace_ambiguous') {
    showAdvanced.value = true
  }
  void loadStatus(true).catch(() => {})
}

async function handleRunLogin() {
  if (mainAction.value.disabled || runningLogin.value || reloginOpen.value)
    return
  if (!password.value && !totp.value.trim() && status.value?.hasPassword) {
    errorMsg.value = ''
    errorCode.value = ''
    errorChoices.value = []
    successMsg.value = ''
    reloginOpen.value = true
    return
  }
  runningLogin.value = true
  errorMsg.value = ''
  errorCode.value = ''
  errorChoices.value = []
  successMsg.value = ''
  try {
    const r = await loginApi.runLogin({
      accountId: props.accountId,
      ...(password.value ? { password: password.value } : {}),
      ...(totp.value.trim() ? { totp: totp.value.trim() } : {}),
      saveCredentials: saveCredentials.value,
      ...(workspaceId.value.trim() ? { workspaceId: workspaceId.value.trim() } : {}),
      ...(organizationId.value.trim() ? { organizationId: organizationId.value.trim() } : {}),
    })
    status.value = r.account
    password.value = ''
    totp.value = ''
    const planLabel = formatPlanName(r.planType) || 'Plus'
    successMsg.value = `登录成功，新令牌已写回 CPR（套餐：${planLabel}，到期：${dateTime(r.expiresAtMs)}）`
    emit('updated', r.account)
  }
  catch (e) {
    const err = e as ApiError
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
    errorChoices.value = loginChoices(err.details)
    if (needsManualOAuth(err.code)) {
      showManualOAuth.value = true
      if (!authorizeUrl.value && !blockedByDisabled.value && !startingOAuth.value)
        void handleStartOAuth()
    }
    if (err.code === 'workspace_ambiguous') {
      showAdvanced.value = true
    }
    await loadStatus(true).catch(() => {})
  }
  finally {
    runningLogin.value = false
  }
}

async function handleSaveCredentials() {
  if (!password.value && !totp.value.trim()) {
    errorMsg.value = '请先输入要保存的密码或 2FA 密钥'
    return
  }
  savingCreds.value = true
  errorMsg.value = ''
  errorCode.value = ''
  successMsg.value = ''
  try {
    const r = await loginApi.saveCredentials({
      accountId: props.accountId,
      ...(password.value ? { password: password.value } : {}),
      ...(totp.value.trim() ? { totp: totp.value.trim() } : {}),
    })
    status.value = r.account
    autoLoginEnabled.value = r.account.autoLoginEnabled
    password.value = ''
    totp.value = ''
    successMsg.value = '已保存密码与 2FA'
    emit('updated', r.account)
  }
  catch (e) {
    const err = e as ApiError
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
  }
  finally {
    savingCreds.value = false
  }
}

async function handleClearCredentials() {
  savingCreds.value = true
  errorMsg.value = ''
  errorCode.value = ''
  successMsg.value = ''
  try {
    const r = await loginApi.saveCredentials({
      accountId: props.accountId,
      clearPassword: true,
      clearTotp: true,
    })
    status.value = r.account
    autoLoginEnabled.value = r.account.autoLoginEnabled
    successMsg.value = '已清除该账号保存的密码与 2FA 密钥'
    emit('updated', r.account)
  }
  catch (e) {
    const err = e as ApiError
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
  }
  finally {
    savingCreds.value = false
  }
}

async function handleAutoLoginChange() {
  if (!status.value)
    return
  savingAutoLogin.value = true
  errorMsg.value = ''
  errorCode.value = ''
  successMsg.value = ''
  const desired = autoLoginEnabled.value
  try {
    const r = await loginApi.setAutoLogin(props.accountId, desired)
    status.value = r.account
    autoLoginEnabled.value = r.account.autoLoginEnabled
    successMsg.value = desired
      ? '已开启自动重登，失效后将尝试登录一次并通过 TG 推送结果'
      : '已关闭自动重登'
    emit('updated', r.account)
  }
  catch (e) {
    const err = e as ApiError
    autoLoginEnabled.value = status.value.autoLoginEnabled
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
  }
  finally {
    savingAutoLogin.value = false
  }
}

function toggleManualOAuth() {
  showManualOAuth.value = !showManualOAuth.value
  if (showManualOAuth.value && !authorizeUrl.value && !blockedByDisabled.value && !startingOAuth.value)
    void handleStartOAuth()
}

async function handleStartOAuth() {
  startingOAuth.value = true
  errorMsg.value = ''
  errorCode.value = ''
  successMsg.value = ''
  copiedUrl.value = false
  copyFailed.value = false
  try {
    const r = await loginApi.oauthStart({ accountId: props.accountId })
    authorizeUrl.value = r.authorizeUrl
    showManualOAuth.value = true
  }
  catch (e) {
    const err = e as ApiError
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
  }
  finally {
    startingOAuth.value = false
  }
}

async function handleCompleteOAuth() {
  if (!callbackUrl.value.trim()) {
    errorMsg.value = '请粘贴浏览器跳转后的 localhost:1455/auth/callback 完整链接'
    return
  }
  completingOAuth.value = true
  errorMsg.value = ''
  errorCode.value = ''
  successMsg.value = ''
  try {
    const r = await loginApi.oauthComplete({
      accountId: props.accountId,
      callbackUrl: callbackUrl.value.trim(),
    })
    if (r.account) {
      status.value = r.account
      emit('updated', r.account)
    }
    callbackUrl.value = ''
    authorizeUrl.value = ''
    successMsg.value = '手动登录成功，已写回 CPR'
  }
  catch (e) {
    const err = e as ApiError
    errorMsg.value = err.message
    errorCode.value = err.code ?? ''
  }
  finally {
    completingOAuth.value = false
  }
}

function onCallbackPaste() {
  void nextTick(() => {
    if (callbackUrl.value.trim().includes('code=') && !blockedByDisabled.value && !completingOAuth.value)
      void handleCompleteOAuth()
  })
}

async function copyAuthorizeUrl() {
  if (!authorizeUrl.value)
    return
  copiedUrl.value = await copyText(authorizeUrl.value)
  copyFailed.value = !copiedUrl.value
  if (copyFailed.value)
    selectField(authUrlInput.value)
}

function openAuthorizeUrl() {
  if (!authorizeUrl.value)
    return
  openExternal(authorizeUrl.value).catch((e: Error) => {
    errorMsg.value = e.message
  })
}

function selectInput(e: Event) {
  (e.target as HTMLInputElement).select()
}

watch(() => props.accountId, () => {
  password.value = ''
  totp.value = ''
  showPassword.value = false
  showTotp.value = false
  autoLoginEnabled.value = false
  showManualOAuth.value = false
  reloginOpen.value = false
  authorizeUrl.value = ''
  callbackUrl.value = ''
  errorMsg.value = ''
  successMsg.value = ''
  void loadStatus()
})

watch(() => props.refreshSeq, (seq) => {
  if (seq)
    void loadStatus()
})

onMounted(() => void loadStatus())
</script>

<template>
  <section id="account-login-section" class="detail-card" aria-labelledby="account-login-title">
    <div class="detail-card-head">
      <div class="card-title-group">
        <h3 id="account-login-title" class="card-title">
          重新登录
        </h3>
        <template v-if="status">
          <ZTag size="small" :type="status.credentialState === 'ready' ? 'success' : 'warning'">
            {{ credentialStateLabel(status.credentialState) }}
          </ZTag>
          <ZTag size="small" :type="status.hasPassword ? 'success' : 'default'">
            {{ status.hasPassword ? '已保存密码' : '未存密码' }}
          </ZTag>
          <ZTag size="small" :type="status.hasTotp ? 'success' : 'default'">
            {{ status.hasTotp ? '已保存 2FA' : '未存 2FA' }}
          </ZTag>
          <ZTag v-if="status.autoLoginEnabled" size="small" type="success">
            自动重登已开启
          </ZTag>
        </template>
        <span v-if="status?.lastLoginAtMs" class="muted">
          上次登录于 {{ dateTime(status.lastLoginAtMs) }}
        </span>
      </div>

      <div class="card-actions">
        <ZButton
          size="small"
          :disabled="blockedByDisabled"
          :title="manualToggleTitle"
          @click="toggleManualOAuth"
        >
          <Link2 :size="13" aria-hidden="true" />
          {{ showManualOAuth ? '收起手动登录' : '浏览器手动登录' }}
        </ZButton>
      </div>
    </div>

    <p v-if="blockedByDisabled" class="banner banner-warn">
      账号在 CPR 里已停用，启用后才能重新登录。
    </p>
    <p v-if="errorMsg" class="banner banner-error" role="alert">
      {{ formatLoginError(errorCode, errorMsg) }}
    </p>
    <p v-else-if="status?.lastError" class="banner banner-warn">
      上次登录失败（{{ dateTime(status.lastError.atMs) }}）：{{ formatLoginError(status.lastError.code, status.lastError.message) }}
    </p>
    <p
      v-if="status?.autoLoginLast && !status.autoLoginLast.success"
      class="banner banner-warn"
      role="status"
    >
      自动重登失败（{{ dateTime(status.autoLoginLast.atMs) }}）：{{ status.autoLoginLast.trigger }}
      <template v-if="status.autoLoginLast.error">；{{ status.autoLoginLast.error }}</template>
    </p>
    <p
      v-else-if="status?.autoLoginLast?.success"
      class="work-inline-note"
      role="status"
    >
      <span class="state-dot ok" aria-hidden="true" />
      <span>自动重登成功（{{ dateTime(status.autoLoginLast.atMs) }}）：{{ status.autoLoginLast.trigger }}</span>
    </p>
    <div v-if="choices.length" class="flex flex-wrap items-center gap-2 text-xs" role="group" aria-label="选一个重登">
      <span class="text-cp-text-secondary">选一个{{ choices[0]?.kind === 'organization' ? '组织' : '工作区' }}直接重登：</span>
      <ZButton
        v-for="c in choices"
        :key="c.kind + c.id"
        size="small"
        :loading="runningLogin && (workspaceId === c.id || organizationId === c.id)"
        :disabled="runningLogin || blockedByDisabled"
        :title="c.id"
        @click="loginWithChoice(c)"
      >
        {{ c.name || c.id }}
      </ZButton>
    </div>
    <p v-if="successMsg" class="banner banner-ok" role="status">
      {{ successMsg }}
    </p>

    <form class="flex flex-col gap-3.5" @submit.prevent="handleRunLogin">
      <div class="form-grid">
        <label class="form-field">
          <span>登录密码</span>
          <div class="input-with-toggle">
            <input
              v-model="password"
              :type="showPassword ? 'text' : 'password'"
              autocomplete="new-password"
              class="cp-input"
              :placeholder="status?.hasPassword ? '已保存密码，留空则直接使用已保存密码' : '输入 OpenAI 登录密码'"
            >
            <button
              type="button"
              class="input-eye-btn"
              :aria-label="showPassword ? '隐藏密码' : '显示密码'"
              :title="showPassword ? '隐藏密码' : '显示密码'"
              @click="showPassword = !showPassword"
            >
              <EyeOff v-if="showPassword" :size="14" aria-hidden="true" />
              <Eye v-else :size="14" aria-hidden="true" />
            </button>
          </div>
        </label>
        <label class="form-field">
          <span>2FA 密钥（Base32 或 otpauth://）</span>
          <div class="input-with-toggle">
            <input
              v-model="totp"
              :type="showTotp ? 'text' : 'password'"
              autocomplete="off"
              class="cp-input"
              :placeholder="status?.hasTotp ? '已保存 2FA，留空则直接使用已保存 2FA' : '选填：Base32 密钥或 otpauth://totp/ 链接'"
            >
            <button
              type="button"
              class="input-eye-btn"
              :aria-label="showTotp ? '隐藏 2FA 密钥' : '显示 2FA 密钥'"
              :title="showTotp ? '隐藏 2FA 密钥' : '显示 2FA 密钥'"
              @click="showTotp = !showTotp"
            >
              <EyeOff v-if="showTotp" :size="14" aria-hidden="true" />
              <Eye v-else :size="14" aria-hidden="true" />
            </button>
          </div>
        </label>
      </div>

      <div v-if="showAdvanced" class="form-grid">
        <label class="form-field">
          <span>指定 Workspace ID（多工作区时必填）</span>
          <input
            v-model="workspaceId"
            type="text"
            class="cp-input"
            placeholder="例如：ws-..."
          >
        </label>
        <label class="form-field">
          <span>指定 Organization ID（多组织时选填）</span>
          <input
            v-model="organizationId"
            type="text"
            class="cp-input"
            placeholder="例如：org-..."
          >
        </label>
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3 text-xs">
        <div class="flex flex-wrap items-center gap-3.5">
          <label class="cp-check-label">
            <input
              v-model="saveCredentials"
              type="checkbox"
              class="cp-check"
            >
            <span>保存密码与 2FA</span>
          </label>
          <label
            class="cp-check-label"
            :title="autoLoginTitle"
          >
            <input
              v-model="autoLoginEnabled"
              type="checkbox"
              class="cp-check"
              :disabled="!status?.hasPassword || blockedByDisabled || savingAutoLogin || runningLogin"
              @change="handleAutoLoginChange"
            >
            <span>{{ savingAutoLogin ? '正在保存自动重登设置…' : '401 / 令牌过期时自动重登一次' }}</span>
          </label>
          <button
            type="button"
            class="inline-action-btn"
            @click="showAdvanced = !showAdvanced"
          >
            {{ showAdvanced ? '隐藏工作区选项' : '指定工作区 ID' }}
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <ZButton
            type="primary"
            native-type="submit"
            size="small"
            :loading="runningLogin || reloginOpen"
            :disabled="mainAction.disabled"
            :title="mainAction.title"
          >
            <KeyRound :size="13" aria-hidden="true" />
            {{ mainAction.label }}
          </ZButton>
          <ZButton
            size="small"
            :loading="savingCreds"
            :disabled="!password && !totp.trim()"
            :title="saveOnlyTitle"
            @click="handleSaveCredentials"
          >
            <Lock :size="13" aria-hidden="true" /> 仅保存
          </ZButton>
          <ZButton
            v-if="status?.hasPassword || status?.hasTotp"
            size="small"
            type="danger"
            variant="text"
            :loading="savingCreds"
            @click="handleClearCredentials"
          >
            <Trash2 :size="13" aria-hidden="true" /> 清除已存凭据
          </ZButton>
        </div>
      </div>
    </form>

    <div v-if="showManualOAuth" class="sub-panel">
      <div class="detail-card-head">
        <strong class="text-xs text-cp-text">浏览器手动登录（遇到 Cloudflare 验证或邮箱验证码时使用）</strong>
        <ZButton
          size="small"
          :loading="startingOAuth"
          :disabled="blockedByDisabled"
          :title="manualToggleTitle"
          @click="handleStartOAuth"
        >
          {{ authorizeUrl ? '重新生成授权链接' : '生成授权链接' }}
        </ZButton>
      </div>
      <p class="muted note">
        在浏览器中打开下方授权链接完成登录，跳转到 <code>http://localhost:1455/auth/callback?code=...</code> 后，复制完整地址贴回下方将自动提交写回 CPR。
      </p>
      <div v-if="authorizeUrl" class="inline-copy-row">
        <input
          ref="authUrlInput"
          readonly
          :value="authorizeUrl"
          class="cp-input font-mono text-xs"
          aria-label="OAuth 授权链接"
          @focus="selectInput"
        >
        <ZButton v-if="hostOpens" type="primary" size="small" @click="openAuthorizeUrl">
          <ExternalLink :size="13" aria-hidden="true" />
          打开授权链接
        </ZButton>
        <ZButton size="small" @click="copyAuthorizeUrl">
          <Check v-if="copiedUrl" :size="13" aria-hidden="true" />
          <Copy v-else :size="13" aria-hidden="true" />
          {{ copiedUrl ? '已复制' : '复制授权链接' }}
        </ZButton>
      </div>
      <p v-else-if="startingOAuth" class="muted note">
        正在生成授权链接…
      </p>
      <p v-if="copyFailed" class="muted text-xs" role="status">
        浏览器不允许插件页面写剪贴板，链接已选中，按 Ctrl+C（Mac 上 ⌘C）复制
      </p>
      <div class="inline-copy-row">
        <input
          v-model="callbackUrl"
          type="text"
          class="cp-input font-mono text-xs"
          placeholder="粘贴 http://localhost:1455/auth/callback?code=...&state=...（粘贴即自动提交）"
          aria-label="OAuth 回调链接"
          @paste="onCallbackPaste"
        >
        <ZButton
          type="primary"
          size="small"
          :loading="completingOAuth"
          :disabled="blockedByDisabled || !callbackUrl.trim()"
          :title="submitCallbackTitle"
          @click="handleCompleteOAuth"
        >
          提交回调链接
        </ZButton>
      </div>
    </div>

    <ReloginDialog
      v-model="reloginOpen"
      :targets="reloginTargets"
      @succeeded="onReloginSucceeded"
      @failed="onReloginFailed"
    />
  </section>
</template>
