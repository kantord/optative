#![forbid(unsafe_code)]

#[cfg(not(unix))]
compile_error!("optative-process-pool currently only supports Unix targets");

mod process;
mod resource;
mod supervisor;

pub use process::{
    ProcessIdentity, ProcessSource, ProcessState, SHUTDOWN_GRACE_PERIOD, SpawnError,
};
pub use resource::Resource;
pub use supervisor::{ProcessSpec, ProcessSupervisor};

use std::sync::mpsc;

use optative::reconcile::ReconcileErrors;
use optative::{OptativeSet, Reconcile};
use optative_derive::Ephemeral;

#[derive(Debug, PartialEq, Eq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct StreamItem {
    pub key: ProcessIdentity,
    pub stream: StreamKind,
    pub line: String,
}

#[derive(Ephemeral)]
pub struct ProcessPool {
    #[reconciler(output = stream_tx)]
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
