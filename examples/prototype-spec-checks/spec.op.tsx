/**
 * PROTOTYPE — reconciles spec/auth.spec against the checks in tests/.
 *
 *   desired  = every Rule in the spec, keyed by <feature>/<id>, valued by a hash of its body
 *   observed = every `@spec <feature>/<id> #<hash>` marker under the kind's test dir
 *
 *   enter  → rule has no check           → task: write one
 *   update → marker hash != rule hash    → task: check is stale, rewrite it
 *   exit   → marker for a removed rule   → task: delete the orphaned check
 *
 * Usage: esto run --dry-run spec.op.tsx   (show the diff)
 *        esto run spec.op.tsx             (write agent tasks to tasks/)
 */

import { h, Context, unit, prompt, optativeSet, read, sh, hash } from 'esto'
import { parseSpec, MARKER, CHECK_DIRS } from './lib/spec.mjs'

const ruleHash = (body: string) => hash(body).slice(0, 8)

interface Check { key: string; hash: string; file: string }

const survey = (dir: string): Check[] =>
  sh`grep -rnE ${MARKER.source} ${dir} || true`
    .split('\n')
    .filter(Boolean)
    .map(line => {
      const [file] = line.split(':')
      const m = line.match(MARKER)!
      return { key: m[1], hash: m[2], file }
    })

const marker = (r: any) => `// @spec ${r.feature}/${r.id} #${ruleHash(r.body)}`

const checkUnit = (kind: keyof typeof CHECK_DIRS, how: string) => unit({
  key:   (r: any) => r.key ?? `${r.feature}/${r.id}`,
  value: (r: any) => r.hash ?? ruleHash(r.body),
  reconciler: optativeSet({ observe: () => survey(CHECK_DIRS[kind]) }),

  enter: (r: any) => prompt`
Rule ${r.feature}/${r.id} has no ${kind} check. Write ${how} under ${CHECK_DIRS[kind]}/ that enforces:

${r.body}

Put this marker on the line above the check: ${marker(r)}
The check must fail if the behavior described above is broken.`,

  update: (r: any, old: Check) => prompt`
Rule ${r.feature}/${r.id} changed since its check in ${old.file} was written (marker #${old.hash}).
Update the check so it enforces the current rule:

${r.body}

Then replace its marker with: ${marker(r)}`,

  exit: (old: Check) => prompt`
Rule ${old.key} is no longer in the spec. Delete its ${kind} check in ${old.file} (marker #${old.hash}).`,
})

const UnitCheck        = checkUnit('unit', 'a vitest unit test')
const TypeCheck        = checkUnit('type', 'a vitest type test (expectTypeOf / @ts-expect-error) in a .test-d.ts file')
const IntegrationCheck = checkUnit('integration', 'a Playwright test against the running app')
const byKind = { unit: UnitCheck, type: TypeCheck, integration: IntegrationCheck }

const spec = parseSpec(read('spec/auth.spec'))

export default (): unknown => (
  <Context value={spec.context.join('\n')}>
    {spec.rules.map((r: any) => h(byKind[r.kind], r))}
  </Context>
)
