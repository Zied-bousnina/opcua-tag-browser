# opcua-tag-browser 0.2.0 — implementation plan

You are implementing version 0.2.0 of the `opcua-tag-browser` crate, starting
from the published 0.1.3 source in this repository. Work through the phases in
order. Each phase ends with a verification command that must pass before you
continue. Do not skip ahead; do not batch phases together.

## Context

The crate currently browses an OPC UA server's address space into a flat,
cacheable tag list. Version 0.2.0 adds reading (subscriptions and polling),
writing, richer tag metadata sourced from the OPC UA specification, and a
secure-by-default connection layer — while cutting the amount of code a
consumer has to write from roughly 900 lines to under 10.

Dependency: `opcua` 0.12 (the locka99 crate, sync API, `client::prelude::*`).
It is an older API than `async-opcua`; do not attempt to migrate. Some calls
panic on malformed server responses, which is why every service call is
wrapped.

## Hard constraints — violating any of these is a failed implementation

1. **The library never calls `std::process::exit`** and never installs a signal
   handler except inside `Collector::handle_ctrl_c`, which exists precisely
   because the consumer asked for it by name.
2. **`#![forbid(unsafe_code)]` and `#![deny(missing_docs)]` stay in `lib.rs`.**
   Every public item, including every public struct field and enum variant,
   needs a doc comment.
3. **No `println!` or `eprintln!` anywhere in the crate.** Use `log::{trace,
   debug, info, warn, error}`.
4. **No secret ever reaches a log.** `Credentials` must have a hand-written
   `Debug` impl that prints `<redacted>` for passwords.
5. **`Box<dyn Error>` must not appear in any public signature.** Use the
   crate's `Error` type.
6. **Do not change `src/browser.rs`'s continuation-point loop or
   `src/scanner.rs`'s `ancestors`-based cycle detection** beyond what this plan
   specifies. Both encode fixes for real bugs. In particular, cycle detection
   uses an ancestor stack, not a global visited set: per OPC 10000-3 §6.3.3,
   "Multiple BrowsePaths to the same Node shall be treated as separate Nodes",
   so a node reachable by two paths must be expanded under both.
7. **Preserve every existing test.** `tests/scanner.rs` must keep passing
   unchanged except where this plan says otherwise.
8. **If a compile error reveals that an `opcua` 0.12 API differs from what this
   plan assumes, adapt the call site and note it** — do not silently change the
   design to route around it.

---

# Phase 1 — additive groundwork (ships as 0.1.4)

Nothing here breaks any existing consumer. Publish before continuing.

## 1.1 `src/scanner.rs` — edit

Ensure `ScanOptions` derives `Debug, Clone`. Add builder methods, because
`#[non_exhaustive]` blocks struct-literal and struct-update syntax across the
crate boundary:

```rust
impl ScanOptions {
    /// Sets the maximum recursion depth below the root node.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Sets whether to descend into the children of `Variable` nodes.
    pub fn descend_into_variables(mut self, yes: bool) -> Self {
        self.descend_into_variables = yes;
        self
    }
}
```

## 1.2 `src/filter.rs` — edit

Add bounds to the blanket closure impl. Later phases store
`Arc<dyn NodeFilter + Send + Sync>`:

```rust
impl<F> NodeFilter for F
where
    F: Fn(&str) -> bool + Send + Sync,
```

## 1.3 `src/tagset.rs` — NEW

An indexed collection of tags. A large server yields thousands, and linear
lookup per query adds up.

```rust
pub struct TagSet {
    tags: Vec<PlcTag>,
    by_path: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}
```

Public API:

| Method | Behaviour |
|---|---|
| `new(Vec<PlcTag>) -> Self` | Builds both indexes. On duplicate display names, first wins. |
| `find(&self, &str) -> Option<&PlcTag>` | Exact browse path, falling back to display name. |
| `find_by_node_id(&self, &str) -> Option<&PlcTag>` | Linear; node IDs are not indexed. |
| `matching(&self, pattern: &str) -> Vec<&PlcTag>` | Glob over browse paths. |
| `children_of(&self, path: &str) -> Vec<&PlcTag>` | Direct children only, not descendants. |
| `len`, `is_empty`, `into_vec`, `as_slice` | Obvious. |

