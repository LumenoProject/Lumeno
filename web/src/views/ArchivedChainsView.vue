<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Archive, ArrowLeft, Flame, Layers3 } from '@lucide/vue'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const loading = ref(true)

onMounted(async () => {
  try {
    await app.loadArchivedChains()
  } catch (error) {
    app.report(error)
  } finally {
    loading.value = false
  }
})

function archivedAt(value?: string | null) {
  if (!value) return '归档时间未知'
  return new Date(value).toLocaleString('zh-CN', {
    year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  })
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div><p class="eyebrow">ARCHIVE</p><h1>归档契约</h1><p>查看过去的契约和旧版本。</p></div>
      <RouterLink to="/chains" class="text-link"><ArrowLeft :size="15" /> 返回契约链</RouterLink>
    </header>

    <section v-if="loading" class="empty-panel small"><Archive :size="28" /><div><h2>正在读取归档</h2><p>请稍候。</p></div></section>
    <section v-else-if="!app.archivedChains.length" class="empty-panel small"><Archive :size="28" /><div><h2>还没有归档契约</h2><p>归档的契约会显示在这里。</p></div></section>
    <section v-else class="archived-list" aria-label="归档契约列表">
      <article v-for="chain in app.archivedChains" :key="chain.id" class="chain-card archived-chain-card" :class="{ group: chain.kind === 'group' }">
        <div class="chain-icon"><Layers3 v-if="chain.kind === 'group'" :size="18" /><Flame v-else :size="18" /></div>
        <div class="chain-body">
          <div class="chain-title"><h3>{{ chain.name }}</h3><time :datetime="chain.archivedAt || undefined">{{ archivedAt(chain.archivedAt) }}</time></div>
          <p>{{ chain.description || (chain.kind === 'group' ? '按顺序执行其中的所有任务' : chain.sacredTrigger) }}</p>
          <div class="chain-meta">
            <span>{{ chain.kind === 'group' ? '任务群' : '叶子任务' }}</span>
            <span>修订 {{ chain.revision }}</span>
            <span>完成 {{ chain.totalCompletions }}</span>
            <span :class="{ 'failure-text': chain.totalFailures }">失守 {{ chain.totalFailures }}</span>
            <span v-if="chain.kind === 'unit'">预约失守 {{ chain.auxiliaryFailures }}</span>
          </div>
        </div>
      </article>
      <p v-if="app.archivedChains.length === 1000" class="archive-limit-note">当前显示最近归档的 1000 条契约。</p>
    </section>
  </div>
</template>
