<script setup lang="ts">
import type { Account } from '../../api'
import type { Models, OverviewAccount } from '../api'
import { ZButton, ZSegmented } from '@codex-proxy/ui'
import { ArrowLeft } from '@lucide/vue'
import { computed, ref } from 'vue'
import BillingPanel from '../../components/BillingPanel.vue'
import DevicesPanel from '../../components/DevicesPanel.vue'
import UsagePanel from '../../components/UsagePanel.vue'
import { formatPlanName } from '../../format'
import AccountLogin from '../../login/AccountLogin.vue'
import AccountDetail from './AccountDetail.vue'

export type DetailTab = 'probe' | 'billing' | 'usage' | 'access'
const props = defineProps<{ id: string, account?: Account, probe?: OverviewAccount, models: Models | null, initialTab: DetailTab, run?: string }>()
const emit = defineEmits<{ back: [], patch: [patch: Partial<Account>], loginUpdated: [] }>()
const tab = ref(props.initialTab)
const visited = ref(new Set<DetailTab>([props.initialTab]))
const options = computed(() => [
  { label: '探测记录', value: 'probe' },
  ...props.account ? [
    { label: '订阅与账单', value: 'billing' },
    { label: '官方用量', value: 'usage' },
    { label: '登录与设备', value: 'access' },
  ] : [],
])
const summary = computed(() => props.account ?? props.probe)
function select(next: DetailTab) {
  tab.value = next
  visited.value.add(next)
}
</script>

<template>
  <div class="probe-detail-page">
    <header class="detail-header">
      <ZButton variant="text" aria-label="返回列表" @click="emit('back')"><ArrowLeft :size="16" aria-hidden="true" /> 返回列表</ZButton>
      <div class="min-w-0 flex-1">
        <h1 class="m-0 break-all text-lg font-semibold">{{ summary?.email || summary?.name || id }}</h1>
        <p class="m-0 mt-1 flex flex-wrap items-center gap-2 text-cp-xs text-cp-text-tertiary">
          <span v-if="summary?.plan_type" class="account-plan">{{ formatPlanName(summary.plan_type) }}</span>
          <span v-if="summary && !summary.enabled" class="text-cp-warning-text">已停用</span>
          <span v-if="summary?.email && summary.name !== summary.email">{{ summary.name }}</span>
        </p>
      </div>
      <ZSegmented :model-value="tab" :options="options" aria-label="账号详情分类" @update:model-value="select($event as DetailTab)" />
    </header>

    <AccountDetail v-if="visited.has('probe')" v-show="tab === 'probe'" :account-id="id" :models="models" :initial-run="run" />
    <template v-if="account">
      <BillingPanel v-if="visited.has('billing')" v-show="tab === 'billing'" :account-id="id" :payment="account.payment" :blocked="account.blocker?.message ?? null" @payment="payment => emit('patch', { payment })" />
      <UsagePanel v-if="visited.has('usage')" v-show="tab === 'usage'" :account-id="id" :summary="account.usage" :blocked="account.blocker?.message ?? null" @checked="usage => emit('patch', { usage })" />
      <div v-if="visited.has('access')" v-show="tab === 'access'" class="probe-detail-page">
        <AccountLogin :account-id="id" @updated="emit('loginUpdated')" />
        <DevicesPanel :account-id="id" :blocked="account.blocker?.message ?? null" />
      </div>
    </template>
  </div>
</template>
