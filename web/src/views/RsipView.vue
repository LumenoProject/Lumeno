<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { Archive, ChevronRight, GitBranch, Plus, ShieldCheck, X } from '@lucide/vue'
import { api, mutation } from '../lib/api'
import { useAppStore } from '../stores/app'
import type { NegativeState, RollbackPreview, RsipNode } from '../types'

const app = useAppStore()
const negatives = ref<NegativeState[]>([])
const nodes = ref<RsipNode[]>([])
const wizard = ref(false)
const step = ref(1)
const saving = ref(false)
const failure = ref<RsipNode | null>(null)
const failurePreview = ref<RollbackPreview | null>(null)
const failureReason = ref('')
const form = reactive({
  negativeStateId: '', negativeTitle: '', description: '', cues: '', consequences: '',
  traceback: ['', ''], intervention: 1, title: '', triggerCondition: '', requiredAction: '', intendedPrevention: '', parentId: '',
})
const activeNodes = computed(() => {
  const active = nodes.value.filter((node) => node.state === 'active')
  const activeIds = new Set(active.map((node) => node.id))
  const children = new Map<string | null, RsipNode[]>()
  for (const node of active) {
    const parent = node.parentId && activeIds.has(node.parentId) ? node.parentId : null
    const siblings = children.get(parent) ?? []
    siblings.push(node)
    children.set(parent, siblings)
  }
  const ordered: RsipNode[] = []
  const visit = (parent: string | null) => {
    for (const node of children.get(parent) ?? []) {
      ordered.push(node)
      visit(node.id)
    }
  }
  visit(null)
  return ordered
})
const draftNodes = computed(() => nodes.value.filter((node) => node.state === 'draft'))
const archivedNodes = computed(() => nodes.value.filter((node) => node.state === 'archived'))
const activeDepths = computed(() => {
  const byId = new Map(activeNodes.value.map((node) => [node.id, node]))
  const depths = new Map<string, number>()
  for (const node of activeNodes.value) {
    let depth = 0
    let parentId = node.parentId
    const seen = new Set<string>()
    while (parentId && depth < 1000 && !seen.has(parentId)) {
      seen.add(parentId)
      const parent = byId.get(parentId)
      if (!parent) break
      depth += 1
      parentId = parent.parentId
    }
    depths.set(node.id, Math.min(depth, 8))
  }
  return depths
})

async function load() {
  try { [negatives.value, nodes.value] = await Promise.all([api<NegativeState[]>('/api/v1/rsip/negative-states'), api<RsipNode[]>('/api/v1/rsip/nodes')]) } catch (error) { app.report(error) }
}
onMounted(load)

function closeWizard() { wizard.value = false; step.value = 1 }
function addTrace() { form.traceback.push(''); form.intervention = form.traceback.length - 1 }

async function saveDraft() {
  saving.value = true
  try {
    let negativeStateId = form.negativeStateId
    if (!negativeStateId) {
      const negative = await mutation<NegativeState>('/api/v1/rsip/negative-states', 'POST', {
        title: form.negativeTitle, description: form.description, cues: form.cues, consequences: form.consequences,
      })
      negativeStateId = negative.id
    }
    await mutation('/api/v1/rsip/nodes', 'POST', {
      negativeStateId,
      parentId: form.parentId || null,
      title: form.title,
      triggerCondition: form.triggerCondition,
      requiredAction: form.requiredAction,
      intendedPrevention: form.intendedPrevention,
      steps: form.traceback.map((decision, index) => ({ decision, isInterventionPoint: index === form.intervention })),
    })
    closeWizard(); await load()
  } catch (error) { app.report(error) } finally { saving.value = false }
}

async function activate(node: RsipNode) {
  try { await mutation(`/api/v1/rsip/nodes/${node.id}/activate`, 'POST', { version: node.version }); await load() } catch (error) { app.report(error) }
}

async function openFailure(node: RsipNode) {
  try {
    failurePreview.value = await api<RollbackPreview>(`/api/v1/rsip/nodes/${node.id}/rollback-preview`)
    failure.value = node
  } catch (error) {
    app.report(error)
  }
}

async function fail() {
  if (!failure.value) return
  try { await mutation(`/api/v1/rsip/nodes/${failure.value.id}/fail`, 'POST', { version: failure.value.version, reason: failureReason.value }); failure.value = null; failurePreview.value = null; failureReason.value = ''; await load() } catch (error) { app.report(error) }
}
</script>

