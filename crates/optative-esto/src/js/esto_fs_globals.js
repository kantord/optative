// esto/fs API globals — eval'd after esto_globals.js.
// Assumes __esto_h, __esto_unit, __esto_sh, __esto_exists, __esto_read,
// __esto_hash, __esto_glob, __esto_is_dir, __esto_cwd, __esto_git_root
// are already on globalThis.

function __rp(children) {
  return Array.isArray(children) ? children[0] : children
}

function __globPaths(pattern) {
  return JSON.parse(globalThis.__esto_glob(pattern))
}

function __isDir(p) { return globalThis.__esto_is_dir(p) }
function __cwd()    { return globalThis.__esto_cwd() }

function __dirname(p) {
  const i = p.lastIndexOf('/')
  return i >= 0 ? p.slice(0, i) : '.'
}

// Returns the claim-File component used inside a supervisor render prop.
function __makeClaimFile() {
  return function File({ name, glob: globPattern, content, children }) {
    return {
      $estoFsClaim: true,
      matcher: name
        ? { kind: 'name', name }
        : globPattern
          ? { kind: 'glob', pattern: globPattern }
          : { kind: 'all' },
      content: content !== undefined ? content : null,
      body: children != null ? __rp(children) : null,
    }
  }
}

// Walk a JSX tree and collect $estoFsClaim descriptors.
function __extractClaims(node) {
  if (node == null || node === false || node === undefined) return { claims: [], body: [] }
  if (Array.isArray(node)) {
    const rs = node.map(__extractClaims)
    return { claims: rs.flatMap(r => r.claims), body: rs.flatMap(r => r.body) }
  }
  if (typeof node !== 'object') return { claims: [], body: [] }
  if (node.$estoFsClaim) return { claims: [node], body: [] }
  if (node.$fragment)    return __extractClaims(node.children)
  if (node.$component) {
    const result = node.$component(node.props)
    return __extractClaims(result)
  }
  return { claims: [], body: [node] }
}

const __PRUNE_MAX_ABS = 10
const __PRUNE_MAX_PCT = 0.5

function __scopeSupervise(absParentDir, name, render) {
  const absDir = absParentDir + '/' + name
  const scopeAbsPaths = __globPaths(absDir + '/**/*').filter(p => !__isDir(p))
  const scopeRels = scopeAbsPaths.map(p => p.slice(absDir.length + 1))

  const tree = render({ File: __makeClaimFile(), Folder: __makeScopedFolder(absDir) })
  const { claims, body } = __extractClaims(tree)

  if (scopeRels.length > 0 && claims.length === 0) {
    throw new Error(
      'esto/fs: circuit breaker — zero claims on non-empty scope "' + name + '" (' +
      scopeRels.length + ' files). Add <File/> to keep all, or check your render prop.'
    )
  }

  const claimMap = new Map()
  function addClaim(rel, content, body) {
    if (!claimMap.has(rel)) claimMap.set(rel, { content: null, bodies: [] })
    const entry = claimMap.get(rel)
    if (content !== null) {
      if (entry.content !== null && entry.content !== content) {
        throw new Error('esto/fs: content conflict for "' + rel + '" — two claims specify different content')
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
      matchedRels = __globPaths(absDir + '/' + c.matcher.pattern)
        .filter(p => !__isDir(p))
        .map(p => p.slice(absDir.length + 1))
    } else {
      const alreadyExists = scopeRels.includes(c.matcher.name)
      matchedRels = (alreadyExists || c.content !== null) ? [c.matcher.name] : []
    }
    for (const rel of matchedRels) addClaim(rel, c.content, c.body)
  }

  const pruneRels = scopeRels.filter(r => !claimMap.has(r))
  if (
    pruneRels.length > __PRUNE_MAX_ABS ||
    (scopeRels.length > 0 && pruneRels.length / scopeRels.length > __PRUNE_MAX_PCT)
  ) {
    throw new Error(
      'esto/fs: circuit breaker — ' + pruneRels.length + '/' + scopeRels.length +
      ' files in "' + name + '" would be pruned. Run --dry-run first, then narrow scope or add more claims.'
    )
  }

  const ManagedFile = __esto_unit({
    key:   f => f.path,
    value: f => f.hash,
    observe: () =>
      __globPaths(absDir + '/**/*')
        .filter(p => !__isDir(p))
        .map(p => {
          const rel = p.slice(absDir.length + 1)
          return { path: rel, absolutePath: p, hash: __esto_hash(__esto_read(p)), desiredContent: null }
        }),
    enter:  f => __esto_sh`mkdir -p ${__dirname(f.absolutePath)} && printf '%s' ${f.desiredContent ?? ''} > ${f.absolutePath}`,
    update: f => { if (f.desiredContent !== null) __esto_sh`printf '%s' ${f.desiredContent} > ${f.absolutePath}` },
    exit:   f => __esto_sh`rm -f ${f.absolutePath}`,
  })

  const managedLeaves = []
  for (const [rel, { content: desiredContent }] of claimMap) {
    const absPath = absDir + '/' + rel
    const desiredHash = desiredContent !== null
      ? __esto_hash(desiredContent)
      : __esto_exists(absPath) ? __esto_hash(__esto_read(absPath)) : __esto_hash('')
    managedLeaves.push(__esto_h(ManagedFile, { path: rel, absolutePath: absPath, hash: desiredHash, desiredContent }))
  }

  const bodyNodes = []
  for (const [rel, { bodies }] of claimMap) {
    for (const bodyFn of bodies) {
      const result = bodyFn({ file: rel })
      if (result != null && result !== false) bodyNodes.push(result)
    }
  }

  return [...managedLeaves, ...body, ...bodyNodes]
}

function __makeScopedFolder(parentDir) {
  return function Folder({ name, glob: globPattern, children }) {
    if (name) return __scopeSupervise(parentDir, name, __rp(children))
    return __makeFolderEnumerate(parentDir)({ glob: globPattern, children })
  }
}

function __makeFolderEnumerate(rootDir) {
  return function Folder({ glob: pattern, children }) {
    const full = rootDir ? rootDir + '/' + pattern : pattern
    const dirs = __globPaths(full).filter(p => __isDir(p))
    const render = __rp(children)
    return dirs.map(absDir => {
      const relDir = rootDir ? absDir.slice(rootDir.length + 1) : absDir
      return render({ dir: relDir, File: __makeFile(absDir), Folder: __makeScopedFolder(absDir) })
    })
  }
}

function __makeFile(rootDir) {
  return function File({ glob: pattern, children }) {
    const full = rootDir ? rootDir + '/' + pattern : pattern
    const paths = __globPaths(full).filter(p => !__isDir(p))
    const rels = rootDir ? paths.map(p => p.slice(rootDir.length + 1)) : paths
    const render = __rp(children)
    return rels.map(file => render({ file }))
  }
}

globalThis.__esto_fs_File = __makeFile(null)

globalThis.__esto_fs_Folder = function Folder({ name, glob: globPattern, children }) {
  if (name) return __scopeSupervise(__cwd(), name, __rp(children))
  return __makeFolderEnumerate(null)({ glob: globPattern, children })
}

globalThis.__esto_fs_GitRepo = function GitRepo({ children }) {
  const root = globalThis.__esto_git_root()
  return __rp(children)({ repoRoot: root, File: __makeFile(root), Folder: __makeScopedFolder(root) })
}
