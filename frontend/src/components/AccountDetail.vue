<script setup lang="ts">
import type { Account } from '../api'
import type { AccountLoginStatus } from '../login/helpers'
import { ZButton, ZTag } from '@codex-proxy/ui'
import {
  ArrowLeft,
  Clock,
  CreditCard,
  ShieldAlert,
  Sparkles,
} from '@lucide/vue'
import { computed } from 'vue'
import {
  ago,
  billingStatusLabel,
  credentialStateLabel,
  dateTime,
  day,
  daysLeft,
  formatPlanWithKind,
  hasPaymentAnomaly,
  hasWork,
  paymentIssue,
  percent,
  title,
} from '../format'
import AccountLogin from '../login/AccountLogin.vue'
import BillingPanel from './BillingPanel.vue'
import DevicesPanel from './DevicesPanel.vue'
import UsagePanel from './UsagePanel.vue'

const props = defineProps<{ account: Account }>()
const emit = defineEmits<{
  back: []
  patch: [id: string, patch: Partial<Account>]
  loginUpdated: [account: AccountLoginStatus]
}>()

const blocked = computed(() => props.account.blocker?.message ?? null)

const workVerdict = computed(() => {
  const u = props.account.usage
  if (!u) {
    return {
      value: '待查询',
      sub: '点击下方「查询用量」读取官方用量',
    }
  }
  if (u.error) {
    return {
      value: '读取失败',
      sub: u.error.message,
    }
  }
  if (hasWork(u)) {
    const shareStr = percent(u.work!.share, u.work!.share < 0.1 ? 1 : 0)
    return {
      value: `Work ${shareStr}`,
      sub: `近 ${u.days} 天共 ${u.work!.days_with_use} 天有用量 · ${ago(u.checked_at_ms)}查`,
    }
  }
  if (u.work) {
    return {
      value: '0% Work',
      sub: `近 ${u.days} 天仅经 Codex 转发 · ${ago(u.checked_at_ms)}查`,
    }
  }
  return {
    value: '未知',
    sub: `明细未读到 · ${ago(u.checked_at_ms)}查`,
  }
})

const paymentSummary = computed(() => {
  const p = props.account.payment
  if (!p) {
    return {
      value: '未查询',
      sub: '点击「查询订阅与账单」获取记录',
    }
  }
  const label = billingStatusLabel[p.status.toLowerCase()] ?? p.status
  return {
    value: label,
    sub: `${day(p.created_at_ms)} · ${ago(p.checked_at_ms)}查`,
  }
})

const graceSummary = computed(() => {
  const graceMs = props.account.payment?.grace_ends_at_ms
  if (graceMs) {
    const left = daysLeft(graceMs)
    return {
      value: `剩 ${left} 天`,
      sub: `宽限期至 ${day(graceMs)}`,
      tone: 'bad' as const,
    }
  }
  if (hasPaymentAnomaly(props.account)) {
    const issue = paymentIssue(props.account.payment?.status)
    return {
      value: issue === 'failed' ? '扣款失败' : '待续费',
      sub: issue === 'failed' ? '最近一笔账单扣款失败或逾期' : '账单处于待支付状态',
      tone: (issue === 'failed' ? 'bad' : 'warn') as 'bad' | 'warn',
    }
  }
  return {
    value: '正常',
    sub: props.account.payment ? '无逾期宽限期' : '随订阅与账单一同检查',
    tone: 'ok' as const,
  }
})

const tokenSummary = computed(() => {
  const exp = props.account.access_token_expires_at_ms
  const stateText = credentialStateLabel(props.account.credential_state)
  if (!exp) {
    return {
      value: stateText,
      sub: '未记录访问令牌到期时间',
      warn: props.account.credential_state !== 'ready' || props.account.blocker?.code === 'token_expired',
    }
  }
  const left = daysLeft(exp)
  const expired = exp <= Date.now() || props.account.blocker?.code === 'token_expired'
  return {
    value: expired ? '已过期' : left > 0 ? `剩 ${left} 天` : '今日到期',
    sub: `${dateTime(exp)} · ${stateText}`,
    warn: expired || props.account.credential_state !== 'ready',
  }
})
</script>

