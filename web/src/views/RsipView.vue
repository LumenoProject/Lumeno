<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  Archive, ChevronRight, FlaskConical, GitBranch, History, Layers3, Pencil,
  Plus, ShieldCheck, X,
} from '@lucide/vue'
import { api, mutation } from '../lib/api'
import {
  activeMemberCount, buildRsipTreeRows, completeActiveDays, groupState,
  type RsipGroupState,
} from '../lib/rsip'
import { useAppStore } from '../stores/app'
import type {
  NegativeState, RollbackPreview, RsipGroup, RsipHistoryPage, RsipOverview,
  RsipPolicy, RsipPolicyMode, RsipTreeNode,
} from '../types'

const app = useAppStore()
const negatives = ref<NegativeState[]>([])
const overview = ref<RsipOverview>({
  localDate: '', canDeployToday: true, policies: [], trials: [], groups: [], nodes: [], memberships: [],
})
const history = ref<RsipTreeNode[]>([])
const historyCursor = ref<string | null>(null)
const loadingHistory = ref(false)
const saving = ref(false)

const wizard = ref(false)
const wizardStep = ref(1)
const trialTarget = ref<RsipPolicy | null>(null)
const trialNote = ref('')
const deployTarget = ref<RsipPolicy | null>(null)
const deploymentTarget = ref('root')
const policyEditor = ref<RsipPolicy | null>(null)
const groupEditor = ref<RsipGroup | 'new' | null>(null)
const groupDeployment = ref<RsipGroup | null>(null)
const groupParentTarget = ref('root')
const failure = ref<RsipTreeNode | null>(null)
const failurePreview = ref<RollbackPreview | null>(null)
const failureReason = ref('')

const form = reactive({
  negativeStateId: '', negativeTitle: '', description: '', cues: '', consequences: '',
  traceback: ['', ''], intervention: 1, mode: 'active' as RsipPolicyMode, title: '',
  triggerCondition: '', requiredAction: '', intendedPrevention: '',
})
const editForm = reactive({ mode: 'active' as RsipPolicyMode, title: '', triggerCondition: '', requiredAction: '', intendedPrevention: '' })
const groupForm = reactive({ name: '', targetMemberCount: 3, minimumActiveMembers: 2 })

const activePolicies = computed(() => overview.value.policies.filter((policy) => !policy.archivedAt))
const archivedPolicies = computed(() => overview.value.policies.filter((policy) => policy.archivedAt))
const activeGroups = computed(() => overview.value.groups.filter((group) => !group.archivedAt))
const archivedGroups = computed(() => overview.value.groups.filter((group) => group.archivedAt))
const treeRows = computed(() => buildRsipTreeRows(overview.value))
const memberNodeIds = computed(() => new Set(overview.value.memberships.map((item) => item.policyNodeId)))
const policyById = computed(() => new Map(overview.value.policies.map((policy) => [policy.id, policy])))
const groupById = computed(() => new Map(overview.value.groups.map((group) => [group.id, group])))
const activePolicyIds = computed(() => new Set(overview.value.nodes.flatMap((node) => node.policyId ? [node.policyId] : [])))
const activeGroupIds = computed(() => new Set(overview.value.nodes.flatMap((node) => node.groupId ? [node.groupId] : [])))
const trialByPolicy = computed(() => new Map(overview.value.trials
  .filter((trial) => {
    const policy = policyById.value.get(trial.policyId)
    return !trial.consumedNodeId && policy?.revisionId === trial.policyRevisionId
  })
  .map((trial) => [trial.policyId, trial])))
