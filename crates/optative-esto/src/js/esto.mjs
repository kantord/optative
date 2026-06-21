import { execSync } from 'node:child_process'

export const defineTarget = (t) => t

function shellQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'"
}

// sh`cmd ${a} ${b}` — interpolations are shell-quoted; template literal string parts are kept verbatim.
// Uses strings.raw so \n in the template stays as the two-character sequence \n (for printf etc.).
// Returns stdout as a string; throws on nonzero exit.
export function sh(strings, ...values) {
  let cmd = strings.raw[0]
  for (let i = 0; i < values.length; i++) {
    cmd += shellQuote(String(values[i])) + strings.raw[i + 1]
  }
  return execSync(cmd, { shell: '/bin/sh', encoding: 'utf8' })
}
