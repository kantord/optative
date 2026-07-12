use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub enum WatchTrigger {
    FsPath(PathBuf),
    GitCommit,
}

pub fn watch_file(
    file: &str,
    triggers: Vec<WatchTrigger>,
    interval: Option<Duration>,
    dry_run: bool,
    quiet: bool,
) -> Result<(), crate::EstoError> {
    let (tx, rx) = mpsc::channel::<()>();

    let tx_watcher = tx.clone();
    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx_watcher.send(());
            }
        })
        .map_err(|e| crate::EstoError::Watch(e.to_string()))?;

    for trigger in &triggers {
        match trigger {
            WatchTrigger::FsPath(path) => {
                watcher
                    .watch(path, RecursiveMode::Recursive)
                    .map_err(|e| crate::EstoError::Watch(e.to_string()))?;
            }
            WatchTrigger::GitCommit => {
                // Watch .git/refs/heads — updated on every local commit.
                let p = PathBuf::from(".git/refs/heads");
                if p.exists() {
                    watcher
                        .watch(&p, RecursiveMode::Recursive)
                        .map_err(|e| crate::EstoError::Watch(e.to_string()))?;
                }
            }
        }
    }

    if !quiet {
        eprintln!("[watch] starting — emit-only (no auto-dispatch)");
    }

    // Initial run
    let _ = crate::run_file(file, dry_run, quiet);

    loop {
        let fired = if let Some(dur) = interval {
            // Wait for event or timeout
            match rx.recv_timeout(dur) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => true,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            // Wait indefinitely for an event (requires at least one trigger)
            match rx.recv() {
                Ok(()) => true,
                Err(_) => break,
            }
        };

        if fired {
            // Drain any queued events (debounce burst)
            while rx.try_recv().is_ok() {}
            if !quiet {
                eprintln!("[watch] re-reconciling");
            }
            let _ = crate::run_file(file, dry_run, quiet);
        }
    }

    Ok(())
}
