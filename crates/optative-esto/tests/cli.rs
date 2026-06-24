use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn esto() -> Command {
    Command::new(env!("CARGO_BIN_EXE_esto"))
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example(name: &str) -> String {
    examples_dir().join(name).to_string_lossy().into_owned()
}

// ── esto run (JS subcommand) ─────────────────────────────────────────────────

#[test]
fn mirror_mjs_creates_output_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.txt"), "alpha=one\nbeta=two\n").unwrap();

    let status = esto()
        .args(["run", &example("mirror.mjs")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert!(status.success(), "esto run mirror.mjs should exit 0");
    assert!(dir.path().join("out/alpha.txt").exists(), "alpha.txt should be created");
    assert!(dir.path().join("out/beta.txt").exists(), "beta.txt should be created");
}

#[test]
fn mirror_mjs_dry_run_exits_with_delta_count() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.txt"), "alpha=one\nbeta=two\n").unwrap();

    let status = esto()
        .args(["run", "--dry-run", &example("mirror.mjs")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    // 2 items enter → exit code 2
    assert_eq!(status.code(), Some(2), "dry-run exit code should equal delta count");
    assert!(!dir.path().join("out").exists(), "dry-run should not create any files");
}

#[test]
fn mirror_mjs_converged_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.txt"), "alpha=one\n").unwrap();

    // First run to reach desired state
    esto()
        .args(["run", &example("mirror.mjs")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    // Second run: already converged → exit 0 and no changes
    let status = esto()
        .args(["run", &example("mirror.mjs")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert!(status.success(), "second run on converged state should exit 0");
}

#[test]
fn mirror_jsx_creates_output_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("manifest.txt"), "alpha=hello\n").unwrap();

    let status = esto()
        .args(["run", &example("mirror.eso.jsx")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert!(status.success(), "esto run mirror.eso.jsx should exit 0");
    assert!(dir.path().join("out/alpha.txt").exists(), "alpha.txt should be created");
}

#[test]
fn grounding_creates_task_and_context_files() {
    let dir = tempfile::tempdir().unwrap();

    let status = esto()
        .args(["run", &example("grounding.eso.jsx")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert!(status.success(), "esto run grounding.eso.jsx should exit 0");
    assert!(dir.path().join("tasks/foo.md").exists(), "tasks/foo.md should be created");
    assert!(dir.path().join("tasks/bar.md").exists(), "tasks/bar.md should be created");

    // Both leaves share the same two context entries → exactly 2 content-addressed context files
    let ctx_count = fs::read_dir(dir.path().join(".esto/context"))
        .unwrap()
        .count();
    assert_eq!(ctx_count, 2, "context files should be deduped: 2 unique strings → 2 files");
}

#[test]
fn grounding_dry_run_exits_with_delta_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();

    let status = esto()
        .args(["run", "--dry-run", &example("grounding.eso.jsx")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    // 2 leaves enter → exit code 2
    assert_eq!(status.code(), Some(2), "dry-run exit code should equal delta count");
    assert!(!dir.path().join("tasks").exists(), "dry-run should not create tasks/");
    assert!(!dir.path().join(".esto").exists(), "dry-run should not create .esto/");
}

#[test]
fn grounding_op_tsx_creates_task_files() {
    let dir = tempfile::tempdir().unwrap();

    let status = esto()
        .args(["run", &example("grounding.op.tsx")])
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert!(status.success(), "esto run grounding.op.tsx should exit 0");
    assert!(dir.path().join("tasks/foo.md").exists(), "tasks/foo.md should be created");
    assert!(dir.path().join("tasks/bar.md").exists(), "tasks/bar.md should be created");
}

// ── esto/fs stdlib ───────────────────────────────────────────────────────────

#[test]
fn esto_fs_file_glob_enters_matched_files() {
    let dir = tempfile::tempdir().unwrap();
    // Create 2 .txt files in root and 1 in a subdir (should not match *.txt)
    fs::write(dir.path().join("alpha.txt"), "a").unwrap();
    fs::write(dir.path().join("beta.txt"), "b").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/gamma.txt"), "g").unwrap();

    // Script: use File glob to enumerate *.txt (root only) and mark each as observed
    let script = r#"
import { h, unit } from 'esto'
import { File } from 'esto/fs'

const Seen = unit({
  key: (x) => x.file,
  value: (x) => x.file,
  observe: () => [],
})

export default () => (
  <File glob="*.txt">{({ file }) => <Seen file={file} />}</File>
)
"#;
    let script_path = dir.path().join("test.op.jsx");
    fs::write(&script_path, script).unwrap();

    let status = esto()
        .args(["run", "--dry-run", script_path.to_str().unwrap()])
        .current_dir(dir.path())
        .status()
        .unwrap();

    // 2 root .txt files matched → dry-run exit code = 2
    assert_eq!(status.code(), Some(2), "File glob '*.txt' should match exactly 2 root-level files");
}

#[test]
fn esto_fs_supervisor_creates_updates_keeps_prunes() {
    let dir = tempfile::tempdir().unwrap();
    // Scope: docs/api/ with 3 pre-existing files
    fs::create_dir_all(dir.path().join("docs/api")).unwrap();
    fs::write(dir.path().join("docs/api/index.md"), "old content").unwrap();
    fs::write(dir.path().join("docs/api/Bogus.md"), "orphan").unwrap();
    fs::write(dir.path().join("docs/api/notes.txt"), "hand-added").unwrap();

    // Script: supervisor claims index.md (with content), *.txt (keep), Bar.md (create)
    // Bogus.md is unclaimed → pruned
    let script = r##"
import { h, Fragment } from 'esto'
import { Folder } from 'esto/fs'

const INDEX = "# API\nAutogenerated."

export default () => (
  <Folder name="docs/api">{({ File }) =>
    <>
      <File name="index.md" content={INDEX} />
      <File name="Bar.md" content={"# Bar"} />
      <File glob="*.txt" />
    </>
  }</Folder>
)
"##;
    let script_path = dir.path().join("test.op.jsx");
    fs::write(&script_path, script).unwrap();

    let status = esto()
        .args(["run", script_path.to_str().unwrap()])
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert!(status.success(), "supervisor run should exit 0");
    // Created
    assert!(dir.path().join("docs/api/Bar.md").exists(), "Bar.md should be created");
    let bar_content = fs::read_to_string(dir.path().join("docs/api/Bar.md")).unwrap();
    assert!(bar_content.contains("Bar"), "Bar.md should contain desired content");
    // Updated (printf writes whatever bytes sh receives; JS \n = newline)
    let index_content = fs::read_to_string(dir.path().join("docs/api/index.md")).unwrap();
    assert!(index_content.contains("API"), "index.md should be updated with new content");
    // Kept
    assert!(dir.path().join("docs/api/notes.txt").exists(), "notes.txt should survive (claimed by *.txt)");
    // Pruned
    assert!(!dir.path().join("docs/api/Bogus.md").exists(), "Bogus.md should be pruned");
}

#[test]
fn esto_fs_supervisor_dry_run_prunes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("owned")).unwrap();
    fs::write(dir.path().join("owned/keep.txt"), "keep").unwrap();
    fs::write(dir.path().join("owned/orphan.txt"), "orphan").unwrap();

    let script = r#"
import { h } from 'esto'
import { Folder } from 'esto/fs'
export default () => (
  <Folder name="owned">{({ File }) => <File glob="keep.txt" />}</Folder>
)
"#;
    let script_path = dir.path().join("test.op.jsx");
    fs::write(&script_path, script).unwrap();

    let status = esto()
        .args(["run", "--dry-run", script_path.to_str().unwrap()])
        .current_dir(dir.path())
        .status()
        .unwrap();

    // dry-run: 1 exit (orphan.txt pruned) → exit code 1
    assert_eq!(status.code(), Some(1), "dry-run should exit with delta count (1 prune)");
    // dry-run writes nothing
    assert!(dir.path().join("owned/orphan.txt").exists(), "dry-run should not delete anything");
}

// ── esto types ───────────────────────────────────────────────────────────────

#[test]
fn types_writes_dts_with_module_declarations() {
    let dir = tempfile::tempdir().unwrap();

    let status = esto()
        .args(["types", "--out", dir.path().to_str().unwrap()])
        .status()
        .unwrap();

    assert!(status.success(), "esto types should exit 0");

    let dts = fs::read_to_string(dir.path().join("esto.d.ts")).unwrap();
    assert!(dts.contains("declare module \"esto\""), "esto.d.ts should contain esto module");
    assert!(dts.contains("declare module \"esto/fs\""), "esto.d.ts should contain esto/fs module");
    assert!(dts.contains("declare namespace JSX"), "esto.d.ts should declare JSX namespace");
    assert!(dts.contains("export function h("), "esto module should export h()");
    assert!(dts.contains("export function unit"), "esto module should export unit");
    assert!(dts.contains("export function exists"), "esto module should export exists");
    assert!(dts.contains("export function GitRepo"), "esto/fs module should export GitRepo");
}

// ── TSV CLI (--to / --from / --once / --fail-on-change) ─────────────────────

#[test]
fn tsv_fail_on_change_exits_one_when_delta() {
    // --to produces one new item; --fail-on-change should make it exit 1
    let status = esto()
        .args([
            "--to", "printf 'mykey\\tmyvalue\\n'",
            "--dry-run",
            "--fail-on-change",
            "--once",
            "--quiet",
        ])
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(1), "fail-on-change with delta should exit 1");
}

#[test]
fn tsv_fail_on_change_exits_zero_when_converged() {
    // --from and --to both return the same item → no delta → exit 0
    let status = esto()
        .args([
            "--from", "printf 'mykey\\tmyvalue\\n'",
            "--to",   "printf 'mykey\\tmyvalue\\n'",
            "--fail-on-change",
            "--once",
            "--quiet",
        ])
        .status()
        .unwrap();

    assert!(status.success(), "fail-on-change with no delta should exit 0");
}
