//! The JSX runtime dispatch function: `h(type, props, ...children)`.
//!
//! This is what JS code calls after JSX has been lowered to plain function
//! calls by [`crate::jsx::transform_source`]. It has exactly three cases,
//! checked in order:
//!
//! 1. `type` is a JS function (a component) — it is called **immediately**,
//!    synchronously, with the merged `{...props, children}` object, and
//!    whatever it returns is returned directly. There is no deferral: no
//!    component can read anything "ambient" (there is no `useContext`
//!    equivalent in this codebase), so calling eagerly here produces
//!    identical results to calling later during reconciliation.
//! 2. `type` is the `Fragment` singleton (marked via [`tags::ESTO_FRAGMENT`])
//!    — the (already [`flatten_children`]-processed) children array is
//!    returned directly, with no wrapping object at all.
//! 3. Anything else — a plain string tag, the `Context` marker object, a
//!    `unit()`-produced kind descriptor, or any other opaque value — is
//!    passed through as inert `{ type, props, children }` data. `Context`
//!    and kind descriptors are recognized structurally by
//!    [`crate::run_script`]'s leaf-collection step from their `type`
//!    (`type.__estoContext` / `type.__estoKind`), not by `h_fn` itself.

use crate::tags;

use rquickjs::function::{Function, Rest};
use rquickjs::{Ctx, IntoJs, Object, Value};

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

/// Serializes `val` via JS `JSON.stringify`. Exposed at `pub(crate)` so
/// `crate::engine`'s leaf-collection step can serialize `Context`'s `data`
/// prop the same way `h_fn` used to when it had a dedicated Context branch.
pub(crate) fn json_stringify<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> rquickjs::Result<String> {
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

    // Case 1: a component function — call it immediately (no deferral) and
    // return whatever it returns, unwrapped.
    if type_arg.is_function() {
        let comp_fn = type_arg.into_function().ok_or_else(|| {
            let e = ctx
                .eval::<Value, _>(r#"new TypeError("esto: JSX type is not callable")"#)
                .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
            ctx.throw(e)
        })?;
        let merged = Object::new(ctx.clone())?;
        if let Some(p) = props.as_object() {
            object_assign(&ctx, merged.clone(), p.clone())?;
        }
        merged.set("children", kids)?;
        return comp_fn.call::<_, Value<'js>>((merged,));
    }

    // Case 2: the Fragment singleton — return the flattened children array
    // directly, with no wrapping object.
    let is_fragment = if let Some(obj) = type_arg.as_object() {
        obj.contains_key(tags::ESTO_FRAGMENT)?
    } else {
        false
    };
    if is_fragment {
        return kids.into_js(&ctx);
    }

    // Case 3: anything else (a plain string tag, the Context marker object,
    // a unit()-produced kind descriptor, a symbol, an opaque value, ...) is
    // passed through as inert `{ type, props, children }` data for the
    // caller to interpret. Nested under its own `props` key (rather than
    // spread flat) so that reserved keys like `type`/`children` on the
    // caller's props object can't collide with this wrapper's own fields.
    // `Context`/kind descriptors are recognized *structurally* from `type`
    // downstream (see `crate::engine`'s leaf-collection step) rather than
    // getting a dedicated branch here.
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
