//! The builtins a reconciler script is written against: `unit()` and its two
//! reconciler backends, the JSX marker singletons the engine walks for, and the
//! effectful helpers a hook or an `observe` reaches for.
//!
//! These live here rather than in an embedder because [`crate::engine`] already
//! reads what they produce — `reconcile_kind` dispatches on the
//! [`tags::ESTO_RECONCILER_KIND`] discriminant that `optativeSet()` sets, and the
//! tree walk keys off [`tags::ESTO_KIND`] from `unit()`. An embedder that
//! registered its own would have to agree with this crate about every tag.
//!
//! Registration is opt-in per builtin: an embedder picks entries out of
//! [`RECONCILER_BUILTINS`], so a runtime that must not touch the world can take
//! `unit` and leave `sh` behind.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use rquickjs::function::{Function, Rest};
use rquickjs::{Array, Ctx, Object, Value};

use crate::runtime::object_assign;
use crate::{EsEntry, tags};

static NEXT_KIND_ID: AtomicU32 = AtomicU32::new(1);

pub fn hex_sha256(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

/// Renders a template-literal interpolation the way JS would, for the tagged
/// templates (`sh`, `prompt`) that build a string out of one.
pub fn js_value_to_string(val: &Value<'_>) -> String {
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

fn unit_fn<'js>(ctx: Ctx<'js>, def: Object<'js>) -> rquickjs::Result<Object<'js>> {
    let desired: Value<'js> = def.get("desired")?;
    if !desired.is_undefined() {
        return Ok(def);
    }
    let id = NEXT_KIND_ID.fetch_add(1, Ordering::Relaxed);
    let result = Object::new(ctx.clone())?;
    result.set(tags::ESTO_KIND, true)?;
    result.set(tags::ESTO_ID, id)?;
    // Copy all def properties into result (Object.assign semantics)
    object_assign(&ctx, result.clone(), def)?;
    Ok(result)
}

pub fn register_unit(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals()
        .set("__esto_unit", Function::new(ctx.clone(), unit_fn)?)?;
    Ok(())
}

/// Builds a `reconciler` descriptor tagged with `backend_kind` (`"optativeSet"`
/// or `"optativeJsonSet"`), copying `opts`'s own properties (`observe`/`file`)
/// onto it — [`crate::engine`]'s `reconcile_kind` reads the tag at runtime to
/// pick which `optative::Reconcile` backend drives a `unit()`'s state.
/// Named `backend_kind`, not `kind`: the Object `unit()` returns is also called
/// "kind" elsewhere in this codebase, a different concept.
fn reconciler_fn<'js>(
    ctx: Ctx<'js>,
    backend_kind: &str,
    opts: Object<'js>,
) -> rquickjs::Result<Object<'js>> {
    let result = Object::new(ctx.clone())?;
    result.set(tags::ESTO_RECONCILER_KIND, backend_kind)?;
    object_assign(&ctx, result.clone(), opts)?;
    Ok(result)
}

fn optative_set_fn<'js>(ctx: Ctx<'js>, opts: Object<'js>) -> rquickjs::Result<Object<'js>> {
    reconciler_fn(ctx, "optativeSet", opts)
}

pub fn register_optative_set(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__esto_optative_set",
        Function::new(ctx.clone(), optative_set_fn)?,
    )?;
    Ok(())
}

fn optative_json_set_fn<'js>(ctx: Ctx<'js>, opts: Object<'js>) -> rquickjs::Result<Object<'js>> {
    reconciler_fn(ctx, "optativeJsonSet", opts)
}

pub fn register_optative_json_set(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__esto_optative_json_set",
        Function::new(ctx.clone(), optative_json_set_fn)?,
    )?;
    Ok(())
}

pub fn register_fragment(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let obj = Object::new(ctx.clone())?;
    obj.set(tags::ESTO_FRAGMENT, true)?;
    ctx.globals().set("__esto_fragment", obj)?;
    Ok(())
}

pub fn register_context_marker(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let obj = Object::new(ctx.clone())?;
    obj.set(tags::ESTO_CONTEXT, true)?;
    ctx.globals().set("__esto_context", obj)?;
    Ok(())
}

pub fn register_exists(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__esto_exists",
        Function::new(ctx.clone(), |path: String| Path::new(&path).exists())?,
    )?;
    Ok(())
}

