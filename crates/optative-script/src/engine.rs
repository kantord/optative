use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rquickjs::function::Function;
use rquickjs::loader::{BuiltinLoader, BuiltinResolver, Loader, Resolver};
use rquickjs::promise::MaybePromise;
use rquickjs::{Array, Context, Ctx, FromJs, Module, Object, Runtime, Value};
use sha2::{Digest, Sha256};

use crate::jsx::transform_source;

// ── resolver ──────────────────────────────────────────────────────────────────

struct ScriptResolver {
    base_dir: PathBuf,
}

impl Resolver for ScriptResolver {
    fn resolve(&mut self, _ctx: &Ctx, base: &str, name: &str) -> rquickjs::Result<String> {
        if !name.starts_with("./") && !name.starts_with("../") {
            return Err(rquickjs::Error::new_resolving(base, name));
        }
        let dir = if Path::new(base).is_absolute() {
            Path::new(base).parent().map(|p| p.to_path_buf()).unwrap_or_else(|| self.base_dir.clone())
        } else {
            self.base_dir.clone()
        };
        dir.join(name)
            .canonicalize()
            .map_err(|_| rquickjs::Error::new_resolving(base, name))?
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| rquickjs::Error::new_resolving(base, name))
    }
}

// ── loader ────────────────────────────────────────────────────────────────────

struct ScriptLoader;

impl Loader for ScriptLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Module<'js>> {
        let source = std::fs::read_to_string(name).map_err(|_| rquickjs::Error::new_loading(name))?;
        let source = if name.ends_with(".jsx") || name.ends_with(".tsx")
            || name.ends_with(".ts") || name.ends_with(".mts")
        {
            transform_source(&source, name)
        } else {
            source
        };
        Module::declare(ctx.clone(), name, source)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

pub fn serde_json_simple_array(items: &[String]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", inner.join(","))
}

fn sha12(s: &str) -> String {
    let hash = Sha256::digest(s.as_bytes());
    format!("{hash:x}")[..12].to_string()
}

fn emit_task(key: &str, context: &[String], context_data: &[String], body: &str) -> std::io::Result<()> {
    std::fs::create_dir_all("tasks")?;
    std::fs::create_dir_all(".esto/context")?;

    let refs: Vec<String> = context.iter().map(|entry| {
        let hash = sha12(entry);
        let path = format!(".esto/context/{hash}.md");
        if !Path::new(&path).exists() {
            let _ = std::fs::write(&path, entry);
        }
        let first = entry.lines().next().unwrap_or("").chars().take(60).collect::<String>();
        format!("  {path} — {first}")
    }).collect();

    let safe: String = key.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();

    let mut sections: Vec<String> = Vec::new();
    if !refs.is_empty() {
        sections.push(format!("Context (read once; same id = same content):\n{}", refs.join("\n")));
    }
    if !context_data.is_empty() {
        let json = if context_data.len() == 1 {
            context_data[0].clone()
        } else {
            format!("[{}]", context_data.join(","))
        };
        sections.push(format!("Structured context:\n```json\n{json}\n```"));
    }

    let content = if sections.is_empty() {
        format!("# {key}\n{body}\n")
    } else {
        format!("# {key}\n{}\n\n{body}\n", sections.join("\n\n"))
    };

    std::fs::write(format!("tasks/{safe}.md"), content)
}

fn await_val<'js, T: FromJs<'js>>(val: Value<'js>) -> rquickjs::Result<T> {
    MaybePromise::from_value(val).finish()
}

fn check_prompt(key: &str, context: &[String], context_data: &[String], val: Value) -> rquickjs::Result<()> {
    if let Some(obj) = val.as_object() {
        if let Ok(prompt_str) = obj.get::<_, String>("$prompt") {
            emit_task(key, context, context_data, &prompt_str).map_err(|_| rquickjs::Error::Unknown)?;
        }
    }
    Ok(())
}

// ── Leaf ──────────────────────────────────────────────────────────────────────

