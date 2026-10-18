// PROTOTYPE parser for the .spec format. Pure: text in, rules out.
//
//   Context: <text>
//   Feature: <name>
//     Rule <id> <kind>:
//       <body lines: prose, Given/When/Then, Examples table>

const KINDS = ['unit', 'type', 'integration']
const RULE = /^Rule ([a-z0-9-]+) <([a-z]+)>:$/

// Whitespace-only edits (re-indenting, aligning steps) must not make checks stale.
const normalize = line => line.trim().replace(/\s+/g, ' ')

export function parseSpec(text) {
  const spec = { context: [], rules: [] }
  let feature = null
  let rule = null

  text.split('\n').forEach((raw, i) => {
    const line = normalize(raw)
    const fail = msg => { throw new Error(`spec line ${i + 1}: ${msg}: "${raw}"`) }

    if (line === '') return
    if (line.startsWith('Context:')) {
      spec.context.push(line.slice('Context:'.length).trim())
      return
    }
    if (line.startsWith('Feature:')) {
      feature = line.slice('Feature:'.length).trim()
      rule = null
      return
    }
    if (line.startsWith('Rule ')) {
      const m = line.match(RULE) ?? fail('expected "Rule <id> <kind>:"')
      if (!feature) fail('Rule outside of a Feature')
      if (!KINDS.includes(m[2])) fail(`unknown kind, expected one of ${KINDS.join(', ')}`)
      rule = { feature, id: m[1], kind: m[2], body: [] }
      spec.rules.push(rule)
      return
    }
    if (!rule) fail('text outside of a Rule')
    rule.body.push(line)
  })

  for (const r of spec.rules) r.body = r.body.join('\n')
  return spec
}

// Marker that ties a check to a rule: // @spec <feature>/<id> #<hash>
export const MARKER = /@spec ([a-z0-9-]+\/[a-z0-9-]+) #([0-9a-f]+)/

// Where each kind of check lives. The surveyor infers the kind from the path,
// so moving a rule to another kind is exit (old check) + enter (new check).
export const CHECK_DIRS = {
  unit: 'tests/unit',
  type: 'tests/types',
  integration: 'tests/e2e',
}
