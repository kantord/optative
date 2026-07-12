use std::path::Path;

/// Circuit-breaker: maximum number of managed files that may be pruned in one supervisor pass.
const PRUNE_MAX_ABS: usize = 10;
/// Circuit-breaker: maximum fraction of the managed set that may be pruned in one supervisor pass.
const PRUNE_MAX_PCT: f64 = 0.5;

use rquickjs::function::Function;
use rquickjs::{Ctx, FromJs, Object, Value};
use sha2::{Digest, Sha256};

// Extract the render prop: children is either a function or [function].
fn render_prop<'js>(ctx: &Ctx<'js>, children: &Value<'js>) -> rquickjs::Result<Function<'js>> {
    if let Some(arr) = children.as_array() {
        arr.get::<Function<'js>>(0)
    } else {
        Function::from_js(ctx, children.clone())
    }
}

// Glob paths filtered by dir/non-dir.
fn glob_filter(pattern: &str, want_dirs: bool) -> Vec<String> {
    glob::glob(pattern)
        .map(|ps| ps.filter_map(|p| p.ok())
            .filter(|p| p.is_dir() == want_dirs)
            .filter_map(|p| p.to_str().map(|s| s.to_owned()))
            .collect())
        .unwrap_or_default()
}

fn hash_str(s: &str) -> String {
    let h = Sha256::digest(s.as_bytes());
    format!("{h:x}")
}

fn throw_js<'js>(ctx: &Ctx<'js>, msg: &str) -> rquickjs::Error {
    let escaped = msg.replace('\\', r"\\").replace('"', "\\\"").replace('\n', "\\n");
    let err_val = ctx.eval::<Value<'js>, _>(format!(r#"new Error("{escaped}")"#))
        .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
    ctx.throw(err_val)
}

// Small JS factory strings: eval'd at call-time to create curried functions.
// They call Rust globals (__esto_fs_fileEnumerate / __esto_fs_folderEnumerate /
// __esto_fs_scopeSupervise) so they require no logic of their own.
const MAKE_FILE_JS: &str = concat!(
    "(function(rootDir){",
    "return function File({glob:pattern,children}){",
    "const r=Array.isArray(children)?children[0]:children;",
    "return globalThis.__esto_fs_fileEnumerate(rootDir,pattern,r)};",
    "})"
);
const MAKE_SCOPED_FOLDER_JS: &str = concat!(
    "(function(parentDir){",
    "return function Folder({name,glob:g,children}){",
    "const r=Array.isArray(children)?children[0]:children;",
    "return name?globalThis.__esto_fs_scopeSupervise(parentDir,name,r)",
    ":globalThis.__esto_fs_folderEnumerate(parentDir,g,r)};",
    "})"
);
// Creates the ManagedFile unit props object, closing over the scope directory D.
const MANAGED_FILE_PROPS_JS: &str = concat!(
    "(function(D){return{",
    "key:f=>f.path,",
    "value:f=>f.hash,",
    "observe:()=>JSON.parse(globalThis.__esto_glob(D+'/**/*'))",
    ".filter(p=>!globalThis.__esto_is_dir(p))",
    ".map(p=>{const r=p.slice(D.length+1);",
    "return{path:r,absolutePath:p,",
    "hash:globalThis.__esto_hash(globalThis.__esto_read(p)),",
    "desiredContent:null}}),",
    "enter:f=>globalThis.__esto_sh`mkdir -p ${f.absolutePath.slice(0,f.absolutePath.lastIndexOf('/'))||'.'} && printf '%s' ${f.desiredContent??''} > ${f.absolutePath}`,",
    "update:f=>{if(f.desiredContent!==null)globalThis.__esto_sh`printf '%s' ${f.desiredContent} > ${f.absolutePath}`},",
    "exit:f=>globalThis.__esto_sh`rm -f ${f.absolutePath}`,",
    "}})"
);

