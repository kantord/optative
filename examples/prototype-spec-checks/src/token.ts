export type Token = { sub: string; exp: number }

export const SKEW_SECONDS = 30

export function verify(token: Token, now: number): 200 | 401 {
  return now <= token.exp + SKEW_SECONDS ? 200 : 401
}
