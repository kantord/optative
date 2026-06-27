pub struct EsEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
}

pub const ES_BUILTINS: &[EsEntry] = &[
    EsEntry { module_path: "esto",    export_name: "h",       global_name: "__esto_h" },
    EsEntry { module_path: "esto",    export_name: "Fragment", global_name: "__esto_fragment" },
    EsEntry { module_path: "esto",    export_name: "Context",  global_name: "__esto_context" },
    EsEntry { module_path: "esto",    export_name: "unit",     global_name: "__esto_unit" },
    EsEntry { module_path: "esto",    export_name: "sh",       global_name: "__esto_sh" },
    EsEntry { module_path: "esto",    export_name: "prompt",   global_name: "__esto_prompt" },
    EsEntry { module_path: "esto",    export_name: "exists",   global_name: "__esto_exists" },
    EsEntry { module_path: "esto",    export_name: "read",     global_name: "__esto_read" },
    EsEntry { module_path: "esto",    export_name: "ls",       global_name: "__esto_ls" },
    EsEntry { module_path: "esto",    export_name: "hash",     global_name: "__esto_hash" },
    EsEntry { module_path: "esto/fs", export_name: "File",     global_name: "__esto_fs_File" },
    EsEntry { module_path: "esto/fs", export_name: "Folder",   global_name: "__esto_fs_Folder" },
    EsEntry { module_path: "esto/fs", export_name: "GitRepo",  global_name: "__esto_fs_GitRepo" },
];

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
