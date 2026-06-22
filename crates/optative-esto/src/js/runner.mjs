import { createHash } from 'node:crypto'
import { mkdirSync, writeFileSync, existsSync } from 'node:fs'

const file = process.argv[2]
const dryRun = process.env.ESTO_DRY_RUN === '1'
const quiet = process.env.ESTO_QUIET === '1'

if (!file) {
  process.stderr.write('esto run: missing file argument\n')
  process.exit(1)
}

// ── task emission (prompt reactions) ──────────────────────────────────────
// Content-addressed context files: identical entry = one file (dedup across leaves).
const sha = (s) => createHash('sha256').update(s).digest('hex').slice(0, 12)

function emitTask(key, contextChain, body) {
  mkdirSync('tasks', { recursive: true })
  mkdirSync('.esto/context', { recursive: true })
  const refs = contextChain.map(entry => {
    const p = `.esto/context/${sha(entry)}.md`
    if (!existsSync(p)) writeFileSync(p, entry)
    return `  ${p} — ${entry.split('\n')[0].slice(0, 60)}`
  })
  const safe = key.replace(/[^\w.-]/g, '_')
  writeFileSync(
    `tasks/${safe}.md`,
    `# ${key}\n` +
    (refs.length ? `Context (read once; same id = same content):\n${refs.join('\n')}\n\n` : '') +
    body + '\n'
  )
}

// ── shared reconcile counters ──────────────────────────────────────────────
let totalEnter = 0, totalUpdate = 0, totalExit = 0, totalUnchanged = 0, totalErrors = 0

function log(event, key) {
  if (!quiet) process.stderr.write(`[${event}] ${key}\n`)
}

// context: string[] — the chain of ancestor Context values for this item.
// fn return value: if { $prompt }, emits a grounded task instead of treating as done.
async function act(event, key, context, fn) {
  log(event, key)
  if (!dryRun) {
    try {
      const r = await fn()
      if (r && r.$prompt) emitTask(key, context, r.$prompt)
    } catch (e) {
      process.stderr.write(`[error] ${key}: ${e.message}\n`)
      totalErrors++
    }
  }
}

// leaves: { item, context }[] — context is the ancestor chain for each item.
// Identical algorithm for Tier 1 and Tier 2; only how leaves are produced differs.
async function reconcileKind(kind, leaves) {
  const C = await kind.observe()
  const dByKey = new Map(leaves.map(l => [kind.key(l.item), l]))
  const cByKey = new Map(C.map(i => [kind.key(i), i]))

  for (const [k, { item: d, context }] of dByKey) {
    if (!cByKey.has(k)) {
      await act('enter', k, context, () => kind.enter(d))
      totalEnter++
    } else {
      const c = cByKey.get(k)
      if (kind.value(d) !== kind.value(c)) {
        await act('update', k, context, () => kind.update(d, c))
        totalUpdate++
      } else {
        totalUnchanged++
      }
    }
  }

  for (const [k, c] of cByKey) {
    if (!dByKey.has(k)) {
      await act('exit', k, [], () => kind.exit(c))
      totalExit++
    }
  }
}

// ── Tier 2: JSX tree → flat { kind, item, context }[] ─────────────────────
// ctx: string[] — accumulated Context values root→leaf.
function reduce(node, ctx = []) {
  if (node == null || node === false) return []
  if (Array.isArray(node))  return node.flatMap(n => reduce(n, ctx))
  if (node.$fragment)       return node.children.flatMap(n => reduce(n, ctx))
  if (node.$context)        return node.children.flatMap(n => reduce(n, node.value == null ? ctx : [...ctx, node.value]))
  if (node.$component)      return reduce(node.$component(node.props), ctx)
  if (node.$kind)           return [{ kind: node.$kind, item: node.item, context: ctx }]
  throw new Error(`esto: unrecognised JSX node: ${JSON.stringify(node).slice(0, 120)}`)
}

// ── dispatch ──────────────────────────────────────────────────────────────
const absUrl = `file://${file}`
const mod = await import(absUrl)

if (file.endsWith('.jsx')) {
  // Tier 2: default export is a function () => JSX tree
  const rootFn = mod.default
  const root = typeof rootFn === 'function' ? rootFn() : rootFn
  const leaves = reduce(root)

  const byKind = new Map()
  for (const leaf of leaves) {
    if (!byKind.has(leaf.kind)) byKind.set(leaf.kind, [])
    byKind.get(leaf.kind).push({ item: leaf.item, context: leaf.context })
  }

  for (const [kind, kindLeaves] of byKind) {
    await reconcileKind(kind, kindLeaves)
  }
} else {
  // Tier 1: default export is a target with desired() and observe()
  const target = mod.default
  const desiredItems = await target.desired()
  const leaves = desiredItems.map(item => ({ item, context: [] }))
  await reconcileKind(target, leaves)
}

if (!quiet) {
  process.stderr.write(`reconciled: ${totalEnter} enter, ${totalUpdate} update, ${totalExit} exit (${totalUnchanged} unchanged)\n`)
}

process.exitCode = dryRun ? (totalEnter + totalUpdate + totalExit) : totalErrors
