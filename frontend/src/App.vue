<script setup lang="ts">
import type { Account } from './api'
import { ZButton, ZConfigProvider, ZSegmented } from '@codex-proxy/ui'
import { zhHans } from '@codex-proxy/ui/locale'
import { RefreshCw, UserRound } from '@lucide/vue'
import { computed, nextTick, onMounted, ref, shallowRef, watch } from 'vue'
import { api } from './api'
import AccountDetail from './components/AccountDetail.vue'
import AccountList from './components/AccountList.vue'
import { dateTime, hasPaymentAnomaly, hasWork } from './format'
import LoginPage from './login/LoginPage.vue'

const section = ref<'accounts' | 'login'>('accounts')
const sections = [
  { label: '账号监控', value: 'accounts' },
  { label: '登录与导入', value: 'login' },
]
const accounts = shallowRef<Account[] | null>(null)
const checkedAt = ref(0)
const opened = ref<string | null>(null)
const loading = ref(false)
const error = ref('')

const current = computed(() => accounts.value?.find(a => a.id === opened.value) ?? null)
const withWork = computed(() => accounts.value?.filter(a => hasWork(a.usage)).length ?? 0)
const withPaymentAnomaly = computed(() => accounts.value?.filter(a => hasPaymentAnomaly(a)).length ?? 0)

async function load() {
  loading.value = true
  error.value = ''
  try {
    const r = await api.accounts()
    accounts.value = r.accounts
    checkedAt.value = r.checked_at_ms
  }
  catch (e) {
    error.value = (e as Error).message
  }
  finally {
    loading.value = false
  }
}

function patch(id: string, p: Partial<Account>) {
  accounts.value = accounts.value?.map(x => x.id === id ? { ...x, ...p } : x) ?? null
}

// 列表一直挂着（只是 v-show 隐藏），从详情或登录页返回时筛选、排序、选中和滚动位置都保留
let listScroll = 0
const app = () => document.querySelector<HTMLElement>('#app')

function saveListScroll() {
  if (section.value === 'accounts' && !opened.value) {
    listScroll = app()?.scrollTop ?? 0
  }
}

function open(id: string) {
  saveListScroll()
  opened.value = id
  nextTick(() => app()?.scrollTo({ top: 0 }))
}

function back() {
  opened.value = null
  nextTick(() => app()?.scrollTo({ top: listScroll }))
}

watch(section, (next, prev) => {
  if (prev === 'accounts' && !opened.value) {
    listScroll = app()?.scrollTop ?? 0
  }
  if (next === 'accounts' && !opened.value) {
    nextTick(() => app()?.scrollTo({ top: listScroll }))
  }
  else {
    nextTick(() => app()?.scrollTo({ top: 0 }))
  }
})

onMounted(load)
</script>

<template>
  <ZConfigProvider :locale="zhHans">
    <main class="panel-page font-sans text-cp-text">
      <header class="panel-header">
        <div class="flex min-w-0 items-center gap-3">
          <span class="panel-logo"><UserRound :size="22" aria-hidden="true" /></span>
          <div class="min-w-0">
            <h1 class="m-0 text-xl font-semibold tracking-tight text-cp-text">
              ChatGPT 账号面板
            </h1>
            <p class="m-0 mt-1 text-cp-xs text-cp-text-tertiary">
              <template v-if="accounts">
                共 {{ accounts.length }} 个 OpenAI OAuth 账号
                <template v-if="withWork"> · {{ withWork }} 个 Work 有用量</template>
                <template v-if="withPaymentAnomaly"> · {{ withPaymentAnomaly }} 个扣款异常</template>
                · 更新于 {{ dateTime(checkedAt) }}
              </template>
              <template v-else>
                官方用量、订阅账单、登录设备与免输重登
              </template>
            </p>
          </div>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <ZSegmented v-model="section" :options="sections" size="small" aria-label="页面切换" />
          <ZButton size="small" :loading="loading" aria-label="刷新" @click="load">
            <RefreshCw :size="14" aria-hidden="true" /> 刷新
          </ZButton>
        </div>
      </header>

      <p v-if="error" class="banner banner-error mb-4" role="alert">
        {{ error }}
      </p>

      <div v-show="section === 'accounts'">
        <AccountDetail
          v-if="current"
          :key="current.id"
          :account="current"
          @back="back"
          @patch="patch"
        />
        <div v-show="!current">
          <AccountList
            :accounts="accounts"
            :loading="loading"
            @open="open"
            @patch="patch"
          />
        </div>
      </div>

      <div v-show="section === 'login'">
        <LoginPage @accounts-changed="load" />
      </div>
    </main>
  </ZConfigProvider>
</template>