Also implement `Deref<Target = [PlcTag]>`, `From<Vec<PlcTag>>`,
`FromIterator<PlcTag>`, and `Debug, Clone, Default`.

Add a private `pub(crate) fn glob_match(pattern: &str, text: &str) -> bool`
supporting `*` (any run, including `/`) and `?` (one character). Implement it
**iteratively with backtracking**, not recursively — a pathological pattern
must not blow the stack. Do not add a glob dependency.

## 1.4 `src/lib.rs` — edit

`mod tagset;` and `pub use tagset::TagSet;`

## 1.5 `tests/tagset.rs` — NEW

Cover, at minimum: find by path; find by display name; ambiguous display name
resolves to the first; `find_by_node_id`; `matching("Machine/*")`;
`matching("*/Speed")`; `matching("Machine/Axis?/Speed")`; `matching("*")`
returns everything; a non-matching pattern returns empty; `children_of` returns
direct children only; `Deref` lets you call `.iter()`.

This file has no `opcua` dependency, so it compiles and runs even if the rest
of the crate has problems. Treat it as your smoke test.

### Verify phase 1

```
cargo test --all-features
cargo clippy --all-features -- -D warnings
cargo doc --no-deps --all-features
```

Bump to `0.1.4`, add a CHANGELOG entry under `### Added`, commit, tag, publish.

---

# Phase 2 — spec-derived tag metadata (breaking, 0.2.0)

This phase makes the crate understand what it is browsing rather than treating
the address space as a generic tree. Everything here comes from
**OPC 10000-3 Part 3: Address Space Model**, Table 13 (§5.6.2, Variable
NodeClass).

## 2.1 `src/tag.rs` — replace

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlcTag {
    /// Localized `DisplayName`. Suitable for showing to a human; not stable
    /// across server locales.
    pub display_name: String,

    /// `BrowseName`, the name portion without the namespace index.
    ///
    /// Unlike `display_name`, this is stable, which is why browse paths are
    /// built from it (OPC 10000-3 §6.2.5).
    #[serde(default)]
    pub browse_name: String,

    /// Node identifier in OPC UA string form, e.g. `ns=3;s="DB1"."Speed"`.
    pub node_id: String,

    /// Node class rendered as text, e.g. `"Variable"`.
    pub node_class: String,

    /// Slash-joined `BrowseName`s from the scan root, e.g. `Machine/Axis1/Speed`.
    pub path: String,

    /// `NodeId` of the value's DataType (Table 13, mandatory attribute).
    #[serde(default)]
    pub data_type: Option<String>,

    /// Whether the value is an array, per Table 13: `-1` scalar, `0` one or
    /// more dimensions, `n >= 1` exactly that many dimensions.
    #[serde(default)]
    pub value_rank: Option<i32>,

    /// `AccessLevel` bit mask (OPC 10000-3 §8.57): bit 0 CurrentRead,
    /// bit 1 CurrentWrite, bit 2 HistoryRead, bit 3 HistoryWrite.
    #[serde(default)]
    pub access_level: Option<u8>,

    /// The server's own `MinimumSamplingInterval` in milliseconds — how fast it
    /// can reasonably sample this value. `0` means continuous monitoring,
    /// `-1` means indeterminate.
    #[serde(default)]
    pub min_sampling_interval: Option<f64>,

    /// Whether the server is actively collecting history for this variable.
    #[serde(default)]
    pub historizing: Option<bool>,

    /// `NodeId` of the `HasTypeDefinition` target, e.g. `PropertyType` versus
    /// `BaseDataVariableType`.
    #[serde(default)]
    pub type_definition: Option<String>,

    /// `NodeId` of the reference type that reached this node — `HasProperty`
    /// (i=46) versus `HasComponent` (i=47).
    #[serde(default)]
    pub reference_type: Option<String>,
}
```

Every new field is `Option` with `#[serde(default)]`, so 0.1.x cache files
still deserialize.

Add these helpers, each documented:

```rust
impl PlcTag {
    /// Whether the server reports the value as readable.
    ///
    /// Unknown access level is treated as readable, since a server that cannot
    /// determine access rights "should state that it is readable and
    /// writeable" (OPC 10000-3 §5.6.2).
    pub fn is_readable(&self) -> bool;

    /// Whether the server reports the value as writable.
    ///
    /// Unknown access level is treated as NOT writable — the conservative
    /// direction for an operation that changes plant state.
    pub fn is_writable(&self) -> bool;

    /// Whether history can be read for this variable.
    pub fn has_history(&self) -> bool;

    /// Whether the value is an array, per `value_rank`.
    pub fn is_array(&self) -> bool;

    /// Whether this node was reached by a `HasProperty` reference, making it
    /// metadata (`EngineeringUnits`, `EURange`) rather than process data.
    pub fn is_property(&self) -> bool;
}
```

Note the deliberate asymmetry: unknown read access defaults permissive,
unknown write access defaults restrictive.

Keep a `PlcTag::new(...)` constructor for the required fields, leaving the
optional ones `None`, so `#[non_exhaustive]` does not block construction by
tests and alternative backends.

## 2.2 `src/browser.rs` — edit

`BrowsedNode` gains three fields. All three are already present in every
`ReferenceDescription` the server returns and are currently discarded:

```rust
    /// `BrowseName` of the node, name portion only.
    pub browse_name: String,
    /// Target of the node's `HasTypeDefinition` reference, if the server sent it.
    pub type_definition: Option<NodeId>,
    /// Reference type by which this node was reached.
    pub reference_type: Option<NodeId>,
```

Update `append_references` to populate them from `reference.browse_name.name`,
`reference.type_definition.node_id`, and `reference.reference_type_id`.

Update `BrowsedNode::new` to take them, and additionally provide
`with_browse_name`, `with_type_definition`, and `with_reference_type` builder
methods so future fields do not force another signature change.

Replace the raw `result_mask: BrowseResultMask::All as u32` with an explicit
mask covering BrowseName, DisplayName, NodeClass, TypeDefinition, and
ReferenceType, and add a comment saying why each is needed.

## 2.3 `src/attributes.rs` — NEW

A batched attribute reader, run once after the tree walk.

```rust
/// Reads the Variable attributes defined in OPC 10000-3 Table 13 for every
/// scanned tag, in batches the server will accept.
///
/// Runs once per scan, and a scan result is normally cached, so the cost is
/// paid once per deployment rather than per start.
pub(crate) fn enrich(
    session: &dyn PlcSession,
    tags: &mut [PlcTag],
    batch_size: usize,
) -> usize;
```

For each tag build five `ReadValueId`s — `AttributeId::DataType`, `ValueRank`,
`AccessLevel`, `MinimumSamplingInterval`, `Historizing` — flatten them into one
request vector, chunk to `min(batch_size, server MaxNodesPerRead)`, and map the
results back positionally.

Requirements:
- Query `Server_ServerCapabilities_OperationLimits_MaxNodesPerRead` first and
  respect it; reuse the same helper the poller uses rather than duplicating it.
- A failed batch must not abort enrichment. Log at `debug` and leave those
  tags' fields `None`.
- Return the count of tags successfully enriched, for the caller to log.
- Never let a bad status code become a wrong value — only write a field when
  the corresponding `DataValue` has `status` good (or absent) and a `Some`
  value.

## 2.4 `src/scanner.rs` — edit

Two changes.

**Build `path` from `browse_name`, not `display_name`.** Populate both
`PlcTag::browse_name` and `PlcTag::display_name`; the path uses the former.
Fall back to `display_name` when `browse_name` is empty.

**Add to `ScanOptions`:**

```rust
    /// Read the Variable attributes from OPC 10000-3 Table 13 after browsing.
    ///
    /// Costs one batched `Read` pass over the discovered nodes. Without it the
    /// corresponding `PlcTag` fields stay `None`, and features that depend on
    /// them — write guarding, per-tag sampling intervals, `writable_only` —
    /// degrade to their conservative defaults.
    pub read_attributes: bool,        // default true

    /// Include nodes reached by a `HasProperty` reference.
    ///
    /// Properties are metadata — `EngineeringUnits`, `EURange`, `Description`
    /// — rather than process data. Excluding them typically removes a large
    /// fraction of a PLC's node count.
    pub include_properties: bool,     // default false
```

