const MAX_FAILURES = 5
const WINDOW_MS = 10 * 60_000
const LOCK_MS = 15 * 60_000
const PASSWORD = 'hunter2'

export type LoginResult = 'ok' | 'wrong-password' | 'locked'

export function createLogin(now: () => number = Date.now) {
  let failures: number[] = []
  let lockedUntil = 0

  return (password: string): LoginResult => {
    const t = now()
    if (t < lockedUntil) return 'locked'
    if (password === PASSWORD) {
      failures = []
      return 'ok'
    }
    failures = [...failures.filter(f => t - f < WINDOW_MS), t]
    if (failures.length >= MAX_FAILURES) {
      lockedUntil = t + LOCK_MS
      failures = []
    }
    return 'wrong-password'
  }
}
