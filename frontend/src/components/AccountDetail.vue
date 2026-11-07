<script setup lang="ts">
import type { Account } from '../api'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { ArrowLeft } from '@lucide/vue'
import { computed } from 'vue'
import { dateTime, kindLabel, title } from '../format'
import AccountLogin from '../login/AccountLogin.vue'
import BillingPanel from './BillingPanel.vue'
import DevicesPanel from './DevicesPanel.vue'
import UsagePanel from './UsagePanel.vue'

const props = defineProps<{ account: Account }>()
const emit = defineEmits<{ back: [], patch: [id: string, patch: Partial<Account>] }>()
const blocked = computed(() => props.account.blocker?.message ?? null)
</script>

<template>
  <div class="detail-page">
    <header class="detail-header">
      <ZButton size="small" aria-label="返回列表" @click="emit('back')"><ArrowLeft :size="14" aria-hidden="true" /> 返回</ZButton>
      <h2 class="detail-title">{{ title(account) }}</h2>
      <ZTag v-if="account.plan_type" size="small">{{ account.plan_type }}</ZTag>
      <ZTag size="small" :type="account.kind === 'team' ? 'info' : 'default'">{{ kindLabel[account.kind] }}</ZTag>
      <ZTag v-if="!account.enabled" size="small" type="warning">CPR 已停用</ZTag>
      <span class="muted">{{ account.email && account.name !== account.email ? `${account.name} · ` : '' }}凭据 {{ account.credential_state }}<template v-if="account.access_token_expires_at_ms"> · 令牌 {{ dateTime(account.access_token_expires_at_ms) }} 到期</template></span>
    </header>
    <UsagePanel :account-id="account.id" :summary="account.usage" :blocked="blocked" @checked="s => emit('patch', account.id, { usage: s })" />
    <BillingPanel :account-id="account.id" :payment="account.payment" :blocked="blocked" @payment="p => emit('patch', account.id, { payment: p })" />
    <DevicesPanel :account-id="account.id" :blocked="blocked" />
    <AccountLogin :account-id="account.id" />
  </div>
</template>