<template>
  <div class="detail-page">
    <!-- 顶部返回栏 -->
    <div class="detail-topbar">
      <ZButton size="small" aria-label="返回列表" @click="emit('back')">
        <ArrowLeft :size="14" aria-hidden="true" /> 返回列表
      </ZButton>
    </div>

    <!-- 顶部账号身份头部（只放邮箱、套餐、CPR 名称、账号 ID） -->
    <section class="verdict-panel" aria-label="账号概要">
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <h2 class="m-0 text-lg font-semibold text-cp-text break-all">
            {{ title(account) }}
          </h2>
          <ZTag size="small" :type="account.kind === 'team' ? 'info' : 'default'">
            {{ formatPlanWithKind(account.plan_type, account.kind) }}
          </ZTag>
          <ZTag v-if="!account.enabled" size="small" type="warning">
            CPR 已停用
          </ZTag>
        </div>
        <p class="m-0 mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-cp-xs text-cp-text-tertiary">
          <span v-if="account.email && account.name !== account.email">
            CPR 名称：{{ account.name }}
          </span>
          <span>
            账号 ID：<code class="font-mono">{{ account.id }}</code>
          </span>
        </p>
      </div>
    </section>

    <!-- 四宫格核心摘要块 (rate-tiles)：Work 结论、最近支付、宽限期、令牌到期 -->
    <section class="rate-tiles" aria-label="关键指标摘要">
      <div class="rate-tile">
        <div class="rate-tile-label">
          <span>Work 结论</span>
          <Sparkles :size="14" aria-hidden="true" />
        </div>
        <div
          class="rate-tile-value"
          :class="{ 'text-bad': Boolean(account.usage?.error) }"
          :style="hasWork(account.usage) ? { color: 'var(--series-6)' } : undefined"
        >
          {{ workVerdict.value }}
        </div>
        <div class="rate-tile-sub">
          {{ workVerdict.sub }}
        </div>
      </div>

      <div class="rate-tile">
        <div class="rate-tile-label">
          <span>最近支付</span>
          <CreditCard :size="14" aria-hidden="true" />
        </div>
        <div
          class="rate-tile-value"
          :class="{
            'text-bad': paymentIssue(account.payment?.status) === 'failed',
            'text-warn': paymentIssue(account.payment?.status) === 'due',
          }"
        >
          {{ paymentSummary.value }}
        </div>
        <div class="rate-tile-sub">
          {{ paymentSummary.sub }}
        </div>
      </div>

      <div class="rate-tile">
        <div class="rate-tile-label">
          <span>宽限期与账单状态</span>
          <ShieldAlert :size="14" aria-hidden="true" />
        </div>
        <div
          class="rate-tile-value"
          :class="{
            'text-bad': graceSummary.tone === 'bad',
            'text-warn': graceSummary.tone === 'warn',
          }"
        >
          {{ graceSummary.value }}
        </div>
        <div class="rate-tile-sub">
          {{ graceSummary.sub }}
        </div>
      </div>

      <div class="rate-tile">
        <div class="rate-tile-label">
          <span>令牌到期</span>
          <Clock :size="14" aria-hidden="true" />
        </div>
        <div class="rate-tile-value" :class="{ 'text-warn': tokenSummary.warn }">
          {{ tokenSummary.value }}
        </div>
        <div class="rate-tile-sub">
          {{ tokenSummary.sub }}
        </div>
      </div>
    </section>

    <!-- 单页上下排：用量 → 订阅与账单 → 重新登录 → 登录设备 -->
    <UsagePanel
      :account-id="account.id"
      :summary="account.usage"
      :blocked="blocked"
      @checked="s => emit('patch', account.id, { usage: s })"
    />

    <BillingPanel
      :account-id="account.id"
      :payment="account.payment"
      :blocked="blocked"
      @payment="p => emit('patch', account.id, { payment: p })"
    />

    <AccountLogin
      :account-id="account.id"
      @updated="a => emit('loginUpdated', a)"
    />

    <DevicesPanel
      :account-id="account.id"
      :blocked="blocked"
    />
  </div>
</template>