const triedPolicyIds = computed(() => new Set(overview.value.trials.map((trial) => trial.policyId)))
const eligibleParents = computed(() => overview.value.nodes.filter((node) => {
  if (memberNodeIds.value.has(node.id)) return false
  if (node.kind === 'policy') return true
  const group = node.groupId ? groupById.value.get(node.groupId) : undefined
  return group ? groupState(overview.value, node, group) === 'effective' : false
}))
const eligibleGroups = computed(() => overview.value.nodes.filter((node) => {
  if (node.kind !== 'group' || !node.groupId) return false
  const group = groupById.value.get(node.groupId)
  return group ? activeMemberCount(overview.value, node.id) < group.targetMemberCount : false
}))
const qualityNotes = computed(() => {
  const notes: string[] = []
  if (form.title.trim().length < 4) notes.push('名称可以再具体一些，方便在树上快速辨认。')
  if (form.triggerCondition.trim().length < 10) notes.push('触发条件最好包含可观察的时间、地点或动作信号。')
  if (form.requiredAction.trim().length < 8) notes.push('必要动作最好小到可以立即执行，并能判断是否完成。')
  if (!form.intendedPrevention.trim()) notes.push('补充它要阻止的结果，有助于以后复盘因果关系。')
  return notes
})

async function load() {
  try {
    [negatives.value, overview.value] = await Promise.all([
      api<NegativeState[]>('/api/v1/rsip/negative-states'),
      api<RsipOverview>('/api/v1/rsip/overview'),
    ])
    history.value = []
    historyCursor.value = null
    await loadHistory()
  } catch (error) {
    app.report(error)
  }
}

async function loadHistory() {
  if (loadingHistory.value || (history.value.length && !historyCursor.value)) return
  loadingHistory.value = true
  try {
    const cursor = historyCursor.value ? `?cursor=${encodeURIComponent(historyCursor.value)}` : ''
    const page = await api<RsipHistoryPage>(`/api/v1/rsip/history${cursor}`)
    history.value.push(...page.items)
    historyCursor.value = page.nextCursor ?? null
  } catch (error) {
    app.report(error)
  } finally {
    loadingHistory.value = false
  }
}

onMounted(load)

function resetWizard() {
  Object.assign(form, {
    negativeStateId: '', negativeTitle: '', description: '', cues: '', consequences: '',
    traceback: ['', ''], intervention: 1, mode: 'active', title: '', triggerCondition: '',
    requiredAction: '', intendedPrevention: '',
  })
  wizardStep.value = 1
}

function closeWizard() {
  wizard.value = false
  resetWizard()
}

function addTrace() {
  if (form.traceback.length >= 100) return
  form.traceback.push('')
  form.intervention = form.traceback.length - 1
}

