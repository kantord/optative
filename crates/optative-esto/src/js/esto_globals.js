// esto API globals — eval'd before any module loading.
// Sets globalThis.__esto_h so the synthetic esto shim can re-export it.
// All other este globals (fragment, context, unit, sh, prompt, ls, console)
// are now registered directly from Rust in builtins.rs.

// JSX factory (Step 4 will move this to Rust)
globalThis.__esto_h = function h(type, props, ...children) {
  const kids = children.flat(Infinity).filter(c => c != null && c !== false)
  if (type && type.__estoFragment)  return { $fragment: true, children: kids }
  if (type && type.__estoContext)   return { $context: true, value: props?.value ?? null, data: props?.data != null ? JSON.stringify(props.data) : null, children: kids }
  if (type?.__estoKind)             return { $kind: type, item: { ...(props ?? {}) } }
  if (typeof type === 'function')   return { $component: type, props: { ...(props ?? {}), children: kids } }
  throw new Error('esto: invalid JSX node type: ' + String(type))
}
