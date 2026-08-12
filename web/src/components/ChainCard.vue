<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { Archive, CalendarClock, ChevronRight, Flame, Layers3, Pencil, Play, X } from '@lucide/vue'
import { useAppStore } from '../stores/app'
import type { Chain } from '../types'

const props = defineProps<{ chain: Chain; compact?: boolean; manage?: boolean; showRecord?: boolean }>()
const app = useAppStore()
const booking = computed(() => app.pendingBookings.find((item) => item.chainId === props.chain.id && item.state === 'pending'))
const bookingBusy = ref(false)
const editing = ref(false)
const archiving = ref(false)
const saving = ref(false)
const edit = reactive({
  parentId: '', name: '', description: '', position: 0,
  sacredTrigger: '', durationMinutes: null as number | null,
  auxiliarySignal: '', auxiliaryDelayMinutes: null as number | null,
  auxiliaryCompletionTrigger: '', groupLimitMinutes: null as number | null,
})

const descendantIds = computed(() => {
  const found = new Set<string>()
  const pending = [props.chain.id]
  while (pending.length) {
    const parent = pending.pop()!
    for (const item of app.chains) {
      if (item.parentId === parent && !found.has(item.id)) {
        found.add(item.id)
        pending.push(item.id)
      }
    }
  }
  return found
})
const parentGroups = computed(() => app.chains.filter((item) =>
  item.kind === 'group' && item.id !== props.chain.id && !descendantIds.value.has(item.id)))

function openEditor() {
  Object.assign(edit, {
    parentId: props.chain.parentId ?? '',
    name: props.chain.name,
    description: props.chain.description,
    position: props.chain.position,
    sacredTrigger: props.chain.sacredTrigger ?? '',
    durationMinutes: props.chain.durationSeconds ? props.chain.durationSeconds / 60 : null,
    auxiliarySignal: props.chain.auxiliarySignal ?? '',
    auxiliaryDelayMinutes: props.chain.auxiliaryDelaySeconds ? props.chain.auxiliaryDelaySeconds / 60 : null,
    auxiliaryCompletionTrigger: props.chain.auxiliaryCompletionTrigger ?? '',
    groupLimitMinutes: props.chain.groupTimeLimitSeconds ? props.chain.groupTimeLimitSeconds / 60 : null,
  })
  editing.value = true
}

async function start() {
  try { await app.startFocus(props.chain, booking.value?.id) } catch (error) { app.report(error) }
}

async function book() {
  if (bookingBusy.value) return
  bookingBusy.value = true
  try { await app.createBooking(props.chain.id) } catch (error) { app.report(error) } finally { bookingBusy.value = false }
}

async function startGroup() {
  try { await app.startGroup(props.chain) } catch (error) { app.report(error) }
}

async function save() {
  saving.value = true
  const unit = props.chain.kind === 'unit'
  try {
    await app.updateChain(props.chain, {
      parentId: edit.parentId || null,
      name: edit.name,
      description: edit.description,
      position: edit.position,
      sacredTrigger: unit ? edit.sacredTrigger : null,
      durationSeconds: unit && edit.durationMinutes ? Math.round(edit.durationMinutes * 60) : null,
      auxiliarySignal: unit ? edit.auxiliarySignal : null,
      auxiliaryDelaySeconds: unit && edit.auxiliaryDelayMinutes ? Math.round(edit.auxiliaryDelayMinutes * 60) : null,
      auxiliaryCompletionTrigger: unit ? edit.auxiliaryCompletionTrigger : null,
      groupTimeLimitSeconds: !unit && edit.groupLimitMinutes ? Math.round(edit.groupLimitMinutes * 60) : null,
    })
    editing.value = false
  } catch { /* store reports */ } finally { saving.value = false }
}

async function archive() {
  saving.value = true
  try {
    await app.archiveChain(props.chain)
    archiving.value = false
  } catch { /* store reports */ } finally { saving.value = false }
}
</script>

