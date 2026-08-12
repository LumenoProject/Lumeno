<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { Archive, Plus, X } from '@lucide/vue'
import ChainTree from '../components/ChainTree.vue'
import { useAppStore } from '../stores/app'
import type { ChainKind, UnitKind } from '../types'

const app = useAppStore()
const showForm = ref(false)
const saving = ref(false)
const form = reactive({
  kind: 'unit' as ChainKind,
  unitKind: 'basic' as UnitKind,
  parentId: '', name: '', description: '', sacredTrigger: '', durationMinutes: 25,
  auxiliarySignal: '', auxiliaryDelayMinutes: 10, auxiliaryCompletionTrigger: '', groupLimitMinutes: 120,
})
const roots = computed(() => app.chains.filter((chain) => !chain.parentId))
const groups = computed(() => app.chains.filter((chain) => chain.kind === 'group'))

function reset() { form.name = ''; form.description = ''; form.sacredTrigger = ''; form.auxiliarySignal = ''; form.auxiliaryCompletionTrigger = ''; showForm.value = false }

async function submit() {
  saving.value = true
  const unit = form.kind === 'unit'
  try {
    await app.createChain({
      kind: form.kind,
      unitKind: unit ? form.unitKind : null,
      parentId: form.parentId || null,
      name: form.name,
      description: form.description,
      position: app.chains.length,
      sacredTrigger: unit ? form.sacredTrigger : null,
      durationSeconds: unit && form.durationMinutes ? form.durationMinutes * 60 : null,
      auxiliarySignal: unit ? form.auxiliarySignal : null,
      auxiliaryDelaySeconds: unit ? form.auxiliaryDelayMinutes * 60 : null,
      auxiliaryCompletionTrigger: unit ? form.auxiliaryCompletionTrigger : null,
      groupTimeLimitSeconds: unit ? null : form.groupLimitMinutes * 60,
    })
    reset()
  } catch { /* store reports */ } finally { saving.value = false }
}
</script>

<template>
  <div class="page">
    <header class="page-header"><div><p class="eyebrow">CTDP</p><h1>契约链</h1><p>管理正在使用的契约。</p></div><div class="page-actions"><RouterLink to="/chains/archive" class="secondary"><Archive :size="16" /> 查看归档</RouterLink><button class="primary" @click="showForm = true"><Plus :size="16" /> 新建契约</button></div></header>
    <section class="principle-note"><strong>严格模式</strong><span>预约需按时开始；修改关键设置会重置连续记录。</span></section>

    <div class="tree-list">
      <section v-for="root in roots" :key="root.id" class="tree-group"><ChainTree :chain="root" :chains="app.chains" /></section>
    </div>

    <div v-if="showForm" class="modal-backdrop" @click.self="reset">
      <form class="editor-modal" @submit.prevent="submit">
        <header><div><p class="eyebrow">NEW CONTRACT</p><h2>写下可执行的承诺</h2></div><button type="button" class="icon-button" @click="reset"><X :size="18" /></button></header>
        <div class="segmented"><button type="button" :class="{ active: form.kind === 'unit' }" @click="form.kind = 'unit'">叶子任务</button><button type="button" :class="{ active: form.kind === 'group' }" @click="form.kind = 'group'">任务群</button></div>
        <label>名称<input v-model="form.name" required maxlength="120" placeholder="例如：晨间深度写作" /></label>
        <label>归属任务群<select v-model="form.parentId"><option value="">顶层</option><option v-for="group in groups" :key="group.id" :value="group.id">{{ group.name }}</option></select></label>
        <label>说明<textarea v-model="form.description" placeholder="为什么值得守住这条链？"></textarea></label>
        <template v-if="form.kind === 'unit'">
          <label>单元类型<select v-model="form.unitKind"><option value="basic">基础</option><option value="assault">突击</option><option value="recon">侦察</option><option value="command">指挥</option><option value="special_ops">特种</option><option value="engineering">工程</option><option value="quartermaster">后勤</option></select></label>
          <label>启动条件<textarea v-model="form.sacredTrigger" required placeholder="当我坐到书桌前并打开文档时…"></textarea></label>
          <div class="two-cols"><label>专注分钟<input v-model.number="form.durationMinutes" type="number" min="1" max="1440" /></label><label>预约响应分钟<input v-model.number="form.auxiliaryDelayMinutes" type="number" min="1" max="1440" /></label></div>
          <label>预约信号<input v-model="form.auxiliarySignal" required placeholder="我明确说：十分钟后开始" /></label>
          <label>预约完成条件<input v-model="form.auxiliaryCompletionTrigger" required placeholder="在截止前启动这条链" /></label>
        </template>
        <label v-else>完成时限（分钟）<input v-model.number="form.groupLimitMinutes" type="number" min="1" max="10080" /></label>
        <footer><button type="button" class="quiet" @click="reset">取消</button><button class="primary" :disabled="saving">{{ saving ? '保存中…' : '建立契约' }}</button></footer>
      </form>
    </div>
  </div>
</template>