async function savePolicy() {
  saving.value = true
  try {
    let negativeStateId = form.negativeStateId
    if (!negativeStateId) {
      const negative = await mutation<NegativeState>('/api/v1/rsip/negative-states', 'POST', {
        title: form.negativeTitle, description: form.description, cues: form.cues, consequences: form.consequences,
      })
      negativeStateId = negative.id
    }
    await mutation('/api/v1/rsip/policies', 'POST', {
      negativeStateId,
      mode: form.mode,
      title: form.title,
      triggerCondition: form.triggerCondition,
      requiredAction: form.requiredAction,
      intendedPrevention: form.intendedPrevention,
      steps: form.traceback.map((decision, index) => ({ decision, isInterventionPoint: index === form.intervention })),
    })
    closeWizard()
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

async function recordTrial() {
  if (!trialTarget.value) return
  saving.value = true
  try {
    await mutation(`/api/v1/rsip/policies/${trialTarget.value.id}/trials`, 'POST', { note: trialNote.value })
    trialTarget.value = null
    trialNote.value = ''
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

function openDeployment(policy: RsipPolicy) {
  deploymentTarget.value = 'root'
  deployTarget.value = policy
}

async function deployPolicy() {
  const policy = deployTarget.value
  const trial = policy ? trialByPolicy.value.get(policy.id) : undefined
  if (!policy || !trial) return
  const [kind, id] = deploymentTarget.value.split(':')
  saving.value = true
  try {
    await mutation(`/api/v1/rsip/policies/${policy.id}/deploy`, 'POST', {
      trialId: trial.id,
      parentNodeId: kind === 'parent' ? id : null,
      groupNodeId: kind === 'group' ? id : null,
    })
    deployTarget.value = null
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

function openPolicyEditor(policy: RsipPolicy) {
  policyEditor.value = policy
  Object.assign(editForm, {
    mode: policy.mode, title: policy.title, triggerCondition: policy.triggerCondition,
    requiredAction: policy.requiredAction, intendedPrevention: policy.intendedPrevention,
  })
}

async function updatePolicy() {
  if (!policyEditor.value) return
  saving.value = true
  try {
    await mutation(`/api/v1/rsip/policies/${policyEditor.value.id}`, 'PUT', {
      version: policyEditor.value.version, ...editForm,
    })
    policyEditor.value = null
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

async function archivePolicy(policy: RsipPolicy) {
  if (!window.confirm(`归档“${policy.title}”？历史部署仍会保留。`)) return
  try {
    await mutation(`/api/v1/rsip/policies/${policy.id}`, 'DELETE')
    await load()
  } catch (error) {
    app.report(error)
  }
}

function openGroupEditor(group: RsipGroup | 'new') {
  groupEditor.value = group
  Object.assign(groupForm, group === 'new'
    ? { name: '', targetMemberCount: 3, minimumActiveMembers: 2 }
    : { name: group.name, targetMemberCount: group.targetMemberCount, minimumActiveMembers: group.minimumActiveMembers })
}

async function saveGroup() {
  if (!groupEditor.value) return
  saving.value = true
  try {
    if (groupEditor.value === 'new') {
      await mutation('/api/v1/rsip/groups', 'POST', groupForm)
    } else {
      await mutation(`/api/v1/rsip/groups/${groupEditor.value.id}`, 'PUT', {
        version: groupEditor.value.version, ...groupForm,
      })
    }
    groupEditor.value = null
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

function openGroupDeployment(group: RsipGroup) {
  groupParentTarget.value = 'root'
  groupDeployment.value = group
}

async function deployGroup() {
  if (!groupDeployment.value) return
  const [, parentId] = groupParentTarget.value.split(':')
  saving.value = true
  try {
    await mutation(`/api/v1/rsip/groups/${groupDeployment.value.id}/deploy`, 'POST', {
      parentNodeId: groupParentTarget.value.startsWith('parent:') ? parentId : null,
    })
    groupDeployment.value = null
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

async function archiveGroup(group: RsipGroup) {
  if (!window.confirm(`归档国策组“${group.name}”？`)) return
  try {
    await mutation(`/api/v1/rsip/groups/${group.id}`, 'DELETE')
    await load()
  } catch (error) {
    app.report(error)
  }
}

async function openFailure(node: RsipTreeNode) {
  try {
    failurePreview.value = await api<RollbackPreview>(`/api/v1/rsip/deployments/${node.id}/rollback-preview`)
    failure.value = node
  } catch (error) {
    app.report(error)
  }
}

async function failDeployment() {
  if (!failure.value) return
  saving.value = true
  try {
    await mutation(`/api/v1/rsip/deployments/${failure.value.id}/fail`, 'POST', {
      version: failure.value.version, reason: failureReason.value,
    })
    failure.value = null
    failurePreview.value = null
    failureReason.value = ''
    await load()
  } catch (error) {
    app.report(error)
  } finally {
    saving.value = false
  }
}

function groupHasHistory(group: RsipGroup) {
  return overview.value.nodes.some((node) => node.groupId === group.id) || history.value.some((node) => node.groupId === group.id)
}

function nodeGroupState(node: RsipTreeNode): RsipGroupState | null {
  const group = node.groupId ? groupById.value.get(node.groupId) : undefined
  return group ? groupState(overview.value, node, group) : null
}

function modeLabel(mode?: RsipPolicyMode | null) {
  return ({ active: '主动', passive: '被动', anchored: '半被动' } as const)[mode ?? 'active']
}

function groupStateLabel(state: RsipGroupState | null) {
  return state ? ({ forming: '形成中', effective: '已点亮', degraded: '已降级' } as const)[state] : ''
}

function formatDate(value?: string | null) {
  return value ? new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium' }).format(new Date(value)) : '—'
}
</script>

<template>
  <div class="page rsip-page">
    <header class="page-header">
      <div><p class="eyebrow">RSIP</p><h1>定式</h1><p>先试行，再把有效动作部署为可以逐轮重建的结构。</p></div>
      <div class="page-actions"><button class="secondary" @click="openGroupEditor('new')"><Layers3 :size="16" /> 新建国策组</button><button class="primary" @click="wizard = true"><Plus :size="16" /> 新建国策</button></div>
    </header>
    <section class="principle-note">
      <strong>{{ overview.canDeployToday ? '今天还可以点亮 1 条国策' : '今天的点亮名额已使用' }}</strong>
      <span>国策必须先完成当天试行；组部署不占用点亮名额。</span>
    </section>

    <section class="section-block">
      <div class="section-heading"><div><p class="eyebrow">ACTIVE TREE</p><h2>当前部署</h2></div><span>{{ overview.nodes.length }} 个活动节点</span></div>
      <div v-if="treeRows.length" class="rsip-tree">
        <article
          v-for="row in treeRows" :key="row.node.id" class="rsip-node"
          :class="[`kind-${row.node.kind}`, `relation-${row.relation}`, nodeGroupState(row.node) ? `group-${nodeGroupState(row.node)}` : '']"
          :style="{ marginLeft: `${row.depth * 2}rem` }"
        >
          <component :is="row.node.kind === 'group' ? Layers3 : ShieldCheck" :size="20" />
          <div>
            <div class="rsip-title-line"><h3>{{ row.node.title }}</h3><span v-if="row.relation === 'member'">组成员</span><span v-else-if="row.node.kind === 'group'">{{ groupStateLabel(nodeGroupState(row.node)) }}</span><span v-else>{{ modeLabel(row.node.policyMode) }}</span></div>
            <template v-if="row.node.kind === 'policy'"><p><strong>触发：</strong>{{ row.node.triggerCondition }}</p><p><strong>动作：</strong>{{ row.node.requiredAction }}</p></template>
            <p v-else-if="row.node.groupId && groupById.get(row.node.groupId)">
              {{ activeMemberCount(overview, row.node.id) }} / {{ groupById.get(row.node.groupId)?.targetMemberCount }} 个成员，最低点亮 {{ groupById.get(row.node.groupId)?.minimumActiveMembers }} 个
            </p>
          </div>
          <button class="danger ghost" @click="openFailure(row.node)">报告失效</button>
        </article>
      </div>
      <div v-else class="empty-inline">还没有活动部署。先在国策库中记录一次成功试行。</div>
    </section>

    <section class="section-block">
      <div class="section-heading"><div><p class="eyebrow">POLICY LIBRARY</p><h2>国策库</h2></div><span>定义与部署彼此独立</span></div>
      <div v-if="activePolicies.length" class="rsip-library">
        <article v-for="policy in activePolicies" :key="policy.id" class="policy-card">
          <div class="policy-card-main">
            <div class="rsip-title-line"><h3>{{ policy.title }}</h3><span>{{ modeLabel(policy.mode) }} · 修订 {{ policy.revision }}</span></div>
            <p>{{ policy.requiredAction }}</p>
            <div class="chain-meta"><span>累计存活 {{ completeActiveDays(policy.totalActiveSeconds) }} 个完整自然日</span><span>部署 {{ policy.deploymentCount }} 次</span><span v-if="activePolicyIds.has(policy.id)" class="accent">当前生效</span></div>
          </div>
          <div class="policy-actions">
            <button class="quiet" title="编辑" @click="openPolicyEditor(policy)"><Pencil :size="15" /></button>
            <button
              v-if="trialByPolicy.has(policy.id) && !activePolicyIds.has(policy.id)" class="primary"
              :disabled="!overview.canDeployToday" @click="openDeployment(policy)"
            >今日点亮 <ChevronRight :size="15" /></button>
            <button v-else-if="!activePolicyIds.has(policy.id) && !triedPolicyIds.has(policy.id)" class="secondary" @click="trialTarget = policy"><FlaskConical :size="15" /> 记录试行成功</button>
            <button v-else-if="!activePolicyIds.has(policy.id)" class="secondary" disabled>当前修订今天不可再试行</button>
            <button class="quiet danger" title="归档" :disabled="activePolicyIds.has(policy.id)" @click="archivePolicy(policy)"><Archive :size="15" /></button>
          </div>
        </article>
      </div>
      <div v-else class="empty-inline">国策库还是空的。</div>
      <p class="habit-note">“累计存活天数”只按实际生效秒数折算完整 24 小时，不代表习惯强度评分。</p>
    </section>

    <section class="section-block">
      <div class="section-heading"><div><p class="eyebrow">POLICY GROUPS</p><h2>国策组</h2></div><span>成员容错与下游依赖分离</span></div>
      <div v-if="activeGroups.length" class="rsip-library">
        <article v-for="group in activeGroups" :key="group.id" class="policy-card group-definition">
          <div><h3>{{ group.name }}</h3><p>目标 {{ group.targetMemberCount }} 个成员；至少 {{ group.minimumActiveMembers }} 个时点亮。</p></div>
          <div class="policy-actions">
            <button class="quiet" :disabled="groupHasHistory(group)" @click="openGroupEditor(group)"><Pencil :size="15" /> 编辑</button>
            <button v-if="!activeGroupIds.has(group.id)" class="secondary" @click="openGroupDeployment(group)">部署组</button>
            <span v-else class="status-pill">已部署</span>
            <button class="quiet danger" :disabled="groupHasHistory(group)" @click="archiveGroup(group)"><Archive :size="15" /></button>
          </div>
        </article>
      </div>
      <div v-else class="empty-inline">还没有国策组。组形成后，下游可以依赖整个组，而不是某一个成员。</div>
    </section>

    <section v-if="history.length" class="section-block">
      <div class="section-heading"><div><p class="eyebrow">DEPLOYMENT HISTORY</p><h2>失效历史</h2></div><History :size="18" /></div>
      <ol class="timeline">
        <li v-for="node in history" :key="node.id"><span class="timeline-dot"></span><div><strong>{{ node.title }}</strong><p>{{ node.failureReason }}</p></div><time>{{ formatDate(node.extinguishedAt) }}</time></li>
      </ol>
      <button v-if="historyCursor" class="secondary history-more" :disabled="loadingHistory" @click="loadHistory">{{ loadingHistory ? '载入中…' : '载入更早记录' }}</button>
    </section>

    <details v-if="archivedPolicies.length || archivedGroups.length" class="archive-section">
      <summary><Archive :size="16" /> 已归档定义 {{ archivedPolicies.length + archivedGroups.length }} 项</summary>
      <div v-for="policy in archivedPolicies" :key="policy.id" class="archived-row"><span>{{ policy.title }}</span><small>国策 · {{ policy.deploymentCount }} 次部署</small></div>
      <div v-for="group in archivedGroups" :key="group.id" class="archived-row"><span>{{ group.name }}</span><small>国策组</small></div>
    </details>

    <div v-if="wizard" class="modal-backdrop">
      <form class="editor-modal wizard" @submit.prevent="wizardStep < 3 ? wizardStep++ : savePolicy()">
        <header><div><p class="eyebrow">GUIDED REVIEW · {{ wizardStep }}/3</p><h2>{{ ['描述问题', '回顾经过', '制定国策'][wizardStep - 1] }}</h2></div><button type="button" class="icon-button" @click="closeWizard"><X :size="18" /></button></header>
        <template v-if="wizardStep === 1">
          <label v-if="negatives.length">选择已有问题<select v-model="form.negativeStateId"><option value="">创建新问题</option><option v-for="negative in negatives" :key="negative.id" :value="negative.id">{{ negative.title }}</option></select></label>
          <template v-if="!form.negativeStateId"><label>问题名称<input v-model="form.negativeTitle" required maxlength="160" /></label><label>发生了什么<textarea v-model="form.description" required maxlength="4000"></textarea></label><label>出现前的信号<textarea v-model="form.cues" maxlength="4000"></textarea></label><label>带来的影响<textarea v-model="form.consequences" maxlength="4000"></textarea></label></template>
        </template>
        <template v-else-if="wizardStep === 2">
          <p class="form-hint">从结果向前回想，并选出最后一个仍可改变的决策点。</p>
          <label v-for="(_, index) in form.traceback" :key="index" class="trace-step"><input v-model="form.intervention" type="radio" :value="index" /><span>{{ index + 1 }}</span><input v-model="form.traceback[index]" required maxlength="1000" :placeholder="index === 0 ? '结果发生前，我做了…' : '再往前一步，我做了…'" /></label>
          <button type="button" class="text-link" @click="addTrace"><Plus :size="15" /> 再向前回溯一步</button>
        </template>
        <template v-else>
          <label>国策名称<input v-model="form.title" required maxlength="160" /></label>
          <label>模式<select v-model="form.mode"><option value="active">主动：信号出现后立即行动</option><option value="passive">被动：持续避免某种动作</option><option value="anchored">半被动：绑定到固定锚点</option></select></label>
          <label>触发条件<textarea v-model="form.triggerCondition" required maxlength="1000"></textarea></label>
          <label>必要动作<textarea v-model="form.requiredAction" required maxlength="1000"></textarea></label>
          <label>它阻止什么<textarea v-model="form.intendedPrevention" maxlength="4000"></textarea></label>
          <aside v-if="qualityNotes.length" class="quality-notes"><strong>可选完善建议</strong><span v-for="note in qualityNotes" :key="note">{{ note }}</span></aside>
        </template>
        <footer><button v-if="wizardStep > 1" type="button" class="quiet" @click="wizardStep--">上一步</button><span v-else></span><button class="primary" :disabled="saving">{{ wizardStep < 3 ? '继续' : saving ? '保存中…' : '保存到国策库' }}</button></footer>
      </form>
    </div>

    <div v-if="trialTarget" class="modal-backdrop" @click.self="trialTarget = null">
      <form class="confirm-modal" @submit.prevent="recordTrial"><p class="eyebrow">TRIAL</p><h2>记录成功试行</h2><p>确认你今天已经完整执行“{{ trialTarget.title }}”。这条记录只能用于今天的当前修订。</p><label>试行备注（可选）<textarea v-model="trialNote" maxlength="1000"></textarea></label><div class="form-actions"><button type="button" class="quiet" @click="trialTarget = null">取消</button><button class="primary" :disabled="saving">确认试行成功</button></div></form>
    </div>

    <div v-if="deployTarget" class="modal-backdrop" @click.self="deployTarget = null">
      <form class="confirm-modal" @submit.prevent="deployPolicy"><p class="eyebrow">DEPLOY</p><h2>点亮“{{ deployTarget.title }}”</h2><label>部署位置<select v-model="deploymentTarget"><option value="root">作为根国策</option><optgroup v-if="eligibleParents.length" label="作为下游依赖"><option v-for="node in eligibleParents" :key="node.id" :value="`parent:${node.id}`">依赖：{{ node.title }}</option></optgroup><optgroup v-if="eligibleGroups.length" label="加入国策组"><option v-for="node in eligibleGroups" :key="node.id" :value="`group:${node.id}`">成员：{{ node.title }}</option></optgroup></select></label><p class="form-hint">组成员只影响组的点亮状态，不能直接承载下游依赖。</p><div class="form-actions"><button type="button" class="quiet" @click="deployTarget = null">取消</button><button class="primary" :disabled="saving || !overview.canDeployToday">确认点亮</button></div></form>
    </div>

    <div v-if="policyEditor" class="modal-backdrop" @click.self="policyEditor = null">
      <form class="editor-modal" @submit.prevent="updatePolicy"><header><div><p class="eyebrow">REVISION</p><h2>编辑国策</h2></div><button type="button" class="icon-button" @click="policyEditor = null"><X :size="18" /></button></header><label>名称<input v-model="editForm.title" required maxlength="160" /></label><label>模式<select v-model="editForm.mode"><option value="active">主动</option><option value="passive">被动</option><option value="anchored">半被动</option></select></label><label>触发条件<textarea v-model="editForm.triggerCondition" required maxlength="1000"></textarea></label><label>必要动作<textarea v-model="editForm.requiredAction" required maxlength="1000"></textarea></label><label>它阻止什么<textarea v-model="editForm.intendedPrevention" maxlength="4000"></textarea></label><p class="form-hint">产生试行或部署历史后，保存会创建不可改写的新修订。</p><footer><span></span><button class="primary" :disabled="saving">保存修订</button></footer></form>
    </div>

    <div v-if="groupEditor" class="modal-backdrop" @click.self="groupEditor = null">
      <form class="confirm-modal" @submit.prevent="saveGroup"><p class="eyebrow">POLICY GROUP</p><h2>{{ groupEditor === 'new' ? '新建国策组' : '编辑国策组' }}</h2><label>名称<input v-model="groupForm.name" required maxlength="160" /></label><div class="two-cols"><label>目标成员数<input v-model.number="groupForm.targetMemberCount" type="number" min="1" max="50" required /></label><label>最低点亮数<input v-model.number="groupForm.minimumActiveMembers" type="number" min="1" :max="groupForm.targetMemberCount" required /></label></div><p class="form-hint">达到最低点亮数后，下游才可依赖该组。已有部署历史的组不能修改结构。</p><div class="form-actions"><button type="button" class="quiet" @click="groupEditor = null">取消</button><button class="primary" :disabled="saving">保存</button></div></form>
    </div>

    <div v-if="groupDeployment" class="modal-backdrop" @click.self="groupDeployment = null">
      <form class="confirm-modal" @submit.prevent="deployGroup"><p class="eyebrow">DEPLOY GROUP</p><h2>部署“{{ groupDeployment.name }}”</h2><label>部署位置<select v-model="groupParentTarget"><option value="root">作为根国策组</option><option v-for="node in eligibleParents" :key="node.id" :value="`parent:${node.id}`">依赖：{{ node.title }}</option></select></label><div class="form-actions"><button type="button" class="quiet" @click="groupDeployment = null">取消</button><button class="primary" :disabled="saving">确认部署</button></div></form>
    </div>

    <div v-if="failure" class="modal-backdrop" @click.self="failure = null">
      <form class="confirm-modal" @submit.prevent="failDeployment"><h2>确认报告失效？</h2><p>“{{ failure.title }}”失效后将熄灭 <strong>{{ failurePreview?.affectedCount ?? 1 }}</strong> 个部署节点<span v-if="failurePreview?.truncated">（列表仅显示前 1000 项）</span>。</p><p v-if="failurePreview?.groupRemainsEffective === true" class="success-note">该成员失效后，国策组仍满足最低点亮数，下游会保留。</p><p v-else-if="failurePreview?.groupRemainsEffective === false" class="danger-note">该成员失效后，国策组会跌破门槛并熄灭其下游；其他成员保留。</p><div v-if="failurePreview?.affected.length" class="impact-list"><span v-for="item in failurePreview.affected.slice(0, 8)" :key="item.id">{{ item.title }}</span></div><label>失效原因<textarea v-model="failureReason" required maxlength="4000" autofocus></textarea></label><div class="form-actions"><button type="button" class="quiet" @click="failure = null; failurePreview = null">取消</button><button class="danger" :disabled="saving">确认熄灭</button></div></form>
    </div>
  </div>
</template>
