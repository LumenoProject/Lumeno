import { describe, expect, it } from 'vitest'
import { detectedTimezoneUpdate, formatCountdown, nextMonotonicTimestamp, remainingMilliseconds } from './time'

describe('strict countdown', () => {
  it('derives time only from the absolute deadline', () => {
    const deadline = '2026-08-10T12:30:00.000Z'
    expect(remainingMilliseconds(deadline, Date.parse('2026-08-10T12:00:00.000Z'))).toBe(1_800_000)
    expect(remainingMilliseconds(deadline, Date.parse('2026-08-10T12:20:00.000Z'))).toBe(600_000)
  })

  it('never becomes negative and rounds display upward', () => {
    expect(remainingMilliseconds('2026-08-10T12:00:00.000Z', Date.parse('2026-08-10T12:00:01.000Z'))).toBe(0)
    expect(formatCountdown(60_001)).toBe('01:01')
    expect(formatCountdown(3_661_000)).toBe('1:01:01')
  })
})

describe('offline operation order', () => {
  it('advances beyond the newest key even if the local clock does not move forward', () => {
    const newest = '2026-08-10T12:00:00.100Z'
    expect(nextMonotonicTimestamp(Date.parse('2026-08-10T11:59:00.000Z'), newest))
      .toBe('2026-08-10T12:00:00.101Z')
    expect(nextMonotonicTimestamp(Date.parse(newest), newest))
      .toBe('2026-08-10T12:00:00.101Z')
  })
})

describe('browser timezone sync', () => {
  it('updates only when the browser timezone differs from the saved value', () => {
    expect(detectedTimezoneUpdate('UTC', 'Asia/Shanghai')).toBe('Asia/Shanghai')
    expect(detectedTimezoneUpdate('Asia/Shanghai', 'Asia/Shanghai')).toBeNull()
    expect(detectedTimezoneUpdate('UTC', '')).toBeNull()
  })
})