struct Leaf<'js> {
    kind_id: u32,
    kind: Object<'js>,
    item: Value<'js>,
    context: Vec<String>,
    context_data: Vec<String>,
}

// ── reduce ────────────────────────────────────────────────────────────────────

fn reduce<'js>(
    ctx: &Ctx<'js>,
    node: Value<'js>,
    context: Vec<String>,
    context_data: Vec<String>,
) -> rquickjs::Result<Vec<Leaf<'js>>> {
    if node.is_null() || node.is_undefined() {
        return Ok(vec![]);
    }
    if let Some(false) = node.as_bool() {
        return Ok(vec![]);
    }
    if let Some(arr) = node.as_array() {
        let mut leaves = vec![];
        for i in 0..arr.len() {
            let child: Value = arr.get(i)?;
            leaves.extend(reduce(ctx, child, context.clone(), context_data.clone())?);
        }
        return Ok(leaves);
    }
    if let Some(obj) = node.as_object() {
        if obj.get::<_, bool>("$fragment").unwrap_or(false) {
            let children: Array = obj.get("children")?;
            let mut leaves = vec![];
            for i in 0..children.len() {
                let child: Value = children.get(i)?;
                leaves.extend(reduce(ctx, child, context.clone(), context_data.clone())?);
            }
            return Ok(leaves);
        }
        if obj.get::<_, bool>("$context").unwrap_or(false) {
            let v: Value = obj.get("value")?;
            let new_ctx = if v.is_null() || v.is_undefined() {
                context.clone()
            } else if let Some(s) = v.as_string() {
                let s = s.to_string()?;
                let mut c = context.clone();
                c.push(s);
                c
            } else {
                context.clone()
            };
            let data_val: Value = obj.get("data").unwrap_or(Value::new_undefined(ctx.clone()));
            let mut new_ctx_data = context_data.clone();
            if !data_val.is_null() && !data_val.is_undefined() {
                if let Some(s) = data_val.as_string() {
                    if let Ok(s) = s.to_string() {
                        new_ctx_data.push(s);
                    }
                }
            }
            let children: Array = obj.get("children")?;
            let mut leaves = vec![];
            for i in 0..children.len() {
                let child: Value = children.get(i)?;
                leaves.extend(reduce(ctx, child, new_ctx.clone(), new_ctx_data.clone())?);
            }
            return Ok(leaves);
        }
        let comp_val: Value = obj.get("$component")?;
        if comp_val.is_function() {
            let comp_fn = comp_val.into_function().ok_or(rquickjs::Error::Unknown)?;
            let props: Value = obj.get("props")?;
            let result: Value = comp_fn.call::<(Value,), Value>((props,))?;
            return reduce(ctx, result, context, context_data);
        }
        let kind_val: Value = obj.get("$kind")?;
        if kind_val.is_object() {
            let kind_obj = kind_val.into_object().ok_or(rquickjs::Error::Unknown)?;
            let kind_id: u32 = kind_obj.get("__estoId").unwrap_or(0);
            let item: Value = obj.get("item")?;
            return Ok(vec![Leaf { kind_id, kind: kind_obj, item, context, context_data }]);
        }
    }
    Err(rquickjs::Error::Unknown)
}

// ── reconcile ─────────────────────────────────────────────────────────────────

pub struct RunStats {
    pub enter: usize,
    pub update: usize,
    pub exit: usize,
    pub unchanged: usize,
    pub errors: usize,
}

struct ReconcileResult {
    enter: usize,
    update: usize,
    exit: usize,
    unchanged: usize,
    errors: usize,
}

