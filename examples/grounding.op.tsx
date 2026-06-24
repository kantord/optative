/**
 * grounding.op.tsx — same as grounding.eso.jsx, written in TypeScript.
 *
 * Demonstrates that .op.tsx files work identically to .op.jsx/.eso.jsx.
 * Type annotations are stripped by oxc before evaluation.
 *
 * Usage: esto run examples/grounding.op.tsx
 */

import { h, Context, unit, prompt } from 'esto'

interface DocProps {
  name: string
}

const Doc = unit({
  key:     (i: DocProps) => i.name,
  value:   (_i: DocProps) => 'needs-doc',
  observe: (): DocProps[] => [],
  enter:   (i: DocProps) => prompt`Document ${i.name}. Be concise: what it does, params, returns.`,
})

export default (): unknown => (
  <Context value="Repo: demo — a tiny library.">
    <Context value="Package: core — published, zero-dep.">
      {(['foo', 'bar'] as string[]).map((n: string) => <Doc name={n} />)}
    </Context>
  </Context>
)
