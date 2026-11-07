<script setup lang="ts">
import type { LastPayment, Subscription, Transaction } from '../api'
import { ZButton, ZTag } from '@codex-proxy/ui'
import { ref } from 'vue'
import { api } from '../api'
import { billingStatusLabel, billingTag, dateTime, day, daysLeft, money, paymentIssue, subscriptionStatusLabel } from '../format'

const props = defineProps<{ accountId: string, payment: LastPayment | null, blocked: string | null }>()
const emit = defineEmits<{ payment: [payment: LastPayment | null] }>()

const subscription = ref<Subscription | null>(null)
const subLoading = ref(false)
const subError = ref('')
const rows = ref<Transaction[] | null>(null)
const cursor = ref('')
const billLoading = ref(false)
const billError = ref('')
const copied = ref('')

async function loadSubscription() {
  subLoading.value = true
  subError.value = ''
  try {
    subscription.value = (await api.subscription(props.accountId)).subscription
  }
  catch (e) {
    subError.value = (e as Error).message
  }
  finally {
    subLoading.value = false
  }
}

async function loadBilling(more = false) {
  billLoading.value = true
  billError.value = ''
  try {
    const r = await api.billing(props.accountId, more ? cursor.value : '')
    rows.value = more ? [...(rows.value ?? []), ...r.history.transactions] : r.history.transactions
    cursor.value = r.history.next_cursor ?? ''
    if (!more) {
      emit('payment', r.payment)
      if (r.history.subscription)
        subscription.value = r.history.subscription
    }
  }
  catch (e) {
    billError.value = (e as Error).message
  }
  finally {
    billLoading.value = false
  }
}

/** 插件页面在隔离 iframe 里，不能开新标签：给出链接让人复制 */
async function copy(url: string, id: string) {
  try {
    await navigator.clipboard.writeText(url)
    copied.value = id
  }
  catch {
    copied.value = ''
  }
}
function select(e: Event) {
  (e.target as HTMLInputElement).select()
}
</script>

<template>
  <section class="detail-card" aria-labelledby="billing-title">
    <div class="detail-card-head">
      <h3 id="billing-title" class="card-title">订阅与账单</h3>
      <ZButton size="small" :loading="subLoading" :disabled="!!blocked" @click="loadSubscription">查询订阅</ZButton>
      <ZButton type="primary" size="small" :loading="billLoading && !rows" :disabled="!!blocked" @click="loadBilling(false)">查询账单</ZButton>
      <span v-if="payment" class="muted">最近一笔 {{ billingStatusLabel[payment.status.toLowerCase()] ?? payment.status }} · {{ day(payment.created_at_ms) }}（记于 {{ dateTime(payment.checked_at_ms) }}）</span>
    </div>
    <p v-if="blocked" class="banner banner-warn">{{ blocked }}</p>
    <p v-if="subError" class="banner banner-error" role="alert">订阅：{{ subError }}</p>
    <dl v-if="subscription" class="facts">
      <div><dt>套餐</dt><dd>{{ subscription.plan_type }}</dd></div>
      <div><dt>状态</dt><dd><ZTag size="small" :type="subscription.status === 'active' ? 'success' : subscription.status === 'past_due' ? 'danger' : 'default'">{{ subscriptionStatusLabel[subscription.status] }}</ZTag></dd></div>
      <div><dt>到期</dt><dd>{{ day(subscription.expires_at_ms) }}</dd></div>
      <div><dt>自动续费</dt><dd>{{ subscription.will_renew === undefined ? '—' : subscription.will_renew ? '是' : '否' }}</dd></div>
      <div v-if="subscription.grace_ends_at_ms"><dt>宽限期</dt><dd class="text-bad">至 {{ day(subscription.grace_ends_at_ms) }}（剩 {{ daysLeft(subscription.grace_ends_at_ms) }} 天）</dd></div>
      <div v-if="subscription.delinquent_since_ms"><dt>逾期开始</dt><dd>{{ day(subscription.delinquent_since_ms) }}</dd></div>
    </dl>
    <p v-if="billError" class="banner banner-error" role="alert">账单：{{ billError }}</p>
    <template v-if="rows">
      <p v-if="!rows.length" class="muted">没有账单记录</p>
      <table v-else class="data-table">
        <thead><tr><th>日期</th><th>金额</th><th>状态</th><th>套餐</th><th>发票</th></tr></thead>
        <tbody>
          <tr v-for="t in rows" :key="t.id" :data-issue="paymentIssue(t.status) ?? undefined">
            <td>{{ day(t.created_at_ms) }}</td>
            <td class="num">{{ money(t.amount, t.currency) }}</td>
            <td><ZTag size="small" :type="billingTag(t.status)">{{ billingStatusLabel[t.status.toLowerCase()] ?? t.status }}</ZTag></td>
            <td>{{ t.plan || '—' }}</td>
            <td class="invoice">
              <template v-if="t.document_url">
                <input class="invoice-url" readonly :value="t.document_url" aria-label="Stripe 发票链接" @focus="select">
                <ZButton size="small" @click="copy(t.document_url, t.id)">{{ copied === t.id ? '已复制' : '复制' }}</ZButton>
              </template>
              <span v-else class="muted">—</span>
            </td>
          </tr>
        </tbody>
      </table>
      <p class="muted note">插件页面不能打开新标签：复制发票链接，到浏览器里打开 Stripe 页面下载发票或收据。</p>
      <div v-if="cursor"><ZButton size="small" :loading="billLoading" @click="loadBilling(true)">加载更早的账单</ZButton></div>
    </template>
  </section>
</template>
