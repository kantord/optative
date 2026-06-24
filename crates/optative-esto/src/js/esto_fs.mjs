// esto/fs — filesystem locator + scope-supervisor components
// import { GitRepo, Folder, File } from 'esto/fs'
//
// Enumerate mode:  <File glob="**/*.ts">{({file}) => …}</File>
// Supervisor mode: <Folder name="docs/api">{({File}) => <File name="x.md" content={c}/></Folder>
//   - <File> children are CLAIMS; unclaimed files in scope are pruned.
//   - Circuit breaker: zero-claims-on-non-empty + 10 abs / 50% relative prune limits.

import { h, unit, sh, exists, read, hash } from 'esto'

// esto's h() wraps a single-function child in an array; unwrap it.
function rp(children) {
  return Array.isArray(children) ? children[0] : children
}

function globPaths(pattern) {
  return JSON.parse(globalThis.__esto_glob(pattern))
}

function isDir(p) { return globalThis.__esto_is_dir(p) }

function cwd() { return globalThis.__esto_cwd() }

function dirname(p) {
  const i = p.lastIndexOf('/')
  return i >= 0 ? p.slice(0, i) : '.'
}

// ── Claim descriptor ──────────────────────────────────────────────────────────
// ClaimFile is the File handed to a supervisor render prop.
// It records what the user claims, returning { $estoFsClaim } instead of JSX.

function makeClaimFile() {
  return function File({ name, glob: globPattern, content, children }) {
    return {
      $estoFsClaim: true,
      matcher: name
        ? { kind: 'name', name }
        : globPattern
          ? { kind: 'glob', pattern: globPattern }
          : { kind: 'all' },
      content: content !== undefined ? content : null,
      body: children != null ? rp(children) : null,
    }
  }
}

// ── extractClaims ─────────────────────────────────────────────────────────────
// Walk a JSX tree, calling $component functions to surface claim descriptors.
// Returns { claims: [...], body: [...] } where body is passthrough JSX for reduce().

function extractClaims(node) {
  if (node == null || node === false || node === undefined) return { claims: [], body: [] }
  if (Array.isArray(node)) {
    const rs = node.map(extractClaims)
    return { claims: rs.flatMap(r => r.claims), body: rs.flatMap(r => r.body) }
  }
  if (typeof node !== 'object') return { claims: [], body: [] }
  if (node.$estoFsClaim) return { claims: [node], body: [] }
  if (node.$fragment) return extractClaims(node.children)
  if (node.$component) {
    const result = node.$component(node.props)
    return extractClaims(result)
  }
  // $kind, $context, $prompt — passthrough body nodes for reduce()
  return { claims: [], body: [node] }
}

// ── Safety thresholds ─────────────────────────────────────────────────────────

const PRUNE_MAX_ABS = 10
const PRUNE_MAX_PCT = 0.5

// ── scopeSupervise ────────────────────────────────────────────────────────────
// Core supervisor logic: resolve claims, compute desired set, emit managed-file
// leaf nodes + body nodes. No changes to reduce()/reconcile_kind() in Rust.

