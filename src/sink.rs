//! Where collected values go.

use crate::tag::PlcTag;
use crate::variant::{variant_as_bool, variant_as_f64, variant_as_i64};
use opcua::client::prelude::Variant;
use std::time::SystemTime;

/// Which mechanism produced a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Pushed by the server through a subscription.
    Subscribe,
    /// Read by the polling loop.
    Poll,
}

impl Source {
    /// Lowercase label, for logs and serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Subscribe => "subscribe",
            Source::Poll => "poll",
        }
    }
}

/// A tag whose value or quality changed.
///
/// Borrowed rather than owned: a busy server produces thousands of these per
/// second, and most sinks serialize immediately rather than storing them.
#[derive(Debug)]
#[non_exhaustive]
pub struct TagChange<'a> {
    /// When the change was observed by this process.
    pub timestamp: SystemTime,
    /// Name of the collector that saw it.
    pub collector: &'a str,
    /// The tag itself, carrying display name, node ID, and browse path.
    pub tag: &'a PlcTag,
    /// The raw value. Use the accessors below rather than parsing
    /// [`value`](Self::value).
    pub variant: &'a Variant,
    /// Rendered value, from [`format_variant`](crate::format_variant).
    pub value: &'a str,
    /// Rendered quality, from [`format_quality`](crate::format_quality).
    pub quality: &'a str,
    /// Whether the server reported Good quality.
    pub good: bool,
    /// Whether this arrived by subscription or by poll.
    pub source: Source,
}

impl TagChange<'_> {
    /// The value as a float, if it is numeric.
    ///
    /// ```no_run
    /// # use opcua_tag_browser::TagChange;
    /// # fn demo(event: &TagChange<'_>) {
    /// if event.as_f64().is_some_and(|v| v > 100.0) {
    ///     println!("{} is high", event.tag.path);
    /// }
    /// # }
    /// ```
    pub fn as_f64(&self) -> Option<f64> {
        variant_as_f64(self.variant)
    }

    /// The value as a signed integer, if it is integral.
    pub fn as_i64(&self) -> Option<i64> {
        variant_as_i64(self.variant)
    }

    /// The value as a boolean, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        variant_as_bool(self.variant)
    }

    /// The tag's browse path, for brevity at call sites.
    pub fn path(&self) -> &str {
        &self.tag.path
    }
}

/// A connection state transition.
#[derive(Debug)]
#[non_exhaustive]
pub struct ConnectionChange<'a> {
    /// When the transition was observed.
    pub timestamp: SystemTime,
    /// Name of the collector whose connection changed.
    pub collector: &'a str,
    /// Endpoint involved.
    pub endpoint_url: &'a str,
    /// `true` when the link came back, `false` when it dropped.
    pub connected: bool,
}

/// Receives everything a [`Collector`](crate::Collector) observes.
///
/// Implementations must be cheap and non-blocking. They are called from the
/// subscription callback and the polling loop, and slow work there backs up the
/// OPC UA event loop. Queue to another thread if you need to do I/O — see
/// [`JsonlSink`](crate::sinks::JsonlSink) for that pattern.
///
/// ```
/// use opcua_tag_browser::{TagChange, TagSink};
///
/// struct Printer;
///
/// impl TagSink for Printer {
///     fn tag_changed(&self, event: &TagChange<'_>) {
///         println!("{} = {}", event.path(), event.value);
///     }
/// }
/// ```
pub trait TagSink: Send + Sync {
    /// Called for every value or quality change.
    fn tag_changed(&self, event: &TagChange<'_>);

    /// Called when the connection drops or returns. Optional.
    fn connection_changed(&self, _event: &ConnectionChange<'_>) {}
}

/// Any suitable closure is a sink.
///
/// ```no_run
/// use opcua_tag_browser::{Collector, TagChange};
///
/// # fn main() -> opcua_tag_browser::Result<()> {
/// Collector::new("line1", "opc.tcp://localhost:4840")
///     .insecure()
///     .sink(|e: &TagChange<'_>| println!("{} = {}", e.path(), e.value))
///     .run()
/// # }
/// ```
impl<F> TagSink for F
where
    F: Fn(&TagChange<'_>) + Send + Sync,
{
    fn tag_changed(&self, event: &TagChange<'_>) {
        self(event);
    }
}