fn call_lifecycle<'js>(
    _ctx: &Ctx<'js>,
    kind: &Object<'js>,
    method: &str,
    args: Vec<Value<'js>>,
    key: &str,
    context: &[String],
    context_data: &[String],
    dry_run: bool,
    errors: &mut usize,
) {
    if dry_run { return; }
    let fn_val: Value = match kind.get(method) { Ok(v) => v, Err(_) => return };
    if !fn_val.is_function() { return; }
    let func = match fn_val.into_function() { Some(f) => f, None => return };
    let result: rquickjs::Result<Value> = match args.len() {
        0 => func.call::<(), Value>(()),
        1 => func.call::<(Value,), Value>((args[0].clone(),)),
        2 => func.call::<(Value, Value), Value>((args[0].clone(), args[1].clone())),
        _ => return,
    };
    match result {
        Err(e) => { eprintln!("[error] {key}: {e}"); *errors += 1; }
        Ok(raw) => match await_val::<Value>(raw) {
            Err(e) => { eprintln!("[error] {key}: {e}"); *errors += 1; }
            Ok(resolved) => {
                if let Err(e) = check_prompt(key, context, context_data, resolved) {
                    eprintln!("[error] {key}: {e}"); *errors += 1;
                }
            }
        }
    }
}

fn reconcile_kind<'js>(
    ctx: &Ctx<'js>,
    kind: &Object<'js>,
    leaves: Vec<Leaf<'js>>,
    dry_run: bool,
    quiet: bool,
) -> rquickjs::Result<ReconcileResult> {
    let mut r = ReconcileResult { enter: 0, update: 0, exit: 0, unchanged: 0, errors: 0 };

    let observe_fn: Function = kind.get("observe")?;
    let obs_raw: Value = observe_fn.call::<(), Value>(())?;
    let obs_val: Value = await_val(obs_raw)?;
    let obs_arr = obs_val.into_array().ok_or(rquickjs::Error::Unknown)?;

    let key_fn: Function = kind.get("key")?;
    let value_fn: Function = kind.get("value")?;

    let mut current: HashMap<String, (Value<'js>, String)> = HashMap::new();
    for i in 0..obs_arr.len() {
        let item: Value = obs_arr.get(i)?;
        let k: String = key_fn.call::<(Value,), String>((item.clone(),))?;
        let v: String = value_fn.call::<(Value,), String>((item.clone(),))?;
        current.insert(k, (item, v));
    }

    let mut desired: HashMap<String, (Value<'js>, String, Vec<String>, Vec<String>)> = HashMap::new();
    for leaf in leaves {
        let k: String = key_fn.call::<(Value,), String>((leaf.item.clone(),))?;
        let v: String = value_fn.call::<(Value,), String>((leaf.item.clone(),))?;
        desired.insert(k, (leaf.item, v, leaf.context, leaf.context_data));
    }

    for (k, (d_item, d_val, ctx_chain, ctx_data)) in &desired {
        match current.get(k) {
            None => {
                if !quiet { eprintln!("[enter] {k}"); }
                r.enter += 1;
                call_lifecycle(ctx, kind, "enter", vec![d_item.clone()], k, ctx_chain, ctx_data, dry_run, &mut r.errors);
            }
            Some((c_item, c_val)) => {
                if d_val != c_val {
                    if !quiet { eprintln!("[update] {k}"); }
                    r.update += 1;
                    call_lifecycle(ctx, kind, "update", vec![d_item.clone(), c_item.clone()], k, ctx_chain, ctx_data, dry_run, &mut r.errors);
                } else {
                    r.unchanged += 1;
                }
            }
        }
    }

    for (k, (c_item, _)) in &current {
        if !desired.contains_key(k) {
            if !quiet { eprintln!("[exit] {k}"); }
            r.exit += 1;
            call_lifecycle(ctx, kind, "exit", vec![c_item.clone()], k, &[], &[], dry_run, &mut r.errors);
        }
    }

    Ok(r)
}

// ── run_script ────────────────────────────────────────────────────────────────

pub fn run_script(
    path: &str,
    entries: &[crate::EsEntry],
    setup: fn(&Ctx<'_>) -> rquickjs::Result<()>,
    dry_run: bool,
    quiet: bool,
) -> Result<RunStats, crate::ScriptError> {
    let abs_path = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| crate::ScriptError::Worker(e.to_string()))?;
    let base_dir = abs_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let path_str = abs_path.to_str()
        .ok_or_else(|| crate::ScriptError::Worker("non-UTF8 file path".into()))?
        .to_string();
    let needs_transform = path_str.ends_with(".jsx") || path_str.ends_with(".tsx")
        || path_str.ends_with(".ts") || path_str.ends_with(".mts");
    let is_jsx = path_str.ends_with(".jsx") || path_str.ends_with(".tsx");

    let runtime = Runtime::new().map_err(|e| crate::ScriptError::Worker(e.to_string()))?;

    let mut module_groups: HashMap<&'static str, Vec<&crate::EsEntry>> = HashMap::new();
    for e in entries {
        module_groups.entry(e.module_path).or_default().push(e);
    }
    let builtin_resolver = module_groups.keys()
        .fold(BuiltinResolver::default(), |r, path| r.with_module(*path));
    let builtin_loader = module_groups.iter()
        .fold(BuiltinLoader::default(), |l, (path, es)| {
            l.with_module(*path, crate::synthetic_module_source_for_entries(es))
        });
    runtime.set_loader(
        (builtin_resolver, ScriptResolver { base_dir }),
        (builtin_loader, ScriptLoader),
    );

    let context = Context::full(&runtime).map_err(|e| crate::ScriptError::Worker(e.to_string()))?;

    let (enter, update, exit, unchanged, errors) = context
        .with(|ctx| -> rquickjs::Result<(usize, usize, usize, usize, usize)> {
            setup(&ctx)?;

            let src = std::fs::read_to_string(&path_str)
                .map_err(|_| rquickjs::Error::new_loading(&path_str))?;
            let src = if needs_transform { transform_source(&src, &path_str) } else { src };
            let module = Module::declare(ctx.clone(), path_str.clone(), src)?;
            let (module, promise) = module.eval()?;
            promise.finish::<()>()?;

            let default_val: Value = module.get("default")?;

            let leaves: Vec<Leaf> = if is_jsx {
                let root_fn = default_val.into_function().ok_or(rquickjs::Error::Unknown)?;
                let root: Value = root_fn.call::<(), Value>(())?;
                reduce(&ctx, root, vec![], vec![])?
            } else {
                let target = default_val.into_object().ok_or(rquickjs::Error::Unknown)?;
                let desired_fn: Function = target.get("desired")?;
                let desired_raw: Value = desired_fn.call::<(), Value>(())?;
                let desired_val: Value = await_val(desired_raw)?;
                let desired_arr = desired_val.into_array().ok_or(rquickjs::Error::Unknown)?;
                let mut leaves = Vec::new();
                for i in 0..desired_arr.len() {
                    let item: Value = desired_arr.get(i)?;
                    leaves.push(Leaf { kind_id: 0, kind: target.clone(), item, context: vec![], context_data: vec![] });
                }
                leaves
            };

            let mut by_kind: HashMap<u32, (Object, Vec<Leaf>)> = HashMap::new();
            for leaf in leaves {
                let entry = by_kind.entry(leaf.kind_id).or_insert_with(|| (leaf.kind.clone(), vec![]));
                entry.1.push(leaf);
            }

            let mut total_enter = 0usize;
            let mut total_update = 0usize;
            let mut total_exit = 0usize;
            let mut total_unchanged = 0usize;
            let mut total_errors = 0usize;

            for (_, (kind_obj, kind_leaves)) in by_kind {
                let res = reconcile_kind(&ctx, &kind_obj, kind_leaves, dry_run, quiet)?;
                total_enter += res.enter;
                total_update += res.update;
                total_exit += res.exit;
                total_unchanged += res.unchanged;
                total_errors += res.errors;
            }

            Ok((total_enter, total_update, total_exit, total_unchanged, total_errors))
        })
        .map_err(|e| crate::ScriptError::Worker(e.to_string()))?;

    if !quiet {
        eprintln!("reconciled: {enter} enter, {update} update, {exit} exit ({unchanged} unchanged)");
    }

    Ok(RunStats { enter, update, exit, unchanged, errors })
}