function scopeSupervise(absParentDir, name, render) {
  const absDir = `${absParentDir}/${name}`

  // Current scope files
  const scopeAbsPaths = globPaths(`${absDir}/**/*`).filter(p => !isDir(p))
  const scopeRels = scopeAbsPaths.map(p => p.slice(absDir.length + 1))

  // Call render prop with claim-File + scoped Folder
  const tree = render({
    File: makeClaimFile(),
    Folder: makeScopedFolder(absDir),
  })

  const { claims, body } = extractClaims(tree)

  // Zero-claims guard
  if (scopeRels.length > 0 && claims.length === 0) {
    throw new Error(
      `esto/fs: circuit breaker — zero claims on non-empty scope "${name}" (${scopeRels.length} files). ` +
      `Add <File/> to keep all, or check your render prop.`
    )
  }

  // Resolve claims → claimMap: relPath → { content: string|null, bodies: fn[] }
  const claimMap = new Map()

  function addClaim(rel, content, body) {
    if (!claimMap.has(rel)) claimMap.set(rel, { content: null, bodies: [] })
    const entry = claimMap.get(rel)
    if (content !== null) {
      if (entry.content !== null && entry.content !== content) {
        throw new Error(`esto/fs: content conflict for "${rel}" — two claims specify different content`)
      }
      entry.content = content
    }
    if (body) entry.bodies.push(body)
  }

  for (const c of claims) {
    let matchedRels
    if (c.matcher.kind === 'all') {
      matchedRels = [...scopeRels]
    } else if (c.matcher.kind === 'glob') {
      matchedRels = globPaths(`${absDir}/${c.matcher.pattern}`)
        .filter(p => !isDir(p))
        .map(p => p.slice(absDir.length + 1))
    } else {
      // name: include only if file exists OR claim has content (CREATE)
      const alreadyExists = scopeRels.includes(c.matcher.name)
      matchedRels = (alreadyExists || c.content !== null) ? [c.matcher.name] : []
    }
    for (const rel of matchedRels) addClaim(rel, c.content, c.body)
  }

  // Prune check
  const pruneRels = scopeRels.filter(r => !claimMap.has(r))
  if (
    pruneRels.length > PRUNE_MAX_ABS ||
    (scopeRels.length > 0 && pruneRels.length / scopeRels.length > PRUNE_MAX_PCT)
  ) {
    throw new Error(
      `esto/fs: circuit breaker — ${pruneRels.length}/${scopeRels.length} files in "${name}" would be pruned. ` +
      `Run --dry-run first, then narrow scope or add more claims.`
    )
  }

  // ManagedFile unit for this scope (one unit per supervisor call — unique __estoId)
  const ManagedFile = unit({
    key:   (f) => f.path,
    value: (f) => f.hash,
    observe: () =>
      globPaths(`${absDir}/**/*`)
        .filter(p => !isDir(p))
        .map(p => {
          const rel = p.slice(absDir.length + 1)
          return { path: rel, absolutePath: p, hash: hash(read(p)), desiredContent: null }
        }),
    enter: (f) => sh`mkdir -p ${dirname(f.absolutePath)} && printf '%s' ${f.desiredContent ?? ''} > ${f.absolutePath}`,
    update: (f) => {
      if (f.desiredContent !== null) {
        sh`printf '%s' ${f.desiredContent} > ${f.absolutePath}`
      }
    },
    exit:  (f) => sh`rm -f ${f.absolutePath}`,
  })

  // Desired leaf nodes — one per claimed file
  const managedLeaves = []
  for (const [rel, { content: desiredContent }] of claimMap) {
    const absPath = `${absDir}/${rel}`
    const desiredHash = desiredContent !== null
      ? hash(desiredContent)
      : exists(absPath) ? hash(read(absPath)) : hash('')
    managedLeaves.push(h(ManagedFile, { path: rel, absolutePath: absPath, hash: desiredHash, desiredContent }))
  }

  // Body nodes from claim render props (assertions, sub-units, etc.)
  const bodyNodes = []
  for (const [rel, { bodies }] of claimMap) {
    for (const bodyFn of bodies) {
      const result = bodyFn({ file: rel })
      if (result != null && result !== false) bodyNodes.push(result)
    }
  }

  return [...managedLeaves, ...body, ...bodyNodes]
}

// ── Folder factories ──────────────────────────────────────────────────────────

// Supervisor Folder (dispatches on name vs glob)
function makeScopedFolder(parentDir) {
  return function Folder({ name, glob: globPattern, children }) {
    if (name) return scopeSupervise(parentDir, name, rp(children))
    return makeFolderEnumerate(parentDir)({ glob: globPattern, children })
  }
}

// Enumerate Folder (unchanged behavior from original)
function makeFolderEnumerate(rootDir) {
  return function Folder({ glob: pattern, children }) {
    const full = rootDir ? `${rootDir}/${pattern}` : pattern
    const dirs = globPaths(full).filter(p => isDir(p))
    const render = rp(children)
    return dirs.map(absDir => {
      const relDir = rootDir ? absDir.slice(rootDir.length + 1) : absDir
      return render({ dir: relDir, File: makeFile(absDir), Folder: makeScopedFolder(absDir) })
    })
  }
}

// ── File factory (enumerate mode) ────────────────────────────────────────────

function makeFile(rootDir) {
  return function File({ glob: pattern, children }) {
    const full = rootDir ? `${rootDir}/${pattern}` : pattern
    const paths = globPaths(full).filter(p => !isDir(p))
    const rels = rootDir ? paths.map(p => p.slice(rootDir.length + 1)) : paths
    const render = rp(children)
    return rels.map(file => render({ file }))
  }
}

// ── Exports ───────────────────────────────────────────────────────────────────

export const File = makeFile(null)

export function Folder({ name, glob: globPattern, children }) {
  if (name) return scopeSupervise(cwd(), name, rp(children))
  return makeFolderEnumerate(null)({ glob: globPattern, children })
}

export function GitRepo({ children }) {
  const root = globalThis.__esto_git_root()
  return rp(children)({ repoRoot: root, File: makeFile(root), Folder: makeScopedFolder(root) })
}
