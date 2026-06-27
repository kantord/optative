use crate::builtins;

pub struct EsEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
    pub register: fn(&rquickjs::Ctx<'_>) -> rquickjs::Result<()>,
}

pub const ES_BUILTINS: &[EsEntry] = &[
    // esto module — Rust-backed (direct export = Rust global)
    EsEntry { module_path: "esto", export_name: "exists",   global_name: "__esto_exists",   register: builtins::register_exists },
    EsEntry { module_path: "esto", export_name: "read",     global_name: "__esto_read",     register: builtins::register_read },
    EsEntry { module_path: "esto", export_name: "hash",     global_name: "__esto_hash",     register: builtins::register_hash },
    // esto module — JS-backed (set by esto_globals.js eval; noop until Steps 3–4)
    EsEntry { module_path: "esto", export_name: "h",        global_name: "__esto_h",        register: builtins::noop },
    EsEntry { module_path: "esto", export_name: "Fragment",  global_name: "__esto_fragment", register: builtins::noop },
    EsEntry { module_path: "esto", export_name: "Context",  global_name: "__esto_context",  register: builtins::noop },
    EsEntry { module_path: "esto", export_name: "unit",     global_name: "__esto_unit",     register: builtins::noop },
    EsEntry { module_path: "esto", export_name: "sh",       global_name: "__esto_sh",       register: builtins::noop },
    EsEntry { module_path: "esto", export_name: "prompt",   global_name: "__esto_prompt",   register: builtins::noop },
    EsEntry { module_path: "esto", export_name: "ls",       global_name: "__esto_ls",       register: builtins::noop },
    // esto/fs module — JS-backed (set by esto_fs_globals.js eval; noop until Steps 5–6)
    EsEntry { module_path: "esto/fs", export_name: "File",   global_name: "__esto_fs_File",   register: builtins::noop },
    EsEntry { module_path: "esto/fs", export_name: "Folder", global_name: "__esto_fs_Folder", register: builtins::noop },
    EsEntry { module_path: "esto/fs", export_name: "GitRepo",global_name: "__esto_fs_GitRepo",register: builtins::noop },
];

pub fn register_builtins(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    for entry in ES_BUILTINS {
        (entry.register)(ctx)?;
    }
    Ok(())
}

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
