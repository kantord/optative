use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rquickjs::function::Function;
use rquickjs::loader::{BuiltinLoader, BuiltinResolver, Loader, Resolver};
use rquickjs::promise::MaybePromise;
use rquickjs::{Array, Context, Ctx, FromJs, Module, Object, Runtime, Value};
use sha2::{Digest, Sha256};

use crate::jsx::transform_source;
use crate::tags;

/// kind_id=0 is the synthetic bucket: non-JSX targets and leaves with a missing __estoId.
/// Real unit kinds are assigned ids starting at 1 by NEXT_KIND_ID in builtins/esto.rs.
const SYNTHETIC_KIND_ID: u32 = 0;

/// SHA-256 prefix length used as a short context identifier in task file names.
/// 12 hex chars give ~48 bits — low collision risk for typical context set sizes.
const CONTEXT_HASH_LEN: usize = 12;
/// Maximum number of characters taken from the first line of a context entry
/// for the human-readable reference comment in task files.
const CONTEXT_PREVIEW_LEN: usize = 60;

struct ScriptResolver {
    base_dir: PathBuf,
}

impl Resolver for ScriptResolver {
    fn resolve(&mut self, _ctx: &Ctx, base: &str, name: &str) -> rquickjs::Result<String> {
        if !name.starts_with("./") && !name.starts_with("../") {
            return Err(rquickjs::Error::new_resolving(base, name));
        }
        let dir = if Path::new(base).is_absolute() {
            Path::new(base)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.base_dir.clone())
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

struct ScriptLoader;

impl Loader for ScriptLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Module<'js>> {
        let source =
            std::fs::read_to_string(name).map_err(|_| rquickjs::Error::new_loading(name))?;
        let source = if is_script_file(name) {
            transform_source(&source, name)
        } else {
            source
        };
        Module::declare(ctx.clone(), name, source)
    }
}

pub(crate) fn is_script_file(name: &str) -> bool {
    matches!(
        name.rsplit_once('.').map(|(_, e)| e),
        Some("jsx" | "tsx" | "ts" | "mts")
    )
}

pub fn serde_json_simple_array(items: &[String]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", inner.join(","))
}

fn sha12(s: &str) -> String {
    let hash = Sha256::digest(s.as_bytes());
    format!("{hash:x}")[..CONTEXT_HASH_LEN].to_string()
}

