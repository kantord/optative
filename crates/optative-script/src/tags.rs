/// Internal flag set by `__esto_fragment` to mark Fragment singletons.
pub const ESTO_FRAGMENT: &str = "__estoFragment";
/// Internal flag set by `__esto_context` to mark Context singletons.
pub const ESTO_CONTEXT: &str = "__estoContext";
/// Internal flag set on objects returned by `unit()` to mark them as kind descriptors.
pub const ESTO_KIND: &str = "__estoKind";
/// Auto-assigned numeric identifier for each unique unit kind.
pub const ESTO_ID: &str = "__estoId";
/// Marker property set on objects returned by the fs claim-File component.
pub const FS_CLAIM: &str = "$estoFsClaim";
/// Discriminant set by `optativeSet()`/`optativeJsonSet()` on `unit()`'s
/// `reconciler` descriptor, naming which backend `reconcile_kind` should drive.
pub const ESTO_RECONCILER_KIND: &str = "__estoReconcilerKind";
