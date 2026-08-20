//! esto's own builtins. The reconciler vocabulary (`unit`, `optativeSet`,
//! `optativeJsonSet`), the JSX markers and the effectful helpers moved to
//! `optative_script::builtins`, which is where the engine that reads their tags
//! lives; `prompt` stays here because a task for an agent is esto's concept, not
//! the engine's.

use optative_script::builtins::js_value_to_string;

use rquickjs::function::{Function, Rest};
use rquickjs::{Array, Ctx, Object, Value};

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
