use std::path::Path;

use rquickjs::function::Function;
use rquickjs::Ctx;
use sha2::{Digest, Sha256};

use crate::js_runtime::serde_json_simple_array;

// ── Internal globals (used by JS globals shims; not exported from esto/esto-fs) ──

pub fn register_sh_exec(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__sh_exec", Function::new(ctx.clone(), |cmd: String| -> rquickjs::Result<String> {
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .map_err(|_| rquickjs::Error::Unknown)?;
        if !out.status.success() {
            return Err(rquickjs::Error::Unknown);
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    })?)?;
    Ok(())
}

pub fn register_ls_json(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_ls_json", Function::new(ctx.clone(), |dir: String| -> String {
        let entries: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect())
            .unwrap_or_default();
        serde_json_simple_array(&entries)
    })?)?;
    Ok(())
}

pub fn register_console_print(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__console_print", Function::new(ctx.clone(), |level: String, msg: String| {
        eprintln!("[{level}] {msg}");
    })?)?;
    Ok(())
}

pub fn register_glob(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_glob", Function::new(ctx.clone(), |pattern: String| -> String {
        let matches: Vec<String> = glob::glob(&pattern)
            .map(|paths| {
                paths.filter_map(|p| p.ok())
                     .filter_map(|p| p.to_str().map(|s| s.to_owned()))
                     .collect()
            })
            .unwrap_or_default();
        serde_json_simple_array(&matches)
    })?)?;
    Ok(())
}

pub fn register_git_root(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_git_root", Function::new(ctx.clone(), || -> rquickjs::Result<String> {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|_| rquickjs::Error::Unknown)?;
        if !out.status.success() { return Err(rquickjs::Error::Unknown); }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    })?)?;
    Ok(())
}

pub fn register_is_dir(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_is_dir", Function::new(ctx.clone(), |path: String| {
        Path::new(&path).is_dir()
    })?)?;
    Ok(())
}

pub fn register_cwd(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_cwd", Function::new(ctx.clone(), || -> rquickjs::Result<String> {
        std::env::current_dir()
            .map(|p| p.to_str().unwrap_or(".").to_owned())
            .map_err(|_| rquickjs::Error::Unknown)
    })?)?;
    Ok(())
}

// ── Exported globals (each has an EsEntry in the registry) ───────────────────

pub fn register_exists(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_exists", Function::new(ctx.clone(), |path: String| {
        Path::new(&path).exists()
    })?)?;
    Ok(())
}

pub fn register_read(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_read", Function::new(ctx.clone(), |path: String| -> rquickjs::Result<String> {
        std::fs::read_to_string(&path).map_err(|_| rquickjs::Error::Unknown)
    })?)?;
    Ok(())
}

pub fn register_hash(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_hash", Function::new(ctx.clone(), |data: String| {
        let hash = Sha256::digest(data.as_bytes());
        format!("{hash:x}")
    })?)?;
    Ok(())
}

// ── Placeholder for JS-backed entries (set by ctx.eval of globals shims) ─────

pub fn noop(_ctx: &Ctx<'_>) -> rquickjs::Result<()> { Ok(()) }

// ── Register all internal (non-exported) globals ─────────────────────────────

pub fn register_internal(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    register_sh_exec(ctx)?;
    register_ls_json(ctx)?;
    register_console_print(ctx)?;
    register_glob(ctx)?;
    register_git_root(ctx)?;
    register_is_dir(ctx)?;
    register_cwd(ctx)?;
    Ok(())
}
