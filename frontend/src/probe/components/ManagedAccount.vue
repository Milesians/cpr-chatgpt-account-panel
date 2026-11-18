<script setup lang="ts">
import type { Account } from '../../api'
import type { Models, OverviewAccount } from '../api'
import { ZButton, ZSegmented } from '@codex-proxy/ui'
import { ArrowLeft, Check, ChevronLeft, ChevronRight, Copy } from '@lucide/vue'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { copyText } from '../../clipboard'
import BillingPanel from '../../components/BillingPanel.vue'
import DevicesPanel from '../../components/DevicesPanel.vue'
import UsagePanel from '../../components/UsagePanel.vue'
import { formatPlanName } from '../../format'
import AccountLogin from '../../login/AccountLogin.vue'
import AccountDetail from './AccountDetail.vue'

export type DetailTab = 'probe' | 'billing' | 'usage' | 'access'
const props = defineProps<{
  id: string
  account?: Account
  probe?: OverviewAccount
  models: Models | null
  initialTab: DetailTab
  run?: string
  accountIds?: string[]
}>()
const emit = defineEmits<{
  back: []
  patch: [patch: Partial<Account>]
  loginUpdated: []
  select: [id: string]
  'update:tab': [tab: DetailTab]
}>()
const tab = ref(props.initialTab)
const visited = ref(new Set<DetailTab>([props.initialTab]))
const loginRefreshSeq = ref(0)
const options = computed(() => [
  { label: '探测记录', value: 'probe' },
  ...props.account
    ? [
        { label: '订阅与账单', value: 'billing' },
        { label: '官方用量', value: 'usage' },
        { label: '登录与设备', value: 'access' },
      ]
    : [],
])
const summary = computed(() => props.account ?? props.probe)
const displayIdentity = computed(() => summary.value?.email || summary.value?.name || props.id)

const navIds = computed(() => props.accountIds ?? [])
const currentIndex = computed(() => navIds.value.indexOf(props.id))
const prevId = computed(() => currentIndex.value > 0 ? navIds.value[currentIndex.value - 1] ?? null : null)
const nextId = computed(() => currentIndex.value >= 0 && currentIndex.value < navIds.value.length - 1 ? navIds.value[currentIndex.value + 1] ?? null : null)

const copiedIdentity = ref(false)
let copyTimer: ReturnType<typeof setTimeout> | null = null
async function copyIdentity() {
  const ok = await copyText(displayIdentity.value)
  if (!ok)
    return
  copiedIdentity.value = true
  if (copyTimer)
    clearTimeout(copyTimer)
  copyTimer = setTimeout(() => {
    copiedIdentity.value = false
  }, 2000)
}

function select(next: DetailTab) {
  tab.value = next
  visited.value.add(next)
  emit('update:tab', next)
}

function onDevicesLoginUpdated() {
  loginRefreshSeq.value++
  emit('loginUpdated')
}

function onKeydown(e: KeyboardEvent) {
  if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey)
    return
  const target = e.target as HTMLElement | null
  if (target && (['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) || target.isContentEditable))
    return
  if (document.querySelector('[role="dialog"]'))
    return
  if (e.key === 'ArrowLeft' && prevId.value) {
    e.preventDefault()
    emit('select', prevId.value)
  }
  else if (e.key === 'ArrowRight' && nextId.value) {
    e.preventDefault()
    emit('select', nextId.value)
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  if (copyTimer)
    clearTimeout(copyTimer)
})
</script>

<template>
  <div class="probe-detail-page">
    <header class="detail-header detail-header-bar">
      <ZButton variant="text" aria-label="返回列表" @click="emit('back')">
        <ArrowLeft :size="16" aria-hidden="true" /> 返回列表
      </ZButton>
      <div class="min-w-0 flex-1">
        <div class="flex min-w-0 items-center gap-1.5">
          <h1 class="m-0 truncate text-lg font-semibold text-cp-text" :title="displayIdentity">{{ displayIdentity }}</h1>
          <button
            type="button"
            class="icon-ghost-btn"
            :aria-label="copiedIdentity ? '已复制账号邮箱' : '复制账号邮箱'"
            :title="copiedIdentity ? '已复制' : '复制邮箱'"
            @click="copyIdentity"
          >
            <Check v-if="copiedIdentity" :size="14" class="text-cp-success" aria-hidden="true" />
            <Copy v-else :size="14" aria-hidden="true" />
          </button>
        </div>
        <p class="m-0 mt-0.5 flex flex-wrap items-center gap-2 text-cp-xs text-cp-text-tertiary">
          <span v-if="summary?.plan_type" class="account-plan">{{ formatPlanName(summary.plan_type) }}</span>
          <span v-if="summary && !summary.enabled" class="text-cp-warning-text">已停用</span>
          <span v-if="summary?.email && summary.name !== summary.email">{{ summary.name }}</span>
        </p>
      </div>
      <div class="detail-header-controls">
        <div v-if="navIds.length > 1 && currentIndex >= 0" class="account-pager" role="group" aria-label="切换账号">
          <button
            type="button"
            class="account-pager-btn"
            :disabled="!prevId"
            aria-label="上一个账号"
            title="上一个账号 (Alt+←)"
            @click="prevId && emit('select', prevId)"
          >
            <ChevronLeft :size="15" aria-hidden="true" />
          </button>
          <span class="account-pager-pos">{{ currentIndex + 1 }} / {{ navIds.length }}</span>
          <button
            type="button"
            class="account-pager-btn"
            :disabled="!nextId"
            aria-label="下一个账号"
            title="下一个账号 (Alt+→)"
            @click="nextId && emit('select', nextId)"
          >
            <ChevronRight :size="15" aria-hidden="true" />
          </button>
        </div>
        <ZSegmented :model-value="tab" :options="options" aria-label="账号详情分类" @update:model-value="select($event as DetailTab)" />
      </div>
    </header>

    <AccountDetail v-if="visited.has('probe')" v-show="tab === 'probe'" :account-id="id" :models="models" :initial-run="run" />
    <template v-if="account">
      <BillingPanel v-if="visited.has('billing')" v-show="tab === 'billing'" :account-id="id" :payment="account.payment" :blocked="account.blocker?.message ?? null" :active="tab === 'billing'" @payment="payment => emit('patch', { payment })" />
      <UsagePanel v-if="visited.has('usage')" v-show="tab === 'usage'" :account-id="id" :summary="account.usage" :blocked="account.blocker?.message ?? null" :active="tab === 'usage'" @checked="usage => emit('patch', { usage })" />
      <div v-if="visited.has('access')" v-show="tab === 'access'" class="probe-detail-page">
        <AccountLogin :account-id="id" :refresh-seq="loginRefreshSeq" @updated="emit('loginUpdated')" />
        <DevicesPanel
          :account-id="id"
          :account-name="account.name"
          :account-email="account.email"
          :plan-type="account.plan_type"
          :blocked="account.blocker?.message ?? null"
          :active="tab === 'access'"
          @login-updated="onDevicesLoginUpdated"
        />
      </div>
    </template>
  </div>
</template>