pub fn register_read(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__esto_read",
        Function::new(ctx.clone(), |path: String| -> rquickjs::Result<String> {
            std::fs::read_to_string(&path).map_err(rquickjs::Error::Io)
        })?,
    )?;
    Ok(())
}

pub fn register_hash(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__esto_hash",
        Function::new(ctx.clone(), |data: String| hex_sha256(&data))?,
    )?;
    Ok(())
}

pub fn register_ls(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__esto_ls",
        Function::new(ctx.clone(), |dir: String| -> Vec<String> {
            std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default()
        })?,
    )?;
    Ok(())
}

fn sh_fn<'js>(
    ctx: Ctx<'js>,
    strings: Value<'js>,
    rest: Rest<Value<'js>>,
) -> rquickjs::Result<String> {
    let strings_obj = strings.as_object().ok_or_else(|| {
        let err = ctx
            .eval::<Value, _>(r#"new Error("sh: first argument must be a template object")"#)
            .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
        ctx.throw(err)
    })?;
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
        .map_err(rquickjs::Error::Io)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stderr = stderr.trim();
        let mut msg = format!("shell command failed ({})\n  $ {cmd}", out.status);
        if !stderr.is_empty() {
            msg.push_str("\n  stderr:\n");
            for line in stderr.lines() {
                msg.push_str("    ");
                msg.push_str(line);
                msg.push('\n');
            }
        }
        return Err(rquickjs::Exception::throw_message(&ctx, msg.trim_end()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn register_sh(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals()
        .set("__esto_sh", Function::new(ctx.clone(), sh_fn)?)?;
    Ok(())
}

/// The builtins above, as module exports. `module_path` is `"esto"` because that
/// is what every `.op.tsx` in the wild imports from; an embedder wanting another
/// name builds its own entries around the same `register` functions.
///
/// Split into two slices so that an embedder can take the vocabulary without the
/// I/O: [`RECONCILER_BUILTINS`] cannot touch the world, [`EFFECTFUL_BUILTINS`]
/// exists to.
pub const RECONCILER_BUILTINS: &[EsEntry] = &[
    EsEntry {
        module_path: "esto",
        export_name: "h",
        global_name: "__esto_h",
        register: crate::register_h,
    },
    EsEntry {
        module_path: "esto",
        export_name: "unit",
        global_name: "__esto_unit",
        register: register_unit,
    },
    EsEntry {
        module_path: "esto",
        export_name: "optativeSet",
        global_name: "__esto_optative_set",
        register: register_optative_set,
    },
    EsEntry {
        module_path: "esto",
        export_name: "optativeJsonSet",
        global_name: "__esto_optative_json_set",
        register: register_optative_json_set,
    },
    EsEntry {
        module_path: "esto",
        export_name: "Fragment",
        global_name: "__esto_fragment",
        register: register_fragment,
    },
    EsEntry {
        module_path: "esto",
        export_name: "Context",
        global_name: "__esto_context",
        register: register_context_marker,
    },
];

/// Builtins that read or change the world. Registering these in a runtime that
/// evaluates on a latency budget is what a caller is opting into by naming them.
pub const EFFECTFUL_BUILTINS: &[EsEntry] = &[
    EsEntry {
        module_path: "esto",
        export_name: "sh",
        global_name: "__esto_sh",
        register: register_sh,
    },
    EsEntry {
        module_path: "esto",
        export_name: "read",
        global_name: "__esto_read",
        register: register_read,
    },
    EsEntry {
        module_path: "esto",
        export_name: "ls",
        global_name: "__esto_ls",
        register: register_ls,
    },
    EsEntry {
        module_path: "esto",
        export_name: "exists",
        global_name: "__esto_exists",
        register: register_exists,
    },
    EsEntry {
        module_path: "esto",
        export_name: "hash",
        global_name: "__esto_hash",
        register: register_hash,
    },
];

/// Runs the `register` hook of every entry, which is what actually puts the
/// globals a synthetic module's exports point at into a context. Building the
/// module source from a slice and registering that same slice are two separate
/// steps, and skipping this one fails at import time rather than at build time.
pub fn register_all(ctx: &Ctx<'_>, entries: &[EsEntry]) -> rquickjs::Result<()> {
    for entry in entries {
        (entry.register)(ctx)?;
    }
    Ok(())
}
