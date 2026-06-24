// esto/fs — filesystem locator components
// import { GitRepo, Folder, File } from 'esto/fs'
//
// Components are render-prop: <GitRepo>{({ repoRoot, File, Folder }) => …}</GitRepo>
// Paths handed to render props are root-relative (never absolute machine paths).

// esto's h() puts a single-function child in an array: unwrap it.
function rp(children) {
  return Array.isArray(children) ? children[0] : children
}

function globPaths(pattern) {
  return JSON.parse(globalThis.__esto_glob(pattern))
}

// Factory: returns a File component rooted at rootDir (null = use pattern as-is).
function makeFile(rootDir) {
  return function File({ glob: pattern, children }) {
    const full = rootDir ? `${rootDir}/${pattern}` : pattern
    const paths = globPaths(full).filter(p => !globalThis.__esto_is_dir(p))
    const rels = rootDir ? paths.map(p => p.slice(rootDir.length + 1)) : paths
    const render = rp(children)
    return rels.map(file => render({ file }))
  }
}

// Factory: returns a Folder component rooted at rootDir.
function makeFolder(rootDir) {
  return function Folder({ glob: pattern, children }) {
    const full = rootDir ? `${rootDir}/${pattern}` : pattern
    const dirs = globPaths(full).filter(p => globalThis.__esto_is_dir(p))
    const render = rp(children)
    return dirs.map(absDir => {
      const relDir = rootDir ? absDir.slice(rootDir.length + 1) : absDir
      return render({
        dir: relDir,
        File: makeFile(absDir),
        Folder: makeFolder(absDir),
      })
    })
  }
}

// Top-level File/Folder: glob relative to CWD; returns paths as given by the OS.
export const File = makeFile(null)
export const Folder = makeFolder(null)

// GitRepo: resolves git rev-parse --show-toplevel; hands down File/Folder scoped to repoRoot.
export function GitRepo({ children }) {
  const root = globalThis.__esto_git_root()
  return rp(children)({
    repoRoot: root,
    File: makeFile(root),
    Folder: makeFolder(root),
  })
}
