/**
 * mirror.mjs — esto run example
 *
 * Mirrors a manifest.txt (name=content lines) into out/<name>.txt files.
 * Demonstrates desired/observe/enter/update/exit with the sh tagged template.
 *
 * Usage:
 *   printf 'alpha=one\nbeta=two\n' > manifest.txt
 *   esto run mirror.mjs               # creates out/alpha.txt, out/beta.txt
 *   esto run mirror.mjs               # no-op (already converged)
 *   printf 'alpha=ONE\nbeta=two\n' >> manifest.txt && esto run mirror.mjs  # updates alpha
 *   esto run --dry-run mirror.mjs     # shows diff without writing; exit = delta count
 */

import { unit, sh, read, ls, exists, optativeSet } from 'esto'

const write = (i) => sh`mkdir -p out && printf '%s\n' ${i.content} > out/${i.name}.txt`

export default unit({
  key:   (i) => i.name,
  value: (i) => i.content,

  desired: () => {
    const text = read('manifest.txt')
    return text.trim().split('\n').filter(Boolean).map(line => {
      const eq = line.indexOf('=')
      return eq === -1
        ? { name: line.trim(), content: '' }
        : { name: line.slice(0, eq).trim(), content: line.slice(eq + 1).trim() }
    })
  },

  reconciler: optativeSet({
    observe: () => {
      if (!exists('out')) return []
      return ls('out')
        .filter(f => f.endsWith('.txt'))
        .map(f => ({
          name: f.slice(0, -4),
          content: read(`out/${f}`).trim(),
        }))
    },
  }),

  enter:  (i) => write(i),
  update: (i) => write(i),
  exit:   (i) => sh`rm -f out/${i.name}.txt`,
})
