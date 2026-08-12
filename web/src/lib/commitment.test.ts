import { describe, expect, it } from 'vitest'
import { COMMITMENT_GRACE_MS, commitmentCanBeCancelled, commitmentCancellationRemaining, isPendingCommitment } from './commitment'
import type { PendingCommitment } from '../types'

const occurredAt = '2026-08-12T12:00:00.000Z'
const commitment: PendingCommitment = {
  id: '018f0000-0000-7000-8000-000000000001',
  userId: '018f0000-0000-7000-8000-000000000002',
  kind: 'focus',
  chainId: '018f0000-0000-7000-8000-000000000003',
  occurredAt,
  cancellableUntil: new Date(Date.parse(occurredAt) + COMMITMENT_GRACE_MS).toISOString(),
}

describe('commitment grace window', () => {
  it('allows cancellation only before the ten-second deadline', () => {
    expect(commitmentCancellationRemaining(commitment, Date.parse(occurredAt))).toBe(10_000)
    expect(commitmentCanBeCancelled(commitment, Date.parse(occurredAt) + 9_999)).toBe(true)
    expect(commitmentCanBeCancelled(commitment, Date.parse(occurredAt) + 10_000)).toBe(false)
  })

  it('rejects malformed persisted commitments', () => {
    expect(isPendingCommitment(commitment)).toBe(true)
    expect(isPendingCommitment({ ...commitment, kind: 'unknown' })).toBe(false)
    expect(isPendingCommitment({ ...commitment, cancellableUntil: 'not-a-date' })).toBe(false)
    expect(isPendingCommitment({
      ...commitment,
      cancellableUntil: new Date(Date.parse(occurredAt) + COMMITMENT_GRACE_MS + 1).toISOString(),
    })).toBe(false)
  })
})
