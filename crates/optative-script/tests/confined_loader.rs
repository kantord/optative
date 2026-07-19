//! Confirms `run_script_with_loader` actually plugs in a caller-supplied
//! resolver/loader pair, using `ConfinedFsResolver`/`ConfinedFsLoader` as the
//! example opt-in policy. Contrasted against `run_script`'s default
//! (non-extension-fallback) behavior to show the custom pair was genuinely
//! exercised, not incidentally compatible.

use std::sync::{Arc, Mutex};

use optative_script::loader::{ConfinedFsLoader, ConfinedFsResolver};
use optative_script::{Ctx, run_script, run_script_with_loader};

fn noop_setup(_ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    Ok(())
}

/// Writes a fixture where `entry.js` imports `./sibling` (no extension) and
/// exports a minimal "kind" object with an empty `desired()`.
fn write_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("sibling.js"), "export const marker = 1;").unwrap();
    let entry = dir.join("entry.js");
    std::fs::write(
        &entry,
        "import { marker } from './sibling';\nexport default { desired: () => marker && [] };",
    )
    .unwrap();
    entry
}

#[test]
fn run_script_with_loader_uses_the_supplied_confined_resolver() {
    let tmp = std::env::temp_dir().join(format!(
        "optative_script_run_with_loader_{}",
        std::process::id()
    ));
    let entry = write_fixture(&tmp);

    let loaded_paths = Arc::new(Mutex::new(Vec::new()));
    let stats = run_script_with_loader(
        entry.to_str().unwrap(),
        &[],
        noop_setup,
        true,
        true,
        None,
        ConfinedFsResolver::new(tmp.clone()),
        ConfinedFsLoader::new(loaded_paths),
    )
    .expect("extension-fallback resolver should resolve './sibling' to sibling.js");

    assert_eq!(stats.enter, 0);
    assert_eq!(stats.errors, 0);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn default_run_script_rejects_the_same_extensionless_import() {
    let tmp = std::env::temp_dir().join(format!(
        "optative_script_run_default_{}",
        std::process::id()
    ));
    let entry = write_fixture(&tmp);

    let result = run_script(entry.to_str().unwrap(), &[], noop_setup, true, true, None);
    assert!(
        result.is_err(),
        "default resolver has no extension fallback, expected './sibling' to fail to resolve"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