Add matching builder methods. Apply `include_properties` during the walk, using
the `reference_type` now on `BrowsedNode`; `HasProperty` is `i=46`.

`TreeScanner::scan` calls `attributes::enrich` when `read_attributes` is set,
after the walk completes and before returning the `ScanReport`.

## 2.5 `src/session.rs` — edit

Add to the `PlcSession` trait and to `OpcUaSession`:

```rust
    /// Writes values to nodes on the server.
    ///
    /// The returned status codes correspond one-to-one with `nodes_to_write`.
    /// An overall `Ok` does not mean every individual write succeeded.
    fn write(&self, nodes_to_write: &[WriteValue]) -> Result<Vec<StatusCode>, StatusCode>;
```

Implement it as `catch_panic(|| self.session.read().write(nodes_to_write))`,
matching the existing pattern. This is breaking for anyone who implemented
`PlcSession` — that is why it lands in 0.2.0.

## 2.6 `src/error.rs` — edit

Add these variants. `Error` is already `#[non_exhaustive]`, so this is additive
for matchers:

```rust
    /// A collector was run without a sink.
    MissingSink,
    /// A sink could not be initialized.
    SinkSetup(String),
    /// Credentials would have been sent over an unencrypted channel.
    InsecureCredentials,
    /// No tag matched the supplied browse path or display name.
    UnknownTag { path: String },
    /// A node ID string could not be parsed.
    BadNodeId { node_id: String },
    /// The server reports this variable as not writable.
    NotWritable { path: String },
    /// A `Write` service call failed.
    Write { node_id: String, status: StatusCode },
    /// A `Read` service call failed.
    Read { node_id: String, status: StatusCode },
```

Give each a `#[error(...)]` message that names the tag or node involved.

### Verify phase 2

```
cargo check --no-default-features --features json-cache
cargo test --no-default-features --features json-cache
cargo clippy --no-default-features --features json-cache -- -D warnings
```

Checking the core without monitoring isolates any later failure to new code.

---

# Phase 3 — secure connections (0.2.0)

## 3.1 `src/connection.rs` — replace

The current default is plaintext, anonymous, and trusts any certificate. That
is correct for an isolated machine network and wrong as a published default,
because defaults are what most users inherit without reading.

Define:

```rust
/// How messages on the wire are protected.
pub enum Security {
    /// No signing, no encryption. Everything is readable on the wire.
    None,
    /// Signed but not encrypted. Detects tampering; does not prevent reading.
    Sign(SecurityPolicy),
    /// Signed and encrypted.
    SignAndEncrypt(SecurityPolicy),
}

impl Security {
    /// Whether this configuration leaves traffic readable on the wire.
    pub fn is_plaintext(&self) -> bool;
}

/// How the client identifies itself to the server.
pub enum Credentials {
    Anonymous,
    UserName { user: String, password: String },
}
```

`Credentials` must have a **hand-written `Debug`** printing
`UserName { user: "...", password: "<redacted>" }`. Do not derive it.

`ConnectOptions` gains `security` and `credentials`, and its `Default` becomes:

```rust
trust_server_certs: false,
security: Security::SignAndEncrypt(SecurityPolicy::Basic256Sha256),
credentials: Credentials::Anonymous,
```

Add `ConnectOptions::insecure()` returning the old behaviour, and builder
methods `application_name`, `application_uri`, `session_timeout_ms`,
`security`, `user_name`, `trust_server_certs`.

Add a private `validate()` called at the top of `connect()`:

- `Credentials::UserName` with `security.is_plaintext()` returns
  `Error::InsecureCredentials`. Do not send it and then warn.
- When `security.is_plaintext()`, emit a single `log::warn!` naming the
  endpoint.

## 3.2 `examples/scan_and_dump.rs` — edit

Change `ConnectOptions::default()` to `ConnectOptions::insecure()` with a
comment explaining that machine networks rarely have PKI deployed. Without
this, the example breaks against a plaintext server.

---

# Phase 4 — reading (0.2.0)