fn make_file_curried<'js>(ctx: &Ctx<'js>, root_dir: &str) -> rquickjs::Result<Value<'js>> {
    let factory: Function<'js> = ctx.eval(MAKE_FILE_JS)?;
    factory.call::<_, Value<'js>>((root_dir,))
}

fn make_scoped_folder<'js>(ctx: &Ctx<'js>, parent_dir: &str) -> rquickjs::Result<Value<'js>> {
    let factory: Function<'js> = ctx.eval(MAKE_SCOPED_FOLDER_JS)?;
    factory.call::<_, Value<'js>>((parent_dir,))
}

fn fs_file_fn<'js>(ctx: Ctx<'js>, props: Object<'js>) -> rquickjs::Result<Value<'js>> {
    let pattern: String = props.get("glob")?;
    let children: Value<'js> = props.get("children")?;
    let render: Function<'js> = render_prop(&ctx, &children)?;
    let files = glob_filter(&pattern, false);
    let results: Vec<Value<'js>> = files.iter().map(|file| {
        let arg = Object::new(ctx.clone())?;
        arg.set("file", file.as_str())?;
        render.call::<_, Value<'js>>((arg,))
    }).collect::<rquickjs::Result<_>>()?;
    rquickjs::IntoJs::into_js(results, &ctx)
}

pub fn register_fs_file(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_fs_File", Function::new(ctx.clone(), fs_file_fn)?)?;
    Ok(())
}

fn fs_folder_fn<'js>(ctx: Ctx<'js>, props: Object<'js>) -> rquickjs::Result<Value<'js>> {
    let name_str = props.get::<_, Value<'js>>("name")?
        .as_string().and_then(|s| s.to_string().ok()).unwrap_or_default();
    let children: Value<'js> = props.get("children")?;
    let render: Function<'js> = render_prop(&ctx, &children)?;

    if !name_str.is_empty() {
        let cwd = std::env::current_dir()
            .map(|p| p.to_str().unwrap_or(".").to_owned())
            .map_err(rquickjs::Error::Io)?;
        return scope_supervise(&ctx, &cwd, &name_str, render);
    }

    let pattern: String = props.get("glob")?;
    folder_enumerate(&ctx, None, &pattern, render)
}

pub fn register_fs_folder(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_fs_Folder", Function::new(ctx.clone(), fs_folder_fn)?)?;
    Ok(())
}

fn fs_git_repo_fn<'js>(ctx: Ctx<'js>, props: Object<'js>) -> rquickjs::Result<Value<'js>> {
    let root: String = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(rquickjs::Error::Io)
        .and_then(|out| if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        } else {
            Err(rquickjs::Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("git rev-parse exited with {}", out.status),
            )))
        })?;
    let children: Value<'js> = props.get("children")?;
    let render: Function<'js> = render_prop(&ctx, &children)?;
    let file_fn = make_file_curried(&ctx, &root)?;
    let folder_fn = make_scoped_folder(&ctx, &root)?;
    let arg = Object::new(ctx.clone())?;
    arg.set("repoRoot", root.as_str())?;
    arg.set("File", file_fn)?;
    arg.set("Folder", folder_fn)?;
    render.call::<_, Value<'js>>((arg,))
}

pub fn register_fs_git_repo(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set("__esto_fs_GitRepo", Function::new(ctx.clone(), fs_git_repo_fn)?)?;
    Ok(())
}

struct ClaimsResult<'js> {
    claims: Vec<Object<'js>>,
    body: Vec<Value<'js>>,
}

