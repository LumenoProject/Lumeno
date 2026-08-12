import type { PendingCommitment } from '../types'

export const COMMITMENT_GRACE_MS = 10_000

export function commitmentCancellationRemaining(commitment: PendingCommitment, now = Date.now()) {
  return Math.max(0, Date.parse(commitment.cancellableUntil) - now)
}

export function commitmentCanBeCancelled(commitment: PendingCommitment, now = Date.now()) {
  return commitmentCancellationRemaining(commitment, now) > 0
}

export function isPendingCommitment(value: unknown): value is PendingCommitment {
  if (!value || typeof value !== 'object') return false
  const item = value as Partial<PendingCommitment>
  const occurredAt = typeof item.occurredAt === 'string' ? Date.parse(item.occurredAt) : Number.NaN
  const cancellableUntil = typeof item.cancellableUntil === 'string'
    ? Date.parse(item.cancellableUntil)
    : Number.NaN
  return typeof item.id === 'string' && item.id.length > 0
    && typeof item.userId === 'string' && item.userId.length > 0
    && (item.kind === 'focus' || item.kind === 'booking')
    && typeof item.chainId === 'string' && item.chainId.length > 0
    && Number.isFinite(occurredAt)
    && Number.isFinite(cancellableUntil)
    && cancellableUntil - occurredAt === COMMITMENT_GRACE_MS
    && (item.bookingId === undefined || item.bookingId === null || typeof item.bookingId === 'string')
    && (item.groupRunId === undefined || item.groupRunId === null || typeof item.groupRunId === 'string')
}
