# optative

Simple generic traits for building reconciler systems. 

Reconciliation, in this context, means that you manage certain items (such as processes, cloud resources, or UI components), and control the desired state. 

You define lifecycle events:
- How to create an item
- How to update an item
- How to delete an item

You also decide what the desired state looks like, and when the reconciliation needs to happen. Optative takes care of calling the lifecycle events to achieve yor desired state.

Extracted from my in-progress project [tauler](https://github.com/kantord/tauler) (a data-driven widgeting system) because the pattern turned out to be useful well beyond that project: process pools, connection pools, file watchers, subscriptions, etc.

The walkthrough below builds everything against a tiny REST API that stores one personal greeting per person. The `Api` client itself is plumbing - its full source lives in [`crates/optative/tests/common/mod.rs`](crates/optative/tests/common/mod.rs) - but for the tutorial, all you need to know is:

- `api.create(&greeting)` does `POST /greetings/<name>` with the message as the body
- `api.update(&greeting)` does `PUT /greetings/<name>`
- `api.remove(&greeting)` does `DELETE /greetings/<name>`

## Tutorial — declarative state with `OptativeSet`

### Step 1. Define a data type that models our resource

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Greeting {
    person: String,
    message: String,
}
```

### Step 2. Implement `Lifecycle` to teach optative how to manage one

We implement the Lifecycle trait by delegating the work to our API client.

```rust
use optative::Lifecycle;

impl Lifecycle for Greeting {
    type Key = String;
    type State = Greeting;        // current tracked state
    type Context = Api;           // the REST client
    type Output = ();
    type Error = ureq::Error;

    fn key(&self) -> String { self.person.clone() }

    fn enter(self, api: &mut Api, _: &mut ()) -> Result<Greeting, Self::Error> {
        api.create(&self)?;
        Ok(self)
    }

    fn reconcile_self(self, state: &mut Greeting, api: &mut Api, _: &mut ()) -> Result<(), Self::Error> {
        if state.message != self.message {
            api.update(&self)?;
            *state = self;
        }
        Ok(())
    }

    fn exit(state: Greeting, api: &mut Api, _: &mut ()) -> Result<(), Self::Error> {
        api.remove(&state)
    }
}
```

### Step 3. Initialize a store and the API client

```rust
use optative::{OptativeSet, Reconcile};

let mut api = Api { base_url: "http://greetings.example".into() };
let mut store: OptativeSet<Greeting> = OptativeSet::new();
```

If the server already has greetings, seed them so they skip `enter`:

```rust
let existing: Vec<(String, Greeting)> = /* fetch from the API */;
let mut store = OptativeSet::<Greeting>::with_initial_state(existing);
```

### Step 4. Declare your initial desired set

The remote state will converge to it automatically.

```rust
store.reconcile(vec![
    Greeting { person: "ada".into(),   message: "hello, ada".into() },
    Greeting { person: "grace".into(), message: "welcome, grace".into() },
], &mut api, &mut ());
```

-> `POST /greetings/ada`, `POST /greetings/grace`.

### Step 5. Change your mind

```rust
store.reconcile(vec![
    Greeting { person: "ada".into(), message: "good morning, ada".into() },
], &mut api, &mut ());
```

-> `PUT /greetings/ada` (message differs), `DELETE /greetings/grace` (no longer in the desired set). optative diffed the two passes and called the right hook for each item.

## CLI: `esto`

`esto` runs declarative `.op.tsx`/`.eso.jsx` scripts instead of a Rust `Lifecycle` impl:
define a `unit()` (key/value/observe/enter/update/exit) and a JSX tree describing the
desired set, and `esto run <file>` diffs it against `observe()`'s reported current state,
calling the right hook for each item — the same reconciliation `OptativeSet` does, just
scripted. `esto --help` / `esto <subcommand> --help` has the full command reference
(`run`, `watch`, `types`, `type-check`).

Worked examples:
- **[`examples/`](./examples)** — minimal, self-contained scripts in this repo.
- **[kantord/meta.op](https://github.com/kantord/meta.op)** — governance across a whole
  GitHub account, e.g. `ci/cargo-test-coverage.op.tsx`: every Rust repo should have a CI
  workflow running `cargo test`.
- **[kantord/insane-forms](https://github.com/kantord/insane-forms)** —
  `docs/api.op.tsx` (generate an API doc stub per public export),
  `docs/doc-coverage.op.tsx` (flag public exports missing a JSDoc comment), and
  `docs/widget-taxonomy.op.tsx` (enforce a structural invariant across a component set).

## License

MIT OR Apache-2.0
