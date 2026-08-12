import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { ApiProblem, api, mutation, setCsrfToken } from '../lib/api'
import { COMMITMENT_GRACE_MS, commitmentCanBeCancelled, isPendingCommitment } from '../lib/commitment'
import {
  clearLocalFocus, clearPendingCommitment, enqueue, flushOutbox, loadLocalFocus,
  loadPendingCommitment, outboxCount, saveLocalFocus, savePendingCommitment,
} from '../lib/offline'
import { detectedTimezoneUpdate } from '../lib/time'
import type { Booking, Chain, CommitmentRecord, FocusSession, GroupRun, Me, PendingCommitment } from '../types'

export const useAppStore = defineStore('app', () => {
  const me = ref<Me | null>(null)
  const authReady = ref(false)
  const busy = ref(false)
  const error = ref('')
  const chains = ref<Chain[]>([])
  const archivedChains = ref<Chain[]>([])
  const bookings = ref<Booking[]>([])
  const commitmentRecord = ref<CommitmentRecord>({ completions: 0, failures: 0 })
  const activeFocus = ref<FocusSession | null>(null)
  const pendingGroupJudgment = ref<GroupRun | null>(null)
  const pendingCommitment = ref<PendingCommitment | null>(null)
  const pendingCommitmentBusy = ref(false)
  const online = ref(navigator.onLine)
  const queued = ref(0)

  const chainById = computed(() => new Map(chains.value.map((chain) => [chain.id, chain])))
  const unitChains = computed(() => chains.value.filter((chain) => chain.kind === 'unit'))
  const pendingBookings = computed(() => bookings.value.filter((booking) =>
    booking.state === 'pending' || booking.state === 'awaiting_judgment'))
  const pendingBookingJudgment = computed(() =>
    bookings.value.find((booking) => booking.state === 'awaiting_judgment') ?? null)
  const activeBooking = computed(() =>
    bookings.value.find((booking) => booking.state === 'pending') ?? null)

  async function bootstrap() {
    window.addEventListener('online', () => { online.value = true; void sync() })
    window.addEventListener('offline', () => { online.value = false })
    queued.value = await outboxCount()
    try {
      me.value = await api<Me>('/api/v1/me')
      setCsrfToken(me.value.csrfToken)
      await syncBrowserTimezone()
      await refresh()
      await restorePendingCommitment()
      if (online.value) await sync()
    } catch (cause) {
      if (!(cause instanceof ApiProblem && cause.status === 401)) report(cause)
    } finally {
      authReady.value = true
    }
  }

  async function syncBrowserTimezone() {
    if (!me.value) return
    const timezone = detectedTimezoneUpdate(me.value.timezone)
    if (!timezone) return
    try {
      me.value = await mutation<Me>('/api/v1/me', 'PUT', { timezone })
      setCsrfToken(me.value.csrfToken)
    } catch (cause) {
      report(cause)
    }
  }

  async function refresh() {
    if (!me.value) return
    const [loadedChains, loadedBookings, serverFocus] = await Promise.all([
      api<Chain[]>('/api/v1/chains'),
      api<Booking[]>('/api/v1/bookings'),
      api<FocusSession | null>('/api/v1/focus/active'),
    ])
    // Booking reconciliation happens while loading bookings. Fetching the record afterwards
    // prevents a just-expired booking from appearing before its terminal event is counted.
    const loadedCommitmentRecord = await api<CommitmentRecord>('/api/v1/stats/commitment-record')
    chains.value = loadedChains
    bookings.value = loadedBookings
    commitmentRecord.value = loadedCommitmentRecord
    activeFocus.value = serverFocus ?? await loadLocalFocus<FocusSession>() ?? null
  }

  async function sync() {
    if (!me.value || !navigator.onLine) return
    try {
      queued.value = await flushOutbox()
      await refresh()
    } catch (cause) {
      report(cause)
    }
  }

  async function restorePendingCommitment() {
    const stored = await loadPendingCommitment<unknown>()
    if (!isPendingCommitment(stored) || stored.userId !== me.value?.id) {
      if (stored !== undefined) await clearPendingCommitment()
      return
    }
    const alreadyCommitted = stored.kind === 'focus'
      ? activeFocus.value?.id === stored.id
      : bookings.value.some((booking) => booking.id === stored.id)
    if (alreadyCommitted) {
      await clearPendingCommitment()
      return
    }
    pendingCommitment.value = stored
  }

  async function createChain(payload: Record<string, unknown>) {
    return run(async () => {
      await mutation('/api/v1/chains', 'POST', payload)
      await refresh()
    })
  }

  async function updateChain(chain: Chain, payload: Record<string, unknown>) {
    return run(async () => {
      await mutation(`/api/v1/chains/${chain.id}`, 'PUT', { ...payload, version: chain.version })
      await refresh()
    })
  }

  async function archiveChain(chain: Chain) {
    return run(async () => {
      const mode = chain.kind === 'group' ? '?mode=archive_subtree' : ''
      await mutation(`/api/v1/chains/${chain.id}${mode}`, 'DELETE')
      archivedChains.value = []
      await refresh()
    })
  }

  async function loadArchivedChains() {
    archivedChains.value = []
    archivedChains.value = await api<Chain[]>('/api/v1/chains/archived')
  }

  async function createBooking(chainId: string) {
    if (pendingCommitment.value) throw new Error('已有等待生效的操作')
    if (pendingBookings.value.some((booking) => booking.chainId === chainId)) {
      throw new Error('该任务已有待处理预约')
    }
    await stageCommitment({ kind: 'booking', chainId })
  }

  async function terminateBooking(booking: Booking) {
    return run(async () => {
      const updated = await mutation<Booking>(`/api/v1/bookings/${booking.id}/terminate`, 'POST', {
        version: booking.version,
      })
      bookings.value = bookings.value.map((item) => item.id === updated.id ? updated : item)
      await refresh()
    })
  }

  async function startGroup(group: Chain) {
    if (!navigator.onLine) throw new Error('创建任务群需要联网')
    let run = await api<GroupRun | null>(`/api/v1/groups/${group.id}/runs/active`)
    run ??= await mutation<GroupRun>(`/api/v1/groups/${group.id}/runs`, 'POST')
    if (run.state === 'awaiting_judgment') {
      pendingGroupJudgment.value = run
      return
    }
    const next = run.nextChainId ? chainById.value.get(run.nextChainId) : undefined
    if (!next) { await refresh(); return }
    await startFocus(next, undefined, run.id)
  }

  async function resolveGroup(decision: 'failure' | 'allow', rule?: string, precedentId?: string) {
    const run = pendingGroupJudgment.value
    if (!run) return
    const result = await mutation<GroupRun>(`/api/v1/group-runs/${run.id}/judgment`, 'POST', {
      decision,
      precedentId: decision === 'allow' ? precedentId : undefined,
      newRule: decision === 'allow' && !precedentId ? rule : undefined,
      version: run.version,
      occurredAt: new Date().toISOString(),
    })
    pendingGroupJudgment.value = null
    if (result.state === 'running' && result.nextChainId) {
      const next = chainById.value.get(result.nextChainId)
      if (next) await startFocus(next, undefined, result.id)
    }
    await refresh()
  }

  async function startFocus(chain: Chain, bookingId?: string, groupRunId?: string) {
    if (activeFocus.value) throw new Error('已有进行中的专注')
    if (pendingCommitment.value) throw new Error('已有等待生效的操作')
    await stageCommitment({
      kind: 'focus',
      chainId: chain.id,
      bookingId: bookingId ?? null,
      groupRunId: groupRunId ?? null,
    })
  }

  async function stageCommitment(input: Pick<PendingCommitment, 'kind' | 'chainId' | 'bookingId' | 'groupRunId'>) {
    if (!me.value) throw new Error('登录状态尚未就绪')
    const occurredAt = new Date().toISOString()
    const commitment: PendingCommitment = {
      id: crypto.randomUUID(),
      userId: me.value.id,
      kind: input.kind,
      chainId: input.chainId,
      occurredAt,
      cancellableUntil: new Date(Date.parse(occurredAt) + COMMITMENT_GRACE_MS).toISOString(),
      ...(input.bookingId === undefined ? {} : { bookingId: input.bookingId }),
      ...(input.groupRunId === undefined ? {} : { groupRunId: input.groupRunId }),
    }
    pendingCommitmentBusy.value = true
    pendingCommitment.value = commitment
    try {
      await savePendingCommitment(commitment)
    } catch (cause) {
      pendingCommitment.value = null
      report(cause)
      throw cause
    } finally {
      pendingCommitmentBusy.value = false
    }
  }

  async function cancelPendingCommitment() {
    const commitment = pendingCommitment.value
    if (!commitment || pendingCommitmentBusy.value || !commitmentCanBeCancelled(commitment)) return
    pendingCommitmentBusy.value = true
    try {
      await clearPendingCommitment()
      if (pendingCommitment.value?.id === commitment.id) pendingCommitment.value = null
    } finally {
      pendingCommitmentBusy.value = false
    }
  }

  async function commitPendingCommitment() {
    const commitment = pendingCommitment.value
    if (!commitment || pendingCommitmentBusy.value || commitmentCanBeCancelled(commitment)) return
    if (commitment.kind === 'booking' && !navigator.onLine) return
    pendingCommitmentBusy.value = true
    error.value = ''
    try {
      if (commitment.kind === 'booking') await commitBooking(commitment)
      else await commitFocus(commitment)
      if (pendingCommitment.value?.id === commitment.id) pendingCommitment.value = null
      await clearPendingCommitment()
    } catch (cause) {
      if (!(cause instanceof TypeError)) {
        if (pendingCommitment.value?.id === commitment.id) pendingCommitment.value = null
        await clearPendingCommitment()
      }
      report(cause)
      throw cause
    } finally {
      pendingCommitmentBusy.value = false
    }
  }

  async function commitBooking(commitment: PendingCommitment) {
    const created = await mutation<Booking>(`/api/v1/chains/${commitment.chainId}/bookings`, 'POST', {
      clientBookingId: commitment.id,
      occurredAt: commitment.occurredAt,
    }, commitment.id)
    bookings.value = [...bookings.value.filter((booking) => booking.id !== created.id), created]
      .sort((left, right) => Date.parse(left.expiresAt) - Date.parse(right.expiresAt))
    await refresh()
  }

  async function commitFocus(commitment: PendingCommitment) {
    if (activeFocus.value) throw new Error('已有进行中的专注')
    const chain = chainById.value.get(commitment.chainId)
    if (!chain) throw new Error('准备启动的任务已不存在')
    const occurredAt = commitment.occurredAt
    const id = commitment.id
    const local: FocusSession = {
      id,
      chainId: chain.id,
      bookingId: commitment.bookingId ?? null,
      groupRunId: commitment.groupRunId ?? null,
      state: 'running',
      startedAt: occurredAt,
      completionAvailableAt: chain.durationSeconds
        ? new Date(Date.parse(occurredAt) + chain.durationSeconds * 1000).toISOString()
        : null,
      version: 1,
      localOnly: !navigator.onLine,
    }
    const body = {
      bookingId: commitment.bookingId ?? undefined,
      groupRunId: commitment.groupRunId ?? undefined,
      clientSessionId: id,
      occurredAt,
    }
    activeFocus.value = local
    await saveLocalFocus(local)
    try {
      if (!navigator.onLine) throw new TypeError('offline')
      activeFocus.value = await mutation<FocusSession>(`/api/v1/chains/${chain.id}/focus`, 'POST', body, id)
      await clearLocalFocus()
      await refresh()
    } catch (cause) {
      if (cause instanceof TypeError) {
        try {
          await enqueue({ id, path: `/api/v1/chains/${chain.id}/focus`, method: 'POST', body })
          queued.value = await outboxCount()
          return
        } catch (enqueueError) {
          activeFocus.value = null
          await clearLocalFocus()
          throw enqueueError
        }
      }
      activeFocus.value = null
      await clearLocalFocus()
      throw cause
    }
  }

  async function completeFocus() {
    const focus = activeFocus.value
    if (!focus) return
    if (focus.completionAvailableAt && Date.parse(focus.completionAvailableAt) > Date.now()) {
      throw new Error('倒计时尚未结束')
    }
    await focusMutation(`/api/v1/focus/${focus.id}/complete`, {}, () => {
      focus.state = 'succeeded'
      activeFocus.value = null
    })
  }

  async function resolveFocus(
    action: 'interrupt' | 'early_end',
    reason: string,
    decision: 'failure' | 'allow',
    rule?: string,
    precedentId?: string,
  ) {
    const focus = activeFocus.value
    if (!focus) return
    const occurredAt = new Date().toISOString()
    const alreadyRaised = focus.state === 'awaiting_judgment'
    const raisePath = `/api/v1/focus/${focus.id}/judgment`
    const resolvePath = `/api/v1/focus/${focus.id}/judgment/resolve`
    const raise = { action, reason, occurredAt }
    const resolve = {
      decision,
      precedentId: decision === 'allow' ? precedentId : undefined,
      newRule: decision === 'allow' && !precedentId ? rule : undefined,
      version: alreadyRaised ? focus.version : focus.version + 1,
      occurredAt,
    }
    const raiseId = crypto.randomUUID()
    const resolveId = crypto.randomUUID()
    if (!navigator.onLine || focus.localOnly) {
      if (!alreadyRaised) await enqueue({ id: raiseId, path: raisePath, method: 'POST', body: raise })
      await enqueue({ id: resolveId, path: resolvePath, method: 'POST', body: resolve })
      queued.value = await outboxCount()
      if (decision === 'failure' || action === 'early_end') {
        activeFocus.value = null
        await clearLocalFocus()
      }
      return
    }
    if (!alreadyRaised) {
      try {
        await mutation(raisePath, 'POST', raise, raiseId)
      } catch (cause) {
        if (!(cause instanceof TypeError)) throw cause
        await enqueue({ id: raiseId, path: raisePath, method: 'POST', body: raise })
        await enqueue({ id: resolveId, path: resolvePath, method: 'POST', body: resolve })
        queued.value = await outboxCount()
        return
      }
    }
    let result: FocusSession
    try {
      result = await mutation<FocusSession>(resolvePath, 'POST', resolve, resolveId)
    } catch (cause) {
      if (!(cause instanceof TypeError)) throw cause
      await enqueue({ id: resolveId, path: resolvePath, method: 'POST', body: resolve })
      queued.value = await outboxCount()
      return
    }
    activeFocus.value = result.state === 'running' || result.state === 'completion_ready' ? result : null
    await refresh()
  }

  async function resolveBooking(
    booking: Booking,
    decision: 'failure' | 'allow',
    rule?: string,
    precedentId?: string,
  ) {
    await mutation<Booking>(`/api/v1/bookings/${booking.id}/judgment`, 'POST', {
      decision,
      precedentId: decision === 'allow' ? precedentId : undefined,
      newRule: decision === 'allow' && !precedentId ? rule : undefined,
      version: booking.version,
      occurredAt: new Date().toISOString(),
    })
    bookings.value = bookings.value.filter((item) => item.id !== booking.id)
    await refresh()
  }

  async function focusMutation(path: string, body: unknown, localSuccess: () => void) {
    const localOnly = activeFocus.value?.localOnly
    const actionId = crypto.randomUUID()
    if (!navigator.onLine || localOnly) {
      await enqueue({ id: actionId, path, method: 'POST', body })
      localSuccess()
      await clearLocalFocus()
      queued.value = await outboxCount()
      return
    }
    try {
      await mutation(path, 'POST', body, actionId)
    } catch (cause) {
      if (!(cause instanceof TypeError)) throw cause
      await enqueue({ id: actionId, path, method: 'POST', body })
      queued.value = await outboxCount()
    }
    localSuccess()
    await refresh()
  }

  async function run<T>(operation: () => Promise<T>) {
    busy.value = true
    error.value = ''
    try { return await operation() } catch (cause) { report(cause); throw cause } finally { busy.value = false }
  }

  function report(cause: unknown) {
    error.value = cause instanceof Error ? cause.message : '发生未知错误'
  }

  function login() { window.location.assign('/api/v1/auth/login?return_path=/') }

  return {
    me, authReady, busy, error, chains, archivedChains, bookings, commitmentRecord, activeFocus, pendingGroupJudgment,
    pendingCommitment, pendingCommitmentBusy, online, queued,
    chainById, unitChains, pendingBookings, pendingBookingJudgment, activeBooking,
    bootstrap, refresh, sync, createChain, updateChain, archiveChain, loadArchivedChains, createBooking, terminateBooking, startGroup, resolveGroup,
    startFocus, cancelPendingCommitment, commitPendingCommitment,
    completeFocus, resolveFocus, resolveBooking, login, report,
  }
})
