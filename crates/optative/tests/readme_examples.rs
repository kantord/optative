//! Compile-checks the snippets shown in README.md so they don't drift.

use optative::{Lifecycle, ManagedSet, Reconcile, ReconcileErrors};

struct Greeter {
    name: String,
}

impl std::fmt::Display for Greeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Lifecycle for Greeter {
    type Key = String;
    type State = ();
    type Context = ();
    type Output = ();
    type Error = std::convert::Infallible;

    fn key(&self) -> String {
        self.name.clone()
    }
    fn enter(self, _: &mut (), _: &mut ()) -> Result<(), Self::Error> {
        Ok(())
    }
    fn reconcile_self(self, _: &mut (), _: &mut (), _: &mut ()) -> Result<(), Self::Error> {
        Ok(())
    }
    fn exit(_: (), _: &mut (), _: &mut ()) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn managed_set_example_compiles() {
    let mut set: ManagedSet<Greeter> = ManagedSet::new();
    set.reconcile(vec![Greeter { name: "ada".into() }], &mut (), &mut ());
    set.reconcile(vec![], &mut (), &mut ());
}

struct LatestOnly<T: Lifecycle> {
    current: Option<(T::Key, T::State)>,
}

impl<T: Lifecycle> Reconcile<T> for LatestOnly<T> {
    fn reconcile(
        &mut self,
        desired: impl IntoIterator<Item = T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
    ) -> ReconcileErrors<T::Key, T::Error> {
        let mut errors = ReconcileErrors::new();
        let last = desired.into_iter().last();
        if let Some((key, state)) = self.current.take() {
            if let Err(e) = T::exit(state, ctx, output) {
                errors.push((key, e));
            }
        }
        if let Some(item) = last {
            let key = item.key();
            match item.enter(ctx, output) {
                Ok(state) => self.current = Some((key, state)),
                Err(e) => errors.push((key, e)),
            }
        }
        errors
    }
}

#[test]
fn latest_only_example_compiles() {
    let mut latest: LatestOnly<Greeter> = LatestOnly { current: None };
    latest.reconcile(vec![Greeter { name: "ada".into() }], &mut (), &mut ());
    latest.reconcile(vec![Greeter { name: "grace".into() }], &mut (), &mut ());
    latest.reconcile(vec![], &mut (), &mut ());
}
