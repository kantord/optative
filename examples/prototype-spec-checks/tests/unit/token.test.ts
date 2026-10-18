import { expect, test } from 'vitest'
import { verify } from '../../src/token'

const now = 1_000_000

// @spec token-expiry/rejects-expired #dfc78d08
test('expired token is rejected', () => {
  expect(verify({ sub: 'u', exp: now - 60 }, now)).toBe(401)
})

// Orphan: this rule was removed from the spec. The reconciler should flag it.
// @spec token-expiry/accepts-unsigned #0badc0de
test('unsigned token is accepted', () => {
  expect(verify({ sub: 'u', exp: now + 60 }, now)).toBe(200)
})
