<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Hourglass } from '@lucide/vue'
import { useAppStore } from '../stores/app'
import { api } from '../lib/api'
import type { Precedent } from '../types'

const app = useAppStore()
const decision = ref<'failure' | 'allow'>('failure')
const rule = ref('')
const precedentId = ref('')
const precedents = ref<Precedent[]>([])
const busy = ref(false)

onMounted(async () => {
  try {
    const rows = await api<Precedent[]>(`/api/v1/chains/${app.pendingGroupJudgment!.chainId}/precedents`)
    precedents.value = rows.filter((item) => item.scope === 'group_deadline')
  } catch (error) {
    app.report(error)
  }
})

async function submit() {
  if (busy.value || (decision.value === 'allow' && !precedentId.value && !rule.value.trim())) return
  busy.value = true
  try {
    await app.resolveGroup(decision.value, rule.value.trim() || undefined, precedentId.value || undefined)
  } catch (error) { app.report(error) } finally { busy.value = false }
}
</script>

<template>
  <div class="modal-backdrop">
    <form class="confirm-modal" @submit.prevent="submit">
      <div class="sheet-title"><Hourglass :size="19" /><strong>任务群未按时完成</strong></div>
      <h2>{{ app.chainById.get(app.pendingGroupJudgment!.chainId)?.name }}</h2>
      <p>请选择记为失败，或将本次情况设为例外。</p>
      <div class="segmented"><button type="button" :class="{ active: decision === 'failure' }" @click="decision = 'failure'">判定失败</button><button type="button" :class="{ active: decision === 'allow' }" @click="decision = 'allow'">设为例外</button></div>
      <template v-if="decision === 'allow'">
        <label v-if="precedents.length">已有例外规则<select v-model="precedentId"><option value="">新增例外规则</option><option v-for="item in precedents" :key="item.id" :value="item.id">{{ item.rule }}</option></select></label>
        <label v-if="!precedentId">例外规则<input v-model="rule" required placeholder="今后在什么情况下可以例外？" /></label>
      </template>
      <div class="form-actions"><button class="danger" :disabled="busy">确认判定</button></div>
    </form>
  </div>
</template>