fn emit_task(
    key: &str,
    context: &[String],
    context_data: &[String],
    body: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all("tasks")?;
    std::fs::create_dir_all(".esto/context")?;

    let refs: Vec<String> = context
        .iter()
        .map(|entry| {
            let hash = sha12(entry);
            let path = format!(".esto/context/{hash}.md");
            if !Path::new(&path).exists() {
                let _ = std::fs::write(&path, entry);
            }
            let first = entry
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(CONTEXT_PREVIEW_LEN)
                .collect::<String>();
            format!("  {path} — {first}")
        })
        .collect();

    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let mut sections: Vec<String> = Vec::new();
    if !refs.is_empty() {
        sections.push(format!(
            "Context (read once; same id = same content):\n{}",
            refs.join("\n")
        ));
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

fn check_prompt(
    key: &str,
    context: &[String],
    context_data: &[String],
    val: Value,
) -> rquickjs::Result<()> {
    if let Some(obj) = val.as_object()
        && let Ok(prompt_str) = obj.get::<_, String>("$prompt")
    {
        emit_task(key, context, context_data, &prompt_str).map_err(rquickjs::Error::Io)?;
    }
    Ok(())
}

struct DesiredEntry<'js> {
    item: Value<'js>,
    value_str: String,
    context: Vec<String>,
    context_data: Vec<String>,
}

struct Leaf<'js> {
    kind_id: u32,
    kind: Object<'js>,
    item: Value<'js>,
    context: Vec<String>,
    context_data: Vec<String>,
}

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
        if obj.get::<_, bool>(tags::FRAG).unwrap_or(false) {
            let children: Array = obj.get("children")?;
            let mut leaves = vec![];
            for i in 0..children.len() {
                let child: Value = children.get(i)?;
                leaves.extend(reduce(ctx, child, context.clone(), context_data.clone())?);
            }
            return Ok(leaves);
        }
        if obj.get::<_, bool>(tags::CTX).unwrap_or(false) {
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
            if !data_val.is_null()
                && !data_val.is_undefined()
                && let Some(s) = data_val.as_string()
                && let Ok(s) = s.to_string()
            {
                new_ctx_data.push(s);
            }
            let children: Array = obj.get("children")?;
            let mut leaves = vec![];
            for i in 0..children.len() {
                let child: Value = children.get(i)?;
                leaves.extend(reduce(ctx, child, new_ctx.clone(), new_ctx_data.clone())?);
            }
            return Ok(leaves);
        }
        let comp_val: Value = obj.get(tags::COMPONENT)?;
        if comp_val.is_function() {
            let comp_fn = comp_val.into_function().ok_or_else(|| {
                let e = ctx
                    .eval::<Value, _>(r#"new TypeError("esto: $component is not callable")"#)
                    .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
                ctx.throw(e)
            })?;
            let props: Value = obj.get("props")?;
            let result: Value = comp_fn.call::<(Value,), Value>((props,))?;
            return reduce(ctx, result, context, context_data);
        }
        let kind_val: Value = obj.get(tags::KIND)?;
        if kind_val.is_object() {
            let kind_obj = kind_val.into_object().ok_or_else(|| {
                let e = ctx
                    .eval::<Value, _>(r#"new TypeError("esto: $kind is not an object")"#)
                    .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
                ctx.throw(e)
            })?;
            let kind_id: u32 = kind_obj.get(tags::ESTO_ID).unwrap_or(SYNTHETIC_KIND_ID);
            let item: Value = obj.get("item")?;
            return Ok(vec![Leaf {
                kind_id,
                kind: kind_obj,
                item,
                context,
                context_data,
            }]);
        }
    }
    {
        let err = ctx
            .eval::<Value, _>(r#"new TypeError("esto: reduce encountered an unknown node type")"#)
            .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
        Err(ctx.throw(err))
    }
}

pub struct RunStats {
    pub enter: usize,
    pub update: usize,
    pub exit: usize,
    pub unchanged: usize,
    pub errors: usize,
}

fn call_and_check<'js>(
    func: Function<'js>,
    args: Vec<Value<'js>>,
    key: &str,
    context: &[String],
    context_data: &[String],
) -> rquickjs::Result<()> {
    let raw: Value = match args.len() {
        0 => func.call::<(), Value>(()),
        1 => func.call::<(Value,), Value>((args[0].clone(),)),
        2 => func.call::<(Value, Value), Value>((args[0].clone(), args[1].clone())),
        _ => return Ok(()),
    }?;
    let resolved: Value = await_val(raw)?;
    check_prompt(key, context, context_data, resolved)
}

#[allow(clippy::too_many_arguments)]
fn call_lifecycle<'js>(
    kind: &Object<'js>,
    method: &str,
    args: Vec<Value<'js>>,
    key: &str,
    context: &[String],
    context_data: &[String],
    dry_run: bool,
    errors: &mut usize,
) {
    if dry_run {
        return;
    }
    let fn_val: Value = match kind.get(method) {
        Ok(v) => v,
        Err(_) => return,
    };
    if !fn_val.is_function() {
        return;
    }
    let func = match fn_val.into_function() {
        Some(f) => f,
        None => return,
    };
    if let Err(e) = call_and_check(func, args, key, context, context_data) {
        eprintln!("[error] {key}: {e}");
        *errors += 1;
    }
}

