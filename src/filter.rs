//! Deciding which discovered nodes are worth keeping.

/// Decides whether a node should be kept during a scan.
///
/// Any `Fn(&str) -> bool` implements this, so a closure works directly:
///
/// ```
/// use opcua_tag_browser::NodeFilter;
///
/// let only_axes = |name: &str| name.starts_with("Axis");
/// assert!(only_axes.accepts("Axis1"));
/// assert!(!only_axes.accepts("Icon"));
/// ```
pub trait NodeFilter {
    /// Returns `true` if a node with this display name should be kept.
    fn accepts(&self, display_name: &str) -> bool;
}

impl<F> NodeFilter for F
where
    F: Fn(&str) -> bool,
{
    fn accepts(&self, display_name: &str) -> bool {
        self(display_name)
    }
}

/// Keeps every node.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptAll;

impl NodeFilter for AcceptAll {
    fn accepts(&self, _display_name: &str) -> bool {
        true
    }
}

/// Drops nodes that are server furniture rather than process data.
///
/// Rejects `Icon`, `Server`, and separator rows containing `------`. These are
/// conventions seen on Siemens and comparable server implementations; if your
/// vendor uses different junk names, write your own filter instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultNodeFilter;

impl NodeFilter for DefaultNodeFilter {
    fn accepts(&self, display_name: &str) -> bool {
        !(display_name.eq_ignore_ascii_case("Icon")
            || display_name.eq_ignore_ascii_case("Server")
            || display_name.contains("------"))
    }
}