## 4.1 `src/sink.rs` — NEW

```rust
/// Which mechanism produced a value.
pub enum Source { Subscribe, Poll }

/// A tag whose value or quality changed.
pub struct TagChange<'a> {
    pub timestamp: SystemTime,
    pub collector: &'a str,
    pub tag: &'a PlcTag,
    /// The raw value. Prefer the typed accessors over parsing `value`.
    pub variant: &'a Variant,
    pub value: &'a str,
    pub quality: &'a str,
    pub good: bool,
    pub source: Source,
}
```

Borrowed, not owned. A busy server produces thousands of these per second and
most sinks serialize immediately; owning would allocate several strings per
event for nothing.

Accessors on `TagChange`: `as_f64`, `as_i64`, `as_bool`, `path()`.

```rust
pub trait TagSink: Send + Sync {
    fn tag_changed(&self, event: &TagChange<'_>);
    fn connection_changed(&self, _event: &ConnectionChange<'_>) {}
}
```

The default body on `connection_changed` makes a minimal sink three lines.

Add a blanket impl so a closure is a sink:

```rust
impl<F> TagSink for F where F: Fn(&TagChange<'_>) + Send + Sync
```

## 4.2 `src/variant.rs` — edit

Add `variant_as_f64`, `variant_as_i64`, `variant_as_bool`, each handling
`Variant::Variant(inner)` recursively. `variant_as_i64` must **reject** floats
rather than truncating — silently dropping a fractional part is rarely what a
caller wanted. `variant_as_i64` on `UInt64` uses `i64::try_from(...).ok()`.

## 4.3 `src/sinks/{mod,log_sink,jsonl}.rs` — NEW

`LogSink` (unconditional): logs each change at `info`, connection transitions
at `info`/`warn`.

`JsonlSink` (feature `jsonl-sink`): daily files at
`<dir>/<collector>_<YYYYMMDD>.jsonl`. **Serialization and file I/O run on a
dedicated thread fed by an `mpsc` channel** — the sink is called from the
subscription callback, and blocking there backs up the OPC UA event loop. Keep
one open `BufWriter` per `(collector, date)` so files rotate at local midnight,
and flush after every line so `tail -f` shows live data.

## 4.4 `src/monitor/mod.rs` — NEW

`MonitorOptions`, merging what were separate subscriber and poller settings:

| Field | Default | Note |
|---|---|---|
| `max_subscribed_items` | 800 | measured, not from a spec |
| `sub_chunk_size` | 500 | |
| `request_batch_size` | 50 | |
| `sampling_interval` | 250 ms | fallback when the tag has no `min_sampling_interval` |
| `publishing_interval` | 100 ms | |
| `lifetime_count` | 100 | |
| `max_keep_alive_count` | 10 | |
| `poll_interval` | 1 s | |
| `poll_batch_size` | 50 | |
| `health_check_interval` | 3 s | |
| `reconnect_delay` | 10 s | |
| `deadband` | `None` | suppress numeric changes below this |

Document that these defaults were measured against one Siemens PLC exposing
about 2,500 tags — they are a starting point, not a specification. Add builder
methods for the ones a user realistically tunes.

## 4.5 `src/monitor/subscriber.rs` — NEW

Chunked subscription creation with per-item status checking.

**Use each tag's own sampling interval where the server provided one:**

```rust
let sampling = tag.min_sampling_interval
    .filter(|&ms| ms > 0.0)
    .unwrap_or(options.sampling_interval.as_millis() as f64);
```

The `> 0.0` filter matters: per Table 13, `0` means continuous monitoring and
`-1` means indeterminate, so neither should be passed through as a literal
interval. This means each `MonitoredItemCreateRequest` gets its own
`MonitoringParameters` rather than a shared one.

Other requirements:
- `queue_size: 1`, `discard_oldest: true` — a change log wants the latest value
  only.
- Check **per-item** `status_code` in the `CreateMonitoredItems` response and
  return rejected tags to the caller. Ignoring per-item status is how a server
  silently drops half your tags with no error.
- Skip `Variant::ExtensionObject` in the callback; struct members are
  subscribed individually.
