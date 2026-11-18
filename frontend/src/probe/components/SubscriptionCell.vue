<script setup lang="ts">
import type { Account } from '../../api'
import { computed } from 'vue'
import {
  billingStatusLabel,
  dateTime,
  day,
  graceTooltip,
  hasGracePeriod,
  paymentIssue,
  subscriptionBillingDateText,
  subscriptionDetailText,
  subscriptionStateOf,
  subscriptionStatusLabel,
} from '../../format'

const { account, reading, error } = defineProps<{ account?: Account, reading?: boolean, error?: string }>()
const emit = defineEmits<{ open: [] }>()
const payment = computed(() => account?.payment ?? null)
const status = computed(() => payment.value ? subscriptionStateOf(payment.value) : undefined)
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
const billingDate = computed(() => subscriptionBillingDateText(payment.value))
const detail = computed(() => subscriptionDetailText(payment.value))
const graceFirst = computed(() => Boolean(payment.value?.grace_ends_at_ms))
const title = computed(() => {
  if (error)
    return `本次查询失败：${error}`
  const p = payment.value
  if (!p)
    return account?.blocker?.message || '查看订阅与账单'
  const billStatus = p.created_at_ms ? (billingStatusLabel[p.status.toLowerCase()] ?? p.status) : ''
  const billNote = p.created_at_ms
    ? `最近账单 ${day(p.created_at_ms)}（${billStatus}）`
    : (status.value && status.value !== 'free' ? '未查账单日（点此查看或勾选后点「查账单」）' : '')
  const expiryNote = p.subscription_expires_at_ms
    ? `${p.subscription_will_renew && status.value === 'active' ? '预计续费' : '当前周期到期'} ${day(p.subscription_expires_at_ms)}`
    : ''
  const checkedAt = p.subscription_checked_at_ms ?? ((status.value || p.grace_ends_at_ms) ? p.checked_at_ms : undefined)
  return [
    graceTooltip(p),
    billNote,
    expiryNote,
    checkedAt ? `订阅查询于 ${dateTime(checkedAt)}` : '',
    '查看订阅与账单',
  ].filter(Boolean).join('；')
})
</script>

<template>
  <button v-if="account" class="subscription-cell" type="button" :data-tone="tone" :title="title" @click="emit('open')">
    <span class="subscription-status"><i aria-hidden="true" />{{ label }}</span>
    <template v-if="graceFirst">
      <span v-if="detail" class="subscription-meta">{{ detail }}</span>
      <span v-if="billingDate" class="subscription-meta subscription-billing-date">{{ billingDate }}</span>
    </template>
    <template v-else>
      <span v-if="billingDate" class="subscription-meta subscription-billing-date">{{ billingDate }}</span>
      <span v-if="detail" class="subscription-meta">{{ detail }}</span>
    </template>
    <span v-if="status && status !== 'past_due' && !payment?.grace_ends_at_ms && issue" class="subscription-error">{{ issue === 'failed' ? '最近扣款失败' : '账单待支付' }}</span>
    <span v-if="error" class="subscription-error">查询失败{{ payment ? ' · 保留上次结果' : '' }}</span>
  </button>
  <span v-else class="subscription-cell text-cp-text-tertiary">{{ reading ? '载入中…' : '订阅未载入' }}</span>
</template>