fn reconcile_kind<'js>(
    ctx: &Ctx<'js>,
    kind: &Object<'js>,
    leaves: Vec<Leaf<'js>>,
    dry_run: bool,
    quiet: bool,
) -> rquickjs::Result<RunStats> {
    let mut r = RunStats {
        enter: 0,
        update: 0,
        exit: 0,
        unchanged: 0,
        errors: 0,
    };

    let observe_fn: Function = kind.get("observe")?;
    let obs_raw: Value = observe_fn.call::<(), Value>(())?;
    let obs_val: Value = await_val(obs_raw)?;
    let obs_arr = obs_val.into_array().ok_or_else(|| {
        let err = ctx
            .eval::<Value, _>(r#"new TypeError("esto: observe() must return an array")"#)
            .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
        ctx.throw(err)
    })?;

    let key_fn: Function = kind.get("key")?;
    let value_fn: Function = kind.get("value")?;

    let mut current: HashMap<String, (Value<'js>, String)> = HashMap::new();
    for i in 0..obs_arr.len() {
        let item: Value = obs_arr.get(i)?;
        let k: String = key_fn.call::<(Value,), String>((item.clone(),))?;
        let v: String = value_fn.call::<(Value,), String>((item.clone(),))?;
        current.insert(k, (item, v));
    }

    let mut desired: HashMap<String, DesiredEntry<'js>> = HashMap::new();
    for leaf in leaves {
        let k: String = key_fn.call::<(Value,), String>((leaf.item.clone(),))?;
        let v: String = value_fn.call::<(Value,), String>((leaf.item.clone(),))?;
        desired.insert(
            k,
            DesiredEntry {
                item: leaf.item,
                value_str: v,
                context: leaf.context,
                context_data: leaf.context_data,
            },
        );
    }

    for (k, entry) in &desired {
        match current.get(k) {
            None => {
                if !quiet {
                    eprintln!("[enter] {k}");
                }
                r.enter += 1;
                call_lifecycle(
                    kind,
                    "enter",
                    vec![entry.item.clone()],
                    k,
                    &entry.context,
                    &entry.context_data,
                    dry_run,
                    &mut r.errors,
                );
            }
            Some((c_item, c_val)) => {
                if entry.value_str != *c_val {
                    if !quiet {
                        eprintln!("[update] {k}");
                    }
                    r.update += 1;
                    call_lifecycle(
                        kind,
                        "update",
                        vec![entry.item.clone(), c_item.clone()],
                        k,
                        &entry.context,
                        &entry.context_data,
                        dry_run,
                        &mut r.errors,
                    );
                } else {
                    r.unchanged += 1;
                }
            }
        }
    }

    for (k, (c_item, _)) in &current {
        if !desired.contains_key(k) {
            if !quiet {
                eprintln!("[exit] {k}");
            }
            r.exit += 1;
            call_lifecycle(
                kind,
                "exit",
                vec![c_item.clone()],
                k,
                &[],
                &[],
                dry_run,
                &mut r.errors,
            );
        }
    }

    Ok(r)
}

/// Builds a fresh `Runtime`/`Context` pair, wiring up the synthetic builtin
/// modules derived from `entries` alongside a caller-supplied `resolver`/
/// `loader` pair for everything else (typically `./`/`../` relative
/// filesystem imports).
fn build_runtime<R, L>(
    entries: &[crate::EsEntry],
    resolver: R,
    loader: L,
) -> rquickjs::Result<(Runtime, Context)>
where
    R: Resolver + 'static,
    L: Loader + 'static,
{
    let runtime = Runtime::new()?;
    let mut module_groups: HashMap<&'static str, Vec<&crate::EsEntry>> = HashMap::new();
    for e in entries {
        module_groups.entry(e.module_path).or_default().push(e);
    }
    let builtin_resolver = module_groups
        .keys()
        .fold(BuiltinResolver::default(), |r, path| r.with_module(*path));
    let builtin_loader = module_groups
        .iter()
        .fold(BuiltinLoader::default(), |l, (path, es)| {
            l.with_module(*path, crate::synthetic_module_source_for_entries(es))
        });
    runtime.set_loader((builtin_resolver, resolver), (builtin_loader, loader));
    let context = Context::full(&runtime)?;
    Ok((runtime, context))
}

fn collect_leaves<'js>(
    ctx: &Ctx<'js>,
    path_str: &str,
    src: &str,
    is_jsx: bool,
) -> rquickjs::Result<Vec<Leaf<'js>>> {
    let module = Module::declare(ctx.clone(), path_str.to_string(), src.to_string())?;
    let (module, promise) = module.eval()?;
    promise.finish::<()>()?;

    let default_val: Value = module.get("default")?;

    let leaves = if is_jsx {
        let root_fn = default_val.into_function().ok_or_else(|| {
            let err = ctx
                .eval::<Value, _>(
                    r#"new TypeError("esto: JSX script default export must be a function")"#,
                )
                .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
            ctx.throw(err)
        })?;
        let root: Value = root_fn.call::<(), Value>(())?;
        reduce(ctx, root, vec![], vec![])?
    } else {
        let target = default_val.into_object().ok_or_else(|| {
            let err = ctx
                .eval::<Value, _>(
                    r#"new TypeError("esto: non-JSX script default export must be an object")"#,
                )
                .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
            ctx.throw(err)
        })?;
        let desired_fn: Function = target.get("desired")?;
        let desired_raw: Value = desired_fn.call::<(), Value>(())?;
        let desired_val: Value = await_val(desired_raw)?;
        let desired_arr = desired_val.into_array().ok_or_else(|| {
            let err = ctx
                .eval::<Value, _>(r#"new TypeError("esto: desired() must return an array")"#)
                .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
            ctx.throw(err)
        })?;
        let mut leaves = Vec::new();
        for i in 0..desired_arr.len() {
            let item: Value = desired_arr.get(i)?;
            leaves.push(Leaf {
                kind_id: SYNTHETIC_KIND_ID,
                kind: target.clone(),
                item,
                context: vec![],
                context_data: vec![],
            });
        }
        leaves
    };

    Ok(leaves)
}

/// Runs `path` with the default filesystem resolver/loader: relative imports
/// resolve against the script's own directory with no path confinement and
/// no extension-fallback (the import specifier must match the target
/// filename exactly). This is esto's existing behavior, unchanged.
pub fn run_script(
    path: &str,
    entries: &[crate::EsEntry],
    setup: fn(&Ctx<'_>) -> rquickjs::Result<()>,
    dry_run: bool,
    quiet: bool,
) -> Result<RunStats, crate::ScriptError> {
    let abs_path = std::path::Path::new(path)
        .canonicalize()
        .map_err(crate::ScriptError::Io)?;
    let base_dir = abs_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    run_script_with_loader(
        path,
        entries,
        setup,
        dry_run,
        quiet,
        ScriptResolver { base_dir },
        ScriptLoader,
    )
}

/// Like [`run_script`], but with a caller-supplied resolver/loader pair
/// instead of the default [`ScriptResolver`]/[`ScriptLoader`]. Use this to
/// opt into a different import-resolution policy — for example
/// [`crate::loader::ConfinedFsResolver`]/[`crate::loader::ConfinedFsLoader`]
/// for path-confined, extension-fallback resolution.
pub fn run_script_with_loader<R, L>(
    path: &str,
    entries: &[crate::EsEntry],
    setup: fn(&Ctx<'_>) -> rquickjs::Result<()>,
    dry_run: bool,
    quiet: bool,
    resolver: R,
    loader: L,
) -> Result<RunStats, crate::ScriptError>
where
    R: Resolver + 'static,
    L: Loader + 'static,
{
    let abs_path = std::path::Path::new(path)
        .canonicalize()
        .map_err(crate::ScriptError::Io)?;
    let path_str = abs_path
        .to_str()
        .ok_or_else(|| crate::ScriptError::InvalidPath(abs_path.to_string_lossy().into_owned()))?
        .to_string();
    let needs_transform = is_script_file(&path_str);
    let is_jsx = path_str.ends_with(".jsx") || path_str.ends_with(".tsx");

    let src_raw = std::fs::read_to_string(&path_str).map_err(crate::ScriptError::Io)?;
    let src = if needs_transform {
        transform_source(&src_raw, &path_str)
    } else {
        src_raw
    };

    let (_runtime, context) = build_runtime(entries, resolver, loader)
        .map_err(|e| crate::ScriptError::Worker(e.to_string()))?;

    let stats = context
        .with(|ctx| -> rquickjs::Result<RunStats> {
            setup(&ctx)?;

            let leaves = collect_leaves(&ctx, &path_str, &src, is_jsx)?;

            let mut by_kind: HashMap<u32, (Object, Vec<Leaf>)> = HashMap::new();
            for leaf in leaves {
                let entry = by_kind
                    .entry(leaf.kind_id)
                    .or_insert_with(|| (leaf.kind.clone(), vec![]));
                entry.1.push(leaf);
            }

            let mut stats = RunStats {
                enter: 0,
                update: 0,
                exit: 0,
                unchanged: 0,
                errors: 0,
            };

            for (_, (kind_obj, kind_leaves)) in by_kind {
                let res = reconcile_kind(&ctx, &kind_obj, kind_leaves, dry_run, quiet)?;
                stats.enter += res.enter;
                stats.update += res.update;
                stats.exit += res.exit;
                stats.unchanged += res.unchanged;
                stats.errors += res.errors;
            }

            Ok(stats)
        })
        .map_err(|e| crate::ScriptError::Worker(e.to_string()))?;

    if !quiet {
        eprintln!(
            "reconciled: {} enter, {} update, {} exit ({} unchanged)",
            stats.enter, stats.update, stats.exit, stats.unchanged
        );
    }

    Ok(stats)
}
