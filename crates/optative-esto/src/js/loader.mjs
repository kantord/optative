import { readFileSync } from 'node:fs'

// Minimal inline JSX → h() transform. Self-contained; no sucrase/babel dependency.
// Handles: self-closing, open/close, fragments, expression/string/boolean props, spread.
// exprContent() recursively calls transformJSX so JSX inside {map(() => <X />)} is also transformed.
function transformJSX(src) {
  let i = 0

  function eatStr(q) {
    let s = src[i++]
    while (i < src.length) {
      if (src[i] === '\\') { s += src[i++] + src[i++]; continue }
      s += src[i]; if (src[i++] === q) break
    }
    return s
  }

  function eatTemplate() {
    let s = src[i++] // `
    while (i < src.length) {
      if (src[i] === '\\') { s += src[i++] + src[i++]; continue }
      if (src[i] === '$' && src[i + 1] === '{') {
        s += src[i++] + src[i++] // ${
        let d = 1
        while (i < src.length && d > 0) {
          if (src[i] === '{') d++
          else if (src[i] === '}') { if (--d === 0) break }
          else if (src[i] === '`') { s += eatTemplate(); continue }
          s += src[i++]
        }
        s += src[i++]; continue // }
      }
      s += src[i]; if (src[i++] === '`') break
    }
    return s
  }

  // Collect content between { } (exclusive), handling nested braces and strings.
  // Recursively calls transformJSX so JSX inside expressions is also transformed.
  function exprContent() {
    let raw = '', d = 1
    while (i < src.length && d > 0) {
      const c = src[i]
      if (c === '{') { d++; raw += src[i++]; continue }
      if (c === '}') { if (--d === 0) break; raw += src[i++]; continue }
      if (c === '"' || c === "'") { raw += eatStr(c); continue }
      if (c === '`') { raw += eatTemplate(); continue }
      raw += src[i++]
    }
    return transformJSX(raw)
  }

  function parseElement() {
    // Fragment <>
    if (src[i] === '>') {
      i++
      const kids = parseChildren()
      while (i < src.length && src[i] !== '>') i++; i++
      return kids.length ? `h(Fragment, null, ${kids.join(', ')})` : `h(Fragment, null)`
    }

    let name = ''
    while (i < src.length && /[\w.]/.test(src[i])) name += src[i++]
    if (!name) return '<'

    const attrs = []
    while (i < src.length) {
      while (i < src.length && /\s/.test(src[i])) i++
      if (src[i] === '/' || src[i] === '>') break
      if (src[i] === '{') {
        i++
        const expr = exprContent(); i++
        attrs.push('...' + expr.replace(/^\s*\.\.\.\s*/, ''))
        continue
      }
      let aname = ''
      while (i < src.length && /[\w-]/.test(src[i])) aname += src[i++]
      if (!aname) { i++; continue }
      while (i < src.length && /\s/.test(src[i])) i++
      if (src[i] === '=') {
        i++
        if (src[i] === '"' || src[i] === "'") {
          attrs.push(`${aname}: ${eatStr(src[i])}`)
        } else if (src[i] === '{') {
          i++
          const expr = exprContent(); i++
          attrs.push(`${aname}: ${expr}`)
        }
      } else {
        attrs.push(`${aname}: true`)
      }
    }

    const propsStr = attrs.length ? `{ ${attrs.join(', ')} }` : 'null'

    if (src[i] === '/') { i += 2; return `h(${name}, ${propsStr})` }

    i++ // >
    const kids = parseChildren()
    while (i < src.length && src[i] !== '>') i++; i++
    const childStr = kids.length ? `, ${kids.join(', ')}` : ''
    return `h(${name}, ${propsStr}${childStr})`
  }

  function parseChildren() {
    const kids = []
    while (i < src.length) {
      if (src[i] === '<' && src[i + 1] === '/') break
      if (src[i] === '<') { i++; kids.push(parseElement()); continue }
      if (src[i] === '{') {
        i++
        const expr = exprContent(); i++
        const e = expr.trim(); if (e) kids.push(e)
        continue
      }
      let text = ''
      while (i < src.length && src[i] !== '<' && src[i] !== '{') text += src[i++]
      const t = text.trim(); if (t) kids.push(JSON.stringify(t))
    }
    return kids
  }

  function isJSXOpen(at) {
    if (src[at] !== '<') return false
    const n = src[at + 1]
    if (n === '>') return true
    if (!/[A-Za-z]/.test(n)) return false
    let j = at - 1
    while (j >= 0 && /\s/.test(src[j])) j--
    if (j < 0) return true
    const prev = src.slice(Math.max(0, j - 5), j + 1)
    return /[=,([?:!&|~^%+\-*]$/.test(prev) || prev.endsWith('return') || prev.endsWith('=>')
  }

  let out = ''
  while (i < src.length) {
    const c = src[i]
    if (c === '"' || c === "'") { out += eatStr(c); continue }
    if (c === '`') { out += eatTemplate(); continue }
    if (c === '/' && src[i + 1] === '/') { while (i < src.length && src[i] !== '\n') out += src[i++]; continue }
    if (c === '/' && src[i + 1] === '*') {
      out += src[i++] + src[i++]
      while (i < src.length && !(src[i] === '*' && src[i + 1] === '/')) out += src[i++]
      if (i < src.length) out += src[i++] + src[i++]
      continue
    }
    if (isJSXOpen(i)) { i++; out += parseElement(); continue }
    out += src[i++]
  }
  return out
}

export async function resolve(specifier, context, nextResolve) {
  if (specifier === 'esto') {
    return { shortCircuit: true, url: process.env.ESTO_RUNTIME_URL }
  }
  return nextResolve(specifier, context)
}

export async function load(url, context, nextLoad) {
  if (url.endsWith('.jsx')) {
    const path = new URL(url).pathname
    const src = readFileSync(path, 'utf8')
    return { shortCircuit: true, format: 'module', source: transformJSX(src) }
  }
  return nextLoad(url, context)
}