fn extract_claims<'js>(node: Value<'js>) -> rquickjs::Result<ClaimsResult<'js>> {
    if node.is_null() || node.is_undefined() || node.as_bool() == Some(false) {
        return Ok(ClaimsResult { claims: vec![], body: vec![] });
    }
    if let Some(arr) = node.as_array() {
        let mut all_claims = Vec::new();
        let mut all_body = Vec::new();
        for i in 0..arr.len() {
            let r = extract_claims(arr.get(i)?)?;
            all_claims.extend(r.claims);
            all_body.extend(r.body);
        }
        return Ok(ClaimsResult { claims: all_claims, body: all_body });
    }
    if !node.is_object() {
        return Ok(ClaimsResult { claims: vec![], body: vec![] });
    }
    // node is an Object — inspect without consuming it first
    let (is_claim, is_fragment, is_component) = {
        let obj = node.as_object().unwrap();
        (obj.contains_key("$estoFsClaim")?, obj.contains_key("$fragment")?, obj.contains_key("$component")?)
    };
    if is_claim {
        return Ok(ClaimsResult { claims: vec![node.as_object().unwrap().clone()], body: vec![] });
    }
    if is_fragment {
        let children: Value<'js> = node.as_object().unwrap().get("children")?;
        return extract_claims(children);
    }
    if is_component {
        let obj = node.as_object().unwrap();
        let component: Function<'js> = obj.get("$component")?;
        let props: Value<'js> = obj.get("props")?;
        let _ = obj;
        let result: Value<'js> = component.call::<_, Value<'js>>((props,))?;
        return extract_claims(result);
    }
    Ok(ClaimsResult { claims: vec![], body: vec![node] })
}

struct ClaimEntry<'js> {
    content: Option<String>,
    bodies: Vec<Function<'js>>,
}

