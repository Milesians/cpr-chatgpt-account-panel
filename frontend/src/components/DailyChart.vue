<script setup lang="ts">
import type { BreakdownDay } from '../api'
import type { Slot } from '../surfaces'
import { computed, ref } from 'vue'
import { compact, shortDate } from '../format'
import { bySlot, SLOTS, slotColour, slotLabel } from '../surfaces'

const props = defineProps<{ days: BreakdownDay[], unit: string }>()
const order: Slot[] = [...SLOTS, 'other']
const rows = computed(() => props.days.map((d) => {
  const slots = bySlot(d.surfaces)
  return { date: d.date, total: d.total, parts: order.filter(s => slots.has(s)).map(s => ({ slot: s, value: slots.get(s)! })) }
}))
const max = computed(() => Math.max(0, ...rows.value.map(r => r.total)))
const present = computed(() => order.filter(s => rows.value.some(r => r.parts.some(p => p.slot === s))))
const hovered = ref<number | null>(null)
const active = computed(() => hovered.value === null ? null : rows.value[hovered.value] ?? null)
const step = computed(() => Math.max(1, Math.ceil(rows.value.length / 10)))
</script>

<template>
  <figure class="daily-chart">
    <div class="chart-legend" aria-label="图例">
      <span v-for="s in present" :key="s" class="legend-item"><i class="swatch" :style="{ background: slotColour(s) }" />{{ slotLabel(s) }}</span>
      <span v-if="!present.length" class="muted">这段时间没有用量</span>
    </div>
    <div class="chart-tip" aria-live="polite">
      <template v-if="active">
        <strong>{{ active.date }}</strong>
        <span v-for="p in active.parts" :key="p.slot" class="legend-item"><i class="swatch" :style="{ background: slotColour(p.slot) }" />{{ slotLabel(p.slot) }} {{ compact(p.value) }}</span>
        <span v-if="!active.parts.length" class="muted">没有用量</span>
      </template>
      <span v-else class="muted">悬停或点按一天看明细 · 单位：{{ unit }}</span>
    </div>
    <div class="chart-plot" @mouseleave="hovered = null">
      <button
        v-for="(r, i) in rows" :key="r.date" type="button" class="chart-col"
        :aria-label="`${r.date}：${r.parts.map(p => `${slotLabel(p.slot)} ${compact(p.value)}`).join('，') || '没有用量'}`"
        :data-active="hovered === i" @mouseenter="hovered = i" @focus="hovered = i" @click="hovered = i"
      >
        <span class="chart-stack" :style="{ height: `${max ? (r.total / max) * 100 : 0}%` }">
          <i v-for="p in r.parts" :key="p.slot" :style="{ flexGrow: p.value, background: slotColour(p.slot) }" />
        </span>
        <span class="chart-x">{{ i % step === 0 || i === rows.length - 1 ? shortDate(r.date) : '' }}</span>
      </button>
    </div>
  </figure>
</template>
