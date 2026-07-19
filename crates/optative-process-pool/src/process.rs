use std::collections::BTreeMap;
use std::io::{BufRead, Write as IoWrite};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use optative::Lifecycle;

use super::{StreamItem, StreamKind};

/// How long [`Lifecycle::exit`] waits for SIGTERM before escalating to SIGKILL.
pub const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(10);

/// Stable identity for a process: uniquely identifies which process to manage.
/// Used as the key in `Lifecycle` so that `OptativeSet` can track processes by identity.
#[derive(Hash, Eq, PartialEq, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProcessIdentity {
    pub bin: String,
    pub key: String,
}

// NOTE: env uses BTreeMap (not HashMap) for deterministic ordering; HashMap doesn't implement Hash.
#[derive(Clone, Debug)]
pub struct ProcessSource {
    pub identity: ProcessIdentity,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub current_dir: Option<PathBuf>,
    pub props: Option<serde_json::Value>,
}

pub struct ProcessState {
    pub child: std::process::Child,
    pub event_tx: mpsc::Sender<serde_json::Value>,
    pub last_sent_props: Option<serde_json::Value>,
}

/// Error type for process spawning failures.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("failed to spawn {bin}: {source}")]
    ProcessSpawnFailed {
        bin: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to resolve resource: {source}")]
    ResourceResolutionFailed {
        #[source]
        source: std::io::Error,
    },
}

fn spawn_stdout_thread(
    stdout: std::process::ChildStdout,
    identity: ProcessIdentity,
    tx: mpsc::Sender<StreamItem>,
) {
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    let item = StreamItem {
                        key: identity.clone(),
                        stream: StreamKind::Stdout,
                        line: l,
                    };
                    if tx.send(item).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn spawn_stderr_thread(stderr: std::process::ChildStderr, bin_name: String) {
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(l) => tracing::warn!(module = %bin_name, "{l}"),
                Err(_) => break,
            }
        }
    });
}

fn spawn_stdin_thread(
    mut stdin: std::process::ChildStdin,
    event_rx: mpsc::Receiver<serde_json::Value>,
) {
    thread::spawn(move || {
        while let Ok(event) = event_rx.recv() {
            let line = serde_json::to_string(&event).unwrap_or_default() + "\n";
            if stdin.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
    });
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}{}", home, &path[1..])
    } else if path == "~" {
        std::env::var("HOME").unwrap_or_default()
    } else {
        path.to_string()
    }
}

pub(super) fn spawn_process(
    spec: ProcessSource,
    tx: &mpsc::Sender<StreamItem>,
) -> Result<ProcessState, SpawnError> {
    let bin = expand_tilde(&spec.identity.bin);
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(&spec.args);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(ref dir) = spec.current_dir {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::piped());

    // Each child leads its own process group (pgid == its pid) so exit() can
    // signal the whole group, reaching grandchildren the child doesn't forward
    // signals to. Trade-off: terminal-generated signals (Ctrl-C) no longer
    // reach children; teardown is exclusively exit()-driven.
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(SpawnError::ProcessSpawnFailed { bin, source: e });
        }
    };

    if let Some(stdout) = child.stdout.take() {
        spawn_stdout_thread(stdout, spec.identity.clone(), tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_stderr_thread(stderr, spec.identity.bin.clone());
    }
    let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>();
    if let Some(stdin) = child.stdin.take() {
        spawn_stdin_thread(stdin, event_rx);
    }

    Ok(ProcessState {
        child,
        event_tx,
        last_sent_props: None,
    })
}

impl std::fmt::Display for ProcessSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.identity.bin)
    }
}

impl Lifecycle for ProcessSource {
    type Key = ProcessIdentity;
    type State = ProcessState;
    type Context = ();
    type Output = mpsc::Sender<StreamItem>;
    type Error = SpawnError;

    fn key(&self) -> ProcessIdentity {
        self.identity.clone()
    }

    fn enter(self, _ctx: &mut (), output: &mut Self::Output) -> Result<Self::State, Self::Error> {
        let props = self.props.clone();
        let mut state = spawn_process(self, output)?;
        if let Some(p) = props {
            let _ = state.event_tx.send(p.clone());
            state.last_sent_props = Some(p);
        }
        Ok(state)
    }

