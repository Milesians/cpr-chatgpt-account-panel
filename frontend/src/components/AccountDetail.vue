<script setup lang="ts">
import type { Account } from '../api'
import { ZButton, ZSegmented, ZTag } from '@codex-proxy/ui'
import {
  ArrowLeft,
  Clock,
  CreditCard,
  ShieldAlert,
  Sparkles,
} from '@lucide/vue'
import { computed, ref } from 'vue'
import {
  ago,
  billingStatusLabel,
  billingTag,
  dateTime,
  day,
  daysLeft,
  hasPaymentAnomaly,
  hasWork,
  kindLabel,
  percent,
  rowAccentTone,
  title,
} from '../format'
import AccountLogin from '../login/AccountLogin.vue'
import BillingPanel from './BillingPanel.vue'
import DevicesPanel from './DevicesPanel.vue'
import UsagePanel from './UsagePanel.vue'

const props = defineProps<{ account: Account }>()
const emit = defineEmits<{ back: [], patch: [id: string, patch: Partial<Account>] }>()

const activeTab = ref<'all' | 'usage' | 'billing' | 'devices' | 'login'>('all')
const tabOptions = [
  { label: '全部模块', value: 'all' },
  { label: '官方用量', value: 'usage' },
  { label: '订阅与账单', value: 'billing' },
  { label: '登录设备', value: 'devices' },
  { label: '重登与凭据', value: 'login' },
]

const blocked = computed(() => props.account.blocker?.message ?? null)
const panelTone = computed(() => rowAccentTone(props.account))

const workVerdict = computed(() => {
  const u = props.account.usage
  if (!u) {
    return {
      pill: '未查询',
      pillClass: 'neutral',
      value: '待查询',
      sub: '点击下方「查询」读取官方用量',
    }
  }
  if (u.error) {
    return {
      pill: '查询报错',
      pillClass: 'bad',
      value: '读取失败',
      sub: u.error.message,
    }
  }
  if (hasWork(u)) {
    const shareStr = percent(u.work!.share, u.work!.share < 0.1 ? 1 : 0)
    return {
      pill: `Work 占 ${shareStr}`,
      pillClass: 'work',
      value: `Work ${shareStr}`,
      sub: `最近 ${u.days} 天共 ${u.work!.days_with_use} 天有用量 · ${ago(u.checked_at_ms)}`,
    }
  }
  if (u.work) {
    return {
      pill: '无 Work 用量',
      pillClass: 'ok',
      value: '0% Work',
      sub: `最近 ${u.days} 天仅经 Codex 转发 · ${ago(u.checked_at_ms)}`,
    }
  }
  return {
    pill: '明细未读到',
    pillClass: 'warn',
    value: '未知',
    sub: `查询于 ${ago(u.checked_at_ms)}`,
  }
})

const paymentSummary = computed(() => {
  const p = props.account.payment
  if (!p) {
    return {
      value: '未查询',
      sub: '点击「查询账单」获取记录',
      tagType: 'default' as const,
    }
  }
  const label = billingStatusLabel[p.status.toLowerCase()] ?? p.status
  return {
    value: label,
    sub: `${day(p.created_at_ms)} · 记于 ${ago(p.checked_at_ms)}`,
    tagType: billingTag(p.status),
  }
})

const graceSummary = computed(() => {
  const graceMs = props.account.payment?.grace_ends_at_ms
  if (graceMs) {
    const left = daysLeft(graceMs)
    return {
      value: `剩 ${left} 天`,
      sub: `宽限期至 ${day(graceMs)}`,
      warn: true,
    }
  }
  if (hasPaymentAnomaly(props.account)) {
    return {
      value: '需处理续费',
      sub: '账单处于待支付或扣款失败状态',
      warn: true,
    }
  }
  return {
    value: '正常',
    sub: props.account.payment ? '无逾期宽限期' : '随账单与订阅一同检查',
    warn: false,
  }
})

