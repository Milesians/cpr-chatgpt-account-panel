<script setup lang="ts">
import type { Slice } from '../surfaces'
import { computed } from 'vue'

/** 列表行里的占比饼图（环形）；图例和数值由外面的弹出卡片给 */
const props = withDefaults(defineProps<{ parts: Slice[], size?: number, label: string }>(), { size: 40 })

const R = 15
const r = 8.5
const C = 16

function point(rad: number, radius: number): string {
  return `${(C + radius * Math.sin(rad)).toFixed(3)} ${(C - radius * Math.cos(rad)).toFixed(3)}`
}

function arc(a0: number, a1: number): string {
  const large = a1 - a0 > Math.PI ? 1 : 0
  return `M ${point(a0, R)} A ${R} ${R} 0 ${large} 1 ${point(a1, R)} L ${point(a1, r)} A ${r} ${r} 0 ${large} 0 ${point(a0, r)} Z`
}

const paths = computed(() => {
  const total = props.parts.reduce((s, p) => s + p.value, 0)
  if (total <= 0)
    return []
  if (props.parts.length === 1)
    return [{ key: props.parts[0]!.key, colour: props.parts[0]!.colour, d: `${arc(0, Math.PI)} ${arc(Math.PI, Math.PI * 2)}` }]
  let a = 0
  return props.parts.map((p) => {
    const span = (p.value / total) * Math.PI * 2
    const d = arc(a, a + span)
    a += span
    return { key: p.key, colour: p.colour, d }
  })
})
</script>

<template>
  <svg viewBox="0 0 32 32" :width="size" :height="size" class="mini-donut" role="img" :aria-label="label">
    <path v-for="p in paths" :key="p.key" :d="p.d" :fill="p.colour" class="mini-donut-slice" />
  </svg>
</template>
