use std::path::Path;

use rquickjs::function::Function;
use rquickjs::Ctx;

use optative_script::serde_json_simple_array;

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
            .map_err(rquickjs::Error::Io)?;
        if !out.status.success() { return Err(rquickjs::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "git rev-parse --show-toplevel failed"))); }
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
            .map_err(rquickjs::Error::Io)
    })?)?;
    Ok(())
}
