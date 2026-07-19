//! **Experimental.** Reconciliation CLI for `.op.tsx`/`.eso.jsx` scripts, built on
//! [optative-script](https://github.com/kantord/optative)'s scripting engine, which
//! itself reconciles via [optative](https://github.com/kantord/optative)'s
//! `Lifecycle`/`OptativeSet`. Expect breaking changes between 0.x releases.

pub mod builtins;
pub mod registry;
pub mod types;
pub mod watch;

pub fn run_file(file: &str, dry_run: bool, quiet: bool) -> Result<(), EstoError> {
    fn setup(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
        builtins::register_internal(ctx)?;
        registry::register_builtins(ctx)
    }
    let stats = optative_script::run_script(file, registry::ES_BUILTINS, setup, dry_run, quiet)
        .map_err(|e| EstoError::Run(e.to_string()))?;

    let exit_code = if dry_run {
        stats.enter + stats.update + stats.exit
    } else {
        stats.errors
    };
    if exit_code != 0 {
        std::process::exit(exit_code as i32);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum EstoError {
    #[error("{0}")]
    Run(String),
    #[error("watch error: {0}")]
    Watch(String),
}
