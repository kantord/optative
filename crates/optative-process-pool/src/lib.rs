mod process;

pub use process::{ProcessIdentity, ProcessSource, ProcessState, SpawnError};

use std::sync::mpsc;

use optative::reconcile::ReconcileErrors;
use optative::{OptativeSet, Reconcile};

#[derive(Debug, PartialEq, Eq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct StreamItem {
    pub key: (String, Option<String>),
    pub stream: StreamKind,
    pub line: String,
}

pub struct ProcessPool {
    inner: OptativeSet<ProcessSource>,
    stream_tx: mpsc::Sender<StreamItem>,
}

impl ProcessPool {
    pub fn new(stream_tx: mpsc::Sender<StreamItem>) -> Self {
        Self {
            inner: OptativeSet::new(),
            stream_tx,
        }
    }
    pub fn reconcile(
        &mut self,
        desired: Vec<ProcessSource>,
    ) -> ReconcileErrors<ProcessIdentity, SpawnError> {
        self.inner.reconcile(desired, &mut (), &mut self.stream_tx)
    }
    pub fn get(&self, identity: &ProcessIdentity) -> Option<&ProcessState> {
        self.inner.get(identity)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ProcessIdentity, &ProcessState)> {
        self.inner.iter()
    }
}

impl Drop for ProcessPool {
    fn drop(&mut self) {
        self.inner
            .reconcile(Vec::new(), &mut (), &mut self.stream_tx);
    }
}
