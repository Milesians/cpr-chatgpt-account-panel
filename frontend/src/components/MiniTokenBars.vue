<script setup lang="ts">
import { computed, ref } from 'vue'
import { compact, percent, shortDate } from '../format'

/** 列表行里的每日 Token 柱状图：一天一根，悬停或点按看当天详情 */
const props = defineProps<{ days: { date: string, tokens: number }[] }>()

const WEEK = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']
const max = computed(() => Math.max(0, ...props.days.map(d => d.tokens)))
const total = computed(() => props.days.reduce((s, d) => s + d.tokens, 0))
const active = ref<number | null>(null)
const day = computed(() => (active.value !== null ? props.days[active.value] ?? null : null))

function weekday(date: string): string {
  const t = Date.parse(`${date}T00:00:00Z`)
  return Number.isNaN(t) ? '' : WEEK[new Date(t).getUTCDay()]!
}

function height(tokens: number): string {
  if (!max.value || tokens <= 0)
    return '0'
  return `${Math.max(7, (tokens / max.value) * 100)}%`
}

/** 弹出卡片靠哪边：两头的柱子贴边对齐，避免超出单元格 */
const tipSide = computed(() => {
  const n = props.days.length
  if (active.value === null || n < 3)
    return 'center'
  if (active.value < n / 4)
    return 'start'
  if (active.value >= (n * 3) / 4)
    return 'end'
  return 'center'
})
const tipLeft = computed(() => {
  const n = props.days.length
  return active.value === null || !n ? '0' : `${((active.value + 0.5) / n) * 100}%`
})
</script>

<template>
  <div class="mini-bars" :class="{ 'is-dense': days.length > 14 }" @mouseleave="active = null">
    <button
      v-for="(d, i) in days"
      :key="d.date"
      type="button"
      class="mini-bar"
      :class="{ 'is-active': active === i, 'is-dim': active !== null && active !== i }"
      :aria-label="`${shortDate(d.date)} ${weekday(d.date)}：${compact(d.tokens)} Token`"
      :aria-expanded="active === i"
      @mouseenter="active = i"
      @focus="active = i"
      @blur="active = null"
      @click.stop="active = active === i ? null : i"
    >
      <i :style="{ height: height(d.tokens) }" />
    </button>
    <div
      v-if="day"
      class="mini-bars-tip"
      :data-side="tipSide"
      :style="{ left: tipLeft }"
      role="status"
    >
      <span class="muted">{{ shortDate(day.date) }} {{ weekday(day.date) }}</span>
      <strong>{{ day.tokens.toLocaleString('zh-CN') }} Token</strong>
      <span v-if="day.tokens === 0" class="muted">当天没有用量</span>
      <span v-else class="muted">
        占 {{ days.length }} 天的 {{ percent(day.tokens / total, day.tokens / total < 0.1 ? 1 : 0) }}<template v-if="day.tokens === max"> · 这几天最高</template>
      </span>
    </div>
  </div>
</template>
