/**
 * mirror.eso.jsx — esto run JSX (Tier 2) example
 *
 * Same behavior as mirror.mjs, now defined with JSX components.
 * File defines a Kind (File) and a component (Manifest) that returns instances.
 * The runner groups leaf instances by kind, calls kind.observe(), and diffs.
 *
 * Usage: same as mirror.mjs, just point to this file.
 *   printf 'alpha=one\nbeta=two\n' > manifest.txt
 *   esto run examples/mirror.eso.jsx
 */

import { h, unit, sh, exists, read, ls, hash } from 'esto'

const sig = (s) => hash(s).slice(0, 8)
const write = (i) => sh`mkdir -p out && printf 'sig=%s\ncontent=%s\n' ${i.sig} ${i.content} > out/${i.name}.txt`

// Kind: no desired() — desired items come from JSX leaf instances (<File ...props />)
const File = unit({
  key:   (i) => i.name,
  value: (i) => i.sig,
  observe: () => exists('out')
    ? ls('out').filter(f => f.endsWith('.txt')).map(f => {
        const m = read(`out/${f}`).match(/^sig=(.*)$/m)
        return { name: f.slice(0, -4), sig: m ? m[1] : '' }
      })
    : [],
  enter:  (i) => write(i),
  update: (i) => write(i),
  exit:   (i) => sh`rm -f out/${i.name}.txt`,
})

// Component: returns an array of <File /> instances (one per manifest line)
const Manifest = () => {
  const text = exists('manifest.txt') ? read('manifest.txt') : ''
  return text.split('\n').map(l => l.trim()).filter(l => l.includes('=')).map(l => {
    const eq = l.indexOf('=')
    const name = l.slice(0, eq).trim()
    const content = l.slice(eq + 1).trim()
    return <File name={name} content={content} sig={sig(content)} />
  })
}

// Root: a function returning the JSX tree
export default () => <Manifest />
