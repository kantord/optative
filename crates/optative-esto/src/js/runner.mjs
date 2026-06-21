const file = process.argv[2]
const dryRun = process.env.ESTO_DRY_RUN === '1'
const quiet = process.env.ESTO_QUIET === '1'

if (!file) {
  process.stderr.write('esto run: missing file argument\n')
  process.exit(1)
}

// Accept absolute paths only (Rust passes canonicalized path)
const absUrl = `file://${file}`
const mod = await import(absUrl)
const target = mod.default

const D = await target.desired()
const C = await target.observe()

const dByKey = new Map(D.map(i => [target.key(i), i]))
const cByKey = new Map(C.map(i => [target.key(i), i]))

let enter = 0, update = 0, exitN = 0, unchanged = 0, errors = 0

function log(event, key) {
  if (!quiet) process.stderr.write(`[${event}] ${key}\n`)
}

async function act(event, key, fn) {
  log(event, key)
  if (!dryRun) {
    try { await fn() }
    catch (e) {
      process.stderr.write(`[error] ${key}: ${e.message}\n`)
      errors++
    }
  }
}

for (const [k, d] of dByKey) {
  if (!cByKey.has(k)) {
    await act('enter', k, () => target.enter(d))
    enter++
  } else {
    const c = cByKey.get(k)
    if (target.value(d) !== target.value(c)) {
      await act('update', k, () => target.update(d, c))
      update++
    } else {
      unchanged++
    }
  }
}

for (const [k, c] of cByKey) {
  if (!dByKey.has(k)) {
    await act('exit', k, () => target.exit(c))
    exitN++
  }
}

if (!quiet) {
  process.stderr.write(`reconciled: ${enter} enter, ${update} update, ${exitN} exit (${unchanged} unchanged)\n`)
}

process.exitCode = dryRun ? (enter + update + exitN) : errors
