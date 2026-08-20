//! The reconciler vocabulary is usable from this crate alone — no `esto`.
//!
//! Both claims matter to an embedder that is not a CLI: the first is that a
//! `unit()` script runs at all, the second is that taking the vocabulary does
//! not drag in the builtins that touch the world. A runtime evaluating on a
//! latency budget registers `RECONCILER_BUILTINS` and nothing else, and needs
//! that to be a guarantee rather than a convention.

use optative_script::builtins::{EFFECTFUL_BUILTINS, RECONCILER_BUILTINS, register_all};
use optative_script::{Ctx, run_script};

fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    register_all(ctx, RECONCILER_BUILTINS)
}

const UNIT_SCRIPT: &str = r#"
import { h, unit, optativeSet } from 'esto'

const Thing = unit({
  key: (i) => i.name,
  value: (i) => i.name,
  reconciler: optativeSet({ observe: () => [] }),
  enter: (i) => i.name,
})

export default () => <Thing name="alpha" />
"#;

fn write_entry(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("optative_script_builtins_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, source).unwrap();
    path
}

#[test]
fn reconciler_builtins_alone_can_drive_a_unit() {
    let entry = write_entry("unit.jsx", UNIT_SCRIPT);
    let stats = run_script(
        entry.to_str().unwrap(),
        RECONCILER_BUILTINS,
        setup,
        true,
        true,
        None,
    )
    .expect("script with only RECONCILER_BUILTINS should run");

    assert_eq!(stats.enter, 1, "the unobserved item should want to enter");
    assert_eq!(stats.errors, 0);
}

const SH_SCRIPT: &str =
    "import { h, sh } from 'esto'\nexport default () => { sh`true`; return [] }\n";

#[test]
fn reconciler_builtins_alone_do_not_export_sh() {
    let entry = write_entry("with_sh.jsx", SH_SCRIPT);
    let result = run_script(
        entry.to_str().unwrap(),
        RECONCILER_BUILTINS,
        setup,
        true,
        true,
        None,
    );

    assert!(
        result.is_err(),
        "importing sh should fail when only RECONCILER_BUILTINS is registered"
    );
}

/// The control for the test above: the same script, refused for the missing
/// export and not for anything else about it.
#[test]
fn the_same_script_runs_once_the_effectful_builtins_are_added() {
    fn setup_with_effects(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
        register_all(ctx, RECONCILER_BUILTINS)?;
        register_all(ctx, EFFECTFUL_BUILTINS)
    }

    let entry = write_entry("with_sh_allowed.jsx", SH_SCRIPT);
    let entries = [RECONCILER_BUILTINS, EFFECTFUL_BUILTINS].concat();
    run_script(
        entry.to_str().unwrap(),
        &entries,
        setup_with_effects,
        true,
        true,
        None,
    )
    .expect("sh should import once EFFECTFUL_BUILTINS is registered too");
}
