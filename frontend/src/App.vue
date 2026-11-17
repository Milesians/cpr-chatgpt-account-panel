<script setup lang="ts">
import { ZConfigProvider } from '@codex-proxy/ui'
import { zhHans } from '@codex-proxy/ui/locale'
import ProbeWorkspace from './probe/ProbeWorkspace.vue'
</script>

<template>
  <ZConfigProvider :locale="zhHans">
    <main class="panel-page font-sans text-cp-text">
      <ProbeWorkspace />
    </main>
  </ZConfigProvider>
</template>
