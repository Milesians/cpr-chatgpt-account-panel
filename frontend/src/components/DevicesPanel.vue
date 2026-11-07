<script setup lang="ts">
import type { Device, Devices, SignOutTarget } from '../api'
import { ZButton, ZDialog, ZEmpty, ZTag } from '@codex-proxy/ui'
import { KeyRound, Laptop, LogOut, MapPin, RefreshCw } from '@lucide/vue'
import { computed, nextTick, ref } from 'vue'
import { api } from '../api'
import { dateTime } from '../format'
import { loginApi } from '../login/client'

const props = defineProps<{ accountId: string, blocked: string | null }>()
const emit = defineEmits<{ focusLogin: [] }>()

const devices = ref<Devices | null>(null)
const loading = ref(false)
const error = ref('')
const notice = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    devices.value = (await api.devices(props.accountId)).devices
  }
  catch (e) {
    error.value = (e as Error).message
  }
  finally {
    loading.value = false
  }
}

const active = computed(() => devices.value?.devices.filter(d => !d.signed_out) ?? [])
const others = computed(() => active.value.filter(d => !d.in_use))

function where(d: Device): string {
  return [d.city, d.region, d.country].filter(Boolean).join(', ')
}

// 登出前在应用内确认一次
type Pending = { target: SignOutTarget, title: string, message: string }
const pending = ref<Pending | null>(null)
const confirmOpen = ref(false)
const working = ref(false)

function ask(target: SignOutTarget, title: string, message: string) {
  pending.value = { target, title, message }
  confirmOpen.value = true
}
function askDevice(d: Device) {
  ask({ device: d.id }, '登出这台设备', `登出「${d.name || d.platform || '未命名设备'}」。这台设备上的 ChatGPT / Codex 需要重新登录。`)
}
function askOthers() {
  ask({ others: true }, '登出其他设备', `登出 CPR 正在使用的登录以外的 ${others.value.length} 台设备。每登出一台前都会重新核对列表，出现新设备就停下。`)
}
function askAll() {
  ask({ all: true }, '登出所有设备', '连 CPR 正在使用的登录一起登出（放在最后）。之后 CPR 里这个账号的令牌会失效，需要重新登录写回 CPR。期间新登录的设备也会登出。')
}

const cprVoided = ref(false)
const relogging = ref(false)

function scrollToLogin() {
  emit('focusLogin')
  nextTick(() => {
    const el = document.querySelector<HTMLElement>('#account-login-section')
    el?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    const input = el?.querySelector<HTMLInputElement>('input')
    input?.focus()
  })
}

async function quickRelogin() {
  relogging.value = true
  error.value = ''
  try {
    await loginApi.runLogin({ accountId: props.accountId })
    cprVoided.value = false
    notice.value = '已使用保存的加密凭据重新登录并写回 CPR'
    await load()
  }
  catch {
    scrollToLogin()
  }
  finally {
    relogging.value = false
  }
}

async function confirm() {
  if (!pending.value)
    return
  working.value = true
  error.value = ''
  notice.value = ''
  cprVoided.value = false
  try {
    const r = await api.signOut(props.accountId, pending.value.target)
    cprVoided.value = Boolean(r.cpr_voided)
    notice.value = r.cpr_voided
      ? `已登出 ${r.signed_out} 台设备，CPR 的令牌已失效，需要重新登录写回 CPR`
      : `已登出 ${r.signed_out} 台设备`
  }
  catch (e) {
    error.value = (e as Error).message
  }
  finally {
    working.value = false
    confirmOpen.value = false
  }
  if (!cprVoided.value)
    await load()
  else
    devices.value = null
}
</script>

