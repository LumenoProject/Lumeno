<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { AlertTriangle, Check, Flame, ShieldAlert, X } from '@lucide/vue'
import { useAppStore } from '../stores/app'
import { formatCountdown, remainingMilliseconds } from '../lib/time'
import { api } from '../lib/api'
import type { Precedent } from '../types'

const app = useAppStore()
const now = ref(Date.now())
const pendingAction = app.activeFocus?.pendingAction
const judgment = ref<'interrupt' | 'early_end' | null>(
  pendingAction === 'interrupt' || pendingAction === 'early_end' ? pendingAction : null,
)
const reason = ref(app.activeFocus?.pendingReason ?? '')
const decision = ref<'failure' | 'allow'>('failure')
const rule = ref('')
const precedentId = ref('')
const precedents = ref<Precedent[]>([])
const submitting = ref(false)
const timer = window.setInterval(() => { now.value = Date.now() }, 250)
onBeforeUnmount(() => window.clearInterval(timer))

const focus = computed(() => app.activeFocus!)
const chain = computed(() => app.chainById.get(focus.value.chainId))
const remaining = computed(() => remainingMilliseconds(focus.value.completionAvailableAt, now.value))
const ready = computed(() => !focus.value.completionAvailableAt || remaining.value === 0)
const clock = computed(() => formatCountdown(remaining.value))

watch(judgment, async (value) => {
  if (!value || precedents.value.length || !chain.value) return
  try {
    const rows = await api<Precedent[]>(`/api/v1/chains/${chain.value.id}/precedents`)
    precedents.value = rows.filter((item) => item.scope === 'main')
  } catch (error) {
    app.report(error)
  }
})

async function complete() {
  if (submitting.value || judgment.value) return
  submitting.value = true
  try { await app.completeFocus() } catch (error) { app.report(error) } finally { submitting.value = false }
}

async function submitJudgment() {
  if (submitting.value || !judgment.value || !reason.value.trim()
    || (decision.value === 'allow' && !precedentId.value && !rule.value.trim())) return
  submitting.value = true
  try {
    await app.resolveFocus(
      judgment.value,
      reason.value.trim(),
      decision.value,
      rule.value.trim() || undefined,
      precedentId.value || undefined,
    )
    judgment.value = null
  } catch (error) {
    app.report(error)
  } finally { submitting.value = false }
}
</script>

<template>
  <div class="focus-layer" role="dialog" aria-modal="true" aria-label="严格专注会话">
    <div class="focus-topline">
      <span><Flame :size="15" /> 专注进行中</span>
      <span v-if="focus.localOnly" class="offline-pill">离线记录 · 待同步</span>
    </div>
    <div class="focus-center">
      <p class="eyebrow">{{ chain?.sacredTrigger || '自由专注' }}</p>
      <h1>{{ chain?.name || '专注任务' }}</h1>
      <div class="clock" :class="{ ready }">{{ ready ? '可完成' : clock }}</div>
      <p class="focus-note">计时不会暂停。</p>
      <button class="primary complete-button" :disabled="!ready || submitting || judgment !== null" @click="complete">
        <Check :size="18" /> 确认完成
      </button>
      <div class="judgment-actions">
        <button class="quiet" :disabled="submitting || judgment !== null" @click="judgment = 'interrupt'"><ShieldAlert :size="16" /> 报告中断</button>
        <button class="quiet" :disabled="submitting || judgment !== null" @click="judgment = 'early_end'"><X :size="16" /> 提前结束</button>
      </div>
    </div>

    <form v-if="judgment" class="judgment-sheet" @submit.prevent="submitJudgment">
      <div class="sheet-title"><AlertTriangle :size="18" /><strong>确认本次结果</strong></div>
      <p>记录原因并选择结果。提前结束若设为例外，本次计为完成。</p>
      <label>发生了什么<textarea v-model="reason" required autofocus placeholder="简单描述实际情况"></textarea></label>
      <div class="segmented">
        <button type="button" :class="{ active: decision === 'failure' }" @click="decision = 'failure'">判定失败</button>
        <button type="button" :class="{ active: decision === 'allow' }" @click="decision = 'allow'">设为例外</button>
      </div>
      <template v-if="decision === 'allow'">
        <label v-if="precedents.length">已有例外规则<select v-model="precedentId"><option value="">新增例外规则</option><option v-for="item in precedents" :key="item.id" :value="item.id">{{ item.rule }}</option></select></label>
        <label v-if="!precedentId">例外规则<input v-model="rule" required placeholder="今后在什么条件下可以例外？" /></label>
      </template>
      <div class="form-actions"><button type="button" class="quiet" @click="judgment = null">返回计时</button><button class="danger" :disabled="submitting">确认判定</button></div>
    </form>
  </div>
</template>
