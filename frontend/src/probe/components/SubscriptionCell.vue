<script setup lang="ts">
import type { Account, LastPayment } from '../../api'
import { computed } from 'vue'
import { dateTime, day, graceText, graceTooltip, hasGracePeriod, paymentIssue, subscriptionStatusLabel } from '../../format'

const { account, reading, error } = defineProps<{ account?: Account, reading?: boolean, error?: string }>()
const emit = defineEmits<{ open: [] }>()
const payment = computed(() => account?.payment ?? null)
function subscriptionState(p: LastPayment) {
  return p.subscription_status ?? (p.created_at_ms ? undefined : p.status)
}
const status = computed(() => payment.value ? subscriptionState(payment.value) : undefined)
const issue = computed(() => payment.value?.created_at_ms ? paymentIssue(payment.value.status) : null)
const tone = computed(() => hasGracePeriod(payment.value) || ['past_due', 'expired'].includes(status.value ?? '') || issue.value === 'failed'
  ? 'bad'
  : status.value === 'inactive' || issue.value === 'due' ? 'warn' : status.value === 'active' ? 'ok' : 'neutral')
const label = computed(() => {
  if (status.value)
    return subscriptionStatusLabel[status.value] ?? '未知'
  if (hasGracePeriod(payment.value))
    return '付款逾期'
  return issue.value === 'failed' ? '扣款异常' : issue.value === 'due' ? '账单待支付' : reading ? '查询中…' : '未查订阅'
})
const detail = computed(() => {
  const p = payment.value
  if (p?.grace_ends_at_ms)
    return graceText(p.grace_ends_at_ms)
  if (p?.subscription_expires_at_ms) {
    const verb = p.subscription_will_renew && status.value === 'active' ? '续费' : '到期'
    return `${day(p.subscription_expires_at_ms)} ${verb}`
  }
  if (status.value === 'active' && p?.subscription_will_renew !== undefined)
    return p.subscription_will_renew ? '自动续费' : '不自动续费'
  return ''
})
const title = computed(() => {
  if (error)
    return `本次查询失败：${error}`
  const p = payment.value
  if (!p)
    return account?.blocker?.message || '查看订阅与账单'
  const checkedAt = p.subscription_checked_at_ms ?? ((status.value || p.grace_ends_at_ms) ? p.checked_at_ms : undefined)
  return [graceTooltip(p), checkedAt ? `订阅查询于 ${dateTime(checkedAt)}` : '', '查看订阅与账单'].filter(Boolean).join('；')
})
</script>

<template>
  <button v-if="account" class="subscription-cell" type="button" :data-tone="tone" :title="title" @click="emit('open')">
    <span class="subscription-status"><i aria-hidden="true" />{{ label }}</span>
    <span v-if="detail" class="subscription-meta">{{ detail }}</span>
    <span v-if="status && status !== 'past_due' && !payment?.grace_ends_at_ms && issue" class="subscription-error">{{ issue === 'failed' ? '最近扣款失败' : '账单待支付' }}</span>
    <span v-if="error" class="subscription-error">查询失败{{ payment ? ' · 保留上次结果' : '' }}</span>
  </button>
  <span v-else class="subscription-cell text-cp-text-tertiary">{{ reading ? '载入中…' : '订阅未载入' }}</span>
</template>
