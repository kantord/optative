pub mod esto;
pub mod fs;
pub mod internal;

pub(super) fn hex_sha256(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

pub use esto::{
    register_context_marker, register_exists, register_fragment, register_h, register_hash,
    register_ls, register_prompt, register_read, register_sh, register_unit,
};
pub use fs::{register_fs_file, register_fs_folder, register_fs_git_repo};
pub use internal::{
    register_console, register_console_print, register_cwd, register_git_root, register_glob,
    register_is_dir,
};

pub fn register_fs_internal(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    use rquickjs::function::Function;
    ctx.globals().set("__esto_fs_claimFile", Function::new(ctx.clone(), fs::fs_claim_file_fn)?)?;
    ctx.globals().set("__esto_fs_fileEnumerate", Function::new(ctx.clone(), fs::fs_file_enumerate_fn)?)?;
    ctx.globals().set("__esto_fs_folderEnumerate", Function::new(ctx.clone(), fs::fs_folder_enumerate_fn)?)?;
    ctx.globals().set("__esto_fs_scopeSupervise", Function::new(ctx.clone(), fs::fs_scope_supervise_fn)?)?;
    Ok(())
}

pub fn register_internal(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    register_console_print(ctx)?;
    register_console(ctx)?;
    register_glob(ctx)?;
    register_git_root(ctx)?;
    register_is_dir(ctx)?;
    register_cwd(ctx)?;
    register_fs_internal(ctx)?;
    Ok(())
}
