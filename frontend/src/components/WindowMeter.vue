<script setup lang="ts">
import { computed } from 'vue'
import { dateTime } from '../format'

const props = defineProps<{ label: string, used: number | null, resetsAt?: number | null }>()
const level = computed(() => props.used === null ? 'empty' : props.used >= 90 ? 'bad' : props.used >= 70 ? 'warn' : 'ok')
</script>

<template>
  <div class="meter" :data-level="level" :title="resetsAt ? `${dateTime(resetsAt)} 重置` : undefined">
    <span class="meter-label">{{ label }}</span>
    <span class="meter-track" role="meter" :aria-label="`${label}已用`" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="used ?? undefined">
      <i :style="{ width: `${Math.min(100, Math.max(0, used ?? 0))}%` }" />
    </span>
    <span class="meter-value">{{ used === null ? '—' : `${Math.round(used)}%` }}</span>
  </div>
</template>
