import { execSync } from 'node:child_process'

export const Fragment = Symbol('esto.Fragment')
export const Context  = Symbol('esto.Context')

// Classic JSX factory. type = kind | component fn | Fragment | Context.
export function h(type, props, ...children) {
  const kids = children.flat(Infinity).filter(c => c != null && c !== false)
  if (type === Fragment)          return { $fragment: true, children: kids }
  if (type === Context)           return { $context: true, value: props?.value ?? null, children: kids }
  if (type?.__estoKind)           return { $kind: type, item: { ...(props ?? {}) } }
  if (typeof type === 'function') return { $component: type, props: { ...(props ?? {}), children: kids } }
  throw new Error(`esto: invalid JSX node type: ${String(type)}`)
}

// Tier 1 (target has desired()): identity — keep as-is for backward compat.
// Tier 2 (no desired): kind descriptor, grouped by the runner from JSX leaf instances.
export function defineTarget(def) {
  if (def.desired) return def
  return { __estoKind: true, ...def }
}

// prompt`...${val}...` — plain interpolation (NOT shell-escaped).
// Returns { $prompt: string }; a kind's enter/update can return this to emit a grounded task.
export const prompt = (strings, ...values) =>
  ({ $prompt: strings.reduce((a, s, i) => a + s + (i < values.length ? String(values[i]) : ''), '') })

function shellQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'"
}

// sh`cmd ${a} ${b}` — interpolations are shell-quoted; template literal string parts verbatim.
// Uses strings.raw so \n in the template stays as \n (for printf etc.).
// Returns stdout as a string; throws on nonzero exit.
export function sh(strings, ...values) {
  let cmd = strings.raw[0]
  for (let i = 0; i < values.length; i++) {
    cmd += shellQuote(String(values[i])) + strings.raw[i + 1]
  }
  return execSync(cmd, { shell: '/bin/sh', encoding: 'utf8' })
}