<template>
  <div class="page">
    <header class="page-header"><div><p class="eyebrow">RSIP</p><h1>定式</h1><p>回顾问题，为下一次制定行动。</p></div><button class="primary" @click="wizard = true"><Plus :size="16" /> 新建定式</button></header>
    <section class="principle-note"><strong>每天可激活 1 条</strong><span>草稿数量不限。</span></section>

    <section class="section-block">
      <div class="section-heading"><div><p class="eyebrow">ACTIVE TREE</p><h2>正在生效</h2></div></div>
      <div v-if="activeNodes.length" class="rsip-tree">
        <article v-for="node in activeNodes" :key="node.id" class="rsip-node" :style="{ marginLeft: `${(activeDepths.get(node.id) ?? 0) * 2}rem` }">
          <ShieldCheck :size="20" /><div><h3>{{ node.title }}</h3><p><strong>触发：</strong>{{ node.triggerCondition }}</p><p><strong>动作：</strong>{{ node.requiredAction }}</p></div><button class="danger ghost" @click="openFailure(node)">报告失效</button>
        </article>
      </div>
      <div v-else class="empty-inline">还没有生效的定式。</div>
    </section>

    <section v-if="draftNodes.length" class="section-block">
      <div class="section-heading"><div><p class="eyebrow">DRAFTS</p><h2>待激活草稿</h2></div></div>
      <article v-for="node in draftNodes" :key="node.id" class="draft-row"><GitBranch :size="18" /><div><strong>{{ node.title }}</strong><p>{{ node.requiredAction }}</p></div><button class="secondary" @click="activate(node)">今日激活 <ChevronRight :size="15" /></button></article>
    </section>

    <details v-if="archivedNodes.length" class="archive-section"><summary><Archive :size="16" /> 已归档 {{ archivedNodes.length }} 项</summary><div v-for="node in archivedNodes" :key="node.id" class="archived-row"><span>{{ node.title }}</span><small>{{ node.failureReason || '已归档' }}</small></div></details>

    <div v-if="wizard" class="modal-backdrop">
      <form class="editor-modal wizard" @submit.prevent="step < 3 ? step++ : saveDraft()">
        <header><div><p class="eyebrow">GUIDED REVIEW · {{ step }}/3</p><h2>{{ ['描述问题', '回顾经过', '制定行动'][step - 1] }}</h2></div><button type="button" class="icon-button" @click="closeWizard"><X :size="18" /></button></header>
        <template v-if="step === 1">
          <label v-if="negatives.length">选择已有问题<select v-model="form.negativeStateId"><option value="">创建新问题</option><option v-for="negative in negatives" :key="negative.id" :value="negative.id">{{ negative.title }}</option></select></label>
          <template v-if="!form.negativeStateId">
            <label>问题名称<input v-model="form.negativeTitle" required placeholder="例如：深夜无目的刷屏" /></label>
            <label>发生了什么<textarea v-model="form.description" required placeholder="简单描述实际情况"></textarea></label>
            <label>出现前的信号<textarea v-model="form.cues" placeholder="当时的时间、地点、情绪或动作"></textarea></label>
            <label>带来的影响<textarea v-model="form.consequences" placeholder="这件事造成了什么影响？"></textarea></label>
          </template>
        </template>
        <template v-else-if="step === 2">
          <p class="form-hint">从结果开始向前回想，并选出最值得改变的一步。</p>
          <label v-for="(_, index) in form.traceback" :key="index" class="trace-step"><input v-model="form.intervention" type="radio" :value="index" /><span>{{ index + 1 }}</span><input v-model="form.traceback[index]" required :placeholder="index === 0 ? '结果发生前，我做了…' : '再往前一步，我做了…'" /></label>
          <button type="button" class="text-link" @click="addTrace"><Plus :size="15" /> 再向前回溯一步</button>
        </template>
        <template v-else>
          <label>定式名称<input v-model="form.title" required placeholder="例如：手机停泊定式" /></label>
          <label>父定式<select v-model="form.parentId"><option value="">无（根定式）</option><option v-for="node in activeNodes" :key="node.id" :value="node.id">{{ node.title }}</option></select></label>
          <label>触发条件<textarea v-model="form.triggerCondition" required placeholder="当最后一个可干预信号出现时…"></textarea></label>
          <label>必要动作<textarea v-model="form.requiredAction" required placeholder="我立即执行一个足够小且可验证的动作…"></textarea></label>
          <label>它阻止什么<textarea v-model="form.intendedPrevention" placeholder="说明动作与负面结果之间的因果关系"></textarea></label>
        </template>
        <footer><button v-if="step > 1" type="button" class="quiet" @click="step--">上一步</button><span v-else></span><button class="primary" :disabled="saving">{{ step < 3 ? '继续' : saving ? '保存中…' : '保存为草稿' }}</button></footer>
      </form>
    </div>

    <div v-if="failure" class="modal-backdrop" @click.self="failure = null">
      <form class="confirm-modal" @submit.prevent="fail"><h2>归档相关定式？</h2><p>“{{ failure.title }}”及其下级定式都会归档，共 <strong>{{ failurePreview?.affectedCount ?? 1 }}</strong> 项。</p><label>失效原因<textarea v-model="failureReason" required autofocus></textarea></label><div class="form-actions"><button type="button" class="quiet" @click="failure = null; failurePreview = null">取消</button><button class="danger">确认归档</button></div></form>
    </div>
  </div>
</template>
