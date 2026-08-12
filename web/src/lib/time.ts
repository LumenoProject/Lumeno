export function remainingMilliseconds(deadline: string | null | undefined, now = Date.now()) {
  return deadline ? Math.max(0, Date.parse(deadline) - now) : 0
}

export function formatCountdown(milliseconds: number) {
  const seconds = Math.ceil(Math.max(0, milliseconds) / 1000)
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const rest = seconds % 60
  return hours
    ? `${hours}:${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`
    : `${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`
}

export function nextMonotonicTimestamp(now: number, newest?: string) {
  const parsedNewest = newest ? Date.parse(newest) : Number.NaN
  const lowerBound = Number.isFinite(parsedNewest) ? parsedNewest + 1 : 0
  return new Date(Math.max(now, lowerBound)).toISOString()
}

export function browserTimezone() {
  return Intl.DateTimeFormat().resolvedOptions().timeZone
}

export function detectedTimezoneUpdate(savedTimezone: string, detectedTimezone = browserTimezone()) {
  return detectedTimezone && detectedTimezone !== savedTimezone ? detectedTimezone : null
}
