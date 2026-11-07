<script setup lang="ts">
import type { CodexUsage, UsageSummary } from '../api'
import { ZButton, ZSegmented } from '@codex-proxy/ui'
import { computed, ref } from 'vue'
import { api } from '../api'
import { ago, compact, dateTime, percent, shortDate, unitNote, windowName } from '../format'
import { featureLabel, isWork, slotColour, slotOf, surfaceLabel } from '../surfaces'
import DailyChart from './DailyChart.vue'
import ShareBars from './ShareBars.vue'
import WindowMeter from './WindowMeter.vue'

const props = defineProps<{ accountId: string, summary: UsageSummary | null, blocked: string | null }>()
const emit = defineEmits<{ checked: [summary: UsageSummary] }>()

const days = ref('7')
const ranges = [{ label: '7 天', value: '7' }, { label: '14 天', value: '14' }, { label: '30 天', value: '30' }]
const usage = ref<CodexUsage | null>(null)
const loading = ref(false)
const error = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    const r = await api.usage(props.accountId, Number(days.value))
    usage.value = r.usage
    emit('checked', r.summary)
  }
  catch (e) {
    error.value = (e as Error).message
  }
  finally {
    loading.value = false
  }
}

const work = computed(() => usage.value?.work ?? null)
const unit = computed(() => unitNote(usage.value?.breakdown?.unit))
const problems = computed(() => Object.entries(usage.value?.problems ?? {}))
const partName: Record<string, string> = { limits: '额度窗口', tokens: '每日 Token', breakdown: '用量明细' }
const tokenMax = computed(() => Math.max(0, ...(usage.value?.tokens?.daily ?? []).map(d => d.tokens)))
const surfaceColour = (k: string) => slotColour(slotOf(k))
</script>

<template>
  <section class="detail-card" aria-labelledby="usage-title">
    <div class="detail-card-head">
      <h3 id="usage-title" class="card-title">ChatGPT 官方用量</h3>
      <ZSegmented v-model="days" :options="ranges" size="small" aria-label="时间范围" :disabled="loading" />
      <ZButton type="primary" size="small" :loading="loading" :disabled="!!blocked" :title="blocked ?? undefined" @click="load">查询</ZButton>
      <span v-if="usage" class="muted">查询于 {{ dateTime(usage.checked_at_ms) }}</span>
      <span v-else-if="summary" class="muted">上次查询 {{ ago(summary.checked_at_ms) }}（最近 {{ summary.days }} 天）</span>
    </div>
    <p class="muted note">
      经账号在 CPR 绑定的代理读 ChatGPT 自己的用量统计，只发 GET、不消耗额度，点了才查。ChatGPT 按请求方的身份记使用位置；CPR 以 Codex 的身份转发，所以记在 Work 下的用量一定不是经 CPR 用的。
    </p>
    <p v-if="blocked" class="banner banner-warn">{{ blocked }}</p>
    <p v-if="error" class="banner banner-error" role="alert">{{ error }}</p>
    <p v-else-if="!usage && summary?.error" class="banner banner-error">上次查询失败：{{ summary.error.message }}</p>

    <template v-if="usage">
      <div v-if="work" class="work-callout" :data-used="work.total > 0">
        <template v-if="work.total > 0">
          <strong>最近 {{ usage.days }} 天 Work 有用量：占 {{ percent(work.share, work.share < 0.1 ? 1 : 0) }}</strong>
          <span>{{ work.days_with_use }} 天有用量，最近一次 {{ work.last_day ? shortDate(work.last_day) : '—' }} · {{ work.surfaces.map(s => `${surfaceLabel(s.key)} ${compact(s.value)}`).join('、') }}</span>
        </template>
        <strong v-else>最近 {{ usage.days }} 天 Work 没有用量</strong>
      </div>
      <p v-else class="banner banner-warn">用量明细没读到，判断不了 Work 有没有用量</p>

      <div v-if="usage.limits?.windows.length" class="meters">
        <WindowMeter v-for="w in usage.limits.windows" :key="w.window_minutes" :label="windowName(w.window_minutes)" :used="w.used_percent" :resets-at="w.resets_at_ms" />
        <span v-if="usage.limits.limit_reached" class="chip-bad">已触顶</span>
      </div>

      <template v-if="usage.breakdown">
        <DailyChart :days="usage.breakdown.days" :unit="unit" />
        <div class="share-grid">
          <ShareBars title="使用位置" :items="usage.breakdown.surfaces" :label="surfaceLabel" :colour="surfaceColour" />
          <ShareBars title="发起方式" :items="usage.breakdown.features" :label="featureLabel" />
          <ShareBars title="模型" :items="usage.breakdown.models" :label="k => k" />
        </div>
        <p v-if="usage.breakdown.surfaces.some(s => isWork(s.key))" class="muted note">Work 的几个位置（Work、Work 网页、Work 桌面版、Work 移动端）在图上合成一种颜色。</p>
        <p v-if="usage.breakdown.updated_at_ms" class="muted note">ChatGPT 的统计更新于 {{ dateTime(usage.breakdown.updated_at_ms) }}，单位：{{ unit }}</p>
      </template>

      <section v-if="usage.tokens" class="tokens" aria-label="每日 Token">
        <h4>每日 Token</h4>
        <div class="token-bars">
          <span v-for="d in usage.tokens.daily" :key="d.date" class="token-bar" :title="`${d.date}：${compact(d.tokens)}`">
            <i :style="{ height: `${tokenMax ? (d.tokens / tokenMax) * 100 : 0}%` }" />
          </span>
        </div>
        <p class="muted note">
          这段时间 {{ compact(usage.tokens.daily.reduce((s, d) => s + d.tokens, 0)) }}
          <template v-if="usage.tokens.lifetime !== undefined"> · 累计 {{ compact(usage.tokens.lifetime) }}</template>
          <template v-if="usage.tokens.threads !== undefined"> · {{ usage.tokens.threads }} 个对话</template>
        </p>
      </section>

      <ul v-if="problems.length" class="problems">
        <li v-for="[part, msg] in problems" :key="part">{{ partName[part] ?? part }}没读到：{{ msg }}</li>
      </ul>
    </template>
  </section>
</template>
