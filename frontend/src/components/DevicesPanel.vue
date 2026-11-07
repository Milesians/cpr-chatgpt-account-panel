<script setup lang="ts">
import type { Device, Devices, SignOutTarget } from '../api'
import { ZButton, ZDialog, ZEmpty, ZTag } from '@codex-proxy/ui'
import { KeyRound, Laptop, LogOut, MapPin, RefreshCw } from '@lucide/vue'
import { computed, nextTick, ref } from 'vue'
import { api } from '../api'
import { dateTime } from '../format'
import { loginApi } from '../login/client'

const props = defineProps<{ accountId: string, blocked: string | null }>()

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

const signOutOthersTitle = computed(() => {
  if (working.value)
    return '正在执行登出操作，请稍候'
  if (!devices.value?.in_use_known)
    return '认不出 CPR 正在使用哪个登录，只能逐台登出'
  if (!others.value.length)
    return '当前没有其他在线设备可登出'
  return '直接登出除 CPR 正在使用外的其他在线设备'
})

const signOutAllTitle = computed(() => {
  if (working.value)
    return '正在执行登出操作，请稍候'
  if (!active.value.length)
    return '当前没有在线设备可登出'
  return '登出全部在线设备（含 CPR 正在使用的登录）'
})

// 单台登出与登出其他设备直接执行；只有「登出所有设备」保留一次应用内确认
const confirmOpen = ref(false)
const working = ref(false)
const cprVoided = ref(false)
const relogging = ref(false)

async function executeSignOut(target: SignOutTarget) {
  working.value = true
  error.value = ''
  notice.value = ''
  cprVoided.value = false
  try {
    const r = await api.signOut(props.accountId, target)
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

function signOutDevice(d: Device) {
  executeSignOut({ device: d.id })
}

function signOutOthers() {
  executeSignOut({ others: true })
}

function askAll() {
  confirmOpen.value = true
}

function confirmAll() {
  executeSignOut({ all: true })
}

function scrollToLogin() {
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
    notice.value = '已使用已保存密码重新登录并写回 CPR'
    await load()
  }
  catch {
    scrollToLogin()
  }
  finally {
    relogging.value = false
  }
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
        <ZButton
          :type="devices ? undefined : 'primary'"
          size="small"
          :loading="loading"
          :disabled="!!blocked"
          :title="blocked ?? (devices ? '重新读取登录设备列表' : '读取登录设备列表')"
          @click="load"
        >
          <RefreshCw v-if="devices" :size="13" aria-hidden="true" />
          <Laptop v-else :size="13" aria-hidden="true" />
          {{ devices ? '刷新' : '查询设备' }}
        </ZButton>
        <template v-if="devices">
          <ZButton
            size="small"
            :disabled="!devices.in_use_known || !others.length || working"
            :title="signOutOthersTitle"
            @click="signOutOthers"
          >
            登出其他设备
          </ZButton>
          <ZButton
            size="small"
            type="danger"
            :disabled="!active.length || working"
            :title="signOutAllTitle"
            @click="askAll"
          >
            <LogOut :size="13" aria-hidden="true" /> 登出所有设备
          </ZButton>
        </template>
      </div>
    </div>

    <p class="muted note">
      ChatGPT 设置页里这个登录的设备，点了才查。CPR 正在使用的登录不能单独登出；「登出所有设备」会连它一起登出，之后需要重新登录。
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
          <KeyRound :size="13" aria-hidden="true" /> 免输重登
        </ZButton>
        <ZButton size="small" @click="scrollToLogin">
          去重新登录
        </ZButton>
      </div>
    </div>

    <template v-if="devices">
      <ZEmpty v-if="!devices.devices.length" size="small" title="没有设备" />
      <div v-else class="card-list" role="list" aria-label="登录设备列表">
        <div class="card-head device-head" aria-hidden="true">
          <span>设备与平台</span>
          <span>客户端</span>
          <span>最近登录</span>
          <span>地点</span>
          <span class="text-right">操作</span>
        </div>

        <div
          v-for="d in devices.devices"
          :key="d.id"
          role="listitem"
          class="card-row device-row"
          :data-in-use="d.in_use"
        >
          <div class="min-w-0 cell-item" data-label="设备与平台">
            <div class="device-name">
              <span>{{ d.name || '未命名设备' }}</span>
              <ZTag v-if="d.in_use" size="small" type="info">CPR 在用</ZTag>
              <ZTag v-else-if="d.current" size="small">当前</ZTag>
              <ZTag v-if="d.signed_out" size="small">已登出</ZTag>
            </div>
            <div class="muted text-xs mt-0.5">{{ d.platform || '未知平台' }}</div>
          </div>

          <div class="cell-item" data-label="客户端">
            <span class="font-medium text-cp-text">{{ d.apps.join('、') || '—' }}</span>
          </div>

          <div class="cell-item" data-label="最近登录">
            <span class="font-medium tabular-nums text-cp-text">{{ dateTime(d.last_signed_in_ms) }}</span>
          </div>

          <div class="cell-item" data-label="地点">
            <span class="inline-flex items-center gap-1 text-cp-text">
              <MapPin v-if="where(d)" :size="12" class="muted" aria-hidden="true" />
              <span>{{ where(d) || '—' }}</span>
            </span>
          </div>

          <div class="flex justify-end">
            <ZButton
              v-if="!d.signed_out && !d.in_use"
              size="small"
              :disabled="working"
              :title="working ? '正在执行登出操作，请稍候' : '直接登出这台设备'"
              @click="signOutDevice(d)"
            >
              登出
            </ZButton>
          </div>
        </div>
      </div>
    </template>

    <ZDialog
      v-model="confirmOpen"
      title="登出所有设备"
      width="28rem"
      :close-on-click-modal="!working"
      :close-on-press-escape="!working"
      :before-close="() => !working"
    >
      <p class="dialog-text">
        会同时登出 CPR 正在使用的登录，CPR 里这个账号的令牌会立即失效，需要重新登录。
      </p>
      <template #footer>
        <ZButton :disabled="working" @click="confirmOpen = false">取消</ZButton>
        <ZButton type="danger" :loading="working" @click="confirmAll">登出</ZButton>
      </template>
    </ZDialog>
  </section>
</template>
