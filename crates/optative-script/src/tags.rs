/// Marker property set on Fragment objects emitted by `h()`.
pub const FRAG: &str = "$fragment";
/// Marker property set on Context objects emitted by `h()`.
pub const CTX: &str = "$context";
/// Property name under which a component function is stored by `h()`.
pub const COMPONENT: &str = "$component";
/// Property name under which a unit-kind descriptor is stored by `h()`.
pub const KIND: &str = "$kind";

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