<template>
  <section class="detail-card" aria-labelledby="devices-title">
    <div class="detail-card-head">
      <div class="card-title-group">
        <h3 id="devices-title" class="card-title">
          登录设备
        </h3>
        <span v-if="devices" class="muted">
          共 {{ devices.devices.length }} 台（在线 {{ active.length }} 台）· 查询于 {{ dateTime(devices.checked_at_ms) }}
        </span>
      </div>
      <div class="card-actions">
        <ZButton type="primary" size="small" :loading="loading" :disabled="!!blocked" @click="load">
          <RefreshCw v-if="devices" :size="13" aria-hidden="true" />
          <Laptop v-else :size="13" aria-hidden="true" />
          {{ devices ? '刷新' : '查看设备' }}
        </ZButton>
        <template v-if="devices">
          <ZButton
            size="small"
            :disabled="!devices.in_use_known || !others.length || working"
            :title="devices.in_use_known ? undefined : '认不出 CPR 正在使用哪个登录，只能逐台登出'"
            @click="askOthers"
          >
            登出其他设备
          </ZButton>
          <ZButton
            size="small"
            type="danger"
            :disabled="!active.length || working"
            @click="askAll"
          >
            <LogOut :size="13" aria-hidden="true" /> 登出所有设备
          </ZButton>
        </template>
      </div>
    </div>

    <p class="muted note">
      ChatGPT 设置页里这个登录的设备，点了才查。CPR 正在使用的登录不能单独登出；“登出所有设备”会连它一起登出，之后要重新登录。
    </p>

    <p v-if="blocked" class="banner banner-warn">
      {{ blocked }}
    </p>
    <p v-if="error" class="banner banner-error" role="alert">
      {{ error }}
    </p>

    <div
      v-if="notice"
      class="banner banner-ok flex flex-wrap items-center justify-between gap-2"
      role="status"
    >
      <span>{{ notice }}</span>
      <div
        v-if="cprVoided || notice.includes('重新登录')"
        class="inline-flex flex-wrap items-center gap-2"
      >
        <ZButton type="primary" size="small" :loading="relogging" @click="quickRelogin">
          <KeyRound :size="13" aria-hidden="true" /> 一键重新登录
        </ZButton>
        <ZButton size="small" @click="scrollToLogin">
          前往快速登录区
        </ZButton>
      </div>
    </div>

    <template v-if="devices">
      <ZEmpty v-if="!devices.devices.length" size="small" title="没有设备" />
      <div v-else class="card-list" role="list" aria-label="登录设备列表">
        <div
          v-for="d in devices.devices"
          :key="d.id"
          role="listitem"
          class="card-row device-row"
          :data-in-use="d.in_use"
        >
          <div class="min-w-0">
            <div class="device-name">
              <span>{{ d.name || '未命名设备' }}</span>
              <ZTag v-if="d.in_use" size="small" type="info">CPR 在用</ZTag>
              <ZTag v-else-if="d.current" size="small">当前</ZTag>
              <ZTag v-if="d.trusted" size="small" type="success">受信任</ZTag>
              <ZTag v-if="d.signed_out" size="small">已登出</ZTag>
            </div>
            <div class="muted text-xs mt-0.5">{{ d.platform || '未知平台' }}</div>
          </div>

          <div>
            <div class="font-medium text-cp-text">{{ d.apps.join('、') || '—' }}</div>
            <div class="muted text-xs">客户端</div>
          </div>

          <div>
            <div class="font-medium tabular-nums text-cp-text">{{ dateTime(d.last_signed_in_ms) }}</div>
            <div class="muted text-xs">最近登录</div>
          </div>

          <div>
            <div class="flex items-center gap-1 text-cp-text">
              <MapPin v-if="where(d)" :size="12" class="muted" aria-hidden="true" />
              <span>{{ where(d) || '—' }}</span>
            </div>
            <div class="muted text-xs">地点</div>
          </div>

          <div class="flex justify-end">
            <ZButton
              v-if="!d.signed_out && !d.in_use"
              size="small"
              :disabled="working"
              @click="askDevice(d)"
            >
              登出
            </ZButton>
            <span v-else-if="d.in_use" class="muted text-xs">CPR 占用中</span>
          </div>
        </div>
      </div>
    </template>

    <ZDialog
      v-model="confirmOpen"
      :title="pending?.title ?? '登出'"
      width="28rem"
      :close-on-click-modal="!working"
      :close-on-press-escape="!working"
      :before-close="() => !working"
    >
      <p class="dialog-text">{{ pending?.message }}</p>
      <template #footer>
        <ZButton :disabled="working" @click="confirmOpen = false">取消</ZButton>
        <ZButton type="danger" :loading="working" @click="confirm">登出</ZButton>
      </template>
    </ZDialog>
  </section>
</template>
