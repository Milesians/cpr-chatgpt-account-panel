<script setup lang="ts">
import type { ApiError } from '../api'
import type { AccountLoginStatus } from './helpers'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { computed, onMounted, ref, watch } from 'vue'
import { dateTime } from '../format'
import { loginApi } from './client'
import { errorCodeLabel, needsManualOAuth } from './helpers'

const props = defineProps<{ accountId: string }>()

const status = ref<AccountLoginStatus | null>(null)
const encryptionConfigured = ref(false)
const encryptionError = ref<string | null>(null)
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
const canQuickRelogin = computed(() => {
  if (blockedByDisabled.value)
    return false
  return Boolean(password.value.trim() || status.value?.hasPassword)
})

async function loadStatus() {
  loadingStatus.value = true
  errorMsg.value = ''
  errorCode.value = ''
  try {
    const r = await loginApi.accountStatus(props.accountId)
    status.value = r.account
    encryptionConfigured.value = r.encryptionConfigured
    encryptionError.value = r.encryptionError
    saveCredentials.value = r.encryptionConfigured
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
      saveCredentials: saveCredentials.value && encryptionConfigured.value,
      ...(workspaceId.value.trim() ? { workspaceId: workspaceId.value.trim() } : {}),
      ...(organizationId.value.trim() ? { organizationId: organizationId.value.trim() } : {}),
    })
    status.value = r.account
    password.value = ''
    totp.value = ''
    successMsg.value = `登录成功，新令牌已写回 CPR（套餐：${r.planType || 'plus'}，到期：${dateTime(r.expiresAtMs)}）`
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
    successMsg.value = '凭据已使用 AES-256-GCM 加密保存到私有状态'
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
    successMsg.value = '已清除该账号保存的加密密码与 2FA 密钥'
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
    if (r.account)
      status.value = r.account
    callbackUrl.value = ''
    authorizeUrl.value = ''
    successMsg.value = '手动 OAuth 授权换码成功，新令牌已写回 CPR'
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
      <h3 id="account-login-title" class="card-title">重新登录与凭据管理</h3>
      <template v-if="status">
        <ZTag size="small" :type="status.hasPassword ? 'success' : 'default'">
          {{ status.hasPassword ? '已存加密密码' : '未存密码' }}
        </ZTag>
        <ZTag size="small" :type="status.hasTotp ? 'success' : 'default'">
          {{ status.hasTotp ? '已存加密 2FA' : '未存 2FA' }}
        </ZTag>
        <ZTag v-if="!encryptionConfigured" size="small" type="warning">
          未配加密密钥
        </ZTag>
      </template>
      <ZButton
        type="primary"
        size="small"
        :loading="runningLogin"
        :disabled="!canQuickRelogin"
        @click="handleRunLogin"
      >
        {{ status?.hasPassword && !password ? '用已存凭据重新登录' : '登录并写回 CPR' }}
      </ZButton>
      <ZButton
        size="small"
        :disabled="blockedByDisabled"
        @click="showManualOAuth = !showManualOAuth"
      >
        {{ showManualOAuth ? '收起手动授权' : '手动 OAuth 链接兜底' }}
      </ZButton>
      <span v-if="status?.lastLoginAtMs" class="muted">
        上次登录于 {{ dateTime(status.lastLoginAtMs) }}
      </span>
    </div>

    <p v-if="blockedByDisabled" class="banner banner-warn">
      账号在 CPR 里已停用，停用的账号不能经它的绑定代理发起登录请求。
    </p>
    <p v-if="encryptionError" class="banner banner-error" role="alert">
      加密配置错误：{{ encryptionError }}
    </p>
    <p v-if="errorMsg" class="banner banner-error" role="alert">
      <strong v-if="errorCode">[{{ errorCodeLabel(errorCode) }}] </strong>{{ errorMsg }}
    </p>
    <p v-else-if="status?.lastError" class="banner banner-warn">
      上次登录失败（{{ errorCodeLabel(status.lastError.code) }} · {{ dateTime(status.lastError.atMs) }}）：{{ status.lastError.message }}
    </p>
    <p v-if="successMsg" class="banner login-banner-ok" role="status">
      {{ successMsg }}
    </p>

    <div class="login-form-grid">
      <label class="login-field">
        <span>登录密码</span>
        <input
          v-model="password"
          type="password"
          autocomplete="new-password"
          class="login-input"
          :placeholder="status?.hasPassword ? '已加密保存，留空则直接使用已保存密码' : '输入 OpenAI 登录密码'"
        >
      </label>
      <label class="login-field">
        <span>2FA 密钥（Base32 或 otpauth://）</span>
        <input
          v-model="totp"
          type="password"
          autocomplete="off"
          class="login-input"
          :placeholder="status?.hasTotp ? '已加密保存，留空则直接使用已保存 2FA' : '选填：Base32 密钥或 otpauth://totp/ 链接'"
        >
      </label>
    </div>

    <div v-if="showAdvanced" class="login-form-grid">
      <label class="login-field">
        <span>指定 Workspace ID（多工作区时必填）</span>
        <input
          v-model="workspaceId"
          type="text"
          class="login-input"
          placeholder="例如：ws-..."
        >
      </label>
      <label class="login-field">
        <span>指定 Organization ID（多组织时选填）</span>
        <input
          v-model="organizationId"
          type="text"
          class="login-input"
          placeholder="例如：org-..."
        >
      </label>
    </div>

    <div class="login-toolbar">
      <label class="login-checkbox">
        <input
          v-model="saveCredentials"
          type="checkbox"
          :disabled="!encryptionConfigured"
        >
        <span>
          登录时加密保存新输入的密码与 2FA
          <template v-if="!encryptionConfigured">（需先在插件设置中填写 encryptionKey）</template>
        </span>
      </label>
      <div class="login-actions">
        <ZButton size="small" @click="showAdvanced = !showAdvanced">
          {{ showAdvanced ? '隐藏工作区选项' : '指定工作区 ID' }}
        </ZButton>
        <ZButton
          size="small"
          :loading="savingCreds"
          :disabled="!encryptionConfigured || (!password && !totp.trim())"
          @click="handleSaveCredentials"
        >
          仅加密保存凭据
        </ZButton>
        <ZButton
          v-if="status?.hasPassword || status?.hasTotp"
          size="small"
          :loading="savingCreds"
          @click="handleClearCredentials"
        >
          清除已存凭据
        </ZButton>
      </div>
    </div>

    <div v-if="showManualOAuth" class="login-manual-box">
      <div class="detail-card-head">
        <strong class="text-xs">手动 OAuth 授权链接兜底（遇到 Cloudflare 验证或邮箱验证码时使用）</strong>
        <ZButton
          size="small"
          :loading="startingOAuth"
          :disabled="blockedByDisabled"
          @click="handleStartOAuth"
        >
          {{ authorizeUrl ? '重新生成授权链接' : '生成授权链接' }}
        </ZButton>
      </div>
      <p class="muted note">
        在您自己的浏览器中打开下方授权链接完成登录，浏览器跳转到 <code>http://localhost:1455/auth/callback?code=...</code> 后，复制完整地址贴回下方即可经当前账号的代理换取令牌并写回 CPR。
      </p>
      <div v-if="authorizeUrl" class="login-inline-row">
        <input
          readonly
          :value="authorizeUrl"
          class="login-input font-mono text-xs"
          aria-label="OAuth 授权链接"
          @focus="selectInput"
        >
        <ZButton size="small" @click="copyAuthorizeUrl">
          {{ copiedUrl ? '已复制' : '复制链接' }}
        </ZButton>
      </div>
      <div class="login-inline-row">
        <input
          v-model="callbackUrl"
          type="text"
          class="login-input font-mono text-xs"
          placeholder="粘贴 http://localhost:1455/auth/callback?code=...&state=..."
          aria-label="OAuth 回调链接"
        >
        <ZButton
          type="primary"
          size="small"
          :loading="completingOAuth"
          :disabled="blockedByDisabled || !callbackUrl.trim()"
          @click="handleCompleteOAuth"
        >
          完成换码并写回
        </ZButton>
      </div>
    </div>
  </section>
</template>

<style scoped>
.login-form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 12px 16px;
}
.login-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--cp-color-text-secondary);
}
.login-input {
  width: 100%;
  box-sizing: border-box;
  padding: 6px 10px;
  border: 1px solid var(--cp-color-border-secondary);
  border-radius: 8px;
  background: var(--cp-color-bg-container);
  color: var(--cp-color-text);
  font-size: 13px;
}
.login-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 10px;
  font-size: 12px;
}
.login-checkbox {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--cp-color-text-secondary);
  cursor: pointer;
}
.login-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.login-manual-box {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border-radius: 10px;
  background: var(--cp-color-fill-quaternary);
  border: 1px solid var(--cp-color-border-secondary);
}
.login-inline-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.login-banner-ok {
  background: color-mix(in srgb, var(--series-3) 14%, var(--cp-color-bg-container));
  color: var(--cp-color-text);
}
</style>
