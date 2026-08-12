<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { ArrowRight, CheckCircle2, Flame, Gauge, ShieldAlert } from '@lucide/vue'
import ChainCard from '../components/ChainCard.vue'
import { bookingPhase, bookingPhaseDeadline } from '../lib/booking'
import { formatCountdown, remainingMilliseconds } from '../lib/time'
import { useAppStore } from '../stores/app'
import type { Booking } from '../types'

const app = useAppStore()
const now = ref(Date.now())
let previousNow = now.value
const timer = window.setInterval(() => {
  const current = Date.now()
  const crossedExpiry = app.pendingBookings.some((booking) => {
    if (booking.state !== 'pending') return false
    const expiry = Date.parse(booking.expiresAt)
    return expiry > previousNow && expiry <= current
  })
  previousNow = current
  now.value = current
  if (crossedExpiry) void app.refresh()
}, 1000)
onBeforeUnmount(() => window.clearInterval(timer))

const date = new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', weekday: 'long' }).format(new Date())
const topChains = computed(() => app.unitChains.slice(0, 5))
const record = computed(() => {
  const { completions, failures } = app.commitmentRecord
  const attempts = completions + failures
  const bestStreak = Math.max(0, ...app.chains.map((chain) => chain.mainStreak))

  return {
    attempts,
    completions,
    failures,
    bestStreak,
    completionRate: attempts ? Math.round((completions / attempts) * 100) : 0,
  }
})

const recordTitle = computed(() => {
  if (!record.value.attempts) return '第一笔兑现记录，等你写下。'
  if (!record.value.failures) return `你已经守住 ${record.value.completions} 次承诺。`
  const lead = record.value.completions - record.value.failures
  if (lead > 0) return `完成比失守多 ${lead} 次。`
  if (lead === 0) return '完成与失守，暂时持平。'
  return `再完成 ${Math.abs(lead) + 1} 次，重新拉开差距。`
})

function bookingCountdown(deadline: string) {
  return formatCountdown(remainingMilliseconds(deadline, now.value))
}

function bookingStatus(booking: Booking) {
  if (booking.state === 'awaiting_judgment') {
    return booking.confirmationExpiresAt
      ? `确认倒计时 ${bookingCountdown(booking.confirmationExpiresAt)}`
      : '等待判定'
  }
  const phase = bookingPhase(booking, now.value)
  if (phase === 'start_window') return `启动窗口 ${bookingCountdown(booking.expiresAt)}`
  if (phase === 'expired') return '等待判定'
  return `预约倒计时 ${bookingCountdown(bookingPhaseDeadline(booking, now.value))}`
}
</script>

<template>
  <div class="page today-page">
    <header class="page-header hero-header">
      <div><p class="eyebrow">{{ date }}</p><h1>今天，守住一个承诺。</h1></div>
      <RouterLink to="/chains" class="text-link">管理契约链 <ArrowRight :size="15" /></RouterLink>
    </header>

    <section class="metric-strip">
      <article><Flame :size="18" /><div><strong>{{ record.bestStreak }}</strong><span>最佳当前连续</span></div></article>
      <article><CheckCircle2 :size="18" /><div><strong>{{ record.completions }}</strong><span>累计完成</span></div></article>
      <article class="failure-metric"><ShieldAlert :size="18" /><div><strong>{{ record.failures }}</strong><span>累计失守</span></div></article>
      <article><Gauge :size="18" /><div><strong>{{ record.attempts ? `${record.completionRate}%` : '—' }}</strong><span>履约率</span></div></article>
    </section>

    <section class="record-panel" aria-labelledby="record-title">
      <div class="record-copy">
        <p class="eyebrow">COMMITMENT RECORD</p>
        <h2 id="record-title">{{ recordTitle }}</h2>
        <p v-if="record.failures">已记录 {{ record.failures }} 次失守。</p>
        <p v-else-if="record.completions">还没有失守记录。</p>
        <p v-else>完成一次专注后，记录会显示在这里。</p>
      </div>
      <div class="record-progress">
        <div class="progress-label"><span>完成 / 全部判定</span><strong>{{ record.attempts ? `${record.completionRate}%` : '暂无' }}</strong></div>
        <div
          class="progress-track"
          role="progressbar"
          aria-label="履约率"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="record.completionRate"
        >
          <span :style="{ width: `${record.completionRate}%` }"></span>
        </div>
        <div class="record-counts">
          <span><i class="success-dot"></i>{{ record.completions }} 次完成</span>
          <span><i class="failure-dot"></i>{{ record.failures }} 次失守</span>
        </div>
      </div>
    </section>

    <section v-if="!app.chains.length" class="empty-panel section-block">
      <span class="big-glyph">L</span>
      <div><h2>当前没有活动契约链</h2><p>你可以建立新契约，或查看归档。</p></div>
      <div class="empty-actions"><RouterLink to="/chains/archive" class="secondary">查看归档</RouterLink><RouterLink to="/chains" class="primary">建立契约</RouterLink></div>
    </section>

    <section v-else class="section-block">
      <div class="section-heading"><div><p class="eyebrow">FOCUS</p><h2>选择下一步</h2></div><span>启动后不可暂停</span></div>
      <div class="chain-list"><ChainCard v-for="chain in topChains" :key="chain.id" :chain="chain" show-record /></div>
    </section>

    <section v-if="app.pendingBookings.length" class="section-block paper-panel">
      <div class="section-heading"><div><p class="eyebrow">BOOKINGS</p><h2>已经说出口的事</h2></div></div>
      <div class="booking-row" v-for="booking in app.pendingBookings" :key="booking.id">
        <span>{{ app.chainById.get(booking.chainId)?.name }}</span>
        <span>{{ bookingStatus(booking) }}</span>
      </div>
    </section>
  </div>
</template>
