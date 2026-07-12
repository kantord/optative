//! **Experimental.** Hook-based reconciliation CLI built on
//! [optative](https://github.com/kantord/optative) and the scripting engine
//! extracted from [tauler](https://github.com/kantord/tauler). Expect breaking
//! changes between 0.x releases.

pub mod builtins;
pub mod registry;
pub mod watch;
pub mod types;

/// How many times a stateful worker may request shutdown before the dispatch is
/// considered permanently failed. One retry lets a crashed worker respawn once.
const MAX_WORKER_RETRIES: u8 = 1;

use std::io::{BufRead, Write as IoWrite};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use optative::{Lifecycle, OptativeSet};
use optative::reconcile::Reconcile;

pub fn run_file(file: &str, dry_run: bool, quiet: bool) -> Result<(), EstoError> {
    fn setup(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
        builtins::register_internal(ctx)?;
        registry::register_builtins(ctx)
    }
    let stats = optative_script::run_script(
        file,
        registry::ES_BUILTINS,
        setup,
        dry_run,
        quiet,
    ).map_err(|e| EstoError::WorkerError(e.to_string()))?;

    let exit_code = if dry_run { stats.enter + stats.update + stats.exit } else { stats.errors };
    if exit_code != 0 {
        std::process::exit(exit_code as i32);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum EstoError {
    #[error("command failed ({cmd}): {detail}")]
    CommandFailed { cmd: String, detail: String },
    #[error("worker error: {0}")]
    WorkerError(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("watch error: {0}")]
    Watch(String),
    #[error("worker stdout channel closed unexpectedly")]
    WorkerChannelClosed,
    #[error("worker repeatedly requested shutdown")]
    WorkerShutdownLoop,
}

struct WorkerHandle {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout_rx: mpsc::Receiver<String>,
}

pub struct WorkerPool {
    cmd: String,
    handle: Option<WorkerHandle>,
    stateful: bool,
}

impl WorkerPool {
    pub fn new(cmd: String, stateful: bool) -> Self {
        Self { cmd, handle: None, stateful }
    }

    fn spawn(&mut self) -> Result<(), EstoError> {
        let mut child = std::process::Command::new("sh")
            .args(["-c", &self.cmd])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take().unwrap();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => { if tx.send(l).is_err() { break; } }
                    Err(_) => break,
                }
            }
        });

        self.handle = Some(WorkerHandle { child, stdin, stdout_rx: rx });
        Ok(())
    }

    fn kill_and_clear(&mut self) {
        if let Some(h) = self.handle.as_mut() {
            let _ = h.child.kill();
            let _ = h.child.wait();
        }
        self.handle = None;
    }

    pub fn dispatch(&mut self, task_line: String) -> Result<(), EstoError> {
        if !self.stateful {
            return self.dispatch_simple(&task_line);
        }
        if self.handle.is_none() {
            self.spawn()?;
        }
        self.dispatch_with_retry(task_line, MAX_WORKER_RETRIES)
    }

    // Per-item invocation: sh -c "$cmd" _ key [value [old new]]
    // $1=key, $2=value-or-old, $3=new (for update). Exit 0 = success.
    fn dispatch_simple(&self, task_line: &str) -> Result<(), EstoError> {
        let task_args: Vec<&str> = task_line.splitn(3, '\t').collect();
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&self.cmd)
            .arg("_")
            .args(&task_args)
            .stderr(Stdio::inherit())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(EstoError::WorkerError(format!("exited with {status}")))
        }
    }

    fn dispatch_with_retry(&mut self, task_line: String, retries_left: u8) -> Result<(), EstoError> {
        let key = task_line.splitn(2, '\t').next().unwrap_or("").to_string();
        let h = self.handle.as_mut().unwrap();

        h.stdin.write_all(format!("{task_line}\n").as_bytes()).map_err(EstoError::Io)?;
        h.stdin.flush().map_err(EstoError::Io)?;

        loop {
            let line = h.stdout_rx.recv().map_err(|_| {
                EstoError::WorkerChannelClosed
            })?;

            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            match parts.as_slice() {
                ["done", k] if *k == key => return Ok(()),
                ["error", k, msg] if *k == key => {
                    return Err(EstoError::WorkerError((*msg).to_string()))
                }
                ["shutdown"] => {
                    if retries_left == 0 {
                        return Err(EstoError::WorkerShutdownLoop);
                    }
                    self.kill_and_clear();
                    self.spawn()?;
                    return self.dispatch_with_retry(task_line, retries_left - 1);
                }
                _ => {}
            }
        }
    }

    pub fn shutdown(&mut self) {
        self.kill_and_clear();
    }
}

pub struct WorkerPools {
    pub enter: Option<WorkerPool>,
    pub exit: Option<WorkerPool>,
    pub update: Option<WorkerPool>,
    pub quiet: bool,
    pub dry_run: bool,
    pub enter_count: u64,
    pub exit_count: u64,
    pub update_count: u64,
}

impl WorkerPools {
    pub fn shutdown(&mut self) {
        if let Some(p) = self.enter.as_mut() { p.shutdown(); }
        if let Some(p) = self.exit.as_mut() { p.shutdown(); }
        if let Some(p) = self.update.as_mut() { p.shutdown(); }
    }
}

