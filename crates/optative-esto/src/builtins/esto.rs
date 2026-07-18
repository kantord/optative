use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use optative_script::tags;

use rquickjs::function::{Function, Rest};
use rquickjs::{Array, Ctx, Object, Value};

static NEXT_KIND_ID: AtomicU32 = AtomicU32::new(1);

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
        Function::new(ctx.clone(), |data: String| super::hex_sha256(&data))?,
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

fn prompt_fn<'js>(
    ctx: Ctx<'js>,
    strings: Array<'js>,
    rest: Rest<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
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
    ctx.globals()
        .set("__esto_prompt", Function::new(ctx.clone(), prompt_fn)?)?;
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

fn object_assign<'js>(
    ctx: &Ctx<'js>,
    target: Object<'js>,
    source: Object<'js>,
) -> rquickjs::Result<()> {
    let js_object: Object<'js> = ctx.globals().get("Object")?;
    let assign: Function<'js> = js_object.get("assign")?;
    assign.call::<_, Value<'js>>((target, source))?;
    Ok(())
}
