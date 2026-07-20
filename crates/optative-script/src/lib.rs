//! **Experimental.** QuickJS + oxc scripting engine extracted from
//! [tauler](https://github.com/kantord/tauler). Drives the `esto` reconciliation
//! CLI; expect breaking changes between 0.x releases.

mod engine;
pub mod jsx;
pub mod loader;
pub mod runtime;
pub mod tags;

// Re-export rquickjs primitives so plugin authors don't need a direct rquickjs dep.
pub use rquickjs::function::{Function, Rest};
pub use rquickjs::loader::{Loader, Resolver};
pub use rquickjs::{Array, Ctx, Error as JsError, FromJs, IntoJs, Object, Value};

pub use engine::{
    RunStats, build_runtime, run_script, run_script_with_loader, run_script_with_source,
    run_script_with_source_and_loader, serde_json_simple_array,
};
pub use runtime::register_h;

/// One JS builtin exported from a synthetic module.
pub struct EsEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
    pub register: fn(&Ctx<'_>) -> rquickjs::Result<()>,
}

/// Generates the `const X = __global; export { X, ... }` shim for a group of entries.
pub fn synthetic_module_source_for_entries(entries: &[&EsEntry]) -> String {
    let bindings: Vec<String> = entries
        .iter()
        .map(|e| format!("const {} = {};", e.export_name, e.global_name))
        .collect();
    let exports: Vec<&str> = entries.iter().map(|e| e.export_name).collect();
    format!(
        "{} export {{ {} }};",
        bindings.join(" "),
        exports.join(", ")
    )
}

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("{0}")]
    Worker(String),
}