- Build an `Arc<HashMap<String, PlcTag>>` before creating subscriptions so the
  callback can resolve a node ID to a full tag.
- If `create_subscription` itself fails, push that whole chunk to the rejected
  list and continue; do not abort.

## 4.6 `src/monitor/poller.rs` — NEW

Batched cyclic reads for whatever the subscriber rejected.

Requirements:
- Pre-parse every `NodeId` once, outside the loop.
- Query the server's `MaxNodesPerRead` at start and clamp `poll_batch_size`.
- Pass `max_age` equal to one poll interval, so the server may serve a cached
  value rather than hitting the device every cycle.
- On read error: if the session is disconnected, return so the caller can
  reconnect. Otherwise halve the batch size down to a floor of 10 and retry the
  same range; at the floor, skip the batch and advance.
- Change detection keys on **both** the rendered value and the quality, so a
  Good-to-Bad transition at a constant value is still reported.
- When `deadband` is set and both readings are numeric, suppress changes below
  the threshold.
- Check the stop flag in the inner loop, not only the outer one.

## 4.7 `src/monitor/watchdog.rs` — NEW

A thread sampling `is_connected()` at `health_check_interval`, reporting only
**transitions**, not every check.

---

# Phase 5 — writing (0.2.0)

## 5.1 `src/writer.rs` — NEW

```rust
/// Reads and writes individual tag values on demand.
///
/// `Clone` and `Send`, so it can be moved into another thread while a
/// collector's event loop runs.
#[derive(Clone)]
pub struct TagClient {
    session: Arc<dyn PlcSession>,
    tags: Arc<TagSet>,
}
```

| Method | Behaviour |
|---|---|
| `set(&self, path: &str, value: impl Into<Variant>) -> Result<()>` | Resolve, check writability, write, verify status. |
| `set_node(&self, node_id: &str, value: impl Into<Variant>) -> Result<()>` | Same, by raw node ID. |
| `set_many(&self, &[(&str, Variant)]) -> Result<Vec<StatusCode>>` | One service call, one status per input. |
| `get(&self, path: &str) -> Result<String>` | Rendered value. |
| `get_variant(&self, path: &str) -> Result<Variant>` | Raw value. |
| `tags(&self) -> &TagSet` | |
| `is_connected(&self) -> bool` | |

Three things `set` must do, in this order:

1. **Resolve** the path through `TagSet::find`, falling back to treating the
   input as a raw node ID so callers can address nodes the scan filtered out.
   Unresolvable input is `Error::UnknownTag`.
2. **Refuse a write to a tag whose `AccessLevel` says read-only**, returning
   `Error::NotWritable` without a round trip. This is only possible because
   Phase 2 reads `AccessLevel`. When `access_level` is `None`, attempt the
   write and let the server decide.
3. **Check the returned per-node `StatusCode`**, not just the overall
   `Result`. A `Write` that returns `Ok(vec![BadNotWritable])` has failed.

Also implement optional write-verification:

```rust
    /// Writes, then reads back and compares.
    ///
    /// A good status from `Write` means the server accepted the request, not
    /// that the value took effect — a PLC program may immediately overwrite it.
    /// Use this where confirmation matters.
    pub fn set_verified(&self, path: &str, value: impl Into<Variant>) -> Result<VerifyOutcome>;
```

```rust
pub enum VerifyOutcome {
    /// Read-back matched the written value.
    Written,
    /// The write was accepted but the read-back differs.
    Mismatch { expected: String, actual: String },
}
```

## 5.2 `src/collector.rs` — NEW

The single entry point that makes consumer code short. Builder, all methods
documented:

**Construction:** `new(name, endpoint_url)`. Defaults the cache to
`plc_tags_<name>.json` so the common case needs no cache configuration.

**Sinks:** `sink(impl TagSink + 'static)` — takes the sink by value and wraps
it in `Arc` internally; the `Arc` was leaking an implementation detail.
`shared_sink(Arc<dyn TagSink>)` for a sink shared across collectors.
`jsonl(dir)` behind `jsonl-sink`, deferring any directory error to `run()` via
a `deferred_error: Option<Error>` field so the chain stays clean.

