# optative-derive

Procedural macros for the [optative](https://crates.io/crates/optative) reconciler library.

## `#[lifecycle_trace]`

Applied to an `impl Lifecycle for T` block, it wraps `enter` / `reconcile_self` / `exit`
with `tracing` events — `tracing::info!` on success, `tracing::error!` on failure, carrying
`key`, `display_name`, `metadata`, and `error` fields.

```rust
use optative::Lifecycle;
use optative_derive::lifecycle_trace;

#[lifecycle_trace]
impl Lifecycle for MySpec {
    // ... your enter / reconcile_self / exit ...
}
```

## `#[derive(Ephemeral)]`

Generates a `Drop` impl that reconciles `#[reconciler]`-annotated fields to an empty desired
set, so managed items run their `exit` hooks when the owning struct is dropped.

```rust
use optative_derive::Ephemeral;

#[derive(Ephemeral)]
struct Pool {
    #[reconciler(output = sender)]
    set: OptativeSet<MySpec>,
    sender: std::sync::mpsc::Sender<Event>,
}
```

The reconciler field's `Context` type must implement `Default`.

## Call-site requirements

The generated code refers to `serde_json` and `tracing` by name. Any crate using
`#[lifecycle_trace]` must have both in scope (they are not re-exported by this crate, as is
standard for derive macros).

## License

MIT OR Apache-2.0
