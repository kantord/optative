use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc;

use optative::reconcile::ReconcileErrors;
use tempfile::NamedTempFile;

use crate::process::{ProcessIdentity, ProcessSource, ProcessState, SpawnError};
use crate::resource::Resource;
use crate::{ProcessPool, StreamItem};

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessSpec {
    pub identity: ProcessIdentity,
    pub args: Vec<Resource>,
    pub env: BTreeMap<String, Resource>,
    pub current_dir: Option<PathBuf>,
    pub props: Option<serde_json::Value>,
}

struct CachedResolution {
    spec: ProcessSpec,
    source: ProcessSource,
    _handles: Vec<NamedTempFile>,
}

fn resolve_spec(spec: &ProcessSpec) -> Result<(ProcessSource, Vec<NamedTempFile>), std::io::Error> {
    let mut args = Vec::new();
    let mut handles = Vec::new();

    for resource in &spec.args {
        let resolved = resource.resolve()?;
        args.push(resolved.value);
        if let Some(h) = resolved.handle {
            handles.push(h);
        }
    }

    let mut env = BTreeMap::new();
    for (key, resource) in &spec.env {
        let resolved = resource.resolve()?;
        env.insert(key.clone(), resolved.value);
        if let Some(h) = resolved.handle {
            handles.push(h);
        }
    }

    let source = ProcessSource {
        identity: spec.identity.clone(),
        args,
        env,
        current_dir: spec.current_dir.clone(),
        props: spec.props.clone(),
    };

    Ok((source, handles))
}

pub struct ProcessSupervisor {
    pool: ProcessPool,
    states: HashMap<ProcessIdentity, CachedResolution>,
}

impl ProcessSupervisor {
    pub fn new(stream_tx: mpsc::Sender<StreamItem>) -> Self {
        Self {
            pool: ProcessPool::new(stream_tx),
            states: HashMap::new(),
        }
    }

    pub fn reconcile(
        &mut self,
        desired: Vec<ProcessSpec>,
    ) -> ReconcileErrors<ProcessIdentity, SpawnError> {
        let mut resolved = Vec::new();
        let mut new_states: HashMap<ProcessIdentity, CachedResolution> = HashMap::new();
        let mut errors = ReconcileErrors::new();
        let mut needs_restart = Vec::new();

        for spec in desired {
            let identity = spec.identity.clone();

            if let Some(cached) = self.states.remove(&identity) {
                if cached.spec == spec {
                    resolved.push(cached.source.clone());
                    new_states.insert(identity, cached);
                    continue;
                }
                needs_restart.push(identity.clone());
            }

            match resolve_spec(&spec) {
                Ok((source, handles)) => {
                    resolved.push(source.clone());
                    new_states.insert(
                        identity,
                        CachedResolution {
                            spec,
                            source,
                            _handles: handles,
                        },
                    );
                }
                Err(e) => {
                    errors.push((identity, SpawnError::ResourceResolutionFailed { source: e }));
                }
            }
        }

        // ProcessSource::reconcile_self doesn't detect arg changes, so changed
        // specs need an explicit exit-then-enter cycle. First pass excludes
        // them (pool exits the old process), second pass includes them (pool
        // enters the new one).
        if !needs_restart.is_empty() {
            let without: Vec<ProcessSource> = resolved
                .iter()
                .filter(|s| !needs_restart.contains(&s.identity))
                .cloned()
                .collect();
            self.pool.reconcile(without);
        }

        let pool_errors = self.pool.reconcile(resolved);
        errors.extend(pool_errors);

        self.states = new_states;
        errors
    }

    pub fn get(&self, identity: &ProcessIdentity) -> Option<&ProcessState> {
        self.pool.get(identity)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ProcessIdentity, &ProcessState)> {
        self.pool.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn wait_for_line(rx: &mpsc::Receiver<StreamItem>, timeout: Duration) -> Option<StreamItem> {
        rx.recv_timeout(timeout).ok()
    }

    #[test]
    fn string_args_work_identically_to_process_pool() {
        let (tx, rx) = mpsc::channel();
        let mut supervisor = ProcessSupervisor::new(tx);

        let errors = supervisor.reconcile(vec![ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".into(),
                key: "echo".into(),
            },
            args: vec!["-c".into(), "echo hello_from_supervisor".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        }]);
        assert!(errors.is_empty());

        let item = wait_for_line(&rx, Duration::from_secs(2)).expect("should receive stdout");
        assert_eq!(item.line, "hello_from_supervisor");
    }

    #[test]
    fn file_resource_is_passed_as_executable_script() {
        let (tx, rx) = mpsc::channel();
        let mut supervisor = ProcessSupervisor::new(tx);

        let errors = supervisor.reconcile(vec![ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".into(),
                key: "file-test".into(),
            },
            args: vec![Resource::File {
                content: "echo from_file_resource".into(),
            }],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        }]);
        assert!(errors.is_empty());

        let item = wait_for_line(&rx, Duration::from_secs(2)).expect("should receive stdout");
        assert_eq!(item.line, "from_file_resource");
    }

    #[test]
    fn unchanged_spec_does_not_restart_process() {
        let (tx, rx) = mpsc::channel();
        let mut supervisor = ProcessSupervisor::new(tx);

        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".into(),
                key: "stable".into(),
            },
            args: vec!["-c".into(), "echo started; sleep 60".into()],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        supervisor.reconcile(vec![spec.clone()]);
        let item = wait_for_line(&rx, Duration::from_secs(2)).expect("first start");
        assert_eq!(item.line, "started");

        supervisor.reconcile(vec![spec]);
        // If the process restarted, we'd see another "started" line.
        let next = rx.recv_timeout(Duration::from_millis(300));
        assert!(
            next.is_err(),
            "process should not have restarted for unchanged spec"
        );
    }

    #[test]
    fn changed_file_content_restarts_process() {
        let (tx, rx) = mpsc::channel();
        let mut supervisor = ProcessSupervisor::new(tx);

        let mk = |content: &str| ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".into(),
                key: "versioned".into(),
            },
            args: vec![Resource::File {
                content: content.into(),
            }],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        supervisor.reconcile(vec![mk("echo v1")]);
        let item = wait_for_line(&rx, Duration::from_secs(2)).expect("v1");
        assert_eq!(item.line, "v1");

        supervisor.reconcile(vec![mk("echo v2")]);
        let item = wait_for_line(&rx, Duration::from_secs(2)).expect("v2");
        assert_eq!(item.line, "v2");
    }

    #[test]
    fn removing_process_cleans_up_file_resources() {
        let (tx, _rx) = mpsc::channel();
        let mut supervisor = ProcessSupervisor::new(tx);

        let spec = ProcessSpec {
            identity: ProcessIdentity {
                bin: "/bin/sh".into(),
                key: "cleanup".into(),
            },
            args: vec![Resource::File {
                content: "sleep 60".into(),
            }],
            env: BTreeMap::new(),
            current_dir: None,
            props: None,
        };

        supervisor.reconcile(vec![spec]);

        let file_paths: Vec<String> = supervisor
            .states
            .values()
            .flat_map(|c| c._handles.iter())
            .map(|h| h.path().to_string_lossy().into_owned())
            .collect();
        assert!(!file_paths.is_empty(), "should have at least one temp file");
        for p in &file_paths {
            assert!(
                std::path::Path::new(p).exists(),
                "file should exist while process is running"
            );
        }

        supervisor.reconcile(vec![]);

        for p in &file_paths {
            assert!(
                !std::path::Path::new(p).exists(),
                "file should be cleaned up after process exits"
            );
        }
    }
}