**Selection:** `only(iter)`, `matching(glob)`, `select(predicate)`,
`writable_only()`. Selecting fewer tags than `max_subscribed_items` means
everything arrives by subscription and the poller never starts.

**Connection:** `insecure()`, `connect_options(ConnectOptions)`.

**Discovery:** `cache_file(path)`, `cache(Arc<dyn TagRepository + Send + Sync>)`,
`no_cache()`, `force_rescan(bool)`, `filter(...)`, `scan_options(...)`,
`monitor_options(...)`.

**Lifecycle:** `handle() -> CollectorHandle`, `handle_ctrl_c()` behind the
`ctrl-c` feature, `discover() -> Result<TagSet>`,
`client() -> Result<TagClient>`, `run() -> Result<()>`.

`run()` sequence: take any deferred error; require a sink; open and register a
session; spawn the watchdog; load or scan tags; apply the selector; split at
`max_subscribed_items`; subscribe; fold rejects into the poll list; spawn the
poller on its own session; run the event loop inside `panic::catch_unwind`.

`CollectorHandle::stop()` sets the flag **and closes every registered session**.
Document why: a server does not learn an abandoned session is gone until it
times out, and until then its monitored items still count against the budget —
restart a collector a few times without clean shutdown and you exhaust a server
that was nowhere near its limit.

`handle_ctrl_c` calls `handle.stop()` and **nothing else** — no
`process::exit`. `run()` returns, `main` exits, destructors run.

Use a `StopOnDrop(Arc<AtomicBool>)` guard so background threads stop when the
pipeline unwinds.

## 5.3 `src/lib.rs` — replace

Module wiring with feature gates, all public re-exports, and the crate-level
docs. Add a free convenience function:

```rust
/// Scans an endpoint and returns its tags. Connects insecurely, does not cache.
#[cfg(all(feature = "monitoring", feature = "json-cache"))]
pub fn discover(endpoint_url: &str) -> Result<TagSet>;
```

Keep `pub use opcua;` with its explanation that the public API exposes `NodeId`,
`StatusCode`, `Variant`, `NodeClass`, and `SecurityPolicy`, so consumers must
use this path rather than their own `opcua` dependency.

## 5.4 `Cargo.toml` — edit

```toml
version = "0.2.0"
description = "An OPC UA client for PLC data: browse the address space, subscribe, poll, and write values back."

[dependencies]
opcua = { version = "0.12", features = ["client"] }
serde = { version = "1", features = ["derive"] }
log = "0.4"
thiserror = "2"
serde_json = { version = "1", optional = true }
chrono = { version = "0.4", optional = true }
ctrlc = { version = "3", optional = true }

[dev-dependencies]
env_logger = "0.11"

[features]
default = ["json-cache", "monitoring"]
json-cache = ["dep:serde_json"]
monitoring = []
jsonl-sink = ["monitoring", "json-cache", "dep:chrono"]
ctrl-c = ["dep:ctrlc"]
full = ["monitoring", "json-cache", "jsonl-sink", "ctrl-c"]

[package.metadata.docs.rs]
all-features = true
```

Add `[[example]]` entries with `required-features` for any example needing
`full`.

## 5.5 `examples/collect.rs` and `examples/write.rs` — NEW

`collect.rs` under `required-features = ["full"]`: build a collector with
`.insecure().jsonl("logs").handle_ctrl_c()` and run. Under 15 lines.

`write.rs`: obtain a `TagClient`, print the tag count, read one tag, show a
commented-out write.

### Verify phase 5

```
cargo check --all-features
cargo test --all-features
cargo clippy --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo package --list --all-features
```

The last command must list `README.md`, and that file must not be empty — an
earlier release shipped a zero-byte README and crates.io rendered no
documentation at all.

---

# Phase 6 — documentation and release

## 6.1 `README.md` — replace

Use the file supplied alongside this plan.

## 6.2 `CHANGELOG.md` — edit