// key kept in state so exit can dispatch with it
struct HookState { key: String, value: String }

struct HookItem {
    key: String,
    value: String,
}

impl Lifecycle for HookItem {
    type Key = String;
    type State = HookState;
    type Context = WorkerPools;
    type Output = ();
    type Error = EstoError;

    fn key(&self) -> String { self.key.clone() }

    fn enter(self, ctx: &mut WorkerPools, _: &mut ()) -> Result<HookState, EstoError> {
        if !ctx.quiet {
            eprintln!("[enter] {}", self.key);
        }
        ctx.enter_count += 1;
        if !ctx.dry_run {
            if let Some(pool) = ctx.enter.as_mut() {
                pool.dispatch(format!("{}\t{}", self.key, self.value))?;
            }
        }
        Ok(HookState { key: self.key, value: self.value })
    }

    fn reconcile_self(self, state: &mut HookState, ctx: &mut WorkerPools, _: &mut ()) -> Result<(), EstoError> {
        if state.value != self.value {
            if !ctx.quiet {
                eprintln!("[update] {} {:?} -> {:?}", self.key, state.value, self.value);
            }
            ctx.update_count += 1;
            if !ctx.dry_run {
                if let Some(pool) = ctx.update.as_mut() {
                    pool.dispatch(format!("{}\t{}\t{}", self.key, state.value, self.value))?;
                }
            }
            state.value = self.value;
        }
        Ok(())
    }

    fn exit(state: HookState, ctx: &mut WorkerPools, _: &mut ()) -> Result<(), EstoError> {
        if !ctx.quiet {
            eprintln!("[exit] {}", state.key);
        }
        ctx.exit_count += 1;
        if !ctx.dry_run {
            if let Some(pool) = ctx.exit.as_mut() {
                pool.dispatch(format!("{}\t{}", state.key, state.value))?;
            }
        }
        Ok(())
    }
}

fn parse_tsv_lines(text: &str) -> Vec<HookItem> {
    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        match line.splitn(2, '\t').collect::<Vec<_>>().as_slice() {
            [key, value] => items.push(HookItem { key: (*key).to_string(), value: (*value).to_string() }),
            [key] => items.push(HookItem { key: (*key).to_string(), value: String::new() }),
            _ => {}
        }
    }
    items
}

fn run_command_for_pairs(cmd: &str) -> Result<Vec<HookItem>, EstoError> {
    let output = std::process::Command::new("sh")
        .args(["-c", cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        return Err(EstoError::CommandFailed {
            cmd: cmd.to_string(),
            detail: format!("exited with {}", output.status),
        });
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_tsv_lines(&text))
}

pub struct ReconcileConfig {
    pub from: Option<String>,
    pub to: String,
    pub enter: Option<String>,
    pub exit: Option<String>,
    pub update: Option<String>,
    pub rate_limit: Option<Duration>,
    pub reingest_every: Option<u64>,
    /// Run one reconcile cycle then exit. --from (if given) seeds current state.
    pub once: bool,
    /// Keep a single long-lived worker process per hook type (stdin/stdout protocol).
    /// Default (false): spawn a fresh process per item; exit code 0 = success.
    pub stateful: bool,
    /// Suppress per-event stderr log lines.
    pub quiet: bool,
    /// Compute the diff and print it without dispatching any workers.
    pub dry_run: bool,
    /// Exit nonzero if any delta (enter/update/exit) occurred. For CI: verify system is already in desired state.
    pub fail_on_change: bool,
}

fn make_pools(config: &ReconcileConfig) -> WorkerPools {
    WorkerPools {
        enter: config.enter.as_ref().map(|c| WorkerPool::new(c.clone(), config.stateful)),
        exit: config.exit.as_ref().map(|c| WorkerPool::new(c.clone(), config.stateful)),
        update: config.update.as_ref().map(|c| WorkerPool::new(c.clone(), config.stateful)),
        quiet: config.quiet,
        dry_run: config.dry_run,
        enter_count: 0,
        exit_count: 0,
        update_count: 0,
    }
}

fn seed_from(cmd: &str) -> Result<Vec<(String, HookState)>, EstoError> {
    let from_items = run_command_for_pairs(cmd)?;
    Ok(from_items.into_iter().map(|item| {
        let key = item.key.clone();
        (key.clone(), HookState { key, value: item.value })
    }).collect())
}

fn reconcile_step(set: &mut OptativeSet<HookItem>, config: &ReconcileConfig, pools: &mut WorkerPools) -> Result<(), EstoError> {
    let to_items = run_command_for_pairs(&config.to)?;
    let errors = set.reconcile(to_items, pools, &mut ());
    for (key, err) in errors {
        tracing::error!(key = %key, error = %err, "lifecycle error");
    }
    Ok(())
}

pub fn run(config: ReconcileConfig) -> Result<(), EstoError> {
    let mut pools = make_pools(&config);

    if config.once {
        let initial = match &config.from {
            Some(cmd) => seed_from(cmd)?,
            None => vec![],
        };
        let mut set: OptativeSet<HookItem> = OptativeSet::with_initial_state(initial);
        reconcile_step(&mut set, &config, &mut pools)?;
        let unchanged = (set.iter().count() as u64)
            .saturating_sub(pools.enter_count)
            .saturating_sub(pools.update_count);
        if !pools.quiet {
            eprintln!(
                "reconciled: {} enter, {} update, {} exit ({} unchanged)",
                pools.enter_count, pools.update_count, pools.exit_count, unchanged
            );
        }
        pools.shutdown();
        let delta = pools.enter_count + pools.update_count + pools.exit_count;
        if config.fail_on_change && delta > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut set: OptativeSet<HookItem> = match &config.from {
        Some(cmd) => OptativeSet::with_initial_state(seed_from(cmd)?),
        None => OptativeSet::new(),
    };
    let mut step: u64 = 0;

    loop {
        reconcile_step(&mut set, &config, &mut pools)?;

        step += 1;
        if let Some(n) = config.reingest_every {
            if step % n == 0 {
                if let Some(cmd) = &config.from {
                    set = OptativeSet::with_initial_state(seed_from(cmd)?);
                }
            }
        }

        if let Some(rate_limit) = config.rate_limit {
            thread::sleep(rate_limit);
        }
    }
}

#[cfg(test)]
mod tsv_parsing {
    use super::{parse_tsv_lines};

    #[test]
    fn tsv_key_value_pair() {
        let items = parse_tsv_lines("foo\tbar");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "foo");
        assert_eq!(items[0].value, "bar");
    }

    #[test]
    fn tsv_key_only_gives_empty_value() {
        let items = parse_tsv_lines("foo");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "foo");
        assert_eq!(items[0].value, "");
    }

    #[test]
    fn tsv_skips_blank_lines_and_comments() {
        let items = parse_tsv_lines("\n# comment\n\nfoo\tbar\n");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "foo");
    }

    #[test]
    fn tsv_parses_multiple_lines() {
        let items = parse_tsv_lines("a\tv1\nb\tv2\n");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "a");
        assert_eq!(items[0].value, "v1");
        assert_eq!(items[1].key, "b");
        assert_eq!(items[1].value, "v2");
    }

    #[test]
    fn tsv_trims_leading_trailing_whitespace_on_line() {
        let items = parse_tsv_lines("  foo\tbar  ");
        // The line is trimmed before splitting, so key = "foo", value = "bar  "
        assert_eq!(items[0].key, "foo");
    }
}

