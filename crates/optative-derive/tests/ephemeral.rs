use optative::{Lifecycle, OptativeSet, Reconcile};
use optative_derive::Ephemeral;
use std::sync::mpsc;

/// A lifecycle whose `exit` sends a message on the `Output` channel, so a test
/// can observe that exit ran.
struct Tracked {
    id: String,
}

impl std::fmt::Display for Tracked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl Lifecycle for Tracked {
    type Key = String;
    type State = String;
    type Context = ();
    type Output = mpsc::Sender<String>;
    type Error = std::convert::Infallible;

    fn key(&self) -> String {
        self.id.clone()
    }

    fn enter(self, _ctx: &mut (), _output: &mut Self::Output) -> Result<String, Self::Error> {
        Ok(self.id)
    }

    fn reconcile_self(
        self,
        _state: &mut String,
        _ctx: &mut (),
        _output: &mut Self::Output,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn exit(state: String, _ctx: &mut (), output: &mut Self::Output) -> Result<(), Self::Error> {
        let _ = output.send(format!("exited:{state}"));
        Ok(())
    }
}

#[derive(Ephemeral)]
struct Pool {
    #[reconciler(output = events)]
    set: OptativeSet<Tracked>,
    events: mpsc::Sender<String>,
}

#[test]
fn dropping_runs_exit_on_managed_items() {
    let (tx, rx) = mpsc::channel();
    let mut pool = Pool {
        set: OptativeSet::new(),
        events: tx,
    };
    pool.set.reconcile(
        vec![Tracked { id: "a".into() }, Tracked { id: "b".into() }],
        &mut (),
        &mut pool.events,
    );

    // Nothing has exited yet.
    assert!(rx.try_recv().is_err());

    drop(pool);

    let mut exited: Vec<String> = rx.try_iter().collect();
    exited.sort();
    assert_eq!(exited, vec!["exited:a".to_string(), "exited:b".to_string()]);
}
