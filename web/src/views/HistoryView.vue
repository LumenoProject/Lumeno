<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { History } from '@lucide/vue'
import { api } from '../lib/api'
import { useAppStore } from '../stores/app'
import type { ProtocolEvent } from '../types'

const app = useAppStore()
const events = ref<ProtocolEvent[]>([])
const loading = ref(false)
const hasMore = ref(true)

async function loadMore() {
  if (loading.value || !hasMore.value) return
  loading.value = true
  try {
    const params = new URLSearchParams({ limit: '100' })
    const last = events.value.at(-1)
    if (last) {
      params.set('cursorAt', last.recordedAt)
      params.set('cursorId', last.id)
    }
    const page = await api<ProtocolEvent[]>(`/api/v1/events?${params}`)
    events.value.push(...page)
    hasMore.value = page.length === 100
  } catch (error) { app.report(error) } finally { loading.value = false }
}

onMounted(loadMore)
const labels: Record<string, string> = {
  focus_started: '开始专注', focus_succeeded: '完成专注', focus_failed: '专注失败',
  judgment_requested: '请求判定', judgment_allowed: '使用例外规则', booking_fulfilled: '兑现预约',
  booking_failed: '预约失败', booking_allowed: '预约例外', booking_confirmation_timed_out: '预约确认超时',
  group_run_started: '启动任务群', group_run_succeeded: '完成任务群',
}
</script>

<template>
  <div class="page">
    <header class="page-header"><div><p class="eyebrow">LEDGER</p><h1>记录</h1><p>查看每次开始、完成和判定。</p></div></header>
    <section v-if="!events.length" class="empty-panel small"><History :size="28" /><div><h2>还没有记录</h2><p>开始一次专注后，记录会显示在这里。</p></div></section>
    <ol v-else class="timeline">
      <li v-for="event in events" :key="event.id">
        <span class="timeline-dot"></span>
        <div><strong>{{ labels[event.eventType] || event.eventType }}</strong><p>{{ app.chainById.get(event.chainId || '')?.name || event.aggregateType }}</p></div>
        <time>{{ new Date(event.occurredAt).toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) }}</time>
      </li>
    </ol>
    <button v-if="events.length && hasMore" class="secondary history-more" :disabled="loading" @click="loadMore">{{ loading ? '读取中…' : '加载更早记录' }}</button>
  </div>
</template>