fn scope_supervise<'js>(ctx: &Ctx<'js>, parent_dir: &str, name: &str, render: Function<'js>) -> rquickjs::Result<Value<'js>> {
    let abs_dir = format!("{parent_dir}/{name}");

    // Snapshot existing files in scope
    let scope_abs: Vec<String> = glob_filter(&format!("{abs_dir}/**/*"), false);
    let scope_rels: Vec<String> = scope_abs.iter()
        .map(|p| p[abs_dir.len() + 1..].to_string())
        .collect();

    // Call render prop with claim-File and scoped-Folder
    let claim_file: Function<'js> = ctx.globals().get("__esto_fs_claimFile")?;
    let scoped_folder = make_scoped_folder(ctx, &abs_dir)?;
    let render_arg = Object::new(ctx.clone())?;
    render_arg.set("File", claim_file)?;
    render_arg.set("Folder", scoped_folder)?;
    let tree: Value<'js> = render.call::<_, Value<'js>>((render_arg,))?;
    let ClaimsResult { claims, body } = extract_claims(tree)?;

    // Circuit breaker: non-empty scope, zero claims
    if !scope_rels.is_empty() && claims.is_empty() {
        return Err(throw_js(ctx, &format!(
            "esto/fs: circuit breaker — zero claims on non-empty scope {:?} ({} files). Add <File/> to keep all, or check your render prop.",
            name, scope_rels.len()
        )));
    }

    // Resolve claims → claim map
    let mut claim_map: std::collections::HashMap<String, ClaimEntry<'js>> = std::collections::HashMap::new();
    for claim in &claims {
        let matcher: Object<'js> = claim.get("matcher")?;
        let kind: String = matcher.get("kind")?;
        let content_val: Value<'js> = claim.get("content")?;
        let content: Option<String> = if content_val.is_null() || content_val.is_undefined() {
            None
        } else {
            content_val.as_string().and_then(|s| s.to_string().ok())
        };
        let body_val: Value<'js> = claim.get("body")?;
        let body_fn: Option<Function<'js>> = if body_val.is_null() || body_val.is_undefined() {
            None
        } else {
            Some(Function::from_js(ctx, body_val)?)
        };

        let matched: Vec<String> = match kind.as_str() {
            "all" => scope_rels.clone(),
            "glob" => {
                let pattern: String = matcher.get("pattern")?;
                glob_filter(&format!("{abs_dir}/{pattern}"), false)
                    .into_iter().map(|p| p[abs_dir.len() + 1..].to_string()).collect()
            },
            _ => { // "name"
                let n: String = matcher.get("name")?;
                if scope_rels.contains(&n) || content.is_some() { vec![n] } else { vec![] }
            },
        };

        for rel in matched {
            let entry = claim_map.entry(rel.clone()).or_insert(ClaimEntry { content: None, bodies: vec![] });
            if let Some(ref c) = content {
                if let Some(ref ex) = entry.content {
                    if ex != c {
                        return Err(throw_js(ctx, &format!(
                            "esto/fs: content conflict for {:?} — two claims specify different content", rel
                        )));
                    }
                }
                entry.content = Some(c.clone());
            }
            if let Some(ref f) = body_fn { entry.bodies.push(f.clone()); }
        }
    }

    // Circuit breaker: too many pruned
    let prune_count = scope_rels.iter().filter(|r| !claim_map.contains_key(*r)).count();
    if prune_count > PRUNE_MAX_ABS ||
       (!scope_rels.is_empty() && prune_count as f64 / scope_rels.len() as f64 > PRUNE_MAX_PCT) {
        return Err(throw_js(ctx, &format!(
            "esto/fs: circuit breaker — {}/{} files in {:?} would be pruned. Run --dry-run first, then narrow scope or add more claims.",
            prune_count, scope_rels.len(), name
        )));
    }

    // Build ManagedFile unit
    let make_props: Function<'js> = ctx.eval(MANAGED_FILE_PROPS_JS)?;
    let props_obj: Object<'js> = make_props.call::<_, Object<'js>>((abs_dir.as_str(),))?;
    let unit_fn: Function<'js> = ctx.globals().get("__esto_unit")?;
    let managed_file: Value<'js> = unit_fn.call::<_, Value<'js>>((props_obj,))?;
    let h_fn: Function<'js> = ctx.globals().get("__esto_h")?;

    // Create managed leaf nodes + body nodes from claim map
    let mut managed_leaves: Vec<Value<'js>> = Vec::new();
    let mut body_nodes: Vec<Value<'js>> = Vec::new();
    for (rel, entry) in &claim_map {
        let abs_path = format!("{abs_dir}/{rel}");
        let desired_hash = match &entry.content {
            Some(c) => hash_str(c),
            None => if Path::new(&abs_path).exists() {
                std::fs::read_to_string(&abs_path).map(|s| hash_str(&s)).unwrap_or_else(|_| hash_str(""))
            } else {
                hash_str("")
            },
        };
        let desired_content: Value<'js> = match &entry.content {
            Some(c) => rquickjs::String::from_str(ctx.clone(), c)?.into(),
            None => Value::new_null(ctx.clone()),
        };
        let leaf_props = Object::new(ctx.clone())?;
        leaf_props.set("path", rel.as_str())?;
        leaf_props.set("absolutePath", abs_path.as_str())?;
        leaf_props.set("hash", desired_hash.as_str())?;
        leaf_props.set("desiredContent", desired_content)?;
        managed_leaves.push(h_fn.call::<_, Value<'js>>((managed_file.clone(), leaf_props))?);

        for body_fn in &entry.bodies {
            let arg = Object::new(ctx.clone())?;
            arg.set("file", rel.as_str())?;
            let result: Value<'js> = body_fn.call::<_, Value<'js>>((arg,))?;
            if !result.is_null() && !result.is_undefined() && result.as_bool() != Some(false) {
                body_nodes.push(result);
            }
        }
    }

    let mut all: Vec<Value<'js>> = managed_leaves;
    all.extend(body);
    all.extend(body_nodes);
    rquickjs::IntoJs::into_js(all, ctx)
}

