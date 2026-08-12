export type ChainKind = 'unit' | 'group'
export type UnitKind = 'basic' | 'assault' | 'recon' | 'command' | 'special_ops' | 'engineering' | 'quartermaster'

export interface Me {
  id: string
  displayName: string
  email?: string | null
  timezone: string
  csrfToken: string
}

export type NotificationChannelKind = 'bark' | 'webhook'

export interface NotificationChannel {
  configured: boolean
  kind: NotificationChannelKind | null
  endpointHint: string | null
  hasBearerToken: boolean
  barkEncryptionEnabled: boolean
}

export interface Chain {
  id: string
  logicalId: string
  revision: number
  previousRevisionId?: string | null
  parentId?: string | null
  kind: ChainKind
  unitKind?: UnitKind | null
  name: string
  description: string
  position: number
  sacredTrigger?: string | null
  durationSeconds?: number | null
  auxiliarySignal?: string | null
  auxiliaryDelaySeconds?: number | null
  auxiliaryCompletionTrigger?: string | null
  groupTimeLimitSeconds?: number | null
  mainStreak: number
  auxiliaryStreak: number
  totalCompletions: number
  totalFailures: number
  auxiliaryFailures: number
  version: number
  archivedAt?: string | null
}

export interface Booking {
  id: string
  chainId: string
  state: 'pending' | 'fulfilled' | 'awaiting_judgment' | 'failed' | 'allowed'
  scheduledAt: string
  expiresAt: string
  confirmationExpiresAt?: string | null
  resolvedAt?: string | null
  version: number
}

export interface Precedent {
  id: string
  chainId: string
  scope: 'main' | 'auxiliary' | 'group_deadline'
  rule: string
  createdAt: string
}

export interface FocusSession {
  id: string
  chainId: string
  bookingId?: string | null
  groupRunId?: string | null
  state: 'running' | 'completion_ready' | 'awaiting_judgment' | 'succeeded' | 'failed'
  startedAt: string
  completionAvailableAt?: string | null
  pendingAction?: string | null
  pendingReason?: string | null
  version: number
  localOnly?: boolean
}

export interface PendingCommitment {
  id: string
  userId: string
  kind: 'focus' | 'booking'
  chainId: string
  occurredAt: string
  cancellableUntil: string
  bookingId?: string | null
  groupRunId?: string | null
}

export interface GroupRun {
  id: string
  chainId: string
  state: 'running' | 'awaiting_judgment' | 'succeeded' | 'failed'
  startedAt: string
  expiresAt?: string | null
  completedAt?: string | null
  version: number
  nextChainId?: string | null
}

export interface ProtocolEvent {
  id: string
  chainId?: string | null
  aggregateType: string
  aggregateId: string
  eventType: string
  payload: Record<string, unknown>
  occurredAt: string
  recordedAt: string
}

export interface CommitmentRecord {
  completions: number
  failures: number
}

export interface NegativeState {
  id: string
  title: string
  description: string
  cues: string
  consequences: string
  createdAt: string
  updatedAt: string
}

export interface RsipNode {
  id: string
  negativeStateId?: string | null
  interventionStepId?: string | null
  parentId?: string | null
  state: 'draft' | 'active' | 'archived'
  title: string
  triggerCondition: string
  requiredAction: string
  intendedPrevention: string
  activatedOn?: string | null
  activatedAt?: string | null
  archivedAt?: string | null
  failureReason?: string | null
  version: number
  createdAt: string
  updatedAt: string
}

export interface RollbackPreview {
  rootId: string
  affected: Array<{ id: string; title: string }>
  affectedCount: number
  truncated: boolean
}
