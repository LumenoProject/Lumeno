<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { CalendarX2 } from '@lucide/vue'
import { api } from '../lib/api'
import { formatCountdown, remainingMilliseconds } from '../lib/time'
import { useAppStore } from '../stores/app'
import type { Precedent } from '../types'

const app = useAppStore()
const decision = ref<'failure' | 'allow'>('failure')
const precedentId = ref('')
const rule = ref('')
const precedents = ref<Precedent[]>([])
const busy = ref(false)
const booking = computed(() => app.pendingBookingJudgment!)
const now = ref(Date.now())
const remaining = computed(() => remainingMilliseconds(booking.value.confirmationExpiresAt, now.value))
const confirmationExpired = computed(() => remaining.value === 0)
const confirmationClock = computed(() => formatCountdown(remaining.value))
let nextRefreshAt = 0
const timer = window.setInterval(() => {
  now.value = Date.now()
  if (remaining.value === 0 && now.value >= nextRefreshAt) {
    nextRefreshAt = now.value + 2_000
    void app.refresh().catch((error) => {
      app.report(error)
    })
  }
}, 1000)
onBeforeUnmount(() => window.clearInterval(timer))

watch(() => booking.value.chainId, async (chainId) => {
  try {
    const rows = await api<Precedent[]>(`/api/v1/chains/${chainId}/precedents`)
    precedents.value = rows.filter((item) => item.scope === 'auxiliary')
    precedentId.value = ''
    rule.value = ''
    decision.value = 'failure'
  } catch (error) {
    app.report(error)
  }
}, { immediate: true })

async function submit() {
  if (busy.value || confirmationExpired.value
    || (decision.value === 'allow' && !precedentId.value && !rule.value.trim())) return
  busy.value = true
  try {
    await app.resolveBooking(
      booking.value,
      decision.value,
      rule.value.trim() || undefined,
      precedentId.value || undefined,
    )
  } catch (error) {
    app.report(error)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="modal-backdrop">
    <form class="confirm-modal" @submit.prevent="submit">
      <div class="sheet-title"><CalendarX2 :size="19" /><strong>预约未完成</strong></div>
      <h2>{{ app.chainById.get(booking.chainId)?.name }}</h2>
      <p>请在 <strong>{{ confirmationClock }}</strong> 内选择结果。超时未确认将记为失败。</p>
      <div class="segmented">
        <button type="button" :class="{ active: decision === 'failure' }" @click="decision = 'failure'">判定失败</button>
        <button type="button" :class="{ active: decision === 'allow' }" @click="decision = 'allow'">设为例外</button>
      </div>
      <template v-if="decision === 'allow'">
        <label v-if="precedents.length">已有例外规则
          <select v-model="precedentId"><option value="">新增例外规则</option><option v-for="item in precedents" :key="item.id" :value="item.id">{{ item.rule }}</option></select>
        </label>
        <label v-if="!precedentId">例外规则<input v-model="rule" required placeholder="今后在什么条件下可以例外？" /></label>
      </template>
      <div class="form-actions"><button class="danger" :disabled="busy || confirmationExpired">{{ confirmationExpired ? '确认已超时' : '确认判定' }}</button></div>
    </form>
  </div>
</template>