fn folder_enumerate<'js>(ctx: &Ctx<'js>, root: Option<&str>, pattern: &str, render: Function<'js>) -> rquickjs::Result<Value<'js>> {
    let full = root.map(|r| format!("{r}/{pattern}")).unwrap_or_else(|| pattern.to_string());
    let dirs = glob_filter(&full, true);
    let results: Vec<Value<'js>> = dirs.iter().map(|abs_dir| {
        let rel = root.map(|r| abs_dir[r.len() + 1..].to_string()).unwrap_or_else(|| abs_dir.clone());
        let file_fn = make_file_curried(ctx, abs_dir)?;
        let folder_fn = make_scoped_folder(ctx, abs_dir)?;
        let arg = Object::new(ctx.clone())?;
        arg.set("dir", rel.as_str())?;
        arg.set("File", file_fn)?;
        arg.set("Folder", folder_fn)?;
        render.call::<_, Value<'js>>((arg,))
    }).collect::<rquickjs::Result<_>>()?;
    rquickjs::IntoJs::into_js(results, ctx)
}

// claim-File component used inside supervisor render props
pub(super) fn fs_claim_file_fn<'js>(ctx: Ctx<'js>, props: Object<'js>) -> rquickjs::Result<Value<'js>> {
    let name_str = props.get::<_, Value<'js>>("name")?
        .as_string().and_then(|s| s.to_string().ok()).unwrap_or_default();
    let glob_str = props.get::<_, Value<'js>>("glob")?
        .as_string().and_then(|s| s.to_string().ok()).unwrap_or_default();
    let content_val: Value<'js> = props.get("content")?;
    let children_val: Value<'js> = props.get("children")?;

    let matcher = Object::new(ctx.clone())?;
    if !name_str.is_empty() {
        matcher.set("kind", "name")?;
        matcher.set("name", name_str)?;
    } else if !glob_str.is_empty() {
        matcher.set("kind", "glob")?;
        matcher.set("pattern", glob_str)?;
    } else {
        matcher.set("kind", "all")?;
    }
    let content: Value<'js> = if content_val.is_undefined() { Value::new_null(ctx.clone()) } else { content_val };
    let body: Value<'js> = if children_val.is_null() || children_val.is_undefined() {
        Value::new_null(ctx.clone())
    } else if let Some(arr) = children_val.as_array() {
        if arr.len() == 0 {
            Value::new_null(ctx.clone())
        } else {
            arr.get::<Function<'js>>(0)?.into()
        }
    } else {
        Function::from_js(&ctx, children_val)?.into()
    };

    let result = Object::new(ctx.clone())?;
    result.set("$estoFsClaim", true)?;
    result.set("matcher", matcher)?;
    result.set("content", content)?;
    result.set("body", body)?;
    Ok(Value::from_object(result))
}

// 3-arg Rust globals called by the curried JS factory strings
pub(super) fn fs_file_enumerate_fn<'js>(ctx: Ctx<'js>, root_dir: Value<'js>, pattern: String, render: Function<'js>) -> rquickjs::Result<Value<'js>> {
    let root_opt = root_dir.as_string().and_then(|s| s.to_string().ok());
    let full = root_opt.as_deref().map(|r| format!("{r}/{pattern}")).unwrap_or(pattern);
    let paths = glob_filter(&full, false);
    let rels: Vec<String> = root_opt.as_deref()
        .map(|r| paths.iter().map(|p| p[r.len() + 1..].to_string()).collect())
        .unwrap_or(paths);
    let results: Vec<Value<'js>> = rels.iter().map(|file| {
        let arg = Object::new(ctx.clone())?;
        arg.set("file", file.as_str())?;
        render.call::<_, Value<'js>>((arg,))
    }).collect::<rquickjs::Result<_>>()?;
    rquickjs::IntoJs::into_js(results, &ctx)
}

pub(super) fn fs_folder_enumerate_fn<'js>(ctx: Ctx<'js>, root_dir: Value<'js>, pattern: String, render: Function<'js>) -> rquickjs::Result<Value<'js>> {
    let root_opt = root_dir.as_string().and_then(|s| s.to_string().ok());
    folder_enumerate(&ctx, root_opt.as_deref(), &pattern, render)
}

pub(super) fn fs_scope_supervise_fn<'js>(ctx: Ctx<'js>, parent_dir: String, name: String, render: Function<'js>) -> rquickjs::Result<Value<'js>> {
    scope_supervise(&ctx, &parent_dir, &name, render)
}
