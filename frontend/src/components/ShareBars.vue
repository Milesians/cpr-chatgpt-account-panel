<script setup lang="ts">
import type { Share } from '../api'
import { computed } from 'vue'
import { shareOf } from '../format'

const props = defineProps<{ title: string, items: Share[], label: (key: string) => string, colour?: (key: string) => string, limit?: number }>()
const total = computed(() => props.items.reduce((s, i) => s + i.value, 0))
const shown = computed(() => {
  const limit = props.limit ?? 8
  if (props.items.length <= limit)
    return props.items
  const head = props.items.slice(0, limit - 1)
  const rest = props.items.slice(limit - 1).reduce((s, i) => s + i.value, 0)
  return [...head, { key: 'other', value: rest }]
})
const max = computed(() => Math.max(0, ...shown.value.map(i => i.value)))
</script>

<template>
  <section class="share-bars" :aria-label="title">
    <h4>{{ title }}</h4>
    <p v-if="!items.length" class="muted">没有数据</p>
    <ul v-else>
      <li v-for="i in shown" :key="i.key">
        <span class="share-name">
          <i v-if="colour" class="swatch" :style="{ background: colour(i.key) }" />
          {{ label(i.key) }}
        </span>
        <span class="share-track"><i :style="{ width: `${max ? (i.value / max) * 100 : 0}%`, background: colour?.(i.key) ?? 'var(--cp-color-text-tertiary)' }" /></span>
        <span class="share-value">{{ shareOf(i, total) }}</span>
      </li>
    </ul>
  </section>
</template>