const tokenSummary = computed(() => {
  const exp = props.account.access_token_expires_at_ms
  if (!exp) {
    return {
      value: props.account.credential_state,
      sub: '未记录访问令牌到期时间',
      expired: props.account.blocker?.code === 'token_expired',
    }
  }
  const left = daysLeft(exp)
  const expired = exp <= Date.now() || props.account.blocker?.code === 'token_expired'
  return {
    value: expired ? '已过期' : left > 0 ? `剩 ${left} 天` : '今日到期',
    sub: `${dateTime(exp)} · 凭据 ${props.account.credential_state}`,
    expired,
  }
})

function handleFocusLogin() {
  if (activeTab.value !== 'all' && activeTab.value !== 'login') {
    activeTab.value = 'all'
  }
}
</script>

<template>
  <div class="detail-page">
    <!-- 顶部返回与分段切换栏 -->
    <div class="detail-topbar">
      <div class="flex flex-wrap items-center gap-2">
        <ZButton size="small" aria-label="返回列表" @click="emit('back')">
          <ArrowLeft :size="14" aria-hidden="true" /> 返回列表
        </ZButton>
        <span class="text-cp-xs text-cp-text-tertiary">
          账号 ID：<code class="font-mono">{{ account.id }}</code>
        </span>
      </div>
      <ZSegmented
        v-model="activeTab"
        :options="tabOptions"
        size="small"
        aria-label="详情分段显示"
      />
    </div>

    <!-- 顶部账号头判定面板 (verdict-panel) -->
    <section class="verdict-panel" :data-tone="panelTone" aria-label="账号概要">
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <h2 class="m-0 text-lg font-semibold text-cp-text break-all">
            {{ title(account) }}
          </h2>
          <span class="verdict-pill" :class="workVerdict.pillClass">
            {{ workVerdict.pill }}
          </span>
          <ZTag v-if="account.plan_type" size="small">
            {{ account.plan_type.toUpperCase() }}
          </ZTag>
          <ZTag size="small" :type="account.kind === 'team' ? 'info' : 'default'">
            {{ kindLabel[account.kind] }}
          </ZTag>
          <ZTag v-if="!account.enabled" size="small" type="warning">
            CPR 已停用
          </ZTag>
        </div>
        <p class="m-0 mt-1.5 text-cp-xs text-cp-text-secondary">
          <template v-if="account.email && account.name !== account.email">
            别名：{{ account.name }} ·
          </template>
          {{ workVerdict.sub }}
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
        <div class="rate-tile-value flex items-center gap-2">
          <span>{{ paymentSummary.value }}</span>
          <ZTag v-if="account.payment" size="small" :type="paymentSummary.tagType">
            {{ account.payment.status }}
          </ZTag>
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
        <div class="rate-tile-value" :class="{ 'text-bad': graceSummary.warn }">
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
        <div class="rate-tile-value" :class="{ 'text-bad': tokenSummary.expired }">
          {{ tokenSummary.value }}
        </div>
        <div class="rate-tile-sub">
          {{ tokenSummary.sub }}
        </div>
      </div>
    </section>

    <!-- 分段卡片区：使用 v-show 保持已查出的状态不丢失 -->
    <div v-show="activeTab === 'all' || activeTab === 'usage'">
      <UsagePanel
        :account-id="account.id"
        :summary="account.usage"
        :blocked="blocked"
        @checked="s => emit('patch', account.id, { usage: s })"
      />
    </div>

    <div v-show="activeTab === 'all' || activeTab === 'billing'">
      <BillingPanel
        :account-id="account.id"
        :payment="account.payment"
        :blocked="blocked"
        @payment="p => emit('patch', account.id, { payment: p })"
      />
    </div>

    <div v-show="activeTab === 'all' || activeTab === 'devices'">
      <DevicesPanel
        :account-id="account.id"
        :blocked="blocked"
        @focus-login="handleFocusLogin"
      />
    </div>

    <div v-show="activeTab === 'all' || activeTab === 'login'">
      <AccountLogin :account-id="account.id" />
    </div>
  </div>
</template>
