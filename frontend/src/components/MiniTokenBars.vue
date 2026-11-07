<script setup lang="ts">
import { computed } from 'vue'
import { compact, shortDate } from '../format'

/** 列表行里的每日 Token 柱状图：一天一根，悬停看当天数值 */
const props = defineProps<{ days: { date: string, tokens: number }[] }>()

const W = 150
const H = 32
const max = computed(() => Math.max(0, ...props.days.map(d => d.tokens)))
// 7 天时柱子粗、间隔大，30 天时细、间隔小，都占满整个宽度
const GAP = computed(() => props.days.length > 14 ? 1.5 : 4)
const bars = computed(() => {
  const n = props.days.length
  const w = n ? (W - GAP.value * (n - 1)) / n : 0
  return props.days.map((d, i) => {
    const h = max.value ? Math.max(d.tokens > 0 ? 2 : 0, (d.tokens / max.value) * (H - 1)) : 0
    return { ...d, x: i * (w + GAP.value), w, h }
  })
})
</script>

<template>
  <svg :viewBox="`0 0 ${W} ${H}`" :width="W" :height="H" class="mini-bars" role="img" :aria-label="`每日 Token：${days.map(d => `${shortDate(d.date)} ${compact(d.tokens)}`).join('，')}`">
    <line x1="0" :x2="W" :y1="H - 0.5" :y2="H - 0.5" class="mini-bars-base" />
    <g v-for="b in bars" :key="b.date" class="mini-bar">
      <rect :x="b.x" y="0" :width="b.w + GAP" :height="H" class="mini-bar-hit" />
      <path
        v-if="b.h > 0"
        :d="`M ${b.x} ${H} V ${H - b.h + Math.min(2, b.h)} q 0 -${Math.min(2, b.h)} ${Math.min(2, b.w / 2)} -${Math.min(2, b.h)} H ${b.x + b.w - Math.min(2, b.w / 2)} q ${Math.min(2, b.w / 2)} 0 ${Math.min(2, b.w / 2)} ${Math.min(2, b.h)} V ${H} Z`"
        class="mini-bar-fill"
      />
      <title>{{ shortDate(b.date) }}：{{ compact(b.tokens) }} Token</title>
    </g>
  </svg>
</template>
