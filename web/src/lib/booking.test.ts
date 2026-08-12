import { describe, expect, it } from 'vitest'
import { bookingPhase, bookingPhaseDeadline } from './booking'

const booking = {
  scheduledAt: '2026-08-12T12:15:00.000Z',
  expiresAt: '2026-08-12T12:16:30.000Z',
}

describe('booking start window', () => {
  it('counts down to the scheduled start time', () => {
    const now = Date.parse('2026-08-12T12:14:59.999Z')
    expect(bookingPhase(booking, now)).toBe('countdown')
    expect(bookingPhaseDeadline(booking, now)).toBe(booking.scheduledAt)
  })

  it('opens at the scheduled time and includes the 90-second deadline', () => {
    expect(bookingPhase(booking, Date.parse(booking.scheduledAt))).toBe('start_window')
    expect(bookingPhase(booking, Date.parse(booking.expiresAt))).toBe('start_window')
    expect(bookingPhaseDeadline(booking, Date.parse(booking.scheduledAt))).toBe(booking.expiresAt)
  })

  it('expires immediately after the start window', () => {
    expect(bookingPhase(booking, Date.parse(booking.expiresAt) + 1)).toBe('expired')
  })
})
