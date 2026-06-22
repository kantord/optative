/**
 * grounding.eso.jsx — esto run Tier 3 example
 *
 * Demonstrates Context (prose grounding) + prompt reactions.
 * When enter() returns a prompt`...`, esto emits a grounded task file to tasks/<key>.md.
 * Context entries are content-addressed in .esto/context/ (identical entry = one file, dedup).
 *
 * Usage:
 *   esto run examples/grounding.eso.jsx
 *     → tasks/foo.md, tasks/bar.md, .esto/context/ (exactly 2 files)
 *   esto run --dry-run examples/grounding.eso.jsx
 *     → prints [enter] lines, writes nothing, exit code = 2 (delta count)
 */

import { h, Context, defineTarget, prompt } from 'esto'

// Kind: observe returns [] (nothing exists yet) so every desired item triggers enter.
const Doc = defineTarget({
  key:    (i) => i.name,
  value:  (i) => 'needs-doc',
  observe: () => [],
  enter:  (i) => prompt`Document ${i.name}. Be concise: what it does, params, returns.`,
})

export default () => (
  <Context value="Repo: demo — a tiny library.">
    <Context value="Package: core — published, zero-dep.">
      {['foo', 'bar'].map(n => <Doc name={n} />)}
    </Context>
  </Context>
)
