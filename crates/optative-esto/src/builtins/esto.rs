use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use optative_script::tags;

use rquickjs::function::{Function, Rest};
use rquickjs::{Array, Ctx, Object, Value};
use sha2::{Digest, Sha256};

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
    ctx.globals().set("__esto_exists", Function::new(ctx.clone(), |path: String| {
        Path::new(&path).exists()
    })?)?;
    Ok(())
}

pub fn register_read(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_read", Function::new(ctx.clone(), |path: String| -> rquickjs::Result<String> {
        std::fs::read_to_string(&path).map_err(rquickjs::Error::Io)
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

fn sh_fn<'js>(ctx: Ctx<'js>, strings: Value<'js>, rest: Rest<Value<'js>>) -> rquickjs::Result<String> {
    let strings_obj = strings.as_object().ok_or_else(|| {
        let err = ctx.eval::<Value, _>(r#"new Error("sh: first argument must be a template object")"#)
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
        return Err(rquickjs::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("sh: subprocess exited with {}", out.status),
        )));
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

fn flatten_children<'js>(values: Vec<Value<'js>>) -> rquickjs::Result<Vec<Value<'js>>> {
    let mut out = Vec::new();
    for val in values {
        flatten_child(val, &mut out)?;
    }
    Ok(out)
}

fn flatten_child<'js>(val: Value<'js>, out: &mut Vec<Value<'js>>) -> rquickjs::Result<()> {
    if val.is_null() || val.is_undefined() || val.as_bool() == Some(false) {
        return Ok(());
    }
    if let Some(arr) = val.as_array() {
        for i in 0..arr.len() {
            let child: Value<'js> = arr.get(i)?;
            flatten_child(child, out)?;
        }
    } else {
        out.push(val);
    }
    Ok(())
}

fn json_stringify<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> rquickjs::Result<String> {
    let json: Object<'js> = ctx.globals().get("JSON")?;
    let stringify: Function<'js> = json.get("stringify")?;
    stringify.call((val,))
}

fn object_assign<'js>(ctx: &Ctx<'js>, target: Object<'js>, source: Object<'js>) -> rquickjs::Result<()> {
    let js_object: Object<'js> = ctx.globals().get("Object")?;
    let assign: Function<'js> = js_object.get("assign")?;
    assign.call::<_, Value<'js>>((target, source))?;
    Ok(())
}

fn h_fn<'js>(ctx: Ctx<'js>, type_arg: Value<'js>, props: Value<'js>, rest: Rest<Value<'js>>) -> rquickjs::Result<Value<'js>> {
    let kids = flatten_children(rest.0)?;

    // Inspect type markers before moving type_arg
    let (is_fragment, is_context, is_kind) = if let Some(obj) = type_arg.as_object() {
        let frag = obj.contains_key(tags::ESTO_FRAGMENT)?;
        let ctx_mark = obj.contains_key(tags::ESTO_CONTEXT)?;
        let kind = obj.contains_key(tags::ESTO_KIND)?;
        (frag, ctx_mark, kind)
    } else {
        (false, false, false)
    };

    if is_fragment {
        let obj = Object::new(ctx.clone())?;
        obj.set(tags::FRAG, true)?;
        obj.set("children", kids)?;
        return Ok(Value::from_object(obj));
    }

    if is_context {
        let obj = Object::new(ctx.clone())?;
        obj.set(tags::CTX, true)?;
        let (value_val, data_val) = if let Some(p) = props.as_object() {
            let v: Value<'js> = p.get("value")?;
            let value_out = if v.is_undefined() { Value::new_null(ctx.clone()) } else { v };
            let d: Value<'js> = p.get("data")?;
            let data_out = if d.is_null() || d.is_undefined() {
                Value::new_null(ctx.clone())
            } else {
                let s = json_stringify(&ctx, d)?;
                rquickjs::String::from_str(ctx.clone(), &s)?.into()
            };
            (value_out, data_out)
        } else {
            (Value::new_null(ctx.clone()), Value::new_null(ctx.clone()))
        };
        obj.set("value", value_val)?;
        obj.set("data", data_val)?;
        obj.set("children", kids)?;
        return Ok(Value::from_object(obj));
    }

    if is_kind {
        let obj = Object::new(ctx.clone())?;
        obj.set(tags::KIND, type_arg)?;
        let item = Object::new(ctx.clone())?;
        if let Some(p) = props.as_object() {
            object_assign(&ctx, item.clone(), p.clone())?;
        }
        obj.set("item", item)?;
        return Ok(Value::from_object(obj));
    }

    if type_arg.is_function() {
        let obj = Object::new(ctx.clone())?;
        obj.set(tags::COMPONENT, type_arg)?;
        let merged = Object::new(ctx.clone())?;
        if let Some(p) = props.as_object() {
            object_assign(&ctx, merged.clone(), p.clone())?;
        }
        merged.set("children", kids)?;
        obj.set("props", merged)?;
        return Ok(Value::from_object(obj));
    }

    let err = ctx.eval::<Value, _>(r#"new TypeError("esto/h: unsupported element type — expected Fragment, Context, unit, or component function")"#)
        .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
    Err(ctx.throw(err))
}

pub fn register_h(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_h", Function::new(ctx.clone(), h_fn)?)?;
    Ok(())
}
