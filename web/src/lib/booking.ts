import type { Booking } from '../types'

export type BookingPhase = 'countdown' | 'start_window' | 'expired'

export function bookingPhase(
  booking: Pick<Booking, 'scheduledAt' | 'expiresAt'>,
  now = Date.now(),
): BookingPhase {
  if (now < Date.parse(booking.scheduledAt)) return 'countdown'
  if (now <= Date.parse(booking.expiresAt)) return 'start_window'
  return 'expired'
}

export function bookingPhaseDeadline(
  booking: Pick<Booking, 'scheduledAt' | 'expiresAt'>,
  now = Date.now(),
) {
  return bookingPhase(booking, now) === 'countdown' ? booking.scheduledAt : booking.expiresAt
}
