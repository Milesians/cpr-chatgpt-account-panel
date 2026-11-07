<script setup lang="ts">
import type { ApiError } from '../api'
import type { AccountLoginStatus } from './helpers'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { Check, Copy, KeyRound, Link2, Lock, Trash2 } from '@lucide/vue'
import { computed, onMounted, ref, watch } from 'vue'
import { credentialStateLabel, dateTime, formatPlanName } from '../format'
import { loginApi } from './client'
import { formatLoginError, needsManualOAuth, resolveLoginMainAction } from './helpers'

const props = defineProps<{ accountId: string }>()
const emit = defineEmits<{ updated: [account: AccountLoginStatus] }>()

const status = ref<AccountLoginStatus | null>(null)
const loadingStatus = ref(false)
const runningLogin = ref(false)
const savingCreds = ref(false)
const startingOAuth = ref(false)
const completingOAuth = ref(false)

const password = ref('')
const totp = ref('')
const workspaceId = ref('')
const organizationId = ref('')
const saveCredentials = ref(true)
const showAdvanced = ref(false)
const showManualOAuth = ref(false)

const authorizeUrl = ref('')
const callbackUrl = ref('')
const copiedUrl = ref(false)

const errorMsg = ref('')
const errorCode = ref('')
const successMsg = ref('')

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

async function loadStatus() {
  loadingStatus.value = true
  errorMsg.value = ''
  errorCode.value = ''
  try {
    const r = await loginApi.accountStatus(props.accountId)
    status.value = r.account
    if (r.account.pendingOauth) {
      authorizeUrl.value = r.account.pendingOauth.authorizeUrl
      showManualOAuth.value = true
    }
    if (r.account.lastError?.code && needsManualOAuth(r.account.lastError.code)) {
      showManualOAuth.value = true
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

async function handleRunLogin() {
  runningLogin.value = true
  errorMsg.value = ''
  errorCode.value = ''
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
    if (needsManualOAuth(err.code)) {
      showManualOAuth.value = true
    }
    if (err.code === 'workspace_ambiguous') {
      showAdvanced.value = true
    }
    await loadStatus().catch(() => {})
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

async function handleStartOAuth() {
  startingOAuth.value = true
  errorMsg.value = ''
  errorCode.value = ''
  successMsg.value = ''
  copiedUrl.value = false
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

async function copyAuthorizeUrl() {
  if (!authorizeUrl.value)
    return
  try {
    await navigator.clipboard.writeText(authorizeUrl.value)
    copiedUrl.value = true
  }
  catch {
    copiedUrl.value = false
  }
}

function selectInput(e: Event) {
  (e.target as HTMLInputElement).select()
}

watch(() => props.accountId, () => {
  password.value = ''
  totp.value = ''
  authorizeUrl.value = ''
  callbackUrl.value = ''
  errorMsg.value = ''
  successMsg.value = ''
  loadStatus()
})

onMounted(loadStatus)
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
          @click="showManualOAuth = !showManualOAuth"
        >
          <Link2 :size="13" aria-hidden="true" />
          {{ showManualOAuth ? '收起手动登录' : '浏览器手动登录' }}
        </ZButton>
      </div>
    </div>

    <p v-if="blockedByDisabled" class="banner banner-warn">
      账号在 CPR 里已停用，停用的账号不能经它的绑定代理发起登录请求。
    </p>
    <p v-if="errorMsg" class="banner banner-error" role="alert">
      {{ formatLoginError(errorCode, errorMsg) }}
    </p>
    <p v-else-if="status?.lastError" class="banner banner-warn">
      上次登录失败（{{ dateTime(status.lastError.atMs) }}）：{{ formatLoginError(status.lastError.code, status.lastError.message) }}
    </p>
    <p v-if="successMsg" class="banner banner-ok" role="status">
      {{ successMsg }}
    </p>

    <div class="form-grid">
      <label class="form-field">
        <span>登录密码</span>
        <input
          v-model="password"
          type="password"
          autocomplete="new-password"
          class="cp-input"
          :placeholder="status?.hasPassword ? '已保存密码，留空则直接使用已保存密码' : '输入 OpenAI 登录密码'"
        >
      </label>
      <label class="form-field">
        <span>2FA 密钥（Base32 或 otpauth://）</span>
        <input
          v-model="totp"
          type="password"
          autocomplete="off"
          class="cp-input"
          :placeholder="status?.hasTotp ? '已保存 2FA，留空则直接使用已保存 2FA' : '选填：Base32 密钥或 otpauth://totp/ 链接'"
        >
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
      <div class="flex flex-wrap items-center gap-3">
        <label class="inline-flex cursor-pointer items-center gap-1.5 text-cp-text-secondary">
          <input
            v-model="saveCredentials"
            type="checkbox"
          >
          <span>保存密码与 2FA</span>
        </label>
        <button
          type="button"
          class="text-link"
          @click="showAdvanced = !showAdvanced"
        >
          {{ showAdvanced ? '隐藏工作区选项' : '指定工作区 ID' }}
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <ZButton
          type="primary"
          size="small"
          :loading="runningLogin"
          :disabled="mainAction.disabled"
          :title="mainAction.title"
          @click="handleRunLogin"
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
        在您自己的浏览器中打开下方授权链接完成登录，浏览器跳转到 <code>http://localhost:1455/auth/callback?code=...</code> 后，复制完整地址贴回下方即可经当前账号的代理写回 CPR。
      </p>
      <div v-if="authorizeUrl" class="inline-copy-row">
        <input
          readonly
          :value="authorizeUrl"
          class="cp-input font-mono text-xs"
          aria-label="OAuth 授权链接"
          @focus="selectInput"
        >
        <ZButton size="small" @click="copyAuthorizeUrl">
          <Check v-if="copiedUrl" :size="13" aria-hidden="true" />
          <Copy v-else :size="13" aria-hidden="true" />
          {{ copiedUrl ? '已复制' : '复制授权链接' }}
        </ZButton>
      </div>
      <div class="inline-copy-row">
        <input
          v-model="callbackUrl"
          type="text"
          class="cp-input font-mono text-xs"
          placeholder="粘贴 http://localhost:1455/auth/callback?code=...&state=..."
          aria-label="OAuth 回调链接"
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
  </section>
</template>
