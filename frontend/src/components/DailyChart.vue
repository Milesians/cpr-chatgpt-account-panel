<script setup lang="ts">
import type { BreakdownDay } from '../api'
import type { Slot } from '../surfaces'
import { computed, ref } from 'vue'
import { compact, percent, shortDate } from '../format'
import { bySlot, SLOTS, slotColour, slotLabel } from '../surfaces'

const props = defineProps<{ days: BreakdownDay[], unit: string }>()
const order: Slot[] = [...SLOTS, 'other']

const rows = computed(() =>
  props.days.map((d) => {
    const slots = bySlot(d.surfaces)
    const parts = order
      .filter(s => slots.has(s))
      .map(s => ({ slot: s, value: slots.get(s)! }))
    const workVal = slots.get('work') ?? 0
    return {
      date: d.date,
      total: d.total,
      workShare: d.total > 0 ? workVal / d.total : 0,
      parts,
    }
  }),
)

const max = computed(() => Math.max(0, ...rows.value.map(r => r.total)))
const present = computed(() =>
  order.filter(s => rows.value.some(r => r.parts.some(p => p.slot === s))),
)

const hoveredIndex = ref<number | null>(null)
const activeIndex = computed(() => {
  if (hoveredIndex.value !== null && rows.value[hoveredIndex.value])
    return hoveredIndex.value
  return rows.value.length > 0 ? rows.value.length - 1 : null
})

const activeRow = computed(() =>
  activeIndex.value !== null ? rows.value[activeIndex.value] ?? null : null,
)
</script>

<template>
  <figure class="trend-chart" aria-label="每日使用位置走势图">
    <!-- 顶部图例与单位 -->
    <div class="trend-head">
      <div class="flex flex-wrap items-center gap-3" aria-label="图例">
        <span class="font-semibold text-cp-text">每日用量明细</span>
        <span
          v-for="s in present"
          :key="s"
          class="legend-item text-cp-text-secondary"
        >
          <i class="swatch" :style="{ background: slotColour(s) }" />
          {{ slotLabel(s) }}
        </span>
        <span v-if="!present.length" class="muted">这段时间没有用量</span>
      </div>
      <span class="muted">单位：{{ unit }}</span>
    </div>

    <!-- 悬停/选中日的 상세摘要条 (turnstate TrendChart 风格) -->
    <div class="trend-summary" aria-live="polite">
      <template v-if="activeRow">
        <strong class="text-cp-text">{{ activeRow.date }}</strong>
        <span class="muted">总计 {{ compact(activeRow.total) }}</span>
        <span
          v-if="activeRow.workShare > 0"
          class="verdict-pill work"
        >
          Work {{ percent(activeRow.workShare, activeRow.workShare < 0.1 ? 1 : 0) }}
        </span>
        <span
          v-for="p in activeRow.parts"
          :key="p.slot"
          class="legend-item text-cp-text-secondary"
        >
          <i class="swatch" :style="{ background: slotColour(p.slot) }" />
          {{ slotLabel(p.slot) }}
          <strong class="text-cp-text">{{ compact(p.value) }}</strong>
        </span>
        <span v-if="!activeRow.parts.length" class="muted">当日无用量</span>
      </template>
      <span v-else class="muted">悬停或点按柱体查看单日分布</span>
    </div>

    <!-- 柱状堆叠绘图区 -->
    <div class="trend-bars" @mouseleave="hoveredIndex = null">
      <button
        v-for="(r, i) in rows"
        :key="r.date"
        type="button"
        class="trend-bar"
        :class="{ 'is-active': activeIndex === i }"
        :data-active="activeIndex === i"
        :aria-label="`${r.date}：${r.parts.map(p => `${slotLabel(p.slot)} ${compact(p.value)}`).join('，') || '没有用量'}`"
        @mouseenter="hoveredIndex = i"
        @focus="hoveredIndex = i"
        @click="hoveredIndex = i"
      >
        <span
          class="trend-bar-stack"
          :style="{ height: `${max ? Math.max(r.total > 0 ? 6 : 0, (r.total / max) * 100) : 0}%` }"
        >
          <i
            v-for="p in r.parts"
            :key="p.slot"
            class="trend-bar-seg"
            :style="{ flexGrow: p.value, background: slotColour(p.slot) }"
          />
        </span>
      </button>
    </div>

    <!-- X 轴起止日期 -->
    <div v-if="rows.length" class="trend-axis">
      <span>{{ shortDate(rows[0]!.date) }}</span>
      <span v-if="rows.length > 2">{{ shortDate(rows[Math.floor(rows.length / 2)]!.date) }}</span>
      <span>{{ shortDate(rows[rows.length - 1]!.date) }}</span>
    </div>
  </figure>
</template>
