import { expectTypeOf, test } from 'vitest'
import type { Token } from '../../src/token'

// @spec token-expiry/expiry-required #c2786c66
test('Token requires exp', () => {
  expectTypeOf<Token>().toHaveProperty('exp').toEqualTypeOf<number>()
  // @ts-expect-error exp is required
  const t: Token = { sub: 'u' }
  void t
})