    #[allow(clippy::collapsible_if)]
    fn reconcile_self(
        self,
        state: &mut Self::State,
        _ctx: &mut (),
        output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        if matches!(state.child.try_wait(), Ok(Some(_))) {
            tracing::warn!(bin = %self.identity.bin, "process exited");
            let props = self.props.clone();
            let mut new_state = spawn_process(self, output)?;
            if let Some(p) = props {
                let _ = new_state.event_tx.send(p.clone());
                new_state.last_sent_props = Some(p);
            }
            *state = new_state;
        } else if let Some(p) = self.props {
            if state.last_sent_props.as_ref() != Some(&p) {
                let _ = state.event_tx.send(p.clone());
                state.last_sent_props = Some(p);
            }
        }
        Ok(())
    }

    fn exit(
        mut state: Self::State,
        _ctx: &mut (),
        _output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        // The child is its own group leader (spawn sets process_group(0)), so
        // its pid doubles as the pgid; signaling the group reaches grandchildren
        // too. Valid until the child is reaped, and we signal before reaping.
        // ESRCH if the group is already gone is fine; the poll loop reaps it.
        let pgid = nix::unistd::Pid::from_raw(state.child.id() as i32);
        let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM);

        let deadline = Instant::now() + SHUTDOWN_GRACE_PERIOD;
        while Instant::now() < deadline {
            match state.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }

