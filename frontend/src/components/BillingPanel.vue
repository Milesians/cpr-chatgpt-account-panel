<script setup lang="ts">
import type { LastPayment, Subscription, Transaction } from '../api'
import { ZButton, ZEmpty, ZTag } from '@codex-proxy/ui'
import { Check, Copy, CreditCard, ExternalLink, RefreshCw } from '@lucide/vue'
import { computed, ref } from 'vue'
import { api, canOpenExternal, openExternal } from '../api'
import { copyText } from '../clipboard'
import {
  ago,
  billingStatusLabel,
  billingTag,
  day,
  daysLeft,
  formatPlanName,
  money,
  paymentIssue,
  subscriptionStatusLabel,
} from '../format'

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
const copyFailed = ref('')
const hostOpens = canOpenExternal()
let copyTimer: ReturnType<typeof setTimeout> | null = null

const hasData = computed(() => subscription.value !== null || rows.value !== null)
const loadingInitial = computed(() => (subLoading.value || billLoading.value) && !hasData.value)
const loadingRefresh = computed(() => (subLoading.value || billLoading.value) && hasData.value)

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
      if (r.history.subscription && !subscription.value)
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

async function loadAll() {
  await Promise.allSettled([
    loadSubscription(),
    loadBilling(false),
  ])
}

/** 新版 CPR 能经宿主弹窗打开外链；老版本只能复制 */
function openInvoice(url: string) {
  openExternal(url).catch(() => {})
}

async function copy(url: string, id: string) {
  const ok = await copyText(url)
  copied.value = ok ? id : ''
  copyFailed.value = ok ? '' : url
  if (!ok)
    return
  if (copyTimer)
    clearTimeout(copyTimer)
  copyTimer = setTimeout(() => {
    if (copied.value === id)
      copied.value = ''
  }, 2000)
}
</script>

<template>
  <section class="detail-card" aria-labelledby="billing-title">
    <div class="detail-card-head">
      <div class="card-title-group">
        <h3 id="billing-title" class="card-title">
          订阅与账单
        </h3>
        <span v-if="payment" class="muted">
          最近一笔 {{ billingStatusLabel[payment.status.toLowerCase()] ?? payment.status }} · {{ day(payment.created_at_ms) }} · {{ ago(payment.checked_at_ms) }}查
        </span>
      </div>
      <div class="card-actions">
        <ZButton
          type="primary"
          size="small"
          :loading="loadingInitial || loadingRefresh"
          :disabled="!!blocked"
          :title="blocked ?? (hasData ? '重新读取订阅与账单' : '并行读取订阅状态与账单记录')"
          @click="loadAll"
        >
          <RefreshCw v-if="hasData" :size="13" aria-hidden="true" />
          <CreditCard v-else :size="13" aria-hidden="true" />
          {{ hasData ? '刷新' : '查询订阅与账单' }}
        </ZButton>
      </div>
    </div>

    <p v-if="blocked" class="banner banner-warn">
      {{ blocked }}
    </p>
    <p v-if="subError" class="banner banner-error" role="alert">
      订阅：{{ subError }}
    </p>

    <dl v-if="subscription" class="facts-grid">
      <div class="fact-box">
        <dt>套餐</dt>
        <dd>{{ formatPlanName(subscription.plan_type) || subscription.plan_type }}</dd>
      </div>
      <div class="fact-box">
        <dt>订阅状态</dt>
        <dd>
          <ZTag size="small" :type="subscription.status === 'active' ? 'success' : subscription.status === 'past_due' ? 'danger' : 'default'">
            {{ subscriptionStatusLabel[subscription.status] }}
          </ZTag>
        </dd>
      </div>
      <div class="fact-box">
        <dt>当前周期到期</dt>
        <dd>{{ day(subscription.expires_at_ms) }}</dd>
      </div>
      <div class="fact-box">
        <dt>自动续费</dt>
        <dd>{{ subscription.will_renew === undefined ? '—' : subscription.will_renew ? '是' : '否' }}</dd>
      </div>
      <div v-if="subscription.grace_ends_at_ms" class="fact-box">
        <dt>宽限期</dt>
        <dd class="text-bad">至 {{ day(subscription.grace_ends_at_ms) }}（剩 {{ daysLeft(subscription.grace_ends_at_ms) }} 天）</dd>
      </div>
      <div v-if="subscription.delinquent_since_ms" class="fact-box">
        <dt>逾期开始</dt>
        <dd>{{ day(subscription.delinquent_since_ms) }}</dd>
      </div>
    </dl>

    <p v-if="billError" class="banner banner-error" role="alert">
      账单：{{ billError }}
    </p>

    <template v-if="rows">
      <ZEmpty v-if="!rows.length" size="small" title="没有账单记录" />
      <div v-else class="card-list" role="list" aria-label="账单记录列表">
        <div class="card-head billing-head" aria-hidden="true">
          <span>账单日期</span>
          <span>金额</span>
          <span>状态</span>
          <span>套餐描述</span>
          <span class="text-right">发票</span>
        </div>

        <div
          v-for="t in rows"
          :key="t.id"
          role="listitem"
          class="card-row billing-row"
          :data-issue="paymentIssue(t.status) ?? undefined"
        >
          <div class="cell-item" data-label="账单日期">
            <span class="font-semibold text-cp-text">{{ day(t.created_at_ms) }}</span>
          </div>

          <div class="cell-item" data-label="金额">
            <span class="font-semibold tabular-nums text-cp-text">{{ money(t.amount, t.currency) }}</span>
          </div>

          <div class="cell-item" data-label="状态">
            <ZTag size="small" :type="billingTag(t.status)">
              {{ billingStatusLabel[t.status.toLowerCase()] ?? t.status }}
            </ZTag>
          </div>

          <div class="cell-item" data-label="套餐描述">
            <span class="font-medium text-cp-text">{{ t.plan || '—' }}</span>
          </div>

          <div class="invoice-box">
            <ZButton
              v-if="t.document_url && hostOpens"
              size="small"
              :title="t.document_url"
              @click="openInvoice(t.document_url)"
            >
              <ExternalLink :size="13" aria-hidden="true" />
              打开发票
            </ZButton>
            <ZButton
              v-if="t.document_url"
              size="small"
              :title="t.document_url"
              @click="copy(t.document_url, t.id)"
            >
              <Check v-if="copied === t.id" :size="13" aria-hidden="true" />
              <Copy v-else :size="13" aria-hidden="true" />
              {{ copied === t.id ? '已复制' : '复制发票链接' }}
            </ZButton>
            <span v-else class="muted text-xs">无发票链接</span>
          </div>
        </div>
      </div>

      <p v-if="copyFailed" class="muted text-xs" role="status">
        浏览器不允许插件页面写剪贴板，请手动复制：
        <input readonly :value="copyFailed" class="cp-input font-mono text-xs" aria-label="发票链接" @focus="($event.target as HTMLInputElement).select()">
      </p>
      <p class="muted note">
        {{ hostOpens ? '「打开发票」会先弹出 CPR 的确认框，确认后在新标签页打开 Stripe 页面下载发票或收据。' : '插件页面在沙箱中运行，不能直接打开新标签页：复制发票链接后，到浏览器里打开 Stripe 页面下载发票或收据。' }}
      </p>

      <div v-if="cursor">
        <ZButton size="small" :loading="billLoading" @click="loadBilling(true)">
          加载更早的账单
        </ZButton>
      </div>
    </template>
  </section>
</template>
