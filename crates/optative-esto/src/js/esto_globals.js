// esto API globals — eval'd before any module loading.
// Sets globalThis.__esto_* so synthetic module shims can re-export them.
// Rust has already registered: __sh_exec, __esto_exists, __esto_read,
//   __esto_ls_json, __esto_hash, __console_print.

// JSX factory
globalThis.__esto_h = function h(type, props, ...children) {
  const kids = children.flat(Infinity).filter(c => c != null && c !== false)
  if (type && type.__estoFragment)  return { $fragment: true, children: kids }
  if (type && type.__estoContext)   return { $context: true, value: props?.value ?? null, data: props?.data != null ? JSON.stringify(props.data) : null, children: kids }
  if (type?.__estoKind)             return { $kind: type, item: { ...(props ?? {}) } }
  if (typeof type === 'function')   return { $component: type, props: { ...(props ?? {}), children: kids } }
  throw new Error('esto: invalid JSX node type: ' + String(type))
}

// Fragment and Context are marker objects (not symbols) so h() can detect them
// and so they can be stored as QuickJS globals.
globalThis.__esto_fragment = { __estoFragment: true }
globalThis.__esto_context  = { __estoContext:  true }

let __nextKindId = 0
globalThis.__esto_unit = function unit(def) {
  if (def.desired) return def
  return Object.assign({ __estoKind: true, __estoId: __nextKindId++ }, def)
}

globalThis.__esto_prompt = function prompt(strings, ...values) {
  const body = strings.reduce((a, s, i) => a + s + (i < values.length ? String(values[i]) : ''), '')
  return { $prompt: body }
}

function __shellQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'"
}
globalThis.__esto_sh = function sh(strings, ...values) {
  let cmd = strings.raw[0]
  for (let i = 0; i < values.length; i++) {
    cmd += __shellQuote(String(values[i])) + strings.raw[i + 1]
  }
  return globalThis.__sh_exec(cmd)
}

// ls wraps the Rust JSON API
globalThis.__esto_ls = function ls(dir) {
  return JSON.parse(globalThis.__esto_ls_json(dir))
}

// console shim — QuickJS has no built-in console
const __fmt = v => typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v)
const __cprint = (level, args) => globalThis.__console_print(level, args.map(__fmt).join(' '))
globalThis.console = {
  log:   (...a) => __cprint('log',   a),
  error: (...a) => __cprint('error', a),
  warn:  (...a) => __cprint('warn',  a),
  debug: (...a) => __cprint('debug', a),
}