```markdown
## [0.2.0]

### Added
- `Collector`, `CollectorHandle`, and `TagClient`
- `TagSink` trait with `sinks::LogSink` and `sinks::JsonlSink`; closures implement it
- `TagSet` with lookup by path, display name, node ID, and glob
- Writing: `set`, `set_node`, `set_many`, `set_verified`, guarded by `AccessLevel`
- Variable attributes from OPC 10000-3 Table 13 on `PlcTag`: `data_type`,
  `value_rank`, `access_level`, `min_sampling_interval`, `historizing`,
  `type_definition`, `reference_type`, `browse_name`
- Per-tag sampling intervals derived from the server's `MinimumSamplingInterval`
- `Security` and `Credentials`; `ConnectOptions::insecure()`
- `ScanOptions::include_properties` and `read_attributes`
- Features `monitoring`, `jsonl-sink`, `ctrl-c`, `full`

### Changed
- **Breaking:** `ConnectOptions::default()` now signs and encrypts with
  `Basic256Sha256` and rejects untrusted certificates. Previous behaviour is
  `ConnectOptions::insecure()`.
- **Breaking:** `PlcSession` gained a `write` method.
- **Breaking:** `PlcTag::path` is now built from `BrowseName` rather than
  `DisplayName`, which is stable across server locales. Existing caches load,
  but paths may differ; rescan to refresh.
- **Breaking:** `NodeFilter`'s blanket closure impl requires `Send + Sync`.
- `HasProperty` nodes are excluded from scans by default.
```

## 6.3 Release

```
git commit -am "Release 0.2.0"
git tag -a v0.2.0 -m "opcua-tag-browser 0.2.0"
cargo publish --all-features
git push && git push --tags
```

Versions on crates.io are permanent and cannot be reused. Verify
`cargo package --list` before publishing, not after.

---

# Acceptance criteria

The implementation is complete when all of these hold.

**Functional**

- [ ] Browse: recursive walk, continuation points, ancestor-based cycle
      detection, `BrowseName` paths, partial-scan reporting via `ScanReport`
- [ ] Attributes: `AccessLevel`, `DataType`, `ValueRank`,
      `MinimumSamplingInterval`, `Historizing` populated when
      `read_attributes` is on
- [ ] Read: subscription with per-tag sampling, polling with adaptive batching,
      one-shot `get`
- [ ] Write: `set`, `set_many`, `set_verified`, refused on read-only tags
- [ ] Reconnect: poller reopens its own session; watchdog reports transitions
- [ ] Shutdown: `CollectorHandle::stop()` closes every session

**Optimization**

- [ ] Tag list cached; no rescan on restart
- [ ] Server `MaxNodesPerRead` queried and respected
- [ ] Batch size halves on read failure, floor 10
- [ ] `max_age` set so the server may serve cached values
- [ ] Node IDs pre-parsed outside every loop
- [ ] `TagSet` lookup is hash-based, not linear
- [ ] `TagChange` borrows rather than allocating per event
- [ ] `JsonlSink` I/O is off the callback thread
- [ ] `HasProperty` metadata excluded by default

**Security**

- [ ] `ConnectOptions::default()` is encrypted and validates certificates
- [ ] Credentials over plaintext return `Error::InsecureCredentials`
- [ ] `Credentials` `Debug` redacts passwords
- [ ] Writes guarded by `AccessLevel`
- [ ] `#![forbid(unsafe_code)]` present
- [ ] No `process::exit` outside `handle_ctrl_c`'s documented behaviour
- [ ] `opcua` panics contained by `catch_panic`
- [ ] No `println!`/`eprintln!` anywhere

**Quality**

- [ ] `cargo clippy --all-features -- -D warnings` clean
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` clean
- [ ] Every public item documented
- [ ] `cargo package --list` includes a non-empty `README.md`

# Notes on things that may not compile as written

These call sites are written against the `opcua` 0.12 API from documentation
rather than a verified build. If any fails, adapt it and report what the real
signature is:

- `Session::run(...)` — argument type and whether it takes ownership
- `DataValue::status` — `Option<StatusCode>` versus a bare `StatusCode`; the
  `format_quality` and `good` call sites depend on which
- `WriteValue` field names and `DataValue::value_only(...)`
- `UserTokenPolicy` construction for username authentication
- `AttributeId` variant names used in `attributes.rs`
- Whether the `opcua` client feature is spelled `"client"` — confirm with
  `cargo info opcua`