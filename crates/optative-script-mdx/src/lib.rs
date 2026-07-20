//! **Experimental.** Wraps [optative-script](https://github.com/kantord/optative)
//! to add `.op.mdx` (markdown + JSX) script support, entirely additive:
//! `optative-script` itself has no knowledge of markdown. [`run_script`] and
//! [`run_script_with_loader`] dispatch on `path`'s extension — `.op.mdx`
//! files are parsed and lowered to a synthetic TSX source (see [`lower`])
//! before evaluation; every other extension is delegated straight through to
//! `optative_script::run_script`/`run_script_with_loader` unchanged, so
//! callers can point at this crate instead of `optative-script` directly and
//! get both.
//!
//! `.op.mdx` is entry-point only for now: it cannot be `import`ed by other
//! script files (only `esto run some.op.mdx`-style entry-point use works).

pub mod lower;

use optative_script::{Ctx, Loader, Resolver};

const MDX_EXTENSION: &str = ".op.mdx";

#[derive(Debug, thiserror::Error)]
pub enum MdxScriptError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Lower(#[from] lower::LowerError),
    #[error(transparent)]
    Script(#[from] optative_script::ScriptError),
}

/// Like [`optative_script::run_script`], but also handles `.op.mdx` files.
pub fn run_script(
    path: &str,
    entries: &[optative_script::EsEntry],
    setup: fn(&Ctx<'_>) -> Result<(), optative_script::JsError>,
    dry_run: bool,
    quiet: bool,
    limit: Option<usize>,
) -> Result<optative_script::RunStats, MdxScriptError> {
    if path.ends_with(MDX_EXTENSION) {
        let source = std::fs::read_to_string(path)?;
        let lowered = lower::lower_to_tsx(&source, path)?;
        let transformed =
            optative_script::jsx::transform_source(&lowered, lower::synthetic_tsx_path());
        let is_jsx = true;
        optative_script::run_script_with_source(
            path,
            &transformed,
            is_jsx,
            entries,
            setup,
            dry_run,
            quiet,
            limit,
        )
        .map_err(MdxScriptError::Script)
    } else {
        optative_script::run_script(path, entries, setup, dry_run, quiet, limit)
            .map_err(MdxScriptError::Script)
    }
}

/// Like [`optative_script::run_script_with_loader`], but also handles
/// `.op.mdx` files.
#[allow(clippy::too_many_arguments)]
pub fn run_script_with_loader<R, L>(
    path: &str,
    entries: &[optative_script::EsEntry],
    setup: fn(&Ctx<'_>) -> Result<(), optative_script::JsError>,
    dry_run: bool,
    quiet: bool,
    limit: Option<usize>,
    resolver: R,
    loader: L,
) -> Result<optative_script::RunStats, MdxScriptError>
where
    R: Resolver + 'static,
    L: Loader + 'static,
{
    if path.ends_with(MDX_EXTENSION) {
        let source = std::fs::read_to_string(path)?;
        let lowered = lower::lower_to_tsx(&source, path)?;
        let transformed =
            optative_script::jsx::transform_source(&lowered, lower::synthetic_tsx_path());
        let is_jsx = true;
        optative_script::run_script_with_source_and_loader(
            path,
            &transformed,
            is_jsx,
            entries,
            setup,
            dry_run,
            quiet,
            limit,
            resolver,
            loader,
        )
        .map_err(MdxScriptError::Script)
    } else {
        optative_script::run_script_with_loader(
            path, entries, setup, dry_run, quiet, limit, resolver, loader,
        )
        .map_err(MdxScriptError::Script)
    }
}
