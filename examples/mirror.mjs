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

import { defineTarget, sh } from 'esto'
import { readFileSync, readdirSync, existsSync } from 'node:fs'

const write = (i) => sh`mkdir -p out && printf '%s\n' ${i.content} > out/${i.name}.txt`

export default defineTarget({
  key:   (i) => i.name,
  value: (i) => i.content,

  desired: () => {
    const text = readFileSync('manifest.txt', 'utf8')
    return text.trim().split('\n').filter(Boolean).map(line => {
      const eq = line.indexOf('=')
      return eq === -1
        ? { name: line.trim(), content: '' }
        : { name: line.slice(0, eq).trim(), content: line.slice(eq + 1).trim() }
    })
  },

  observe: () => {
    if (!existsSync('out')) return []
    return readdirSync('out')
      .filter(f => f.endsWith('.txt'))
      .map(f => ({
        name: f.slice(0, -4),
        content: readFileSync(`out/${f}`, 'utf8').trim(),
      }))
  },

  enter:  (i) => write(i),
  update: (i) => write(i),
  exit:   (i) => sh`rm -f out/${i.name}.txt`,
})