#[cfg(test)]
mod reconcile {
    use super::{HookItem, HookState, WorkerPools};
    use optative::OptativeSet;
    use optative::reconcile::Reconcile;

    fn dry_pools() -> WorkerPools {
        WorkerPools {
            enter: None, exit: None, update: None,
            quiet: true, dry_run: true,
            enter_count: 0, exit_count: 0, update_count: 0,
        }
    }

    #[test]
    fn reconcile_new_item_increments_enter() {
        let mut set: OptativeSet<HookItem> = OptativeSet::new();
        let mut pools = dry_pools();
        let errors = set.reconcile(vec![HookItem { key: "k".into(), value: "v".into() }], &mut pools, &mut ());
        assert!(errors.is_empty());
        assert_eq!(pools.enter_count, 1);
        assert_eq!(pools.exit_count, 0);
        assert_eq!(pools.update_count, 0);
    }

    #[test]
    fn reconcile_removed_item_increments_exit() {
        let initial = vec![(String::from("k"), HookState { key: String::from("k"), value: String::from("v") })];
        let mut set: OptativeSet<HookItem> = OptativeSet::with_initial_state(initial);
        let mut pools = dry_pools();
        let errors = set.reconcile(vec![], &mut pools, &mut ());
        assert!(errors.is_empty());
        assert_eq!(pools.exit_count, 1);
        assert_eq!(pools.enter_count, 0);
        assert_eq!(pools.update_count, 0);
    }

    #[test]
    fn reconcile_changed_value_increments_update() {
        let initial = vec![(String::from("k"), HookState { key: String::from("k"), value: String::from("v1") })];
        let mut set: OptativeSet<HookItem> = OptativeSet::with_initial_state(initial);
        let mut pools = dry_pools();
        let errors = set.reconcile(vec![HookItem { key: "k".into(), value: "v2".into() }], &mut pools, &mut ());
        assert!(errors.is_empty());
        assert_eq!(pools.update_count, 1);
        assert_eq!(pools.enter_count, 0);
        assert_eq!(pools.exit_count, 0);
    }

    #[test]
    fn reconcile_unchanged_value_no_update() {
        let initial = vec![(String::from("k"), HookState { key: String::from("k"), value: String::from("v") })];
        let mut set: OptativeSet<HookItem> = OptativeSet::with_initial_state(initial);
        let mut pools = dry_pools();
        let errors = set.reconcile(vec![HookItem { key: "k".into(), value: "v".into() }], &mut pools, &mut ());
        assert!(errors.is_empty());
        assert_eq!(pools.update_count, 0);
        assert_eq!(pools.enter_count, 0);
        assert_eq!(pools.exit_count, 0);
    }
}