<template>
  <article class="chain-card" :class="{ compact, group: chain.kind === 'group' }">
    <div class="chain-icon"><Layers3 v-if="chain.kind === 'group'" :size="18" /><Flame v-else :size="18" /></div>
    <div class="chain-body">
      <div class="chain-title"><h3>{{ chain.name }}</h3><span v-if="chain.kind === 'unit'">{{ chain.mainStreak }} 连续</span></div>
      <p>{{ chain.description || (chain.kind === 'group' ? '按顺序执行其中的所有任务' : chain.sacredTrigger) }}</p>
      <div v-if="chain.kind === 'unit'" class="chain-meta">
        <span v-if="chain.durationSeconds">{{ Math.round(chain.durationSeconds / 60) }} 分钟</span>
        <span>预约连续 {{ chain.auxiliaryStreak }}</span>
        <span v-if="showRecord">完成 {{ chain.totalCompletions }}</span>
        <span v-if="showRecord" :class="{ 'failure-text': chain.totalFailures }">失守 {{ chain.totalFailures }}</span>
        <span v-if="booking" class="accent">已预约</span>
      </div>
    </div>
    <div class="chain-actions">
      <button v-if="manage" class="icon-button" title="编辑契约" @click="openEditor"><Pencil :size="16" /></button>
      <button v-if="manage" class="icon-button" title="归档契约" @click="archiving = true"><Archive :size="16" /></button>
      <button v-if="chain.kind === 'unit' && !booking" class="booking-button" title="预约" :disabled="bookingBusy" @click="book"><CalendarClock :size="16" /> {{ bookingBusy ? '准备中…' : '预约' }}</button>
      <button v-if="chain.kind === 'unit'" class="start-button" @click="start"><Play :size="16" /> 开始</button>
      <button v-else class="start-button" title="运行任务群" @click="startGroup">运行 <ChevronRight :size="16" /></button>
    </div>
  </article>

  <div v-if="editing" class="modal-backdrop" @click.self="editing = false">
    <form class="editor-modal" @submit.prevent="save">
      <header><div><p class="eyebrow">EDIT CONTRACT</p><h2>修改契约</h2></div><button type="button" class="icon-button" @click="editing = false"><X :size="18" /></button></header>
      <p class="form-hint">修改关键设置会建立新版本，并重置连续记录。</p>
      <label>名称<input v-model="edit.name" required maxlength="120" /></label>
      <label>归属任务群<select v-model="edit.parentId"><option value="">顶层</option><option v-for="group in parentGroups" :key="group.id" :value="group.id">{{ group.name }}</option></select></label>
      <label>说明<textarea v-model="edit.description" maxlength="4000"></textarea></label>
      <template v-if="chain.kind === 'unit'">
        <label>启动条件<textarea v-model="edit.sacredTrigger" required></textarea></label>
        <div class="two-cols"><label>专注分钟<input v-model.number="edit.durationMinutes" type="number" min="1" max="1440" /></label><label>预约响应分钟<input v-model.number="edit.auxiliaryDelayMinutes" required type="number" min="1" max="1440" /></label></div>
        <label>预约信号<input v-model="edit.auxiliarySignal" required /></label>
        <label>预约完成条件<input v-model="edit.auxiliaryCompletionTrigger" required /></label>
      </template>
      <label v-else>完成时限（分钟）<input v-model.number="edit.groupLimitMinutes" type="number" min="1" max="10080" /></label>
      <footer><button type="button" class="quiet" @click="editing = false">取消</button><button class="primary" :disabled="saving">{{ saving ? '保存中…' : '保存修改' }}</button></footer>
    </form>
  </div>

  <div v-if="archiving" class="modal-backdrop" @click.self="archiving = false">
    <form class="confirm-modal" @submit.prevent="archive">
      <h2>归档“{{ chain.name }}”？</h2>
      <p v-if="chain.kind === 'group'">任务群和其中的所有子任务都会归档。请先结束正在进行的任务或预约。</p>
      <p v-else>归档后可在归档页面中查看。</p>
      <div class="form-actions"><button type="button" class="quiet" @click="archiving = false">取消</button><button class="danger" :disabled="saving">确认归档</button></div>
    </form>
  </div>
</template>