        let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
        let _ = state.child.wait();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessIdentity, ProcessSource};
    use optative::Lifecycle;
    use std::collections::BTreeMap;

    fn make_source(bin: &str) -> ProcessSource {
        ProcessSource {
            identity: ProcessIdentity {
                bin: bin.to_string(),
                key: bin.to_string(),
            },
            args: vec![],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        }
    }

    #[test]
    fn process_identity_has_bin_and_key_fields() {
        let id = ProcessIdentity {
            bin: "mybin".to_string(),
            key: "mykey".to_string(),
        };
        assert_eq!(id.bin, "mybin");
        assert_eq!(id.key, "mykey");
    }

    #[test]
    fn process_identity_derives_hash_eq_partialeq_clone() {
        use std::collections::HashSet;
        let a = ProcessIdentity {
            bin: "bin".to_string(),
            key: "k".to_string(),
        };
        let b = a.clone();
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(!set.insert(b));
    }

    #[test]
    fn process_source_has_identity_fields() {
        let spec = ProcessSource {
            identity: ProcessIdentity {
                bin: "/bin/sh".to_string(),
                key: "my-key".to_string(),
            },
            args: vec!["--flag".to_string()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };
        assert_eq!(spec.identity.bin, "/bin/sh");
        assert_eq!(spec.identity.key, "my-key");
    }

    #[test]
    fn lifecycle_key_returns_identity() {
        let id = ProcessIdentity {
            bin: "/usr/bin/cat".to_string(),
            key: "cat-key".to_string(),
        };
        let returned: ProcessIdentity = make_source("/usr/bin/cat").key();
        assert_eq!(returned.bin, id.bin);
    }

    mod spawn_process {
        use super::super::{SpawnError, spawn_process};
        use std::sync::mpsc;

        #[test]
        fn nonexistent_binary_returns_process_spawn_failed() {
            let (tx, _rx) = mpsc::channel();
            let result = spawn_process(
                super::make_source("/nonexistent/binary/that/cannot/exist"),
                &tx,
            );
            match result {
                Err(SpawnError::ProcessSpawnFailed { bin, .. }) => {
                    assert_eq!(bin, "/nonexistent/binary/that/cannot/exist");
                }
                _ => panic!("expected ProcessSpawnFailed"),
            }
        }

        #[test]
        fn spawned_child_leads_its_own_process_group() {
            let (tx, _rx) = mpsc::channel();
            let mut spec = super::make_source("/bin/sleep");
            spec.args = vec!["60".to_string()];
            let mut state = spawn_process(spec, &tx).expect("spawn must succeed");

            let pid = nix::unistd::Pid::from_raw(state.child.id() as i32);
            let pgid = nix::unistd::getpgid(Some(pid));

            let _ = state.child.kill();
            let _ = state.child.wait();

            assert_eq!(
                pgid.expect("getpgid must succeed"),
                pid,
                "child must be the leader of its own process group"
            );
        }

        #[test]
        fn tilde_bin_is_expanded_to_home_dir() {
            let home = std::env::var("HOME").expect("HOME must be set");
            let (tx, _rx) = mpsc::channel();
            let result = spawn_process(super::make_source("~/nonexistent-tilde-test-binary"), &tx);
            match result {
                Err(SpawnError::ProcessSpawnFailed { bin, .. }) => {
                    assert!(
                        !bin.starts_with('~'),
                        "bin must not contain literal ~; got: {bin}"
                    );
                    assert!(
                        bin.starts_with(&home),
                        "bin must start with HOME ({home}); got: {bin}"
                    );
                }
                _ => panic!("expected ProcessSpawnFailed"),
            }
        }
    }

    mod lifecycle {
        use super::super::{ProcessIdentity, ProcessSource, SHUTDOWN_GRACE_PERIOD, SpawnError};
        use optative::Lifecycle;
        use std::collections::BTreeMap;
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        #[test]
        fn reconcile_self_propagates_err_when_restart_spawn_fails() {
            let (mut tx, _rx) = mpsc::channel();

            let mut state = ProcessSource {
                identity: ProcessIdentity {
                    bin: "/bin/sh".to_string(),
                    key: "t".to_string(),
                },
                args: vec!["-c".to_string(), "exit 0".to_string()],
                env: BTreeMap::new(),
                current_dir: None,
                props: None,
            }
            .enter(&mut (), &mut tx)
            .expect("enter must succeed with /bin/sh");

            std::thread::sleep(Duration::from_millis(200));
            assert!(
                matches!(state.child.try_wait(), Ok(Some(_))),
                "child should have exited"
            );

            let result = super::make_source("/nonexistent/binary/that/cannot/exist")
                .reconcile_self(&mut state, &mut (), &mut tx);
            match result {
                Err(SpawnError::ProcessSpawnFailed { .. }) => {}
                _ => panic!("expected ProcessSpawnFailed"),
            }
        }

        fn trap_spec(key: &str, trap_body: &str) -> ProcessSource {
            ProcessSource {
                identity: ProcessIdentity {
                    bin: "/bin/sh".to_string(),
                    key: key.to_string(),
                },
                // `wait` is interruptible by signals; foreground `sleep` is not.
                args: vec![
                    "-c".to_string(),
                    format!("trap '{trap_body}' TERM; sleep 60 & wait"),
                ],
                env: BTreeMap::new(),
                current_dir: None,
                props: None,
            }
        }

        #[test]
        fn exit_reaps_graceful_child_via_sigterm() {
            let (mut tx, _rx) = mpsc::channel();
            let state = trap_spec("graceful", "exit 0")
                .enter(&mut (), &mut tx)
                .expect("enter must succeed");
            std::thread::sleep(Duration::from_millis(150));

            let start = Instant::now();
            ProcessSource::exit(state, &mut (), &mut tx).expect("exit must succeed");
            assert!(start.elapsed() < Duration::from_secs(2));
        }

        #[test]
        fn exit_kills_grandchildren_spawned_by_the_child() {
            let (mut tx, _rx) = mpsc::channel();
            let pidfile = tempfile::NamedTempFile::new().expect("tempfile must be created");
            let path = pidfile.path().to_str().expect("utf-8 path").to_string();

            // The shell backgrounds a grandchild, records its pid, then execs
            // into a foreground sleep — so nothing forwards signals to the
            // grandchild.
            let state = ProcessSource {
                identity: ProcessIdentity {
                    bin: "/bin/sh".to_string(),
                    key: "grandchild".to_string(),
                },
                args: vec![
                    "-c".to_string(),
                    format!("sleep 60 & echo $! > {path}; exec sleep 60"),
                ],
                env: BTreeMap::new(),
                current_dir: None,
                props: None,
            }
            .enter(&mut (), &mut tx)
            .expect("enter must succeed");

            let deadline = Instant::now() + Duration::from_secs(2);
            let grandchild_pid = loop {
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                if let Ok(pid) = contents.trim().parse::<i32>() {
                    break nix::unistd::Pid::from_raw(pid);
                }
                assert!(Instant::now() < deadline, "grandchild pid never written");
                std::thread::sleep(Duration::from_millis(20));
            };

            ProcessSource::exit(state, &mut (), &mut tx).expect("exit must succeed");

            // Signal 0 probes existence; ESRCH means the grandchild is gone.
            let deadline = Instant::now() + Duration::from_secs(2);
            while nix::sys::signal::kill(grandchild_pid, None) != Err(nix::errno::Errno::ESRCH) {
                assert!(
                    Instant::now() < deadline,
                    "grandchild survived exit() as an orphan"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        #[test]
        #[ignore = "slow"]
        fn exit_escalates_to_sigkill_when_child_ignores_sigterm() {
            let (mut tx, _rx) = mpsc::channel();
            let state = trap_spec("stubborn", "")
                .enter(&mut (), &mut tx)
                .expect("enter must succeed");
            std::thread::sleep(Duration::from_millis(150));

            let start = Instant::now();
            ProcessSource::exit(state, &mut (), &mut tx).expect("exit must succeed");
            assert!(start.elapsed() >= SHUTDOWN_GRACE_PERIOD);
        }
    }
}
