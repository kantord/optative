use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use rquickjs::function::{Function, Rest};
use rquickjs::{Array, Ctx, Object, Value};
use sha2::{Digest, Sha256};

use crate::js_runtime::serde_json_simple_array;

static NEXT_KIND_ID: AtomicU32 = AtomicU32::new(0);

fn js_value_to_string<'js>(val: &Value<'js>) -> String {
    if let Some(s) = val.as_string() {
        s.to_string().unwrap_or_default()
    } else if let Some(n) = val.as_int() {
        n.to_string()
    } else if let Some(f) = val.as_float() {
        f.to_string()
    } else if val.is_null() {
        "null".to_string()
    } else if val.is_undefined() {
        "undefined".to_string()
    } else if let Some(b) = val.as_bool() {
        b.to_string()
    } else {
        String::new()
    }
}

// ── Internal globals (not exported from esto / esto/fs) ──────────────────────

pub fn register_console_print(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__console_print", Function::new(ctx.clone(), |level: String, msg: String| {
        eprintln!("[{level}] {msg}");
    })?)?;
    Ok(())
}

pub fn register_console(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(concat!(
        "const __fmt = v => typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v);",
        "const __cprint = (level, args) => globalThis.__console_print(level, args.map(__fmt).join(' '));",
        "globalThis.console = {",
        "  log:   (...a) => __cprint('log',   a),",
        "  error: (...a) => __cprint('error', a),",
        "  warn:  (...a) => __cprint('warn',  a),",
        "  debug: (...a) => __cprint('debug', a),",
        "};",
    ))?;
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

// ── Exported globals — Rust-backed (direct I/O) ───────────────────────────────

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

// ── Exported globals — moved from esto_globals.js (Step 3) ───────────────────

pub fn register_fragment(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let obj = Object::new(ctx.clone())?;
    obj.set("__estoFragment", true)?;
    ctx.globals().set("__esto_fragment", obj)?;
    Ok(())
}

pub fn register_context_marker(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let obj = Object::new(ctx.clone())?;
    obj.set("__estoContext", true)?;
    ctx.globals().set("__esto_context", obj)?;
    Ok(())
}

fn unit_fn<'js>(ctx: Ctx<'js>, def: Object<'js>) -> rquickjs::Result<Object<'js>> {
    let desired: Value<'js> = def.get("desired")?;
    if !desired.is_undefined() {
        return Ok(def);
    }
    let id = NEXT_KIND_ID.fetch_add(1, Ordering::Relaxed);
    let result = Object::new(ctx.clone())?;
    result.set("__estoKind", true)?;
    result.set("__estoId", id)?;
    // Copy all def properties into result (Object.assign semantics)
    let js_object: Object<'js> = ctx.globals().get("Object")?;
    let assign: Function<'js> = js_object.get("assign")?;
    assign.call::<_, Value<'js>>((result.clone(), def))?;
    Ok(result)
}

pub fn register_unit(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_unit", Function::new(ctx.clone(), unit_fn)?)?;
    Ok(())
}

fn prompt_fn<'js>(ctx: Ctx<'js>, strings: Array<'js>, rest: Rest<Value<'js>>) -> rquickjs::Result<Value<'js>> {
    let len = strings.len();
    let mut body = strings.get::<String>(0).unwrap_or_default();
    for (i, val) in rest.0.iter().enumerate() {
        body.push_str(&js_value_to_string(val));
        if i + 1 < len {
            body.push_str(&strings.get::<String>(i + 1).unwrap_or_default());
        }
    }
    let obj = Object::new(ctx)?;
    obj.set("$prompt", body)?;
    Ok(Value::from_object(obj))
}

pub fn register_prompt(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_prompt", Function::new(ctx.clone(), prompt_fn)?)?;
    Ok(())
}

fn sh_fn<'js>(strings: Value<'js>, rest: Rest<Value<'js>>) -> rquickjs::Result<String> {
    let strings_obj = strings.as_object().ok_or(rquickjs::Error::Unknown)?;
    let raw: Array<'js> = strings_obj.get("raw")?;
    let mut cmd = raw.get::<String>(0).unwrap_or_default();
    for (i, val) in rest.0.iter().enumerate() {
        let s = js_value_to_string(val);
        let quoted = format!("'{}'", s.replace('\'', "'\\''"));
        cmd.push_str(&quoted);
        cmd.push_str(&raw.get::<String>(i + 1).unwrap_or_default());
    }
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|_| rquickjs::Error::Unknown)?;
    if !out.status.success() {
        return Err(rquickjs::Error::Unknown);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn register_sh(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_sh", Function::new(ctx.clone(), sh_fn)?)?;
    Ok(())
}

pub fn register_ls(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_ls", Function::new(ctx.clone(), |dir: String| -> Vec<String> {
        std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect())
            .unwrap_or_default()
    })?)?;
    Ok(())
}

// ── Placeholder for JS-backed entries (set by ctx.eval of globals shims) ─────

pub fn noop(_ctx: &Ctx<'_>) -> rquickjs::Result<()> { Ok(()) }

// ── Register all internal (non-exported) globals ─────────────────────────────

pub fn register_internal(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    register_console_print(ctx)?;
    register_console(ctx)?;
    register_glob(ctx)?;
    register_git_root(ctx)?;
    register_is_dir(ctx)?;
    register_cwd(ctx)?;
    Ok(())
}
