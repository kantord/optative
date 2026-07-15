//! An optional, opt-in resolver/loader pair that confines relative-import
//! resolution to a fixed directory tree and supports extension-fallback
//! resolution (e.g. `import Foo from './Foo'` finds `./Foo.jsx`).
//!
//! This is *not* used by [`crate::run_script`]'s default behavior — esto
//! scripts already have broad filesystem access via other builtins, so
//! path-confined imports would be a pointless (and behavior-changing)
//! restriction for them. Callers that need sandboxed import resolution
//! (e.g. a caller evaluating semi-trusted layout files) can opt in via
//! [`crate::run_script_with_loader`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rquickjs::loader::{Loader, Resolver};
use rquickjs::{Ctx, Module};

use crate::engine::is_script_file;
use crate::jsx::transform_source;

/// Resolves relative import specifiers (starting with `./` or `../`) against
/// a fixed `base_dir`, using [`oxc_resolver`] for extension-fallback lookup.
/// Resolved paths outside `allowed_root` are rejected.
pub struct ConfinedFsResolver {
    allowed_root: PathBuf,
    base_dir: PathBuf,
    resolver: oxc_resolver::Resolver,
}

impl ConfinedFsResolver {
    /// Both the resolution base directory and the confinement root are
    /// `base_dir` (canonicalized). Imports may not escape it.
    pub fn new(base_dir: PathBuf) -> Self {
        let canonical_root = base_dir.canonicalize().unwrap_or_else(|_| base_dir.clone());
        let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
            modules: vec![],
            extensions: vec![".js".into(), ".jsx".into(), ".ts".into(), ".tsx".into()],
            ..oxc_resolver::ResolveOptions::default()
        });
        Self {
            allowed_root: canonical_root.clone(),
            base_dir: canonical_root,
            resolver,
        }
    }
}

impl Resolver for ConfinedFsResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
    ) -> rquickjs::Result<String> {
        if !name.starts_with("./") && !name.starts_with("../") {
            return Err(rquickjs::Error::new_resolving(base, name));
        }

        let resolve_dir = if Path::new(base).is_absolute() {
            Path::new(base)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.base_dir.clone())
        } else {
            self.base_dir.clone()
        };

        let resolution = self
            .resolver
            .resolve(&resolve_dir, name)
            .map_err(|_| rquickjs::Error::new_resolving(base, name))?;

        let resolved = resolution.full_path().to_path_buf();
        let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());

        if !canonical.starts_with(&self.allowed_root) {
            return Err(rquickjs::Error::new_resolving(base, name));
        }

        canonical
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| rquickjs::Error::new_resolving(base, name))
    }
}

/// Loads JS/JSX/TS/TSX modules from disk, running [`transform_source`] on
/// each file (when [`is_script_file`] recognizes its extension) before
/// handing the source to QuickJS. Records each successfully-loaded path
/// into the shared `loaded_paths` vec, so a caller can e.g. watch those
/// files for changes.
pub struct ConfinedFsLoader {
    loaded_paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl ConfinedFsLoader {
    pub fn new(loaded_paths: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        Self { loaded_paths }
    }
}

impl Loader for ConfinedFsLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Module<'js>> {
        let source =
            std::fs::read_to_string(name).map_err(|_| rquickjs::Error::new_loading(name))?;
        self.loaded_paths.lock().unwrap().push(PathBuf::from(name));
        let source = if is_script_file(name) {
            transform_source(&source, name)
        } else {
            source
        };
        Module::declare(ctx.clone(), name, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{CatchResultExt, Context, Runtime};

    /// Sets up a fresh runtime/context wired with `ConfinedFsResolver`/
    /// `ConfinedFsLoader` confined to `allowed_root`, and evaluates `entry`
    /// (an absolute path) as a module.
    fn eval_confined(allowed_root: &Path, entry: &Path) -> rquickjs::Result<i32> {
        let runtime = Runtime::new()?;
        let loaded_paths = Arc::new(Mutex::new(Vec::new()));
        runtime.set_loader(
            ConfinedFsResolver::new(allowed_root.to_path_buf()),
            ConfinedFsLoader::new(loaded_paths),
        );
        let context = Context::full(&runtime)?;
        context.with(|ctx| {
            let entry_str = entry.to_str().unwrap().to_string();
            let src = std::fs::read_to_string(entry).unwrap();
            let module = Module::declare(ctx.clone(), entry_str, src)?;
            let (module, promise) = module.eval().catch(&ctx).map_err(|e| {
                eprintln!("eval error: {e}");
                rquickjs::Error::Exception
            })?;
            promise.finish::<()>()?;
            module.get::<_, i32>("default")
        })
    }

    #[test]
    fn resolves_sibling_import_without_extension() {
        let tmp = std::env::temp_dir().join(format!(
            "optative_script_confined_ext_{}_{}",
            std::process::id(),
            "resolves_sibling_import_without_extension"
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("sibling.js"), "export const answer = 42;").unwrap();
        let entry = tmp.join("entry.js");
        std::fs::write(
            &entry,
            "import { answer } from './sibling';\nexport default answer;",
        )
        .unwrap();

        let result = eval_confined(&tmp, &entry).unwrap();
        assert_eq!(result, 42);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rejects_import_that_escapes_allowed_root() {
        let tmp = std::env::temp_dir().join(format!(
            "optative_script_confined_escape_{}_{}",
            std::process::id(),
            "rejects_import_that_escapes_allowed_root"
        ));
        let allowed = tmp.join("allowed");
        std::fs::create_dir_all(&allowed).unwrap();
        // Exists on disk, but outside `allowed` — a naive resolver would
        // happily find it via the relative specifier below.
        std::fs::write(tmp.join("outer.js"), "export const secret = 1;").unwrap();
        let entry = allowed.join("entry.js");
        std::fs::write(
            &entry,
            "import { secret } from '../outer';\nexport default secret;",
        )
        .unwrap();

        let result = eval_confined(&allowed, &entry);
        assert!(
            result.is_err(),
            "expected escaping import to be rejected, got: {:?}",
            result.ok()
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
