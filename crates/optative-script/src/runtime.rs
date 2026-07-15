//! The JSX runtime dispatch function: `h(type, props, ...children)`.
//!
//! This is what JS code calls after JSX has been lowered to plain function
//! calls by [`crate::jsx::transform_source`]. It inspects `type` for the
//! marker properties set by the various "special" JSX targets (Fragment,
//! Context, unit-kind descriptors, component functions) and normalizes each
//! into the tagged shape that [`crate::run_script`]'s `reduce()` step expects
//! (see [`crate::tags`]). Anything else — a plain string/opaque tag — is
//! passed through as inert `{ type, props, children }` data for the caller
//! to interpret however it likes.

use crate::tags;

use rquickjs::function::{Function, Rest};
use rquickjs::{Ctx, Object, Value};

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

fn h_fn<'js>(
    ctx: Ctx<'js>,
    type_arg: Value<'js>,
    props: Value<'js>,
    rest: Rest<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
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
            let value_out = if v.is_undefined() {
                Value::new_null(ctx.clone())
            } else {
                v
            };
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

    // Anything else (a plain string tag, a symbol, an opaque value, ...) is
    // not one of the special JSX targets above — pass it through as inert
    // `{ type, props, children }` data for the caller to interpret. Nested
    // under its own `props` key (rather than spread flat) so that reserved
    // keys like `type`/`children` on the caller's props object can't collide
    // with this wrapper's own fields.
    let out = Object::new(ctx.clone())?;
    out.set("type", type_arg)?;
    let props_out = Object::new(ctx.clone())?;
    if let Some(p) = props.as_object() {
        object_assign(&ctx, props_out.clone(), p.clone())?;
        props_out.remove("children")?;
    }
    out.set("props", props_out)?;
    out.set("children", kids)?;
    Ok(Value::from_object(out))
}

/// Registers `h_fn` as the `__esto_h` global, matching the naming convention
/// of the other Rust-backed builtins wired up via [`crate::EsEntry`].
pub fn register_h(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals()
        .set("__esto_h", Function::new(ctx.clone(), h_fn)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn eval_h(js: &str) -> String {
        let runtime = Runtime::new().expect("runtime");
        let context = Context::full(&runtime).expect("context");
        context.with(|ctx| {
            register_h(&ctx).expect("register_h");
            let json: Object = ctx.globals().get("JSON").expect("JSON");
            let stringify: Function = json.get("stringify").expect("stringify");
            let result: Value = ctx.eval(js).expect("eval");
            stringify
                .call::<_, String>((result,))
                .expect("stringify result")
        })
    }

    #[test]
    fn passes_through_plain_tags_as_nested_type_props_children() {
        let out = eval_h(r#"__esto_h("container", { id: "a", disabled: true }, "hello", "world")"#);
        assert_eq!(
            out,
            r#"{"type":"container","props":{"id":"a","disabled":true},"children":["hello","world"]}"#
        );
    }

    #[test]
    fn passthrough_children_are_already_flattened_and_evaluated() {
        // Nested h() calls are evaluated bottom-up by JS itself, so the
        // outer call simply receives the already-produced nested object as
        // one of its (flattened) children — no special handling needed.
        let out = eval_h(r#"__esto_h("outer", null, __esto_h("inner", { x: 1 }, "leaf"))"#);
        assert_eq!(
            out,
            r#"{"type":"outer","props":{},"children":[{"type":"inner","props":{"x":1},"children":["leaf"]}]}"#
        );
    }
}
