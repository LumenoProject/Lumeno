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

export type RsipPolicyMode = 'active' | 'passive' | 'anchored'

export interface RsipPolicy {
  id: string
  revisionId: string
  revision: number
  negativeStateId?: string | null
  interventionStepId?: string | null
  mode: RsipPolicyMode
  title: string
  triggerCondition: string
  requiredAction: string
  intendedPrevention: string
  totalActiveSeconds: number
  deploymentCount: number
  archivedAt?: string | null
  version: number
  createdAt: string
  updatedAt: string
}

export interface RsipTrial {
  id: string
  policyId: string
  policyRevisionId: string
  succeededOn: string
  succeededAt: string
  note: string
  consumedNodeId?: string | null
}

export interface RsipGroup {
  id: string
  name: string
  targetMemberCount: number
  minimumActiveMembers: number
  archivedAt?: string | null
  version: number
  createdAt: string
  updatedAt: string
}

export interface RsipTreeNode {
  id: string
  kind: 'policy' | 'group'
  title: string
  policyMode?: RsipPolicyMode | null
  triggerCondition?: string | null
  requiredAction?: string | null
  policyId?: string | null
  policyRevisionId?: string | null
  groupId?: string | null
  parentNodeId?: string | null
  state: 'active' | 'extinguished'
  deployedOn?: string | null
  deployedAt: string
  extinguishedAt?: string | null
  failureReason?: string | null
  groupEverEffective: boolean
  version: number
}

export interface RsipOverview {
  localDate: string
  canDeployToday: boolean
  policies: RsipPolicy[]
  trials: RsipTrial[]
  groups: RsipGroup[]
  nodes: RsipTreeNode[]
  memberships: Array<{ groupNodeId: string; policyNodeId: string }>
}

export interface RsipHistoryPage {
  items: RsipTreeNode[]
  nextCursor?: string | null
}

export interface RsipToday {
  localDate: string
  canDeployToday: boolean
  pendingTrials: Array<{ id: string; policyId: string; title: string }>
  groups: Array<{
    nodeId: string
    name: string
    activeMembers: number
    minimumActiveMembers: number
    targetMemberCount: number
    everEffective: boolean
  }>
}

export interface RollbackPreview {
  rootId: string
  affected: Array<{ id: string; title: string }>
  affectedCount: number
  truncated: boolean
  groupRemainsEffective?: boolean | null
}
