export const Fragment = Symbol('esto.Fragment')
export const Context  = Symbol('esto.Context')

// Classic JSX factory. type = kind | component fn | Fragment | Context.
export function h(type, props, ...children) {
  const kids = children.flat(Infinity).filter(c => c != null && c !== false)
  if (type === Fragment)          return { $fragment: true, children: kids }
  if (type === Context)           return { $context: true, value: props?.value ?? null, data: props?.data != null ? JSON.stringify(props.data) : null, children: kids }
  if (type?.__estoKind)           return { $kind: type, item: { ...(props ?? {}) } }
  if (typeof type === 'function') return { $component: type, props: { ...(props ?? {}), children: kids } }
  throw new Error(`esto: invalid JSX node type: ${String(type)}`)
}

let __nextKindId = 0

// Tier 1 (unit has desired()): identity — keep as-is for Rust to detect Tier 1 path.
// Tier 2/3 (no desired): stamps __estoKind + __estoId so Rust can group leaves by type.
export function unit(def) {
  if (def.desired) return def
  return { __estoKind: true, __estoId: __nextKindId++, ...def }
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
// Returns stdout as a string; throws on nonzero exit. Delegates to Rust via __sh_exec global.
export function sh(strings, ...values) {
  let cmd = strings.raw[0]
  for (let i = 0; i < values.length; i++) {
    cmd += shellQuote(String(values[i])) + strings.raw[i + 1]
  }
  return globalThis.__sh_exec(cmd)
}

// ── Owned read-only I/O (in-process, no forking) ─────────────────────────────
// Writes go through sh. Importing node:fs / node:crypto is not supported.

/** Returns true if path exists. */
export const exists = (path) => globalThis.__esto_exists(path)

/** Returns file contents as a string. Throws if the file is missing. */
export const read   = (path) => globalThis.__esto_read(path)

/** Returns filenames in dir as a string[]. Returns [] if dir is missing. */
export const ls     = (dir)  => JSON.parse(globalThis.__esto_ls_json(dir))

/** Returns hex SHA-256 of the input string. */
export const hash   = (input) => globalThis.__esto_hash(input)

// ── console shim — QuickJS has no built-in console ───────────────────────────
const __fmt = (v) => typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)
const __print = (level, args) => globalThis.__console_print(level, args.map(__fmt).join(' '))
globalThis.console = {
  log:   (...a) => __print('log',   a),
  error: (...a) => __print('error', a),
  warn:  (...a) => __print('warn',  a),
  debug: (...a) => __print('debug', a),
}
