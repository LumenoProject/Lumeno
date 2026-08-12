<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { AlertTriangle, CalendarClock, Play, X } from '@lucide/vue'
import { bookingPhase, bookingPhaseDeadline } from '../lib/booking'
import { formatCountdown, remainingMilliseconds } from '../lib/time'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const now = ref(Date.now())
const starting = ref(false)
const terminating = ref(false)
const terminationRequested = ref(false)
const refreshing = ref(false)
const booking = computed(() => app.activeBooking!)
const chain = computed(() => app.chainById.get(booking.value.chainId))
const phase = computed(() => bookingPhase(booking.value, now.value))
const remaining = computed(() => remainingMilliseconds(
  bookingPhaseDeadline(booking.value, now.value),
  now.value,
))
const clock = computed(() => formatCountdown(remaining.value))
let nextRefreshAt = 0
const timer = window.setInterval(() => {
  now.value = Date.now()
  if (phase.value === 'expired') void refreshExpiredBooking()
}, 250)
onBeforeUnmount(() => window.clearInterval(timer))

async function refreshExpiredBooking() {
  if (refreshing.value || now.value < nextRefreshAt) return
  refreshing.value = true
  nextRefreshAt = now.value + 2_000
  try { await app.refresh() } catch (error) { app.report(error) } finally { refreshing.value = false }
}

watch(phase, (value) => {
  if (value === 'expired') {
    terminationRequested.value = false
    void refreshExpiredBooking()
  }
}, { immediate: true })

async function start() {
  if (starting.value || terminating.value || terminationRequested.value
    || phase.value !== 'start_window' || !chain.value) return
  starting.value = true
  try { await app.startFocus(chain.value, booking.value.id) } catch (error) { app.report(error) } finally { starting.value = false }
}

function requestTermination() {
  if (starting.value || terminating.value || phase.value === 'expired') return
  terminationRequested.value = true
}

async function terminate() {
  if (terminating.value || !terminationRequested.value || phase.value === 'expired' || !app.online) return
  terminating.value = true
  try { await app.terminateBooking(booking.value) } catch { /* store reports API errors */ } finally { terminating.value = false }
}
</script>

<template>
  <div class="focus-layer booking-layer" role="dialog" aria-modal="true" aria-label="预约倒计时">
    <div class="focus-topline">
      <span><CalendarClock :size="15" /> 预约进行中</span>
      <span v-if="!app.online" class="offline-pill">离线 · 计时继续</span>
    </div>
    <div class="focus-center">
      <p class="eyebrow">{{ chain?.auxiliarySignal || '预约已经生效' }}</p>
      <h1>{{ chain?.name || '预约任务' }}</h1>
      <div class="clock" :class="{ ready: phase === 'start_window' }">
        {{ phase === 'expired' ? '等待判定' : clock }}
      </div>
      <p v-if="phase === 'countdown'" class="focus-note">
        到点后有 90 秒时间开始任务。计时不会暂停。
      </p>
      <p v-else-if="phase === 'start_window'" class="focus-note">
        请在时间结束前开始，否则预约将记为失败。
      </p>
      <p v-else class="focus-note">时间已结束，正在更新状态。</p>
      <button
        v-if="phase === 'start_window'"
        class="primary complete-button"
        :disabled="starting || terminating || terminationRequested"
        @click="start"
      >
        <Play :size="18" /> {{ starting ? '启动中…' : '开始任务' }}
      </button>
      <div v-if="phase !== 'expired'" class="judgment-actions">
        <button class="quiet" :disabled="starting || terminating || terminationRequested" @click="requestTermination">
          <X :size="16" /> 终止预约
        </button>
      </div>
    </div>

    <form v-if="terminationRequested" class="judgment-sheet" @submit.prevent="terminate">
      <div class="sheet-title"><AlertTriangle :size="18" /><strong>确认终止预约？</strong></div>
      <p>终止后需要确认本次预约的结果。</p>
      <p v-if="!app.online" class="offline-pill">联网后才能终止预约。</p>
      <div class="form-actions">
        <button type="button" class="quiet" :disabled="terminating" @click="terminationRequested = false">返回倒计时</button>
        <button class="danger" :disabled="terminating || !app.online">{{ terminating ? '终止中…' : '确认终止' }}</button>
      </div>
    </form>
  </div>
</template>
