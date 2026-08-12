<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { CalendarClock, Flame, Undo2 } from '@lucide/vue'
import { commitmentCancellationRemaining } from '../lib/commitment'
import { formatCountdown, remainingMilliseconds } from '../lib/time'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const now = ref(Date.now())
const commitment = computed(() => app.pendingCommitment!)
const chain = computed(() => app.chainById.get(commitment.value.chainId))
const cancellationRemaining = computed(() => commitmentCancellationRemaining(commitment.value, now.value))
const cancellationSeconds = computed(() => Math.ceil(cancellationRemaining.value / 1000))
const isBooking = computed(() => commitment.value.kind === 'booking')
const deadline = computed(() => {
  const seconds = isBooking.value ? chain.value?.auxiliaryDelaySeconds : chain.value?.durationSeconds
  return seconds ? new Date(Date.parse(commitment.value.occurredAt) + seconds * 1000).toISOString() : null
})
const countdownRemaining = computed(() => remainingMilliseconds(deadline.value, now.value))
const clock = computed(() => deadline.value ? formatCountdown(countdownRemaining.value) : '可完成')
const waitingForNetwork = computed(() => isBooking.value && cancellationRemaining.value === 0 && !app.online)
let nextCommitAt = 0
let retryDelay = 2_000

const timer = window.setInterval(() => {
  now.value = Date.now()
  if (cancellationRemaining.value > 0 || waitingForNetwork.value
    || app.pendingCommitmentBusy || now.value < nextCommitAt) return
  nextCommitAt = now.value + retryDelay
  void app.commitPendingCommitment()
    .then(() => { retryDelay = 2_000 })
    .catch(() => {
      nextCommitAt = Date.now() + retryDelay
      retryDelay = Math.min(retryDelay * 2, 30_000)
    })
}, 100)
watch(() => app.online, (online) => {
  if (!online) return
  retryDelay = 2_000
  nextCommitAt = 0
})
onBeforeUnmount(() => window.clearInterval(timer))

async function cancel() {
  try { await app.cancelPendingCommitment() } catch (error) { app.report(error) }
}
</script>

<template>
  <div class="focus-layer" role="dialog" aria-modal="true" aria-label="操作撤销窗口">
    <div class="focus-topline">
      <span v-if="isBooking"><CalendarClock :size="15" /> 预约倒计时</span>
      <span v-else><Flame :size="15" /> 严格专注倒计时</span>
      <span v-if="!app.online" class="offline-pill">离线 · 计时继续</span>
    </div>
    <div class="focus-center">
      <p class="eyebrow">{{ isBooking ? chain?.auxiliarySignal || '预约准备中' : chain?.sacredTrigger || '专注准备中' }}</p>
      <h1>{{ chain?.name || (isBooking ? '预约任务' : '专注任务') }}</h1>
      <div class="clock">{{ clock }}</div>
      <p v-if="cancellationRemaining > 0" class="focus-note">
        {{ cancellationSeconds }} 秒内可以撤销，之后自动生效。
      </p>
      <p v-else-if="waitingForNetwork" class="focus-note">联网后将自动生效。</p>
      <p v-else class="focus-note">正在生效…</p>
      <button
        v-if="cancellationRemaining > 0"
        class="quiet grace-cancel-button"
        :disabled="app.pendingCommitmentBusy"
        @click="cancel"
      >
        <Undo2 :size="17" /> 取消{{ isBooking ? '预约' : '开始' }}（{{ cancellationSeconds }}）
      </button>
    </div>
  </div>
</template>
